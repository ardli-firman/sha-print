//! The server-side sharing use case: which local queues are shared, and how the UI sees them.
//!
//! The use case owns the selection and the rules around it; the spooler and the network live in
//! adapters (`adapters::printers`, `adapters::ipps`).

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::application::{LocalPrinterCatalog, SharedPrinterSource};
use crate::domain::{AppError, PrinterName, SharedPrinters};

/// File that records the user's selected shared printers and sharing state.
const SETTINGS_FILE: &str = "sharing_state.json";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct SavedSharing {
    enabled: bool,
    printers: Vec<String>,
}

/// One local queue as the UI lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalPrinter {
    name: PrinterName,
    shared: bool,
}

impl LocalPrinter {
    /// One local queue and whether the server shares it.
    pub fn new(name: PrinterName, shared: bool) -> Self {
        Self { name, shared }
    }

    pub fn name(&self) -> &PrinterName {
        &self.name
    }

    /// Whether the server currently shares this queue with clients.
    pub fn shared(&self) -> bool {
        self.shared
    }
}

/// The local queues the server user chose to share.
///
/// The selection lives here whether or not sharing runs, and it is published as it changes: the
/// endpoint reads it on every request, and the discovery advertisement follows it (#36).
pub struct Sharing {
    catalog: Arc<dyn LocalPrinterCatalog>,
    selection: RwLock<SharedPrinters>,
    changes: watch::Sender<SharedPrinters>,
    preference_path: Option<PathBuf>,
    enabled: RwLock<bool>,
}

impl Sharing {
    pub fn new(catalog: Arc<dyn LocalPrinterCatalog>) -> Self {
        Self::with_persistence(catalog, None)
    }

    /// Initializes sharing, restoring previously saved printer selection and enabled state if available.
    pub fn with_persistence(
        catalog: Arc<dyn LocalPrinterCatalog>,
        data_dir: Option<&Path>,
    ) -> Self {
        let preference_path = data_dir.map(|dir| dir.join(SETTINGS_FILE));
        let saved = preference_path
            .as_ref()
            .and_then(|path| match std::fs::read(path) {
                Ok(bytes) => match serde_json::from_slice::<SavedSharing>(&bytes) {
                    Ok(settings) => Some(settings),
                    Err(error) => {
                        log::warn!("saved server sharing settings could not be read: {error}");
                        None
                    }
                },
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => {
                    log::warn!("cannot read server sharing settings: {error}");
                    None
                }
            });

        let (enabled, initial_printers) = match saved {
            Some(settings) => {
                let names: Vec<PrinterName> = settings
                    .printers
                    .iter()
                    .filter_map(|s| PrinterName::parse(s).ok())
                    .collect();
                (settings.enabled, SharedPrinters::new(names))
            }
            None => (false, SharedPrinters::default()),
        };

        let (changes, _) = watch::channel(initial_printers.clone());
        Self {
            catalog,
            selection: RwLock::new(initial_printers),
            changes,
            preference_path,
            enabled: RwLock::new(enabled),
        }
    }

    /// Follows the selection; the receiver also holds the current selection.
    pub fn subscribe(&self) -> watch::Receiver<SharedPrinters> {
        self.changes.subscribe()
    }

    /// Every local queue with its sharing state, ordered by queue name.
    pub async fn local_printers(&self) -> Result<Vec<LocalPrinter>, AppError> {
        let queues = self.catalog.local_printers().await?;
        let selected = self.selected()?;
        Ok(queues
            .into_iter()
            .map(|name| {
                let shared = selected.contains(&name);
                LocalPrinter::new(name, shared)
            })
            .collect())
    }

    /// Replaces the selection with `names` and returns the resulting list of local queues.
    ///
    /// Every name must be a local queue: sharing a queue the spooler does not report would leave
    /// clients with a printer that can never accept a job.
    pub async fn set_shared(&self, names: Vec<PrinterName>) -> Result<Vec<LocalPrinter>, AppError> {
        let queues = self.catalog.local_printers().await?;
        for name in &names {
            if !queues.contains(name) {
                return Err(AppError::invalid_input(format!(
                    "'{name}' is not a local printer queue"
                )));
            }
        }

        let selection = SharedPrinters::new(names);
        {
            let mut current = self
                .selection
                .write()
                .map_err(|_| AppError::internal("printer selection lock is poisoned"))?;
            *current = selection.clone();
        }
        // Only a real change is published: re-selecting the same queues must not make the endpoint
        // or the advertisement do work again.
        if *self.changes.borrow() != selection {
            self.changes.send_replace(selection);
        }
        self.persist()?;
        self.local_printers().await
    }

    /// Records whether sharing was explicitly started or stopped, and persists the choice.
    pub fn set_sharing_enabled(&self, enabled: bool) -> Result<(), AppError> {
        {
            let mut current = self
                .enabled
                .write()
                .map_err(|_| AppError::internal("printer selection lock is poisoned"))?;
            *current = enabled;
        }
        self.persist()
    }

    /// Whether server sharing should be automatically started on startup.
    ///
    /// Sharing autostarts only if sharing was enabled before restart AND at least one valid shared printer exists.
    pub fn is_autostart_enabled(&self) -> bool {
        let enabled = self.enabled.read().map(|e| *e).unwrap_or(false);
        let has_printers = !self.shared_printers().is_empty();
        enabled && has_printers
    }

    /// Restores the saved selection against the currently available local queues.
    ///
    /// Filters out missing queues from the selection. If at least one valid printer remains,
    /// sharing will autostart if enabled was true. If all saved printers are gone,
    /// the selection is empty and autostart will be false.
    pub async fn restore(&self) -> Result<(), AppError> {
        let queues = self.catalog.local_printers().await?;
        let mut updated_selection = Vec::new();
        let mut changed = false;
        {
            let current = self
                .selection
                .read()
                .map_err(|_| AppError::internal("printer selection lock is poisoned"))?;
            for printer in current.as_slice() {
                if queues.contains(printer) {
                    updated_selection.push(printer.clone());
                } else {
                    log::info!("filtered out removed printer: {}", printer.as_str());
                    changed = true;
                }
            }
        }

        if changed {
            let selection = SharedPrinters::new(updated_selection);
            if selection.is_empty() {
                if let Ok(mut enabled) = self.enabled.write() {
                    *enabled = false;
                }
            }
            {
                let mut current = self
                    .selection
                    .write()
                    .map_err(|_| AppError::internal("printer selection lock is poisoned"))?;
                *current = selection.clone();
            }
            if *self.changes.borrow() != selection {
                self.changes.send_replace(selection);
            }
            self.persist()?;
        }
        Ok(())
    }

    /// Persists current sharing settings to disk.
    fn persist(&self) -> Result<(), AppError> {
        let Some(path) = &self.preference_path else {
            return Ok(());
        };
        let printers: Vec<String> = {
            let selection = self
                .selection
                .read()
                .map_err(|_| AppError::internal("printer selection lock is poisoned"))?;
            selection
                .as_slice()
                .iter()
                .map(|n| n.as_str().to_string())
                .collect()
        };
        let enabled = self
            .enabled
            .read()
            .map_err(|_| AppError::internal("printer selection lock is poisoned"))?;
        let saved = SavedSharing {
            enabled: *enabled,
            printers,
        };
        let bytes = serde_json::to_vec(&saved)
            .map_err(|_| AppError::internal("cannot serialize server sharing settings"))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|_| AppError::internal("cannot create settings directory"))?;
        }
        std::fs::write(path, bytes)
            .map_err(|_| AppError::internal("cannot write server sharing settings"))?;
        Ok(())
    }

    /// The queues currently shared with clients.
    pub fn selected(&self) -> Result<SharedPrinters, AppError> {
        self.selection
            .read()
            .map(|selection| selection.clone())
            .map_err(|_| AppError::internal("printer selection lock is poisoned"))
    }
}

impl SharedPrinterSource for Sharing {
    fn shared_printers(&self) -> Vec<PrinterName> {
        self.selection
            .read()
            .map(|selection| selection.as_slice().to_vec())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ErrorCode;
    use crate::test_support::temporary_directory;
    use async_trait::async_trait;
    use std::time::Duration;

    /// A catalog over a fixed set of queues.
    struct FakeCatalog {
        queues: Vec<PrinterName>,
    }

    impl FakeCatalog {
        fn new(names: &[&str]) -> Arc<Self> {
            Arc::new(Self {
                queues: names
                    .iter()
                    .map(|name| PrinterName::parse(name).expect("valid name"))
                    .collect(),
            })
        }
    }

    #[async_trait]
    impl LocalPrinterCatalog for FakeCatalog {
        async fn local_printers(&self) -> Result<Vec<PrinterName>, AppError> {
            Ok(self.queues.clone())
        }
    }

    fn sharing(names: &[&str]) -> Sharing {
        Sharing::new(FakeCatalog::new(names))
    }

    fn name(value: &str) -> PrinterName {
        PrinterName::parse(value).expect("valid name")
    }

    #[tokio::test]
    async fn nothing_is_shared_until_the_user_selects_queues() {
        let sharing = sharing(&["HP LaserJet", "Zebra"]);

        let printers = sharing.local_printers().await.expect("lists queues");

        assert_eq!(printers.len(), 2);
        assert!(printers.iter().all(|printer| !printer.shared()));
        assert_eq!(printers[0].name().as_str(), "HP LaserJet");
        assert!(sharing.selected().expect("selection").is_empty());
        assert!(sharing.shared_printers().is_empty());
    }

    #[tokio::test]
    async fn the_user_can_share_several_queues_at_once() {
        let sharing = sharing(&["HP LaserJet", "Zebra", "Canon"]);

        let printers = sharing
            .set_shared(vec![name("Zebra"), name("Canon")])
            .await
            .expect("selects queues");

        let shared: Vec<&str> = printers
            .iter()
            .filter(|printer| printer.shared())
            .map(|printer| printer.name().as_str())
            .collect();
        assert_eq!(shared, vec!["Zebra", "Canon"]);
        assert_eq!(
            sharing
                .shared_printers()
                .iter()
                .map(PrinterName::as_str)
                .collect::<Vec<_>>(),
            vec!["Zebra", "Canon"]
        );
    }

    #[tokio::test]
    async fn sharing_a_queue_the_spooler_does_not_report_is_rejected() {
        let sharing = sharing(&["HP LaserJet"]);
        sharing
            .set_shared(vec![name("HP LaserJet")])
            .await
            .expect("selects a local queue");

        let error = sharing
            .set_shared(vec![name("HP LaserJet"), name("Ghost")])
            .await
            .expect_err("rejected");

        assert_eq!(error.code(), ErrorCode::InvalidInput);
        assert_eq!(error.message(), "'Ghost' is not a local printer queue");
        // The rejected request must not change what clients see.
        assert_eq!(
            sharing
                .shared_printers()
                .iter()
                .map(PrinterName::as_str)
                .collect::<Vec<_>>(),
            vec!["HP LaserJet"]
        );
    }

    #[tokio::test]
    async fn the_selection_can_be_cleared() {
        let sharing = sharing(&["HP LaserJet"]);
        sharing
            .set_shared(vec![name("HP LaserJet")])
            .await
            .expect("selects a queue");

        let printers = sharing.set_shared(Vec::new()).await.expect("clears");

        assert!(printers.iter().all(|printer| !printer.shared()));
        assert!(sharing.selected().expect("selection").is_empty());
    }

    #[tokio::test]
    async fn a_subscriber_follows_the_selection_while_sharing_runs() {
        let sharing = sharing(&["HP LaserJet", "Zebra"]);
        let mut changes = sharing.subscribe();
        assert!(changes.borrow_and_update().is_empty());

        sharing
            .set_shared(vec![name("Zebra")])
            .await
            .expect("selects a queue");
        changes.changed().await.expect("publishes the selection");
        assert_eq!(changes.borrow_and_update().as_slice(), &[name("Zebra")]);

        // Selecting what is already selected is not worth waking anything up for.
        sharing
            .set_shared(vec![name("Zebra")])
            .await
            .expect("selects the same queue");
        assert!(
            tokio::time::timeout(Duration::from_millis(50), changes.changed())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn selection_changes_are_visible_while_sharing_runs() {
        let sharing = sharing(&["HP LaserJet", "Zebra"]);

        sharing
            .set_shared(vec![name("Zebra")])
            .await
            .expect("selects a queue");
        assert_eq!(sharing.shared_printers(), vec![name("Zebra")]);

        sharing
            .set_shared(vec![name("HP LaserJet")])
            .await
            .expect("changes the selection");
        assert_eq!(sharing.shared_printers(), vec![name("HP LaserJet")]);
    }

    #[tokio::test]
    async fn selection_and_enabled_state_persist_across_reloads() {
        let path = temporary_directory("persist-reload");

        let sharing1 =
            Sharing::with_persistence(FakeCatalog::new(&["HP LaserJet", "Zebra"]), Some(&path));
        sharing1
            .set_shared(vec![name("Zebra")])
            .await
            .expect("selects Zebra");
        sharing1.set_sharing_enabled(true).expect("enables sharing");

        // Reload from same path
        let sharing2 =
            Sharing::with_persistence(FakeCatalog::new(&["HP LaserJet", "Zebra"]), Some(&path));
        assert_eq!(sharing2.shared_printers(), vec![name("Zebra")]);
        assert!(sharing2.is_autostart_enabled());

        std::fs::remove_dir_all(&path).ok();
    }

    #[tokio::test]
    async fn explicitly_stopping_sharing_persists_stopped_state_while_keeping_selection() {
        let path = temporary_directory("persist-stop");

        let sharing1 =
            Sharing::with_persistence(FakeCatalog::new(&["HP LaserJet", "Zebra"]), Some(&path));
        sharing1
            .set_shared(vec![name("Zebra")])
            .await
            .expect("selects Zebra");
        sharing1.set_sharing_enabled(true).expect("enables sharing");
        sharing1
            .set_sharing_enabled(false)
            .expect("disables sharing");

        let sharing2 =
            Sharing::with_persistence(FakeCatalog::new(&["HP LaserJet", "Zebra"]), Some(&path));
        assert_eq!(sharing2.shared_printers(), vec![name("Zebra")]);
        assert!(!sharing2.is_autostart_enabled());

        let local = sharing2.local_printers().await.expect("local printers");
        let zebra = local
            .iter()
            .find(|p| p.name().as_str() == "Zebra")
            .expect("zebra");
        assert!(zebra.shared());

        std::fs::remove_dir_all(&path).ok();
    }

    #[tokio::test]
    async fn restore_filters_out_removed_printers_and_still_autostarts_if_any_remain() {
        let path = temporary_directory("restore-filter");

        let sharing1 =
            Sharing::with_persistence(FakeCatalog::new(&["HP LaserJet", "Zebra"]), Some(&path));
        sharing1
            .set_shared(vec![name("HP LaserJet"), name("Zebra")])
            .await
            .expect("selects both");
        sharing1.set_sharing_enabled(true).expect("enables");

        // Zebra was removed while ShaPrint was closed
        let sharing2 = Sharing::with_persistence(FakeCatalog::new(&["HP LaserJet"]), Some(&path));
        sharing2.restore().await.expect("restores and filters");

        assert_eq!(sharing2.shared_printers(), vec![name("HP LaserJet")]);
        assert!(sharing2.is_autostart_enabled());

        std::fs::remove_dir_all(&path).ok();
    }

    #[tokio::test]
    async fn restore_leaves_sharing_stopped_cleanly_if_all_saved_printers_removed() {
        let path = temporary_directory("restore-empty");

        let sharing1 = Sharing::with_persistence(FakeCatalog::new(&["Zebra"]), Some(&path));
        sharing1
            .set_shared(vec![name("Zebra")])
            .await
            .expect("selects Zebra");
        sharing1.set_sharing_enabled(true).expect("enables");

        // All saved printers removed while ShaPrint was closed
        let sharing2 = Sharing::with_persistence(FakeCatalog::new(&["Canon"]), Some(&path));
        sharing2.restore().await.expect("restores and filters");

        assert!(sharing2.shared_printers().is_empty());
        assert!(!sharing2.is_autostart_enabled());

        std::fs::remove_dir_all(&path).ok();
    }
}
