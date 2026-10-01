//! What the previous .NET ShaPrint application left in this user's profile (#41).
//!
//! Strictly read-only: the previous application keeps working, with its own settings intact, after
//! the new one has taken what it can use. Only the Network Channel has an equivalent here, and the
//! previous app's printer queues are only counted, never activated.

use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde::Deserialize;

use crate::application::{LegacyChannel, LegacySettingsSource, LegacySnapshot};
use crate::domain::AppError;

/// Folder the previous application used under the user's local app data.
const LEGACY_FOLDER: &str = "ShaPrint";
/// The previous application's settings file.
const SETTINGS_FILE: &str = "AppSettings.json";
/// The previous application's installed client queues.
const CLIENT_QUEUES_FILE: &str = "ClientConfig.json";
/// The previous application's shared server queues.
const SERVER_QUEUES_FILE: &str = "ServerConfig.json";

/// The extra secret the previous application used to protect the Network Channel.
#[cfg(windows)]
const CHANNEL_ENTROPY: &[u8] = b"ShaPrint-DPAPI-Entropy";

/// The placeholder the previous application stored when the user never chose a Network Channel.
///
/// Keep in sync with `AppSettingsData.NetworkChannel` and `WelcomeViewModel.SaveChannel` in the
/// .NET application.
const LEGACY_PLACEHOLDER_CHANNEL: &str = "DefaultChannel";

/// The one field of the previous application's settings this app can use.
#[derive(Deserialize)]
struct LegacyAppSettings {
    #[serde(default, rename = "EncryptedNetworkChannel")]
    encrypted_network_channel: String,
}

/// Reads the previous application's folder under the user's local app data.
pub struct FileLegacySettings {
    directory: PathBuf,
}

impl FileLegacySettings {
    /// The previous application's folder for this user, when this platform has one.
    pub fn locate() -> Option<Self> {
        local_app_data().map(|root| Self::in_directory(root.join(LEGACY_FOLDER)))
    }

    /// Reads a specific folder; what tests and a user-supplied location use.
    pub fn in_directory(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }
}

impl LegacySettingsSource for FileLegacySettings {
    fn load(&self) -> Result<LegacySnapshot, AppError> {
        let settings = self.directory.join(SETTINGS_FILE);
        let has_settings = settings.is_file();
        Ok(LegacySnapshot {
            has_settings,
            channel: if has_settings {
                read_channel(&settings)
            } else {
                LegacyChannel::Absent
            },
            has_client_queues: has_entries(&self.directory.join(CLIENT_QUEUES_FILE)),
            has_shared_queues: has_entries(&self.directory.join(SERVER_QUEUES_FILE)),
        })
    }
}

/// A computer with no previous ShaPrint application to read.
pub struct NoLegacySettings;

impl LegacySettingsSource for NoLegacySettings {
    fn load(&self) -> Result<LegacySnapshot, AppError> {
        Ok(LegacySnapshot::default())
    }
}

#[cfg(windows)]
fn local_app_data() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
}

#[cfg(not(windows))]
fn local_app_data() -> Option<PathBuf> {
    // The previous application is a Windows product; there is nowhere else to look.
    None
}

/// Reads the previous app's Network Channel, or explains why it cannot be used.
fn read_channel(path: &Path) -> LegacyChannel {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            log::warn!("cannot read the previous app's settings message={error}");
            return LegacyChannel::Absent;
        }
    };
    let settings: LegacyAppSettings = match serde_json::from_slice(&bytes) {
        Ok(settings) => settings,
        Err(_) => {
            log::warn!("the previous app's settings could not be read");
            return LegacyChannel::Absent;
        }
    };
    if settings.encrypted_network_channel.trim().is_empty() {
        return LegacyChannel::Absent;
    }
    match open_legacy_secret(&settings.encrypted_network_channel) {
        Some(channel) => classify_legacy_channel(&channel),
        None => LegacyChannel::Unreadable,
    }
}

/// Decides what a Network Channel the previous app stored means for this app.
///
/// The previous app also *wrote* its well-known placeholder whenever the user left the field blank
/// (`WelcomeViewModel.SaveChannel`), and it treated that value as a weak channel itself. Carrying it
/// forward would hand this app a shared placeholder presented as a real secret, so it counts as an
/// absence of configuration and the user is asked for a channel instead (ADR 0006).
fn classify_legacy_channel(plaintext: &str) -> LegacyChannel {
    let channel = plaintext.trim();
    if channel.is_empty() || channel == LEGACY_PLACEHOLDER_CHANNEL {
        if !channel.is_empty() {
            log::info!("the previous app stored its placeholder Network Channel as if it were set");
        }
        LegacyChannel::Absent
    } else {
        LegacyChannel::Readable(channel.to_owned())
    }
}

/// Decodes and opens a value the previous app protected with DPAPI for this user.
fn open_legacy_secret(encoded: &str) -> Option<String> {
    let protected = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .ok()?;
    let plaintext = unprotect_for_legacy(&protected)?;
    String::from_utf8(plaintext).ok()
}

#[cfg(windows)]
fn unprotect_for_legacy(protected: &[u8]) -> Option<Vec<u8>> {
    match crate::adapters::win_crypto::unprotect(protected, Some(CHANNEL_ENTROPY)) {
        Ok(plaintext) => Some(plaintext),
        Err(error) => {
            // The value is unreadable rather than absent: a different Windows user protected it, or
            // the previous installation's entropy no longer matches.
            log::warn!(
                "Windows would not open the previous app's Network Channel code={}",
                error.code_str()
            );
            None
        }
    }
}

#[cfg(not(windows))]
fn unprotect_for_legacy(_protected: &[u8]) -> Option<Vec<u8>> {
    // DPAPI is a Windows facility, so a legacy value cannot be opened here.
    None
}

/// Whether a queue file holds at least one entry.
///
/// A file whose shape cannot be read still means the user configured something, so it counts as
/// entries: the user is asked to select queues again either way.
fn has_entries(path: &Path) -> bool {
    match std::fs::read(path) {
        Err(_) => false,
        Ok(bytes) => match serde_json::from_slice::<Vec<serde_json::Value>>(&bytes) {
            Ok(entries) => !entries.is_empty(),
            Err(_) => true,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{temporary_directory, unique_value};

    fn write(directory: &Path, name: &str, contents: &str) {
        std::fs::write(directory.join(name), contents).expect("writes the legacy file");
    }

    #[test]
    fn the_placeholders_of_an_unconfigured_previous_app_are_not_channels() {
        // The previous app used this literal in two places, and wrote it into the settings file
        // whenever the user left the field blank.
        assert_eq!(
            classify_legacy_channel("DefaultChannel"),
            LegacyChannel::Absent
        );
        assert_eq!(
            classify_legacy_channel("  DefaultChannel  "),
            LegacyChannel::Absent
        );
        assert_eq!(classify_legacy_channel(""), LegacyChannel::Absent);

        // Anything else is a channel the user chose, and is carried forward as it stands.
        let chosen = unique_value("chosen-channel");
        assert_eq!(
            classify_legacy_channel(&chosen),
            LegacyChannel::Readable(chosen.clone())
        );
        assert_eq!(
            classify_legacy_channel(&format!("  {chosen}  ")),
            LegacyChannel::Readable(chosen)
        );
    }

    #[test]
    fn a_directory_without_the_previous_app_holds_nothing() {
        let directory = temporary_directory("absent");

        let snapshot = FileLegacySettings::in_directory(&directory)
            .load()
            .expect("reads an empty directory");

        assert!(!snapshot.found());
        assert_eq!(snapshot.channel, LegacyChannel::Absent);
        assert!(!snapshot.has_client_queues);
        assert!(!snapshot.has_shared_queues);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn settings_without_a_channel_are_still_settings() {
        let directory = temporary_directory("no-channel");
        write(
            &directory,
            SETTINGS_FILE,
            r#"{"AutoUpdateEnabled":true,"EncryptedNetworkChannel":""}"#,
        );

        let snapshot = FileLegacySettings::in_directory(&directory)
            .load()
            .expect("reads the settings");

        assert!(snapshot.has_settings);
        assert!(snapshot.found());
        // The previous app's well-known default is not a channel the user configured.
        assert_eq!(snapshot.channel, LegacyChannel::Absent);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_protected_channel_that_cannot_be_opened_here_is_reported_as_unreadable() {
        let directory = temporary_directory("sealed");
        // A well-formed envelope that DPAPI cannot open on this machine, or on any machine other
        // than the one that wrote it.
        write(
            &directory,
            SETTINGS_FILE,
            r#"{"EncryptedNetworkChannel":"AQAAANCMnd8BFdERjHoAwE/Cl+sBAAAA"}"#,
        );

        let snapshot = FileLegacySettings::in_directory(&directory)
            .load()
            .expect("reads the settings");

        assert_eq!(snapshot.channel, LegacyChannel::Unreadable);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_settings_file_that_is_not_valid_json_is_treated_as_unconfigured() {
        let directory = temporary_directory("broken");
        write(&directory, SETTINGS_FILE, "not json at all");

        let snapshot = FileLegacySettings::in_directory(&directory)
            .load()
            .expect("reads the settings");

        assert!(snapshot.has_settings);
        assert_eq!(snapshot.channel, LegacyChannel::Absent);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn installed_client_queues_are_counted_but_never_read_as_configuration() {
        let directory = temporary_directory("client-queues");
        write(
            &directory,
            CLIENT_QUEUES_FILE,
            r#"[{"VirtualPrinterName":"ShaPrint Office Printer","PipeName":"shaprint-1","ServerIp":"192.0.2.10"}]"#,
        );

        let snapshot = FileLegacySettings::in_directory(&directory)
            .load()
            .expect("reads the queue file");

        assert!(snapshot.has_client_queues);
        assert!(!snapshot.has_shared_queues);
        // The queue itself is never carried anywhere: only the fact that queues existed.
        assert_eq!(snapshot.channel, LegacyChannel::Absent);
        assert!(!format!("{snapshot:?}").contains("ShaPrint Office Printer"));
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn shared_server_queues_are_counted_and_an_empty_queue_file_is_not() {
        let directory = temporary_directory("server-queues");
        write(&directory, CLIENT_QUEUES_FILE, "[]");
        write(
            &directory,
            SERVER_QUEUES_FILE,
            r#"{"SharedPrinters":["HP LaserJet"]}"#,
        );

        let snapshot = FileLegacySettings::in_directory(&directory)
            .load()
            .expect("reads the queue files");

        assert!(!snapshot.has_client_queues);
        assert!(snapshot.has_shared_queues);
        assert!(snapshot.found());
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_queue_file_whose_shape_is_unknown_still_asks_the_user_to_reselect() {
        let directory = temporary_directory("unknown-shape");
        write(&directory, SERVER_QUEUES_FILE, "{}");

        let snapshot = FileLegacySettings::in_directory(&directory)
            .load()
            .expect("reads the queue file");

        assert!(snapshot.has_shared_queues);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn reading_never_changes_the_previous_apps_files() {
        let directory = temporary_directory("read-only");
        let contents = r#"{"AutoUpdateEnabled":true,"EncryptedNetworkChannel":""}"#;
        write(&directory, SETTINGS_FILE, contents);

        let _ = FileLegacySettings::in_directory(&directory).load();

        let after = std::fs::read_to_string(directory.join(SETTINGS_FILE)).expect("reads back");
        assert_eq!(after, contents);
        assert!(!directory.join("legacy-import.json").exists());
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// Windows is the only platform where the previous app's protection can be reproduced, so this
    /// is the one check that exercises the whole DPAPI leg of the import.
    #[cfg(windows)]
    #[test]
    fn a_channel_the_previous_app_protected_is_read_back() {
        let directory = temporary_directory("windows-channel");
        let channel = unique_value("legacy-channel");
        write_protected_channel(&directory, &channel);

        let snapshot = FileLegacySettings::in_directory(&directory)
            .load()
            .expect("reads the settings");

        assert_eq!(snapshot.channel, LegacyChannel::Readable(channel));
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// The placeholder the previous app wrote when the user left the field blank is not a channel,
    /// even when the previous app stored it as if it were one.
    #[cfg(windows)]
    #[test]
    fn a_protected_placeholder_is_not_imported_as_a_channel() {
        let directory = temporary_directory("windows-placeholder");
        write_protected_channel(&directory, LEGACY_PLACEHOLDER_CHANNEL);

        let snapshot = FileLegacySettings::in_directory(&directory)
            .load()
            .expect("reads the settings");

        assert!(snapshot.has_settings);
        assert_eq!(snapshot.channel, LegacyChannel::Absent);
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// Writes the settings file exactly the way the previous .NET app did: DPAPI with its own
    /// entropy, base64 in the file.
    #[cfg(windows)]
    fn write_protected_channel(directory: &Path, channel: &str) {
        use base64::Engine as _;

        let protected =
            crate::adapters::win_crypto::protect(channel.as_bytes(), Some(CHANNEL_ENTROPY))
                .expect("protects the channel the way the previous app did");
        let encoded = base64::engine::general_purpose::STANDARD.encode(protected);
        write(
            directory,
            SETTINGS_FILE,
            &format!(r#"{{"AutoUpdateEnabled":true,"EncryptedNetworkChannel":"{encoded}"}}"#),
        );
    }

    /// A value protected with different entropy belongs to someone else, so it must not be opened.
    #[cfg(windows)]
    #[test]
    fn a_channel_protected_with_another_entropy_is_not_opened() {
        use base64::Engine as _;

        let directory = temporary_directory("windows-foreign");
        let foreign = unique_value("foreign-channel");
        let foreign_entropy = unique_value("other-app");
        let protected = crate::adapters::win_crypto::protect(
            foreign.as_bytes(),
            Some(foreign_entropy.as_bytes()),
        )
        .expect("protects a foreign value");
        let encoded = base64::engine::general_purpose::STANDARD.encode(protected);
        write(
            &directory,
            SETTINGS_FILE,
            &format!(r#"{{"EncryptedNetworkChannel":"{encoded}"}}"#),
        );

        let snapshot = FileLegacySettings::in_directory(&directory)
            .load()
            .expect("reads the settings");

        assert_eq!(snapshot.channel, LegacyChannel::Unreadable);
        let _ = std::fs::remove_dir_all(&directory);
    }
}
