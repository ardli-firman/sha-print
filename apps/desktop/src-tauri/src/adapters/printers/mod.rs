//! Local printer queue enumeration and job submission.
//!
//! Windows adapters use the spooler; Linux adapters use `libcups`; other platforms report
//! unsupported.

#[cfg(unix)]
pub mod cups;
#[cfg(any(windows, test))]
mod dev_mode;
pub(crate) mod media;
#[cfg(any(windows, test))]
mod page_settings;
pub mod raster;

#[cfg(not(windows))]
mod unsupported;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
mod windows_render;
#[cfg(windows)]
mod windows_test_print;
#[cfg(unix)]
pub use cups::{CupsPrintJobSubmitter, CupsPrinterCatalog, CupsSpoolerReader};
#[cfg(not(windows))]
pub use unsupported::UnsupportedPrintJobSubmitter;
#[cfg(windows)]
pub use windows::WindowsPrintJobSubmitter;
#[cfg(windows)]
pub use windows_test_print::WindowsNativeTestPrint;

#[cfg(not(windows))]
pub use unsupported::UnsupportedPrinterCatalog;
#[cfg(windows)]
pub use windows::{WindowsPrinterCatalog, WindowsSpoolerReader};
