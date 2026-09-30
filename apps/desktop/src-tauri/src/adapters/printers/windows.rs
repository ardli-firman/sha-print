//! Printer enumeration and job submission through the Windows spooler.
//!
//! `EnumPrintersW` and spooler submission are blocking calls, so they run on blocking workers.

use async_trait::async_trait;
use windows_sys::Win32::Foundation::{GetLastError, ERROR_INSUFFICIENT_BUFFER};
use windows_sys::Win32::Graphics::Printing::{EnumPrintersW, PRINTER_ENUM_LOCAL, PRINTER_INFO_4W};

use crate::application::LocalPrinterCatalog;
use crate::domain::{AppError, PrinterName};

use crate::application::{PrintJob, PrintJobSubmitter};

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

    /// Submits one PCL page through the real Windows spooler adapter.
    #[tokio::test]
    #[ignore = "requires a Windows queue that accepts PCL; see the desktop README"]
    async fn smoke_submits_a_print_ready_page_to_a_real_queue() {
        let queue = std::env::var("SHAPRINT_WINDOWS_SMOKE_PRINTER")
            .expect("set SHAPRINT_WINDOWS_SMOKE_PRINTER to an installed queue name");
        let printer = PrinterName::parse(&queue).expect("uses a valid installed queue name");
        let mut document = b"\x1bE\x1b&l0O\x1b&l2A".to_vec();
        document.extend_from_slice(b"ShaPrint Windows real-queue smoke test\r\n");
        document.push(0x0c);
        let job =
            PrintJob::from_ipp_body(document, 0, crate::application::PrintSettings::default());

        let job_id = WindowsPrintJobSubmitter
            .submit(&printer, job)
            .await
            .expect("the real queue accepts the PCL smoke page");

        assert_ne!(job_id, 0);
    }
}

/// Submits printer-ready `application/octet-stream` bytes as RAW to the selected Windows queue.
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

#[repr(C)]
struct DocInfo1W {
    document_name: *const u16,
    output_file: *const u16,
    data_type: *const u16,
}

#[repr(C)]
struct JobInfo2W {
    printer_name: *mut u16,
    machine_name: *mut u16,
    user_name: *mut u16,
    document_name: *mut u16,
    notify_name: *mut u16,
    data_type: *mut u16,
    print_processor: *mut u16,
    parameters: *mut u16,
    driver_name: *mut u16,
    dev_mode: *mut u8,
    status_text: *mut u16,
    security_descriptor: *mut std::ffi::c_void,
    status: u32,
    priority: u32,
    position: u32,
    start_time: u32,
    until_time: u32,
    total_pages: u32,
    size: u32,
    submitted: [u16; 8],
    time: u32,
    pages_printed: u32,
}

#[link(name = "winspool.drv")]
unsafe extern "system" {
    fn OpenPrinterW(
        name: *const u16,
        printer: *mut *mut std::ffi::c_void,
        defaults: *const std::ffi::c_void,
    ) -> i32;
    fn ClosePrinter(printer: *mut std::ffi::c_void) -> i32;
    fn StartDocPrinterW(printer: *mut std::ffi::c_void, level: u32, info: *const u8) -> u32;
    fn StartPagePrinter(printer: *mut std::ffi::c_void) -> i32;
    fn WritePrinter(
        printer: *mut std::ffi::c_void,
        bytes: *const std::ffi::c_void,
        count: u32,
        written: *mut u32,
    ) -> i32;
    fn EndPagePrinter(printer: *mut std::ffi::c_void) -> i32;
    fn EndDocPrinter(printer: *mut std::ffi::c_void) -> i32;
    fn AbortPrinter(printer: *mut std::ffi::c_void) -> i32;
    fn SetJobW(
        printer: *mut std::ffi::c_void,
        job_id: u32,
        level: u32,
        info: *const u8,
        command: u32,
    ) -> i32;
    fn DocumentPropertiesW(
        window: *mut std::ffi::c_void,
        printer: *mut std::ffi::c_void,
        name: *const u16,
        output: *mut u8,
        input: *const u8,
        mode: u32,
    ) -> i32;
}

fn submit_windows_job(printer: &str, job: &PrintJob) -> Result<u32, AppError> {
    use std::ptr;
    let document = job.document();
    if document.len() > u32::MAX as usize {
        return Err(AppError::invalid_input(
            "the print document is too large for the Windows spooler",
        ));
    }
    let printer_wide: Vec<u16> = printer.encode_utf16().chain(std::iter::once(0)).collect();
    let document_wide: Vec<u16> = "ShaPrint job"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let raw_wide: Vec<u16> = "RAW".encode_utf16().chain(std::iter::once(0)).collect();
    let info = DocInfo1W {
        document_name: document_wide.as_ptr(),
        output_file: ptr::null(),
        data_type: raw_wide.as_ptr(),
    };
    let mut handle = ptr::null_mut();
    // SAFETY: pointers refer to NUL-terminated strings and writable handle storage.
    let opened = unsafe { OpenPrinterW(printer_wide.as_ptr(), &mut handle, ptr::null()) };
    if opened == 0 {
        return Err(spooler_error("cannot open the selected Windows printer"));
    }
    let result = submit_open_printer(handle, printer_wide.as_ptr(), &info, job);
    // SAFETY: handle was returned successfully by OpenPrinterW.
    unsafe {
        ClosePrinter(handle);
    }
    result
}

fn submit_open_printer(
    handle: *mut std::ffi::c_void,
    printer_name: *const u16,
    info: &DocInfo1W,
    job: &PrintJob,
) -> Result<u32, AppError> {
    // SAFETY: handle is an opened spooler handle; `info` and its strings stay alive during the call.
    let job_id = unsafe { StartDocPrinterW(handle, 1, (info as *const DocInfo1W).cast()) };
    if job_id == 0 {
        return Err(spooler_error("cannot start a Windows printer job"));
    }

    let document = job.document();
    let result = (|| {
        apply_job_settings(handle, job_id, printer_name, job.settings())?;
        // SAFETY: handle is a live printer handle.
        if unsafe { StartPagePrinter(handle) } == 0 {
            return Err(spooler_error("cannot start a Windows printer page"));
        }
        let mut written = 0u32;
        // SAFETY: document bytes stay alive for the synchronous spooler call.
        let wrote = unsafe {
            WritePrinter(
                handle,
                document.as_ptr().cast(),
                document.len() as u32,
                &mut written,
            )
        };
        if wrote == 0 || written as usize != document.len() {
            return Err(spooler_error(
                "Windows did not accept the complete print document",
            ));
        }
        // SAFETY: page was started above.
        if unsafe { EndPagePrinter(handle) } == 0 {
            return Err(spooler_error("cannot finish the Windows printer page"));
        }
        Ok(())
    })();
    if let Err(error) = result {
        // SAFETY: the job is still open; abort instead of submitting a partial document.
        unsafe {
            AbortPrinter(handle);
        }
        return Err(error);
    }
    // SAFETY: job was started above; all document writes succeeded.
    if unsafe { EndDocPrinter(handle) } == 0 {
        return Err(spooler_error("cannot finish the Windows printer job"));
    }
    Ok(job_id)
}

fn apply_job_settings(
    handle: *mut std::ffi::c_void,
    job_id: u32,
    printer_name: *const u16,
    settings: &crate::application::PrintSettings,
) -> Result<(), AppError> {
    if settings.media.is_none()
        && settings.color.is_none()
        && settings.duplex.is_none()
        && settings.copies.is_none()
    {
        return Ok(());
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
    let mut fields = u32::from_ne_bytes(
        dev_mode[72..76]
            .try_into()
            .map_err(|_| AppError::internal("invalid Windows printer settings"))?,
    );
    if let Some(media) = &settings.media {
        let paper = match media.as_str() {
            "na_letter_8.5x11in" => 1i16,
            "na_legal_8.5x14in" => 5i16,
            "iso_a3_297x420mm" => 8i16,
            "iso_a4_210x297mm" => 9i16,
            "iso_a5_148x210mm" => 11i16,
            _ => {
                return Err(AppError::invalid_input(
                    "the selected media size is not supported by the Windows adapter",
                ))
            }
        };
        // DEVMODEW.dmOrientation is at byte 76; dmPaperSize is at byte 78.
        dev_mode[78..80].copy_from_slice(&paper.to_ne_bytes());
        fields |= 0x0000_0002;
    }
    if let Some(copies) = settings.copies {
        dev_mode[86..88].copy_from_slice(&(copies as i16).to_ne_bytes());
        fields |= 0x0000_0100;
    }
    if let Some(color) = settings.color {
        dev_mode[92..94].copy_from_slice(&(if color { 2i16 } else { 1i16 }).to_ne_bytes());
        fields |= 0x0000_0800;
    }
    if let Some(duplex) = settings.duplex {
        let mode = match duplex {
            crate::application::DuplexMode::Simplex => 1i16,
            crate::application::DuplexMode::LongEdge => 2i16,
            crate::application::DuplexMode::ShortEdge => 3i16,
        };
        dev_mode[94..96].copy_from_slice(&mode.to_ne_bytes());
        fields |= 0x0000_1000;
    }
    dev_mode[72..76].copy_from_slice(&fields.to_ne_bytes());

    let mut job_info = JobInfo2W {
        printer_name: std::ptr::null_mut(),
        machine_name: std::ptr::null_mut(),
        user_name: std::ptr::null_mut(),
        document_name: std::ptr::null_mut(),
        notify_name: std::ptr::null_mut(),
        data_type: std::ptr::null_mut(),
        print_processor: std::ptr::null_mut(),
        parameters: std::ptr::null_mut(),
        driver_name: std::ptr::null_mut(),
        dev_mode: dev_mode.as_mut_ptr(),
        status_text: std::ptr::null_mut(),
        security_descriptor: std::ptr::null_mut(),
        status: 0,
        priority: 0,
        position: 0,
        start_time: 0,
        until_time: 0,
        total_pages: 0,
        size: 0,
        submitted: [0; 8],
        time: 0,
        pages_printed: 0,
    };
    // SAFETY: job info and devmode are valid for the synchronous SetJobW call.
    if unsafe {
        SetJobW(
            handle,
            job_id,
            2,
            (&mut job_info as *mut JobInfo2W).cast(),
            0,
        )
    } == 0
    {
        return Err(spooler_error(
            "Windows rejected the requested print settings",
        ));
    }
    Ok(())
}

fn spooler_error(message: &str) -> AppError {
    let code = unsafe { GetLastError() };
    AppError::internal(format!("{message} (Windows error {code})"))
}
