//! Printer enumeration for platforms without spooler integration.

use async_trait::async_trait;

use crate::application::LocalPrinterCatalog;
use crate::domain::{AppError, PrinterName};

/// Reports that this platform cannot enumerate printer queues.
///
/// The Windows MVP has no Linux or macOS spooler adapter yet (ADR 0001); reporting the gap keeps
/// the shell honest instead of showing an empty, misleading printer list.
#[derive(Debug, Default)]
pub struct UnsupportedPrinterCatalog;

impl UnsupportedPrinterCatalog {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl LocalPrinterCatalog for UnsupportedPrinterCatalog {
    async fn local_printers(&self) -> Result<Vec<PrinterName>, AppError> {
        Err(AppError::unsupported(
            "listing local printer queues is only supported on Windows",
        ))
    }
}
