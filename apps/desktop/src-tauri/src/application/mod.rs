//! Application layer: the runtime coordinator, the sharing, setup, and queue-installation use cases,
//! and the seams they plug into.

mod coordinator;
mod discovery;
mod failures;
mod job_tracker;
mod legacy;
mod lifecycle;
mod printers;
mod queue_installation;
mod runtime;
mod setup;
mod sharing;
mod startup;
mod updater;

pub use coordinator::{RuntimeCoordinator, SHUTDOWN_TIMEOUT, START_TIMEOUT};
pub use discovery::{
    Advertisement, AdvertisementSink, Browse, Discovery, DiscoveryBrowser, DiscoveryService,
    ServerAdvertiser,
};
pub use failures::PrintFailures;
pub use job_tracker::{PrintJobTracker, RequestLease, RestartPermit, DRAIN_COOLDOWN, QUIET_WINDOW};
pub use legacy::{
    ChannelOutcome, ChannelStore, ImportReport, LegacyChannel, LegacyImport, LegacySettingsSource,
    LegacySnapshot, SkippedSetting,
};
pub use lifecycle::{close_action, CloseAction};
pub use printers::{
    DuplexMode, LocalPrinterCatalog, PrintJob, PrintJobSubmitter, PrintOrientation, PrintSettings,
    SharedPrinterSource,
};
pub use queue_installation::{
    ClientProxyState, ClientQueue, QueueInstallation, QueueInstaller, TrustedPrinters,
    TrustedServerPrinters,
};
pub use runtime::{RuntimeService, ServiceContext, ServiceReporter, Shutdown};
pub use setup::{ElevationBroker, Setup, SetupOutcome};
pub use sharing::{LocalPrinter, Sharing};
pub use startup::{Startup, StartupRegistration, StartupStatus};
pub use updater::{
    CheckKind, UpdateCoordinator, UpdateInstaller, UpdatePackage, UpdateRelease, UpdateSource,
    UpdateState, UpdateStatus, INITIAL_CHECK_DELAY, RESTART_POLL_INTERVAL, UPDATE_CHECK_INTERVAL,
};

pub(crate) use coordinator::StatusRegistry;
