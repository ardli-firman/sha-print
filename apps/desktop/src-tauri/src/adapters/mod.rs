//! Service implementations the desktop shell supervises.
//!
//! Windows printer, registry, spooler, and startup APIs stay behind adapters in this module so the
//! later Linux phase can supply its own implementations without moving domain behavior (ADR 0002).
//! Each service here provides the supervised runtime slot: startup, readiness reporting, and
//! cancellation. The protocol work arrives with its own issue (#31 sharing, #34 client proxy).

mod client_proxy;
mod server_sharing;

pub use client_proxy::ClientProxyService;
pub use server_sharing::ServerSharingService;
