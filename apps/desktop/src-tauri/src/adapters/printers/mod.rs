//! Local printer queue enumeration.
//!
//! The Windows adapter reads the spooler; other platforms have no spooler integration in the
//! Windows MVP (ADR 0001 keeps Linux a later phase) and report `unsupported` instead of pretending
//! to have no printers.

#[cfg(not(windows))]
mod unsupported;
#[cfg(windows)]
mod windows;

#[cfg(not(windows))]
pub use unsupported::UnsupportedPrinterCatalog;
#[cfg(windows)]
pub use windows::WindowsPrinterCatalog;
