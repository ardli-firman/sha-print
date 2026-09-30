//! Application layer: the runtime coordinator and the seam the supervised services plug into.

mod coordinator;
mod runtime;

pub use coordinator::{RuntimeCoordinator, SHUTDOWN_TIMEOUT, START_TIMEOUT};
pub use runtime::{RuntimeService, ServiceContext, ServiceReporter, Shutdown};

pub(crate) use coordinator::StatusRegistry;
