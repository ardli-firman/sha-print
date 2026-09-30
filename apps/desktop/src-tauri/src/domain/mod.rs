//! Domain types and rules: service identity, lifecycle states, and stable error codes.
//!
//! Nothing here depends on Tauri, tokio, or the operating system, so the rules stay testable and
//! usable by the later Linux phase.

mod error;
mod service;

pub use error::{AppError, ErrorCode};
pub use service::{RuntimeStatus, ServiceId, ServiceState, ServiceStatus};
