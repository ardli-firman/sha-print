//! Moving supported settings from the previous .NET ShaPrint application (#41).
//!
//! The rules this module owns: the new app takes the previous Network Channel and nothing else,
//! printer queues are always selected again, the previous app's files are only ever read, and a
//! server certificate is never trusted by importing anything. Repeating this import changes
//! nothing, so it is safe to run on every start.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::domain::AppError;

/// File that records what this app already took from the previous one.
const MARKER_FILE: &str = "legacy-import.json";

/// The Network Channel the previous ShaPrint app used.
#[derive(Clone, PartialEq, Eq, Default)]
pub enum LegacyChannel {
    /// The previous app had no channel of its own.
    #[default]
    Absent,
    /// The channel itself. It is never logged, published, or sent over IPC.
    Readable(String),
    /// Present, but Windows would not open it for this user.
    Unreadable,
}

impl std::fmt::Debug for LegacyChannel {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LegacyChannel::Absent => formatter.write_str("Absent"),
            LegacyChannel::Readable(_) => formatter.write_str("Readable(<redacted>)"),
            LegacyChannel::Unreadable => formatter.write_str("Unreadable"),
        }
    }
}

/// What the previous app left for this user.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LegacySnapshot {
    /// Whether the previous app left a settings file at all.
    pub has_settings: bool,
    /// The previous app's Network Channel.
    pub channel: LegacyChannel,
    /// Whether the previous app had installed client printer queues.
    pub has_client_queues: bool,
    /// Whether the previous app had shared server printer queues configured.
    pub has_shared_queues: bool,
}

impl LegacySnapshot {
    /// Whether the previous app left anything for this user.
    pub fn found(&self) -> bool {
        self.has_settings || self.has_client_queues || self.has_shared_queues
    }
}

/// Reads what the previous ShaPrint app left on this computer.
pub trait LegacySettingsSource: Send + Sync + 'static {
    fn load(&self) -> Result<LegacySnapshot, AppError>;
}

/// This app's own Network Channel storage.
#[async_trait]
pub trait ChannelStore: Send + Sync + 'static {
    /// Whether this app already has a Network Channel of its own.
    fn is_configured(&self) -> bool;

    /// Stores the Network Channel. The value is never logged or returned.
    async fn store(&self, channel: &str) -> Result<(), AppError>;
}

/// What happened to the previous app's Network Channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelOutcome {
    /// The previous app has a channel and this one does not: the import will take it.
    Importable,
    /// This app imported the previous app's channel.
    Imported,
    /// This app already has its own channel, so the previous one was left alone.
    KeptExisting,
    /// The previous app had no channel of its own.
    Absent,
    /// The previous app had a channel, but Windows would not open it for this user.
    Unreadable,
}

impl ChannelOutcome {
    /// The stable id used by IPC payloads and the UI.
    pub const fn as_str(self) -> &'static str {
        match self {
            ChannelOutcome::Importable => "importable",
            ChannelOutcome::Imported => "imported",
            ChannelOutcome::KeptExisting => "kept-existing",
            ChannelOutcome::Absent => "absent",
            ChannelOutcome::Unreadable => "unreadable",
        }
    }

    /// The sentence the window shows; never contains the channel itself.
    pub const fn note(self) -> &'static str {
        match self {
            ChannelOutcome::Importable => {
                "The previous ShaPrint app's Network Channel is ready to be imported."
            }
            ChannelOutcome::Imported => {
                "Your Network Channel was imported from the previous ShaPrint app."
            }
            ChannelOutcome::KeptExisting => {
                "This app already has its own Network Channel, so the previous app's value was left alone."
            }
            ChannelOutcome::Absent => {
                "The previous ShaPrint app had no Network Channel of its own. Set one here before clients can print."
            }
            ChannelOutcome::Unreadable => {
                "The previous app had a Network Channel that Windows would not open for this user. Set it again here."
            }
        }
    }
}

/// One setting from the previous app that this app does not take.
///
/// The Network Channel is the only setting with an equivalent today, and it is reported separately
/// as a [`ChannelOutcome`]: everything else is left behind, with the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedSetting {
    /// Stable key, for example `client-queues`.
    pub key: String,
    /// What the user calls it.
    pub label: String,
    /// Why this app does not take it.
    pub reason: String,
}

/// What the window shows about the move from the previous app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportReport {
    /// Whether the previous app left anything for this user.
    pub found: bool,
    /// What happened to the Network Channel.
    pub channel: ChannelOutcome,
    /// Every other setting the previous app had, and why it stayed behind.
    pub settings: Vec<SkippedSetting>,
    /// Whether the previous app had queues the user has to select again in this app.
    pub queues_need_reselection: bool,
}

/// Settings the previous app had that this app has no equivalent for.
const SETTINGS_WITHOUT_EQUIVALENT: [(&str, &str, &str); 4] = [
    (
        "auto-update",
        "Automatic updates",
        "This app does not update itself yet.",
    ),
    (
        "update-channel",
        "Update channel",
        "This app does not update itself yet.",
    ),
    (
        "auto-purge",
        "Automatic print job cleanup",
        "This app does not keep print job history.",
    ),
    (
        "scan-settings",
        "Scan folder and auto-save",
        "Scanning is not part of this app.",
    ),
];

/// Moves the supported settings from the previous ShaPrint app into this one.
pub struct LegacyImport {
    source: Arc<dyn LegacySettingsSource>,
    channel: Arc<dyn ChannelStore>,
    marker_path: Option<PathBuf>,
    last: watch::Sender<Option<ImportReport>>,
}

impl LegacyImport {
    /// `data_dir` is where the record of what was already imported lives; `None` keeps it in
    /// memory, which is what a test without a directory needs.
    pub fn new(
        source: Arc<dyn LegacySettingsSource>,
        channel: Arc<dyn ChannelStore>,
        data_dir: Option<&Path>,
    ) -> Self {
        let (last, _) = watch::channel(None);
        Self {
            source,
            channel,
            marker_path: data_dir.map(|directory| directory.join(MARKER_FILE)),
            last,
        }
    }

    /// The outcome of the last import this app ran, if it ran one.
    pub fn report(&self) -> Option<ImportReport> {
        self.last.borrow().clone()
    }

    /// Follows every import this app runs; the receiver also holds the current report.
    pub fn subscribe(&self) -> watch::Receiver<Option<ImportReport>> {
        self.last.subscribe()
    }

    /// What an import would do, without changing anything.
    ///
    /// This reads the previous app's files and may open a value Windows protects, so a caller on
    /// the async runtime runs it inside a blocking worker.
    pub fn inspect(&self) -> Result<ImportReport, AppError> {
        let snapshot = self.source.load()?;
        let marker = read_marker(self.marker_path.as_deref());
        let outcome = self.outcome(&snapshot, marker.network_channel_imported);
        Ok(self.describe(&snapshot, outcome))
    }

    /// Runs the import and reports what it did.
    ///
    /// Safe to call on every start: it only ever writes while this app has no Network Channel and
    /// the previous app's channel has not already been taken.
    pub async fn apply(&self) -> Result<ImportReport, AppError> {
        // Reading the previous app's files and opening its protected channel is synchronous file and
        // Windows work, so it stays off the async runtime.
        let snapshot = {
            let source = Arc::clone(&self.source);
            off_thread(move || source.load()).await?
        };
        let marker_path = self.marker_path.clone();
        let mut marker = {
            let path = marker_path.clone();
            off_thread(move || Ok(read_marker(path.as_deref()))).await?
        };
        let mut outcome = self.outcome(&snapshot, marker.network_channel_imported);

        if outcome == ChannelOutcome::Importable {
            if let LegacyChannel::Readable(channel) = &snapshot.channel {
                self.channel.store(channel).await?;
                outcome = ChannelOutcome::Imported;
                marker.network_channel_imported = true;
                let path = marker_path.clone();
                if let Err(error) = off_thread(move || -> Result<(), AppError> {
                    write_marker(path.as_deref(), &marker);
                    Ok(())
                })
                .await
                {
                    // Cosmetic: the channel is configured, so a later import already leaves it
                    // alone even when this record cannot be written.
                    log::warn!(
                        "cannot save the legacy import record code={} message={}",
                        error.code_str(),
                        error
                    );
                }
            }
        }

        let report = self.describe(&snapshot, outcome);
        self.last.send_replace(Some(report.clone()));
        Ok(report)
    }

    /// What the previous app's channel means for this app right now.
    fn outcome(&self, snapshot: &LegacySnapshot, already_imported: bool) -> ChannelOutcome {
        match &snapshot.channel {
            LegacyChannel::Unreadable => ChannelOutcome::Unreadable,
            LegacyChannel::Absent => ChannelOutcome::Absent,
            LegacyChannel::Readable(_) => {
                if already_imported {
                    ChannelOutcome::Imported
                } else if self.channel.is_configured() {
                    ChannelOutcome::KeptExisting
                } else {
                    ChannelOutcome::Importable
                }
            }
        }
    }

    /// Builds the report, naming every previous setting this app does not take.
    fn describe(&self, snapshot: &LegacySnapshot, channel: ChannelOutcome) -> ImportReport {
        let mut settings = Vec::new();
        if snapshot.has_settings {
            settings.extend(
                SETTINGS_WITHOUT_EQUIVALENT.map(|(key, label, reason)| skipped(key, label, reason)),
            );
        }
        if snapshot.has_client_queues {
            settings.push(skipped(
                "client-queues",
                "Installed client printers",
                "Printer queues are installed again from this app; the previous app's queues are not activated.",
            ));
        }
        if snapshot.has_shared_queues {
            settings.push(skipped(
                "shared-queues",
                "Shared server printers",
                "Printer queues are selected again in this app; the previous selection is not activated.",
            ));
        }

        ImportReport {
            found: snapshot.found(),
            channel,
            settings,
            queues_need_reselection: snapshot.has_client_queues || snapshot.has_shared_queues,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ImportMarker {
    #[serde(default)]
    network_channel_imported: bool,
}

/// Runs one piece of synchronous file or Windows work off the async runtime.
async fn off_thread<T, F>(work: F) -> Result<T, AppError>
where
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| AppError::internal("the previous app's settings worker stopped"))?
}

/// Reads the record of what this app already took from the previous one.
fn read_marker(path: Option<&Path>) -> ImportMarker {
    let Some(path) = path else {
        return ImportMarker::default();
    };
    match std::fs::read(path) {
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(marker) => marker,
            Err(_) => {
                log::warn!("the legacy import record is invalid; importing again");
                ImportMarker::default()
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => ImportMarker::default(),
        Err(error) => {
            log::warn!("cannot read the legacy import record message={error}");
            ImportMarker::default()
        }
    }
}

/// Records what was imported. A failure here is cosmetic: the Network Channel is configured, so a
/// later import already leaves it alone even when this record cannot be written.
fn write_marker(path: Option<&Path>, marker: &ImportMarker) {
    let Some(path) = path else {
        return;
    };
    let bytes = match serde_json::to_vec(marker) {
        Ok(bytes) => bytes,
        Err(error) => {
            log::warn!("cannot encode the legacy import record message={error}");
            return;
        }
    };
    if let Some(parent) = path.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            log::warn!("cannot create the app data directory message={error}");
            return;
        }
    }
    if let Err(error) = std::fs::write(path, bytes) {
        log::warn!("cannot save the legacy import record message={error}");
    }
}

fn skipped(key: &str, label: &str, reason: &str) -> SkippedSetting {
    SkippedSetting {
        key: key.to_owned(),
        label: label.to_owned(),
        reason: reason.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::temporary_directory;
    use std::sync::Mutex;

    /// The Network Channel the fake previous installation had.
    ///
    /// Generated rather than written out, so no file in the repository contains a value that reads
    /// like a real channel.
    fn legacy_channel() -> &'static str {
        static VALUE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        VALUE.get_or_init(|| crate::test_support::unique_value("legacy-channel"))
    }

    /// A previous installation whose contents a test controls.
    #[derive(Default)]
    struct FakeSource {
        snapshot: LegacySnapshot,
        reads: Mutex<u32>,
    }

    impl FakeSource {
        fn with(snapshot: LegacySnapshot) -> Arc<Self> {
            Arc::new(Self {
                snapshot,
                reads: Mutex::new(0),
            })
        }

        fn reads(&self) -> u32 {
            self.reads.lock().map(|reads| *reads).unwrap_or_default()
        }
    }

    impl LegacySettingsSource for FakeSource {
        fn load(&self) -> Result<LegacySnapshot, AppError> {
            if let Ok(mut reads) = self.reads.lock() {
                *reads += 1;
            }
            Ok(self.snapshot.clone())
        }
    }

    /// The app's own channel storage, recording every value it was given.
    #[derive(Default)]
    struct FakeChannel {
        configured: Mutex<bool>,
        stored: Mutex<Vec<String>>,
    }

    impl FakeChannel {
        fn configured() -> Arc<Self> {
            Arc::new(Self {
                configured: Mutex::new(true),
                stored: Mutex::new(Vec::new()),
            })
        }

        fn stored(&self) -> Vec<String> {
            self.stored
                .lock()
                .map(|stored| stored.clone())
                .unwrap_or_default()
        }
    }

    #[async_trait]
    impl ChannelStore for FakeChannel {
        fn is_configured(&self) -> bool {
            self.configured.lock().map(|value| *value).unwrap_or(false)
        }

        async fn store(&self, channel: &str) -> Result<(), AppError> {
            if let Ok(mut stored) = self.stored.lock() {
                stored.push(channel.to_owned());
            }
            if let Ok(mut configured) = self.configured.lock() {
                *configured = true;
            }
            Ok(())
        }
    }

    fn snapshot(channel: LegacyChannel) -> LegacySnapshot {
        LegacySnapshot {
            has_settings: true,
            channel,
            has_client_queues: false,
            has_shared_queues: false,
        }
    }

    fn import(
        directory: &Path,
        snapshot: LegacySnapshot,
    ) -> (LegacyImport, Arc<FakeChannel>, Arc<FakeSource>) {
        let source = FakeSource::with(snapshot);
        let channel = Arc::new(FakeChannel::default());
        let import = LegacyImport::new(
            Arc::clone(&source) as Arc<dyn LegacySettingsSource>,
            Arc::clone(&channel) as Arc<dyn ChannelStore>,
            Some(directory),
        );
        (import, channel, source)
    }

    #[test]
    fn a_computer_without_the_previous_app_has_nothing_to_import() {
        let directory = temporary_directory("empty");
        let (import, channel, _source) = import(&directory, LegacySnapshot::default());

        let report = import.inspect().expect("inspects");

        assert!(!report.found);
        assert_eq!(report.channel, ChannelOutcome::Absent);
        assert!(report.settings.is_empty());
        assert!(!report.queues_need_reselection);
        assert!(channel.stored().is_empty());
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_channel_the_previous_app_had_is_ready_to_import() {
        let (import, _channel, _source) = import(
            &temporary_directory("ready"),
            snapshot(LegacyChannel::Readable(legacy_channel().to_owned())),
        );

        let report = import.inspect().expect("inspects");

        assert_eq!(report.channel, ChannelOutcome::Importable);
        assert!(report.found);
    }

    #[tokio::test]
    async fn the_previous_channel_is_imported_once() {
        let directory = temporary_directory("once");
        let (import, channel, _source) = import(
            &directory,
            snapshot(LegacyChannel::Readable(legacy_channel().to_owned())),
        );

        let report = import.apply().await.expect("imports");

        assert_eq!(report.channel, ChannelOutcome::Imported);
        assert_eq!(channel.stored(), vec![legacy_channel().to_owned()]);

        // A second start finds the channel already here and writes nothing again.
        let again = import.apply().await.expect("imports again");
        assert_eq!(again.channel, ChannelOutcome::Imported);
        assert_eq!(channel.stored(), vec![legacy_channel().to_owned()]);
        assert_eq!(import.report(), Some(again));
        assert!(directory.join(MARKER_FILE).exists());
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[tokio::test]
    async fn an_app_that_already_has_a_channel_keeps_it() {
        let directory = temporary_directory("keeps");
        let source = FakeSource::with(snapshot(LegacyChannel::Readable(
            legacy_channel().to_owned(),
        )));
        let channel = FakeChannel::configured();
        let import = LegacyImport::new(
            Arc::clone(&source) as Arc<dyn LegacySettingsSource>,
            Arc::clone(&channel) as Arc<dyn ChannelStore>,
            Some(&directory),
        );

        let report = import.apply().await.expect("imports");

        assert_eq!(report.channel, ChannelOutcome::KeptExisting);
        assert!(channel.stored().is_empty());
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[tokio::test]
    async fn a_channel_windows_will_not_open_is_reported_and_not_invented() {
        let directory = temporary_directory("sealed");
        let (import, channel, _source) = import(&directory, snapshot(LegacyChannel::Unreadable));

        let report = import.apply().await.expect("imports");

        assert_eq!(report.channel, ChannelOutcome::Unreadable);
        assert!(channel.stored().is_empty());
        assert!(report.channel.note().contains("would not open"));
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[tokio::test]
    async fn queues_from_the_previous_app_are_never_activated() {
        let directory = temporary_directory("queues");
        let (import, channel, _source) = import(
            &directory,
            LegacySnapshot {
                has_settings: true,
                channel: LegacyChannel::Readable(legacy_channel().to_owned()),
                has_client_queues: true,
                has_shared_queues: true,
            },
        );

        let report = import.apply().await.expect("imports");
        let queue_settings: Vec<&SkippedSetting> = report
            .settings
            .iter()
            .filter(|setting| setting.key.ends_with("queues"))
            .collect();

        assert!(report.queues_need_reselection);
        assert_eq!(queue_settings.len(), 2);
        for setting in queue_settings {
            assert!(
                setting.reason.contains("selected again")
                    || setting.reason.contains("installed again"),
                "{} is not explained as left behind: {}",
                setting.key,
                setting.reason
            );
        }
        // Nothing about a queue ever reaches the channel storage.
        assert_eq!(channel.stored(), vec![legacy_channel().to_owned()]);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[tokio::test]
    async fn settings_without_an_equivalent_are_named_as_left_behind() {
        let directory = temporary_directory("settings");
        let (import, _channel, _source) = import(
            &directory,
            snapshot(LegacyChannel::Readable(legacy_channel().to_owned())),
        );

        let report = import.apply().await.expect("imports");
        let keys: Vec<&str> = report
            .settings
            .iter()
            .map(|setting| setting.key.as_str())
            .collect();

        assert_eq!(
            keys,
            vec![
                "auto-update",
                "update-channel",
                "auto-purge",
                "scan-settings"
            ]
        );
        for setting in &report.settings {
            assert!(!setting.reason.is_empty(), "{} has no reason", setting.key);
        }
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn the_report_never_carries_the_channel_value() {
        let (import, _channel, _source) = import(
            &temporary_directory("redaction"),
            snapshot(LegacyChannel::Readable(legacy_channel().to_owned())),
        );

        let report = import.inspect().expect("inspects");
        let rendered = format!("{report:?}");

        assert_eq!(report.channel, ChannelOutcome::Importable);
        // The snapshot the reader returns is redacted too, so a log line cannot carry the value.
        assert!(
            !format!("{:?}", LegacyChannel::Readable(legacy_channel().to_owned()))
                .contains(legacy_channel())
        );
        assert!(!rendered.contains(legacy_channel()), "{rendered}");
    }

    #[test]
    fn the_source_is_only_ever_read() {
        let (import, _channel, source) = import(
            &temporary_directory("read-only"),
            snapshot(LegacyChannel::Readable(legacy_channel().to_owned())),
        );

        import.inspect().expect("inspects");

        assert_eq!(source.reads(), 1);
    }
}
