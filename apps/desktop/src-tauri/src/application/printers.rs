//! Ports the sharing use case reads printer queues through.

use async_trait::async_trait;

use crate::domain::{AppError, PrinterName};

/// Duplex binding a server can forward to a local queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DuplexMode {
    Simplex,
    LongEdge,
    ShortEdge,
}

/// Common print settings a server can forward to a local queue.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PrintSettings {
    pub media: Option<String>,
    pub color: Option<bool>,
    pub duplex: Option<DuplexMode>,
    pub copies: Option<u16>,
}

/// Printer-ready `application/octet-stream` bytes and requested settings for a shared queue.
pub struct PrintJob {
    body: Vec<u8>,
    document_start: usize,
    settings: PrintSettings,
}

impl PrintJob {
    /// Takes ownership of the IPP body after the endpoint validates its printer-ready document
    /// format, retaining the document as a slice to avoid a second large allocation or copy.
    pub(crate) fn from_ipp_body(
        body: Vec<u8>,
        document_start: usize,
        settings: PrintSettings,
    ) -> Self {
        Self {
            body,
            document_start,
            settings,
        }
    }

    /// The opaque document bytes after the IPP end-of-attributes delimiter.
    pub fn document(&self) -> &[u8] {
        self.body.get(self.document_start..).unwrap_or(&[])
    }

    pub fn settings(&self) -> &PrintSettings {
        &self.settings
    }
}

/// Platform port for submitting a job to a named local queue.
#[async_trait]
pub trait PrintJobSubmitter: Send + Sync + 'static {
    /// Whether the platform currently has a real queue-submission backend.
    fn is_available(&self) -> bool {
        true
    }
    async fn submit(&self, printer: &PrinterName, job: PrintJob) -> Result<u32, AppError>;
}

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
