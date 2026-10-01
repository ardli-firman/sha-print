//! The server-side sharing use case: which local queues are shared, and how the UI sees them.
//!
//! The use case owns the selection and the rules around it; the spooler and the network live in
//! adapters (`adapters::printers`, `adapters::ipps`).

use std::sync::{Arc, RwLock};

use tokio::sync::watch;

use crate::application::{LocalPrinterCatalog, SharedPrinterSource};
use crate::domain::{AppError, PrinterName, SharedPrinters};

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
}

impl Sharing {
    pub fn new(catalog: Arc<dyn LocalPrinterCatalog>) -> Self {
        let (changes, _) = watch::channel(SharedPrinters::default());
        Self {
            catalog,
            selection: RwLock::new(SharedPrinters::default()),
            changes,
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
        self.local_printers().await
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
}
