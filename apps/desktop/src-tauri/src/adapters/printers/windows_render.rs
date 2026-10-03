//! Draw PWG pages through the installed queue's GDI driver, never through RAW WritePrinter.
use windows_sys::Win32::Graphics::Gdi::{
    CreateDCW, DeleteDC, GetDeviceCaps, StretchDIBits, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS, GDI_ERROR, HDC, LOGPIXELSX, LOGPIXELSY, PHYSICALOFFSETX, PHYSICALOFFSETY,
    SRCCOPY,
};
use windows_sys::Win32::Storage::Xps::{AbortDoc, EndDoc, EndPage, StartDocW, StartPage, DOCINFOW};

use super::raster::{pwg_pages, RasterPage};
use super::windows::{prepare_dev_mode, spooler_error, ClosePrinter, OpenPrinterW};
use crate::application::PrintJob;
use crate::domain::AppError;

struct DeviceContext(HDC);
impl Drop for DeviceContext {
    fn drop(&mut self) {
        // SAFETY: this context was created by CreateDCW and is owned by this wrapper.
        unsafe {
            DeleteDC(self.0);
        }
    }
}

pub(super) fn submit_windows_job(printer: &str, job: &PrintJob) -> Result<u32, AppError> {
    // Decode all pages before touching the queue, so malformed later pages print nothing.
    for page in pwg_pages(job.document())? {
        page?;
    }
    let printer_wide: Vec<u16> = printer.encode_utf16().chain(Some(0)).collect();
    let mut handle = std::ptr::null_mut();
    // SAFETY: terminated queue name and writable handle storage live across the call.
    if unsafe { OpenPrinterW(printer_wide.as_ptr(), &mut handle, std::ptr::null()) } == 0 {
        return Err(spooler_error("cannot open the selected Windows printer"));
    }
    let dev_mode = prepare_dev_mode(handle, printer_wide.as_ptr(), job.settings());
    // SAFETY: handle was returned by OpenPrinterW and is no longer needed.
    unsafe {
        ClosePrinter(handle);
    }
    let dev_mode = dev_mode?;
    let driver: Vec<u16> = "WINSPOOL".encode_utf16().chain(Some(0)).collect();
    // SAFETY: names and optional DEVMODE stay alive; CreateDCW copies the settings.
    let dc = unsafe {
        CreateDCW(
            driver.as_ptr(),
            printer_wide.as_ptr(),
            std::ptr::null(),
            dev_mode
                .as_ref()
                .map_or(std::ptr::null(), |dm| dm.as_ptr().cast()),
        )
    };
    if dc.is_null() {
        return Err(spooler_error(
            "cannot create the Windows printer drawing context",
        ));
    }
    let dc = DeviceContext(dc);
    let name: Vec<u16> = "ShaPrint job".encode_utf16().chain(Some(0)).collect();
    let info = DOCINFOW {
        cbSize: std::mem::size_of::<DOCINFOW>() as i32,
        lpszDocName: name.as_ptr(),
        ..Default::default()
    };
    // SAFETY: live DC and document info with a terminated title.
    let job_id = unsafe { StartDocW(dc.0, &info) };
    if job_id <= 0 {
        return Err(spooler_error("cannot start a Windows printer job"));
    }
    let result = (|| {
        for page in pwg_pages(job.document())? {
            draw_page(dc.0, &page?)?;
        }
        // SAFETY: all pages finished successfully in the job opened above.
        if unsafe { EndDoc(dc.0) } <= 0 {
            return Err(spooler_error("cannot finish the Windows printer job"));
        }
        Ok(job_id as u32)
    })();
    if result.is_err() {
        // SAFETY: cancel the open document instead of submitting a partial rendering.
        unsafe {
            AbortDoc(dc.0);
        }
    }
    result
}

fn draw_page(dc: HDC, page: &RasterPage) -> Result<(), AppError> {
    // SAFETY: dc has an active print document.
    if unsafe { StartPage(dc) } <= 0 {
        return Err(spooler_error("cannot start a Windows printer page"));
    }
    let bitmap = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: page.width as i32,
            biHeight: -(page.height as i32), // top-down, matching the PWG row order
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..Default::default()
        },
        ..Default::default()
    };
    // PWG describes the whole sheet. Preserve physical size and use the sheet origin;
    // the installed driver clips only its unprintable margins, without shrinking the page.
    // SAFETY: querying capabilities does not retain any pointers.
    let (dpi_x, dpi_y, offset_x, offset_y) = unsafe {
        (
            GetDeviceCaps(dc, LOGPIXELSX as i32),
            GetDeviceCaps(dc, LOGPIXELSY as i32),
            GetDeviceCaps(dc, PHYSICALOFFSETX as i32),
            GetDeviceCaps(dc, PHYSICALOFFSETY as i32),
        )
    };
    if dpi_x <= 0 || dpi_y <= 0 {
        return Err(spooler_error(
            "the Windows printer has invalid rendering resolution",
        ));
    }
    let width = (u64::from(page.width) * dpi_x as u64 / u64::from(page.dpi[0])) as i32;
    let height = (u64::from(page.height) * dpi_y as u64 / u64::from(page.dpi[1])) as i32;
    if width <= 0 || height <= 0 {
        return Err(AppError::invalid_input(
            "the raster page is too small to print",
        ));
    }
    // SAFETY: pixel buffer contains width*height BGRX pixels matching BITMAPINFO.
    let drawn = unsafe {
        StretchDIBits(
            dc,
            -offset_x,
            -offset_y,
            width,
            height,
            0,
            0,
            page.width as i32,
            page.height as i32,
            page.pixels.as_ptr().cast(),
            &bitmap,
            DIB_RGB_COLORS,
            SRCCOPY,
        )
    };
    if drawn == 0 || drawn == GDI_ERROR {
        return Err(spooler_error(
            "cannot render the page through the Windows printer driver",
        ));
    }
    // SAFETY: the page was started and fully drawn above.
    if unsafe { EndPage(dc) } <= 0 {
        return Err(spooler_error("cannot finish a Windows printer page"));
    }
    Ok(())
}
