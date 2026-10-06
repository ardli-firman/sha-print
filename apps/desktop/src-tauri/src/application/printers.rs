//! Ports the sharing use case reads printer queues through.

use std::sync::Arc;

use async_trait::async_trait;

use crate::domain::{
    AppError, PrinterName, RecognisedClientQueue, SpoolerRecord, SpoolerRecordClassification,
};

/// Duplex binding a server can forward to a local queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DuplexMode {
    Simplex,
    LongEdge,
    ShortEdge,
}

/// Print orientation a server can forward to a local queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrintOrientation {
    Portrait,
    Landscape,
}

/// Common print settings a server can forward to a local queue.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PrintSettings {
    pub media: Option<String>,
    pub color: Option<bool>,
    pub duplex: Option<DuplexMode>,
    pub copies: Option<u16>,
    pub orientation: Option<PrintOrientation>,
}

/// PWG Raster document and requested settings for a shared queue.
pub struct PrintJob {
    body: Vec<u8>,
    document_start: usize,
    settings: PrintSettings,
}

impl PrintJob {
    /// Takes ownership of the IPP body after the endpoint validates its PWG Raster document
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

/// Platform port that reads raw records directly from the platform print spooler.
#[async_trait]
pub trait SpoolerReader: Send + Sync + 'static {
    /// Every printer queue entry in the spooler with its queue name and port/destination.
    async fn read_spooler_records(&self) -> Result<Vec<SpoolerRecord>, AppError>;
}

/// Enumerates the local printer queues a server user can share.
///
/// The Windows adapter reads the spooler; tests and the future Linux phase supply their own
/// implementations (ADR 0002).
#[async_trait]
pub trait LocalPrinterCatalog: Send + Sync + 'static {
    /// Every eligible local, Windows-managed queue, ordered by name.
    /// Recognised client queues and ambiguous records are excluded.
    async fn local_printers(&self) -> Result<Vec<PrinterName>, AppError>;

    /// Every recognised native ShaPrint client queue found in the spooler, ordered by queue name.
    async fn recognised_client_queues(&self) -> Result<Vec<RecognisedClientQueue>, AppError> {
        Ok(Vec::new())
    }
}

/// A printer catalog backed by a spooler reader that evaluates ports and excludes client queues.
pub struct DestinationAwarePrinterCatalog {
    spooler: Arc<dyn SpoolerReader>,
    proxy_authority: String,
}

impl DestinationAwarePrinterCatalog {
    pub fn new(spooler: Arc<dyn SpoolerReader>) -> Self {
        Self::with_authority(
            spooler,
            format!("127.0.0.1:{}", crate::adapters::CLIENT_PROXY_DEFAULT_PORT),
        )
    }

    pub fn with_authority(spooler: Arc<dyn SpoolerReader>, proxy_authority: String) -> Self {
        Self {
            spooler,
            proxy_authority,
        }
    }
}

#[async_trait]
impl LocalPrinterCatalog for DestinationAwarePrinterCatalog {
    async fn local_printers(&self) -> Result<Vec<PrinterName>, AppError> {
        let records = self.spooler.read_spooler_records().await?;
        let mut eligible = Vec::new();
        for record in records {
            if let SpoolerRecordClassification::EligibleLocal(name) =
                record.classify(&self.proxy_authority)
            {
                eligible.push(name);
            }
        }
        eligible.sort();
        eligible.dedup();
        Ok(eligible)
    }

    async fn recognised_client_queues(&self) -> Result<Vec<RecognisedClientQueue>, AppError> {
        let records = self.spooler.read_spooler_records().await?;
        let mut queues = Vec::new();
        for record in records {
            if let SpoolerRecordClassification::RecognisedClientQueue(queue) =
                record.classify(&self.proxy_authority)
            {
                queues.push(queue);
            }
        }
        queues.sort();
        queues.dedup();
        Ok(queues)
    }
}

/// The queues currently shared with clients, read on every IPP query so that changes to the
/// selection take effect while sharing runs.
pub trait SharedPrinterSource: Send + Sync + 'static {
    fn shared_printers(&self) -> Vec<PrinterName>;
}
