//! Service implementations the desktop shell supervises.
//!
//! Windows printer, registry, spooler, and startup APIs stay behind adapters in this module so the
//! later Linux phase can supply its own implementations without moving domain behavior (ADR 0002).
//! Each service provides the supervised runtime slot: startup, readiness reporting, and
//! cancellation, plus the protocol work its issue owns.

pub mod client_connections;
mod client_proxy;
pub mod elevation;
pub mod identity;
pub mod ipps;
pub mod printers;
mod server_sharing;

pub use client_proxy::ClientProxyService;
pub use elevation::SystemElevation;
pub use identity::{FileIdentityStore, IdentityStore, ServerIdentity};
pub use ipps::{IppsServer, DEFAULT_PORT};
pub use server_sharing::ServerSharingService;
