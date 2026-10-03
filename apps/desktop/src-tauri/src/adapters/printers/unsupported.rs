//! Printer enumeration for platforms without spooler integration.

use async_trait::async_trait;

use crate::application::LocalPrinterCatalog;
use crate::domain::{AppError, PrinterName};

use crate::application::{PrintJob, PrintJobSubmitter};

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

/// Reports that this platform cannot submit to a Windows-managed queue.
#[derive(Debug, Default)]
pub struct UnsupportedPrintJobSubmitter;

#[async_trait]
impl PrintJobSubmitter for UnsupportedPrintJobSubmitter {
    fn is_available(&self) -> bool {
        false
    }

    async fn submit(&self, _printer: &PrinterName, _job: PrintJob) -> Result<u32, AppError> {
        Err(AppError::unsupported(
            "local printer submission is only supported on Windows",
        ))
    }
}
