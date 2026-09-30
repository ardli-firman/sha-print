//! Printer enumeration through the Windows spooler.
//!
//! `EnumPrintersW` is a blocking spooler call, so it runs on a blocking worker instead of the async
//! runtime (ADR 0002). Only local queues are listed: network connections belong to whichever server
//! shares them, and the MVP shares the machine's own queues.

use async_trait::async_trait;
use windows_sys::Win32::Foundation::{GetLastError, ERROR_INSUFFICIENT_BUFFER};
use windows_sys::Win32::Graphics::Printing::{EnumPrintersW, PRINTER_ENUM_LOCAL, PRINTER_INFO_4W};

use crate::application::LocalPrinterCatalog;
use crate::domain::{AppError, PrinterName};

/// Spooler level 4 returns queue names; the driver and port details the MVP does not use would need
/// level 2 and a much larger buffer.
const PRINTER_INFO_LEVEL: u32 = 4;

/// Upper bound on the spooler's answer, so a corrupt size cannot make the shell allocate wildly.
const MAX_ENUMERATION_BYTES: u32 = 1 << 20;

/// Lists the local printer queues the spooler reports.
#[derive(Debug, Default)]
pub struct WindowsPrinterCatalog;

impl WindowsPrinterCatalog {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl LocalPrinterCatalog for WindowsPrinterCatalog {
    async fn local_printers(&self) -> Result<Vec<PrinterName>, AppError> {
        tokio::task::spawn_blocking(enumerate_local_queues)
            .await
            .map_err(|error| {
                AppError::internal(format!("printer enumeration did not finish: {error}"))
            })?
    }
}

fn enumerate_local_queues() -> Result<Vec<PrinterName>, AppError> {
    let needed = required_bytes()?;
    if needed == 0 {
        return Ok(Vec::new());
    }

    let stride = std::mem::size_of::<PRINTER_INFO_4W>();
    let entries = (needed as usize).div_ceil(stride);
    let mut buffer: Vec<PRINTER_INFO_4W> = vec![PRINTER_INFO_4W::default(); entries];
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
    let mut queues = Vec::with_capacity(listed.len());
    for entry in listed {
        // Safety: the spooler points at a NUL-terminated name inside the buffer it filled.
        let name = unsafe { wide_string(entry.pPrinterName) };
        match PrinterName::parse(&name) {
            Ok(name) => queues.push(name),
            Err(_) => log::warn!("ignoring a local printer queue with an unusable name"),
        }
    }

    queues.sort();
    queues.dedup();
    Ok(queues)
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
}
