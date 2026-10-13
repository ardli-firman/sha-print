//! IPC boundary: Tauri command adapters, serializable DTOs, and the status event bridge.

#[cfg(feature = "desktop")]
pub mod client_connections;
#[cfg(feature = "desktop")]
pub mod commands;
#[cfg(feature = "desktop")]
pub mod control;
#[cfg(feature = "desktop")]
pub mod discovery;
pub mod dto;
#[cfg(feature = "desktop")]
pub mod emitter;
#[cfg(feature = "desktop")]
pub mod server_settings;
#[cfg(feature = "desktop")]
pub mod updates;
