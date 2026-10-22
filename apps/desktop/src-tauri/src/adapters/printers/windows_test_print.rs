//! Draw a backend-owned page into the installed native Client queue's GDI driver.
//! No direct IPP submission, saved-preference mutation, document export, or raw error text.
use super::{
    dev_mode::loaded_media_dimensions_tenths_mm,
    media::media_for_dimensions,
    windows::{
        enumerate_queue_details, prepare_dev_mode, read_default_dev_mode, ClosePrinter,
        OpenPrinterW,
    },
};
use crate::application::{
    DuplexMode, NativeTestPrint, PrintOrientation, PrintSettings, SubmissionOutcome, TestMedia,
};
use crate::domain::{AppError, RecognisedClientQueue};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::Storage::Xps::{EndDoc, EndPage, StartDocW, StartPage, DOCINFOW};

pub struct WindowsNativeTestPrint;
struct Printer(*mut std::ffi::c_void);
impl Drop for Printer {
    fn drop(&mut self) {
        // SAFETY: this wrapper owns the OpenPrinterW handle.
        unsafe {
            ClosePrinter(self.0);
        }
    }
}
struct Context(HDC);
impl Drop for Context {
    fn drop(&mut self) {
        // SAFETY: this wrapper owns the CreateDCW context.
        unsafe {
            DeleteDC(self.0);
        }
    }
}
fn open(name: &[u16]) -> Result<Printer, AppError> {
    let mut handle = std::ptr::null_mut();
    // SAFETY: a terminated name and writable handle storage remain alive.
    if unsafe { OpenPrinterW(name.as_ptr(), &mut handle, std::ptr::null()) } == 0 {
        return Err(AppError::queue_unavailable(
            "The native Client queue could not be opened.",
        ));
    }
    Ok(Printer(handle))
}
fn media_from_defaults(bytes: &[u8]) -> Option<TestMedia> {
    let fields = u32::from_ne_bytes(bytes.get(72..76)?.try_into().ok()?);
    let (short, long) = loaded_media_dimensions_tenths_mm(bytes, fields)?;
    TestMedia::from_keyword(media_for_dimensions(short, long)?.keyword)
}
fn default_media(target: &RecognisedClientQueue) -> Result<Option<TestMedia>, AppError> {
    let name: Vec<_> = target
        .queue_name()
        .as_str()
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let printer = open(&name)?;
    Ok(media_from_defaults(&read_default_dev_mode(
        printer.0,
        name.as_ptr(),
    )?))
}
#[async_trait::async_trait]
impl NativeTestPrint for WindowsNativeTestPrint {
    async fn default_media(
        &self,
        target: &RecognisedClientQueue,
    ) -> Result<Option<TestMedia>, AppError> {
        let target = target.clone();
        tokio::task::spawn_blocking(move || default_media(&target))
            .await
            .map_err(|_| AppError::queue_unavailable("Requested media could not be read."))?
    }
    async fn submit(&self, target: &RecognisedClientQueue, media: TestMedia) -> SubmissionOutcome {
        let target = target.clone();
        tokio::task::spawn_blocking(move || submit(&target, media))
            .await
            .unwrap_or(SubmissionOutcome::Uncertain)
    }
}
fn submit(target: &RecognisedClientQueue, media: TestMedia) -> SubmissionOutcome {
    use SubmissionOutcome::*;
    let prepared = (|| -> Result<Context, AppError> {
        let name: Vec<_> = target
            .queue_name()
            .as_str()
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let printer = open(&name)?;
        if media_from_defaults(&read_default_dev_mode(printer.0, name.as_ptr())?) != Some(media) {
            return Err(AppError::invalid_state(
                "Requested media changed. Check again before sending.",
            ));
        }
        let settings = PrintSettings {
            media: Some(media.keyword().to_owned()),
            copies: Some(1),
            color: Some(false),
            duplex: Some(DuplexMode::Simplex),
            orientation: Some(PrintOrientation::Portrait),
        };
        let dm = prepare_dev_mode(printer.0, name.as_ptr(), &settings)?
            .ok_or_else(|| AppError::invalid_state("Test print settings could not be applied."))?;
        // The driver may normalize unsupported settings. Never silently accept changed intent.
        let fields = u32::from_ne_bytes([dm[72], dm[73], dm[74], dm[75]]);
        const REQUIRED_SETTINGS: u32 = 0x0000_1901; // orientation, copies, color, duplex
        if fields & REQUIRED_SETTINGS != REQUIRED_SETTINGS
            || media_from_defaults(&dm) != Some(media)
            || [(76, 1i16), (86, 1), (92, 1), (94, 1)]
                .iter()
                .any(|(at, value)| {
                    dm.get(*at..*at + 2)
                        .is_none_or(|bytes| bytes != value.to_ne_bytes())
                })
        {
            return Err(AppError::unsupported(
                "The native driver cannot apply the test print settings.",
            ));
        }
        let driver: Vec<_> = "WINSPOOL".encode_utf16().chain(Some(0)).collect();
        // SAFETY: all strings are terminated and the full driver DEVMODE remains alive.
        let dc = unsafe {
            CreateDCW(
                driver.as_ptr(),
                name.as_ptr(),
                std::ptr::null(),
                dm.as_ptr().cast(),
            )
        };
        if dc.is_null() {
            return Err(AppError::queue_unavailable(
                "The native driver could not start.",
            ));
        }
        let dc = Context(dc);
        // Recheck on the blocking worker immediately before StartDocW. Other applications
        // can still enqueue concurrently; this is deliberately not a queue reservation.
        let (record, pending) = enumerate_queue_details()?
            .into_iter()
            .find(|(record, _)| {
                record
                    .name()
                    .eq_ignore_ascii_case(target.queue_name().as_str())
            })
            .ok_or_else(|| AppError::queue_unavailable("The Client queue is missing."))?;
        if pending != 0
            || crate::application::LocalQueueInspection::from_record(
                record,
                target,
                &format!("127.0.0.1:{}", crate::adapters::CLIENT_PROXY_DEFAULT_PORT),
                Some(pending),
            ) != (crate::application::LocalQueueInspection::Matching {
                pending_jobs: Some(0),
            })
        {
            return Err(AppError::invalid_state(
                "The Client queue is busy or its destination changed.",
            ));
        }
        Ok(dc)
    })();
    let Ok(dc) = prepared else {
        return Rejected;
    };
    let title: Vec<_> = "ShaPrint diagnostic page"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let info = DOCINFOW {
        cbSize: std::mem::size_of::<DOCINFOW>() as i32,
        lpszDocName: title.as_ptr(),
        ..Default::default()
    };
    // SAFETY: the context and document info are valid for the entire synchronous call.
    // Once StartDocW is invoked, errors are conservative: a spooler failure may have
    // accepted work already. No cancellation or automatic retry is attempted here.
    if unsafe { StartDocW(dc.0, &info) } <= 0 {
        return Uncertain;
    }
    // SAFETY: the document was opened above and contains exactly one page.
    if unsafe { StartPage(dc.0) } <= 0 {
        return Uncertain;
    }
    if !draw_page(dc.0, media) {
        return Uncertain;
    }
    // SAFETY: this is the one page/document opened above.
    if unsafe { EndPage(dc.0) } <= 0 || unsafe { EndDoc(dc.0) } <= 0 {
        return Uncertain;
    }
    Accepted
}
fn draw_page(dc: HDC, media: TestMedia) -> bool {
    // SAFETY: querying capabilities retains no pointers.
    let (dx, dy, ox, oy) = unsafe {
        (
            GetDeviceCaps(dc, LOGPIXELSX as i32),
            GetDeviceCaps(dc, LOGPIXELSY as i32),
            GetDeviceCaps(dc, PHYSICALOFFSETX as i32),
            GetDeviceCaps(dc, PHYSICALOFFSETY as i32),
        )
    };
    if dx <= 0 || dy <= 0 {
        return false;
    }
    let (w, h) = media.dimensions_tenths_mm();
    // SAFETY: valid live printer context. Coordinates below are tenths of a millimeter,
    // with the origin at the physical sheet edge, so clipping is visible, not scaled away.
    unsafe {
        if SetMapMode(dc, MM_ANISOTROPIC) == 0
            || SetWindowExtEx(dc, 254, 254, std::ptr::null_mut()) == 0
            || SetViewportExtEx(dc, dx, dy, std::ptr::null_mut()) == 0
            || SetViewportOrgEx(dc, -ox, -oy, std::ptr::null_mut()) == 0
        {
            return false;
        }
        for inset in [20, 50, 100] {
            if Rectangle(dc, inset, inset, i32::from(w) - inset, i32::from(h) - inset) == 0 {
                return false;
            }
        }
        let center_x = i32::from(w) / 2;
        let center_y = i32::from(h) / 2;
        if MoveToEx(dc, center_x - 250, center_y, std::ptr::null_mut()) == 0
            || LineTo(dc, center_x + 250, center_y) == 0
            || MoveToEx(dc, center_x, center_y - 250, std::ptr::null_mut()) == 0
            || LineTo(dc, center_x, center_y + 250) == 0
        {
            return false;
        }
        let font = CreateFontW(
            -30,
            0,
            0,
            0,
            400,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            0,
            0,
            0,
            0,
            std::ptr::null(),
        );
        if font.is_null() {
            return false;
        }
        let old = SelectObject(dc, font);
        let lines = [
            "ShaPrint diagnostic page".to_owned(),
            "Requested media:".to_owned(),
            media.keyword().to_owned(),
            format!("{} x {} mm", f32::from(w) / 10.0, f32::from(h) / 10.0),
            "1 page, 1 copy, grayscale, one-sided".to_owned(),
            "Borders: 2, 5 and 10 mm from each edge".to_owned(),
            "Center cross: 50 x 50 mm".to_owned(),
            "Compare Requested media with Loaded media.".to_owned(),
        ];
        let mut success = true;
        for (line, text) in lines.iter().enumerate() {
            let wide: Vec<_> = text.encode_utf16().collect();
            success &= TextOutW(
                dc,
                150,
                180 + line as i32 * 50,
                wide.as_ptr(),
                wide.len() as i32,
            ) != 0;
        }
        SelectObject(dc, old);
        DeleteObject(font);
        success
    }
}
