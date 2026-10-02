//! Application layer: the runtime coordinator, the sharing, setup, and queue-installation use cases,
//! and the seams they plug into.

mod coordinator;
mod discovery;
mod printers;
mod queue_installation;
mod runtime;
mod setup;
mod sharing;

pub use coordinator::{RuntimeCoordinator, SHUTDOWN_TIMEOUT, START_TIMEOUT};
pub use discovery::{
    Advertisement, AdvertisementSink, Browse, Discovery, DiscoveryBrowser, DiscoveryService,
    ServerAdvertiser,
};
pub use printers::{
    DuplexMode, LocalPrinterCatalog, PrintJob, PrintJobSubmitter, PrintSettings,
    SharedPrinterSource,
};
pub use queue_installation::{
    ClientProxyState, ClientQueue, QueueInstallation, QueueInstaller, TrustedPrinters,
    TrustedServerPrinters,
};
pub use runtime::{RuntimeService, ServiceContext, ServiceReporter, Shutdown};
pub use setup::{ElevationBroker, Setup, SetupOutcome};
pub use sharing::{LocalPrinter, Sharing};

pub(crate) use coordinator::StatusRegistry;
