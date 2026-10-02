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
        let settings = crate::application::PrintSettings {
            copies: Some(1),
            ..Default::default()
        };
        let job = PrintJob::from_ipp_body(document, 0, settings);

        let job_id = WindowsPrintJobSubmitter
            .submit(&printer, job)
            .await
            .expect("the real queue accepts the PCL smoke page");

        assert_ne!(job_id, 0);
    }

    #[test]
    fn dev_mode_mapping_applies_orientation_and_media_size() {
        use crate::application::PrintOrientation;

        let mut devmode = vec![0u8; 120];
        let settings = crate::application::PrintSettings {
            media: Some("na_legal_8.5x14in".to_string()),
            orientation: Some(PrintOrientation::Landscape),
            copies: Some(3),
            color: Some(false),
            duplex: Some(crate::application::DuplexMode::LongEdge),
        };

        apply_settings_to_dev_mode(&mut devmode, &settings).expect("applies settings");

        let fields = u32::from_ne_bytes(devmode[72..76].try_into().unwrap());
        // DM_ORIENTATION = 0x0000_0001
        // DM_PAPERSIZE   = 0x0000_0002
        // DM_COPIES      = 0x0000_0100
        // DM_COLOR       = 0x0000_0800
        // DM_DUPLEX      = 0x0000_1000
        let expected_fields = 0x0000_0001 | 0x0000_0002 | 0x0000_0100 | 0x0000_0800 | 0x0000_1000;
        assert_eq!(fields, expected_fields);

        // dmOrientation at 76..78 (2 = DMORIENT_LANDSCAPE)
        let orientation = i16::from_ne_bytes(devmode[76..78].try_into().unwrap());
        assert_eq!(orientation, 2);

        // dmPaperSize at 78..80 (5 = DMPAPER_LEGAL)
        let paper = i16::from_ne_bytes(devmode[78..80].try_into().unwrap());
        assert_eq!(paper, 5);
    }

    #[test]
    fn dev_mode_mapping_applies_color_modes() {
        for (color, expected_dm) in [(true, 2i16), (false, 1i16)] {
            let mut devmode = vec![0u8; 120];
            let settings = crate::application::PrintSettings {
                color: Some(color),
                ..Default::default()
            };
            apply_settings_to_dev_mode(&mut devmode, &settings).expect("applies settings");
            let fields = u32::from_ne_bytes(devmode[72..76].try_into().unwrap());
            assert_eq!(fields, 0x0000_0800);
            let dm_color = i16::from_ne_bytes(devmode[92..94].try_into().unwrap());
            assert_eq!(dm_color, expected_dm, "color: {color}");
        }
    }

    #[test]
    fn dev_mode_mapping_applies_duplex_modes() {
        use crate::application::DuplexMode;
        for (duplex, expected_dm) in [
            (DuplexMode::Simplex, 1i16),
            (DuplexMode::LongEdge, 2i16),
            (DuplexMode::ShortEdge, 3i16),
        ] {
            let mut devmode = vec![0u8; 120];
            let settings = crate::application::PrintSettings {
                duplex: Some(duplex),
                ..Default::default()
            };
            apply_settings_to_dev_mode(&mut devmode, &settings).expect("applies settings");
            let fields = u32::from_ne_bytes(devmode[72..76].try_into().unwrap());
            assert_eq!(fields, 0x0000_1000);
            let dm_duplex = i16::from_ne_bytes(devmode[94..96].try_into().unwrap());
            assert_eq!(dm_duplex, expected_dm, "duplex: {duplex:?}");
        }
    }

    #[test]
    fn dev_mode_mapping_applies_copies_and_rejects_out_of_range() {
        let mut devmode = vec![0u8; 120];
        let settings = crate::application::PrintSettings {
            copies: Some(7),
            ..Default::default()
        };
        apply_settings_to_dev_mode(&mut devmode, &settings).expect("applies settings");
        let fields = u32::from_ne_bytes(devmode[72..76].try_into().unwrap());
        assert_eq!(fields, 0x0000_0100);
        let dm_copies = i16::from_ne_bytes(devmode[86..88].try_into().unwrap());
        assert_eq!(dm_copies, 7);

        for invalid in [0, 1000] {
            let mut devmode = vec![0u8; 120];
            let settings = crate::application::PrintSettings {
                copies: Some(invalid),
                ..Default::default()
            };
            assert!(
                apply_settings_to_dev_mode(&mut devmode, &settings).is_err(),
                "should reject copies: {invalid}"
            );
        }
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

#[link(name = "winspool")]
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
    fn DocumentPropertiesW(
        window: *mut std::ffi::c_void,
        printer: *mut std::ffi::c_void,
        name: *const u16,
        output: *mut u8,
        input: *const u8,
        mode: u32,
    ) -> i32;
}

#[repr(C)]
struct PrinterDefaultsW {
    p_datatype: *mut u16,
    p_devmode: *mut u8,
    desired_access: u32,
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
    let mut raw_wide: Vec<u16> = "RAW".encode_utf16().chain(std::iter::once(0)).collect();
    let info = DocInfo1W {
        document_name: document_wide.as_ptr(),
        output_file: ptr::null(),
        data_type: raw_wide.as_ptr(),
    };

    let mut query_handle = ptr::null_mut();
    // SAFETY: pointers refer to NUL-terminated strings and writable handle storage.
    let opened = unsafe { OpenPrinterW(printer_wide.as_ptr(), &mut query_handle, ptr::null()) };
    if opened == 0 {
        return Err(spooler_error("cannot open the selected Windows printer"));
    }
    let dev_mode = prepare_dev_mode(query_handle, printer_wide.as_ptr(), job.settings());
    // SAFETY: query_handle was returned successfully by OpenPrinterW.
    unsafe {
        ClosePrinter(query_handle);
    }
    let mut dev_mode = dev_mode?;

    let mut defaults = PrinterDefaultsW {
        p_datatype: raw_wide.as_mut_ptr(),
        p_devmode: match &mut dev_mode {
            Some(dm) => dm.as_mut_ptr(),
            None => ptr::null_mut(),
        },
        desired_access: 0x0000_0008, // PRINTER_ACCESS_USE
    };

    let mut handle = ptr::null_mut();
    // SAFETY: pointers refer to valid NUL-terminated wide string and defaults.
    let opened = unsafe {
        OpenPrinterW(
            printer_wide.as_ptr(),
            &mut handle,
            (&mut defaults as *mut PrinterDefaultsW).cast(),
        )
    };
    if opened == 0 {
        return Err(spooler_error("cannot open the selected Windows printer"));
    }
    let result = submit_open_printer(handle, &info, job);
    // SAFETY: handle was returned successfully by OpenPrinterW.
    unsafe {
        ClosePrinter(handle);
    }
    result
}

fn submit_open_printer(
    handle: *mut std::ffi::c_void,
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

fn prepare_dev_mode(
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
    Ok(Some(dev_mode))
}

fn apply_settings_to_dev_mode(
    dev_mode: &mut [u8],
    settings: &crate::application::PrintSettings,
) -> Result<(), AppError> {
    if dev_mode.len() < 96 {
        return Err(AppError::internal(
            "invalid Windows printer settings buffer",
        ));
    }
    let mut fields = u32::from_ne_bytes(
        dev_mode[72..76]
            .try_into()
            .map_err(|_| AppError::internal("invalid Windows printer settings"))?,
    );
    if let Some(orientation) = settings.orientation {
        let dm_orient = match orientation {
            crate::application::PrintOrientation::Portrait => 1i16,
            crate::application::PrintOrientation::Landscape => 2i16,
        };
        // DEVMODEW.dmOrientation is at byte 76; DM_ORIENTATION is 0x0000_0001
        dev_mode[76..78].copy_from_slice(&dm_orient.to_ne_bytes());
        fields |= 0x0000_0001;
    }
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
        if !(1..=999).contains(&copies) {
            return Err(AppError::invalid_input(
                "the requested copy count is outside the supported range",
            ));
        }
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
    Ok(())
}

fn spooler_error(message: &str) -> AppError {
    let code = unsafe { GetLastError() };
    AppError::internal(format!("{message} (Windows error {code})"))
}
