//! Service implementations the desktop shell supervises.
//!
//! Windows printer, registry, spooler, and startup APIs stay behind adapters in this module so the
//! later Linux phase can supply its own implementations without moving domain behavior (ADR 0002).
//! Each service provides the supervised runtime slot: startup, readiness reporting, and
//! cancellation, plus the protocol work its issue owns.

pub mod client_connections;
mod client_proxy;
pub mod discovery;
pub mod elevation;
pub mod identity;
pub mod ipps;
pub mod legacy;
pub mod port_binding;
pub mod printers;
pub mod queue_installation;
mod server_sharing;
pub mod startup;
#[cfg(windows)]
pub(crate) mod win_crypto;

pub use client_proxy::{client_queue_uri, ClientProxyService, CLIENT_PROXY_DEFAULT_PORT};
pub use elevation::SystemElevation;
pub use identity::{FileIdentityStore, IdentityStore, ServerIdentity};
pub use ipps::{IppsServer, DEFAULT_PORT};
pub use server_sharing::ServerSharingService;
