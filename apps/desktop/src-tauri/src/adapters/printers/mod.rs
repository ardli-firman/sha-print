//! Local printer queue enumeration and job submission.
//!
//! Windows adapters use the spooler; other platforms report unsupported in this Windows MVP.

pub mod raster;

#[cfg(not(windows))]
mod unsupported;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
mod windows_render;
#[cfg(not(windows))]
pub use unsupported::UnsupportedPrintJobSubmitter;
#[cfg(windows)]
pub use windows::WindowsPrintJobSubmitter;

#[cfg(not(windows))]
pub use unsupported::UnsupportedPrinterCatalog;
#[cfg(windows)]
pub use windows::WindowsPrinterCatalog;
