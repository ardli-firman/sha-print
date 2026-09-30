//! Application layer: the runtime coordinator, the sharing and setup use cases, and the seams they
//! plug into.

mod coordinator;
mod printers;
mod runtime;
mod setup;
mod sharing;

pub use coordinator::{RuntimeCoordinator, SHUTDOWN_TIMEOUT, START_TIMEOUT};
pub use printers::{LocalPrinterCatalog, SharedPrinterSource};
pub use runtime::{RuntimeService, ServiceContext, ServiceReporter, Shutdown};
pub use setup::{ElevationBroker, Setup, SetupOutcome};
pub use sharing::{LocalPrinter, Sharing};

pub(crate) use coordinator::StatusRegistry;
