//! Domain types and rules: service identity, lifecycle states, printer sharing, and stable error
//! codes.
//!
//! Nothing here depends on Tauri, tokio, or the operating system, so the rules stay testable and
//! usable by the later Linux phase.

mod error;
mod identity;
mod printer;
mod service;
mod setup;

pub use error::{AppError, ErrorCode};
pub use identity::CertificateFingerprint;
pub use printer::{PrinterName, SharedPrinters};
pub use service::{RuntimeStatus, ServiceId, ServiceState, ServiceStatus};
pub use setup::SetupAction;
