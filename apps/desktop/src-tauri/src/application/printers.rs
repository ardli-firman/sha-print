//! Ports the sharing use case reads printer queues through.

use async_trait::async_trait;

use crate::domain::{AppError, PrinterName};

/// Enumerates the local printer queues a server user can share.
///
/// The Windows adapter reads the spooler; tests and the future Linux phase supply their own
/// implementations (ADR 0002).
#[async_trait]
pub trait LocalPrinterCatalog: Send + Sync + 'static {
    /// Every local, Windows-managed queue, ordered by name.
    async fn local_printers(&self) -> Result<Vec<PrinterName>, AppError>;
}

/// The queues currently shared with clients, read on every IPP query so that changes to the
/// selection take effect while sharing runs.
pub trait SharedPrinterSource: Send + Sync + 'static {
    fn shared_printers(&self) -> Vec<PrinterName>;
}
