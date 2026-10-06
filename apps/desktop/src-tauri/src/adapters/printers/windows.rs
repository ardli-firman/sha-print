//! Printer enumeration and job submission through the Windows spooler.
//!
//! `EnumPrintersW` and spooler submission are blocking calls, so they run on blocking workers.

use std::sync::Arc;

use async_trait::async_trait;
use windows_sys::Win32::Foundation::{GetLastError, ERROR_INSUFFICIENT_BUFFER};
use windows_sys::Win32::Graphics::Printing::{EnumPrintersW, PRINTER_ENUM_LOCAL, PRINTER_INFO_2W};

use crate::application::{DestinationAwarePrinterCatalog, LocalPrinterCatalog, SpoolerReader};
use crate::domain::{AppError, PrinterName, RecognisedClientQueue, SpoolerRecord};

use crate::application::{PrintJob, PrintJobSubmitter};

/// Spooler level 2 returns queue names, driver, and port/destination details needed to
/// distinguish native ShaPrint client queues from eligible local queues.
const PRINTER_INFO_LEVEL: u32 = 2;

/// Upper bound on the spooler's answer, so a corrupt size cannot make the shell allocate wildly.
const MAX_ENUMERATION_BYTES: u32 = 1 << 20;

/// Reads raw records directly from the Windows print spooler using Level 2 enumeration.
#[derive(Debug, Default)]
pub struct WindowsSpoolerReader;

#[async_trait]
impl SpoolerReader for WindowsSpoolerReader {
    async fn read_spooler_records(&self) -> Result<Vec<SpoolerRecord>, AppError> {
        tokio::task::spawn_blocking(enumerate_local_records)
            .await
            .map_err(|error| {
                AppError::internal(format!("printer enumeration did not finish: {error}"))
            })?
    }
}

/// Lists the eligible local printer queues the spooler reports, excluding native ShaPrint client queues.
pub struct WindowsPrinterCatalog {
    inner: DestinationAwarePrinterCatalog,
}

impl Default for WindowsPrinterCatalog {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowsPrinterCatalog {
    pub fn new() -> Self {
        Self {
            inner: DestinationAwarePrinterCatalog::new(Arc::new(WindowsSpoolerReader)),
        }
    }
}

#[async_trait]
impl LocalPrinterCatalog for WindowsPrinterCatalog {
    async fn local_printers(&self) -> Result<Vec<PrinterName>, AppError> {
        self.inner.local_printers().await
    }

    async fn recognised_client_queues(&self) -> Result<Vec<RecognisedClientQueue>, AppError> {
        self.inner.recognised_client_queues().await
    }
}

fn enumerate_local_records() -> Result<Vec<SpoolerRecord>, AppError> {
    let needed = required_bytes()?;
    if needed == 0 {
        return Ok(Vec::new());
    }

    let stride = std::mem::size_of::<PRINTER_INFO_2W>();
    let entries = (needed as usize).div_ceil(stride);
    let mut buffer: Vec<PRINTER_INFO_2W> = vec![PRINTER_INFO_2W::default(); entries];
    let mut capacity = (entries * stride) as u32;
    let mut returned = 0u32;

    // Safety: `buffer` holds `capacity` bytes of writable memory and `capacity` describes it.
    let listed = unsafe {
        EnumPrintersW(
            PRINTER_ENUM_LOCAL,
            std::ptr::null(),
            PRINTER_INFO_LEVEL,
            buffer.as_mut_ptr().cast::<u8>(),
            capacity,
            &mut capacity,
            &mut returned,
        )
    };
    if listed == 0 {
        // Safety: no pointers are involved.
        let code = unsafe { GetLastError() };
        return Err(AppError::internal(format!(
            "cannot read the local printer queues (Windows error {code})"
        )));
    }

    // Safety: the spooler filled the first `returned` entries of `buffer`.
    let listed = unsafe { std::slice::from_raw_parts(buffer.as_ptr(), returned as usize) };
    let mut records = Vec::with_capacity(listed.len());
    for entry in listed {
        // Safety: the spooler points at NUL-terminated strings inside the buffer it filled.
        let name = unsafe { wide_string(entry.pPrinterName) };
        let port = unsafe { wide_string(entry.pPortName) };
        records.push(SpoolerRecord::new(name, port));
    }

    Ok(records)
}

/// Asks the spooler how much memory the listing needs.
fn required_bytes() -> Result<u32, AppError> {
    let mut needed = 0u32;
    let mut returned = 0u32;

    // Safety: a sizing call writes only to the two out-parameters.
    let listed = unsafe {
        EnumPrintersW(
            PRINTER_ENUM_LOCAL,
            std::ptr::null(),
            PRINTER_INFO_LEVEL,
            std::ptr::null_mut(),
            0,
            &mut needed,
            &mut returned,
        )
    };
    if listed != 0 {
        // The spooler answered without a buffer: nothing is installed.
        return Ok(0);
    }

    // Safety: no pointers are involved.
    let code = unsafe { GetLastError() };
    if code != ERROR_INSUFFICIENT_BUFFER {
        return Err(AppError::internal(format!(
            "cannot reach the local print spooler (Windows error {code})"
        )));
    }
    if needed > MAX_ENUMERATION_BYTES {
        return Err(AppError::internal(format!(
            "the spooler reported {needed} bytes of printer data"
        )));
    }
    Ok(needed)
}

/// Reads a NUL-terminated UTF-16 string owned by the spooler.
///
/// # Safety
///
/// `pointer` must be null or point at a NUL-terminated UTF-16 buffer that stays valid for the call.
unsafe fn wide_string(pointer: *const u16) -> String {
    if pointer.is_null() {
        return String::new();
    }
    let mut length = 0usize;
    // Safety: the caller guarantees a terminated buffer.
    while unsafe { *pointer.add(length) } != 0 {
        length += 1;
    }
    // Safety: `length` counts the units before the terminator in the same buffer.
    String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(pointer, length) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn enumerating_local_windows_queues_succeeds_on_windows() {
        let catalog = WindowsPrinterCatalog::new();
        let printers = catalog
            .local_printers()
            .await
            .expect("enumerates without crash");
        for printer in &printers {
            assert!(!printer.as_str().is_empty());
        }
    }

    /// Renders one raster page through the installed driver, including USB inkjet queues.
    #[tokio::test]
    #[ignore = "requires a real Windows printer queue; see the desktop README"]
    async fn smoke_renders_a_raster_page_to_a_real_queue() {
        let queue = std::env::var("SHAPRINT_WINDOWS_SMOKE_PRINTER")
            .expect("set SHAPRINT_WINDOWS_SMOKE_PRINTER to an installed queue name");
        let printer = PrinterName::parse(&queue).expect("uses a valid installed queue name");
        let mut document = b"RaS2".to_vec();
        let mut header = vec![0u8; 1796];
        // 1-inch black square, 1 inch from the top and left of an A4 sheet at 300 dpi.
        for (offset, value) in [
            (276, 300u32),
            (280, 300),
            (372, 2480),
            (376, 3508),
            (384, 8),
            (388, 8),
            (392, 2480),
            (400, 18),
            (420, 1),
        ] {
            header[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
        }
        document.extend(header);
        for y in 0..3508 {
            document.push(0); // one row
            let mut x = 0usize;
            while x < 2480 {
                let black = (300..600).contains(&y) && (300..600).contains(&x);
                let end = if (300..600).contains(&y) && x < 300 {
                    300
                } else if black {
                    600
                } else {
                    2480
                };
                let count = (end - x).min(128);
                document.extend([(count - 1) as u8, if black { 0 } else { 255 }]);
                x += count;
            }
        }
        let settings = crate::application::PrintSettings {
            media: Some("iso_a4_210x297mm".to_owned()),
            copies: Some(1),
            ..Default::default()
        };
        let job = PrintJob::from_ipp_body(document, 0, settings);

        let job_id = WindowsPrintJobSubmitter
            .submit(&printer, job)
            .await
            .expect("the real queue accepts the rendered raster page");

        assert_ne!(job_id, 0);
    }
}

/// Renders PWG Raster pages through the selected queue's installed Windows driver.
#[derive(Debug, Default)]
pub struct WindowsPrintJobSubmitter;

#[async_trait]
impl PrintJobSubmitter for WindowsPrintJobSubmitter {
    async fn submit(&self, printer: &PrinterName, job: PrintJob) -> Result<u32, AppError> {
        let name = printer.as_str().to_owned();
        tokio::task::spawn_blocking(move || submit_windows_job(&name, &job))
            .await
            .map_err(|_| AppError::internal("printer submission worker did not finish"))?
    }
}

#[link(name = "winspool")]
unsafe extern "system" {
    pub(super) fn OpenPrinterW(
        name: *const u16,
        printer: *mut *mut std::ffi::c_void,
        defaults: *const std::ffi::c_void,
    ) -> i32;
    pub(super) fn ClosePrinter(printer: *mut std::ffi::c_void) -> i32;
    fn DocumentPropertiesW(
        window: *mut std::ffi::c_void,
        printer: *mut std::ffi::c_void,
        name: *const u16,
        output: *mut u8,
        input: *const u8,
        mode: u32,
    ) -> i32;
}

use super::dev_mode::apply_settings_to_dev_mode;
use super::windows_render::submit_windows_job;

pub(super) fn prepare_dev_mode(
    handle: *mut std::ffi::c_void,
    printer_name: *const u16,
    settings: &crate::application::PrintSettings,
) -> Result<Option<Vec<u8>>, AppError> {
    if settings.media.is_none()
        && settings.color.is_none()
        && settings.duplex.is_none()
        && settings.copies.is_none()
        && settings.orientation.is_none()
    {
        return Ok(None);
    }
    if settings
        .copies
        .is_some_and(|copies| !(1..=999).contains(&copies))
    {
        return Err(AppError::invalid_input(
            "the requested copy count is outside the supported range",
        ));
    }
    let size = unsafe {
        DocumentPropertiesW(
            std::ptr::null_mut(),
            handle,
            printer_name,
            std::ptr::null_mut(),
            std::ptr::null(),
            0,
        )
    };
    if size <= 0 {
        return Err(spooler_error("cannot read Windows printer settings"));
    }
    let mut dev_mode = vec![0u8; size as usize];
    // SAFETY: `dev_mode` is the buffer size returned by DocumentPropertiesW.
    let prepared = unsafe {
        DocumentPropertiesW(
            std::ptr::null_mut(),
            handle,
            printer_name,
            dev_mode.as_mut_ptr(),
            std::ptr::null(),
            2,
        )
    };
    if prepared != 1 || dev_mode.len() < 96 {
        return Err(spooler_error("cannot prepare Windows printer settings"));
    }
    apply_settings_to_dev_mode(&mut dev_mode, settings)?;
    let requested = dev_mode.clone();
    // SAFETY: both buffers have the size the driver requested and remain alive for this call.
    // Merge public changes into the driver's private settings before passing them to CreateDCW.
    let merged = unsafe {
        DocumentPropertiesW(
            std::ptr::null_mut(),
            handle,
            printer_name,
            dev_mode.as_mut_ptr(),
            requested.as_ptr(),
            8 | 2, // DM_IN_BUFFER | DM_OUT_BUFFER, no configuration dialog
        )
    };
    if merged != 1 {
        return Err(spooler_error("cannot validate Windows printer settings"));
    }
    Ok(Some(dev_mode))
}

pub(super) fn spooler_error(message: &str) -> AppError {
    let code = unsafe { GetLastError() };
    AppError::internal(format!("{message} (Windows error {code})"))
}
