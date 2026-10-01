//! Domain types and rules: service identity, lifecycle states, printer sharing, failed print
//! attempts, and stable error codes.
//!
//! Nothing here depends on Tauri, tokio, or the operating system, so the rules stay testable and
//! usable by the later Linux phase.

mod discovery;
mod error;
mod failure;
mod identity;
mod printer;
mod service;
mod setup;
mod text;

pub use discovery::NearbyServer;
pub use error::{AppError, ErrorCode};
pub use failure::{JobPath, PrintFailure};
pub use identity::CertificateFingerprint;
pub use printer::{PrinterName, SharedPrinters};
pub use service::{RuntimeStatus, ServiceId, ServiceState, ServiceStatus};
pub use setup::SetupAction;

pub(crate) use text::sanitize_text;
