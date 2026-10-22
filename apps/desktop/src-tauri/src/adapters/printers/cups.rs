//! CUPS printer enumeration and PWG Raster job submission through `libcups` (ADR 0018).
//!
//! Uses runtime dynamic loading (`dlopen`) of `libcups.so.2` so that building on Linux does not
//! require `cups-devel` headers installed in minimal build environments while using structured
//! `libcups` C API calls (`httpConnect2`, `cupsGetDests2`, `cupsCreateJob`, `cupsStartDocument`,
//! `cupsWriteRequestData`, `cupsFinishDocument`) at runtime instead of parsing localized CLI text.

use std::ffi::{c_char, c_int, c_long, c_void, CStr, CString};
use std::path::Path;
use std::sync::{Arc, OnceLock};

use async_trait::async_trait;

use crate::adapters::printers::media::media_spec;
use crate::adapters::printers::raster::pwg_pages;
use crate::application::{
    DestinationAwarePrinterCatalog, DuplexMode, LocalPrinterCatalog, PrintJob, PrintJobSubmitter,
    PrintOrientation, PrintSettings, SpoolerReader,
};
use crate::domain::{AppError, PrinterName, RecognisedClientQueue, SpoolerRecord};

const DEFAULT_CUPS_SOCKET: &str = "/run/cups/cups.sock";
const DEFAULT_CUPS_PORT: c_int = 631;
const HTTP_ENCRYPTION_IF_REQUESTED: c_int = 0;
pub(crate) const HTTP_STATUS_CONTINUE: c_int = 100;
pub(crate) const IPP_STATUS_OK: c_int = 0x0000;
pub(crate) const IPP_STATUS_OK_SUBST: c_int = 0x0001;
pub(crate) const IPP_STATUS_OK_CONFLICT: c_int = 0x0002;
const IPP_STATUS_ERROR_NOT_FOUND: c_int = 0x0406;
pub(crate) const IPP_PSTATE_STOPPED: i32 = 5;
pub(crate) const STREAM_CHUNK_BYTES: usize = 64 * 1024;

#[repr(C)]
pub(crate) struct CupsOption {
    pub(crate) name: *const c_char,
    pub(crate) value: *const c_char,
}

#[repr(C)]
pub(crate) struct CupsDest {
    pub(crate) name: *const c_char,
    pub(crate) instance: *const c_char,
    pub(crate) is_default: c_int,
    pub(crate) num_options: c_int,
    pub(crate) options: *const CupsOption,
}

pub(crate) type HttpConnect2Fn = unsafe extern "C" fn(
    host: *const c_char,
    port: c_int,
    addrlist: *mut c_void,
    family: c_int,
    encryption: c_int,
    blocking: c_int,
    msec: c_int,
    cancel: *mut c_int,
) -> *mut c_void;
pub(crate) type HttpCloseFn = unsafe extern "C" fn(http: *mut c_void);
pub(crate) type CupsGetDests2Fn =
    unsafe extern "C" fn(http: *mut c_void, dests: *mut *mut CupsDest) -> c_int;
pub(crate) type CupsGetOptionFn = unsafe extern "C" fn(
    name: *const c_char,
    num_options: c_int,
    options: *const CupsOption,
) -> *const c_char;
pub(crate) type CupsFreeDestsFn = unsafe extern "C" fn(num_dests: c_int, dests: *mut CupsDest);
pub(crate) type CupsAddOptionFn = unsafe extern "C" fn(
    name: *const c_char,
    value: *const c_char,
    num_options: c_int,
    options: *mut *mut CupsOption,
) -> c_int;
pub(crate) type CupsFreeOptionsFn =
    unsafe extern "C" fn(num_options: c_int, options: *mut CupsOption);
pub(crate) type CupsCreateJobFn = unsafe extern "C" fn(
    http: *mut c_void,
    name: *const c_char,
    title: *const c_char,
    num_options: c_int,
    options: *const CupsOption,
) -> c_int;
pub(crate) type CupsStartDocumentFn = unsafe extern "C" fn(
    http: *mut c_void,
    name: *const c_char,
    job_id: c_int,
    docname: *const c_char,
    format: *const c_char,
    last_document: c_int,
) -> c_int;
pub(crate) type CupsWriteRequestDataFn =
    unsafe extern "C" fn(http: *mut c_void, buffer: *const c_char, length: usize) -> c_int;
pub(crate) type CupsFinishDocumentFn =
    unsafe extern "C" fn(http: *mut c_void, name: *const c_char) -> c_int;
pub(crate) type CupsCancelJob2Fn = unsafe extern "C" fn(
    http: *mut c_void,
    name: *const c_char,
    job_id: c_int,
    purge: c_int,
) -> c_int;
pub(crate) type CupsLastErrorFn = unsafe extern "C" fn() -> c_int;
pub(crate) type CupsLastErrorStringFn = unsafe extern "C" fn() -> *const c_char;

#[repr(C)]
pub(crate) struct CupsJob {
    pub(crate) id: c_int,
    pub(crate) dest: *mut c_char,
    pub(crate) title: *mut c_char,
    pub(crate) user: *mut c_char,
    pub(crate) state: c_int,
    pub(crate) size: c_int,
    pub(crate) priority: c_int,
    pub(crate) processing_time: c_long,
    pub(crate) creation_time: c_long,
    pub(crate) completion_time: c_long,
    pub(crate) format: *mut c_char,
}

pub(crate) type CupsGetJobs2Fn = unsafe extern "C" fn(
    http: *mut c_void,
    jobs: *mut *mut CupsJob,
    dest: *const c_char,
    myjobs: c_int,
    whichjobs: c_int,
) -> c_int;
pub(crate) type CupsFreeJobsFn = unsafe extern "C" fn(num_jobs: c_int, jobs: *mut CupsJob);
pub(crate) type CupsGetPPD2Fn =
    unsafe extern "C" fn(http: *mut c_void, name: *const c_char) -> *const c_char;

pub(crate) struct LibCups {
    pub(crate) http_connect2: HttpConnect2Fn,
    pub(crate) http_close: HttpCloseFn,
    pub(crate) cups_get_dests2: CupsGetDests2Fn,
    pub(crate) cups_get_option: CupsGetOptionFn,
    pub(crate) cups_free_dests: CupsFreeDestsFn,
    pub(crate) cups_add_option: CupsAddOptionFn,
    pub(crate) cups_free_options: CupsFreeOptionsFn,
    pub(crate) cups_create_job: CupsCreateJobFn,
    pub(crate) cups_start_document: CupsStartDocumentFn,
    pub(crate) cups_write_request_data: CupsWriteRequestDataFn,
    pub(crate) cups_finish_document: CupsFinishDocumentFn,
    pub(crate) cups_cancel_job2: CupsCancelJob2Fn,
    pub(crate) cups_last_error: CupsLastErrorFn,
    pub(crate) cups_last_error_string: CupsLastErrorStringFn,
    pub(crate) cups_get_jobs2: CupsGetJobs2Fn,
    pub(crate) cups_free_jobs: CupsFreeJobsFn,
    pub(crate) cups_get_ppd2: CupsGetPPD2Fn,
}

// Safety: `LibCups` holds immutable function pointers resolved from the process-wide `libcups.so.2`.
unsafe impl Send for LibCups {}
unsafe impl Sync for LibCups {}

extern "C" {
    fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

const RTLD_NOW: c_int = 2;

pub(crate) fn libcups() -> Result<&'static LibCups, AppError> {
    static LIB: OnceLock<Result<LibCups, String>> = OnceLock::new();
    LIB.get_or_init(load_libcups)
        .as_ref()
        .map_err(|msg| AppError::queue_unavailable(msg.clone()))
}

fn load_libcups() -> Result<LibCups, String> {
    let candidates = ["libcups.so.2\0", "libcups.so\0"];
    let mut handle = std::ptr::null_mut();
    for candidate in candidates {
        // Safety: `candidate` is a valid NUL-terminated string literal.
        let opened = unsafe { dlopen(candidate.as_ptr().cast::<c_char>(), RTLD_NOW) };
        if !opened.is_null() {
            handle = opened;
            break;
        }
    }
    if handle.is_null() {
        return Err("libcups.so.2 is not installed or could not be loaded".to_owned());
    }

    unsafe fn sym<T>(handle: *mut c_void, name: &'static [u8]) -> Result<T, String> {
        let ptr = dlsym(handle, name.as_ptr().cast::<c_char>());
        if ptr.is_null() {
            let symbol = String::from_utf8_lossy(&name[..name.len().saturating_sub(1)]);
            return Err(format!("missing symbol '{symbol}' in libcups"));
        }
        Ok(std::mem::transmute_copy(&ptr))
    }

    // Safety: each symbol signature matches the official CUPS C API in `cups/cups.h`.
    unsafe {
        Ok(LibCups {
            http_connect2: sym(handle, b"httpConnect2\0")?,
            http_close: sym(handle, b"httpClose\0")?,
            cups_get_dests2: sym(handle, b"cupsGetDests2\0")?,
            cups_get_option: sym(handle, b"cupsGetOption\0")?,
            cups_free_dests: sym(handle, b"cupsFreeDests\0")?,
            cups_add_option: sym(handle, b"cupsAddOption\0")?,
            cups_free_options: sym(handle, b"cupsFreeOptions\0")?,
            cups_create_job: sym(handle, b"cupsCreateJob\0")?,
            cups_start_document: sym(handle, b"cupsStartDocument\0")?,
            cups_write_request_data: sym(handle, b"cupsWriteRequestData\0")?,
            cups_finish_document: sym(handle, b"cupsFinishDocument\0")?,
            cups_cancel_job2: sym(handle, b"cupsCancelJob2\0")?,
            cups_last_error: sym(handle, b"cupsLastError\0")?,
            cups_last_error_string: sym(handle, b"cupsLastErrorString\0")?,
            cups_get_jobs2: sym(handle, b"cupsGetJobs2\0")?,
            cups_free_jobs: sym(handle, b"cupsFreeJobs\0")?,
            cups_get_ppd2: sym(handle, b"cupsGetPPD2\0")?,
        })
    }
}

/// Endpoint specification for connecting to a local CUPS server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CupsServerEndpoint {
    host: String,
    port: c_int,
}

impl CupsServerEndpoint {
    /// Resolves the CUPS server from `CUPS_SERVER` if set, or falls back to `/run/cups/cups.sock`
    /// (or `127.0.0.1:631` when the socket file is absent).
    pub fn from_env() -> Self {
        if let Ok(value) = std::env::var("CUPS_SERVER") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Self::parse(trimmed);
            }
        }
        if Path::new(DEFAULT_CUPS_SOCKET).exists() {
            Self {
                host: DEFAULT_CUPS_SOCKET.to_owned(),
                port: DEFAULT_CUPS_PORT,
            }
        } else {
            Self {
                host: "127.0.0.1".to_owned(),
                port: DEFAULT_CUPS_PORT,
            }
        }
    }

    pub fn parse(spec: &str) -> Self {
        let trimmed = spec.trim();
        if trimmed.starts_with('/') {
            return Self {
                host: trimmed.to_owned(),
                port: DEFAULT_CUPS_PORT,
            };
        }
        if let Some((host, port_str)) = trimmed.rsplit_once(':') {
            if let Ok(port) = port_str.parse::<u16>() {
                return Self {
                    host: host.to_owned(),
                    port: c_int::from(port),
                };
            }
        }
        Self {
            host: trimmed.to_owned(),
            port: DEFAULT_CUPS_PORT,
        }
    }

    pub fn resolve(override_endpoint: Option<&Self>) -> Self {
        override_endpoint.cloned().unwrap_or_else(Self::from_env)
    }

    /// Formats the `-h` argument for `lpadmin` when a non-default server is configured.
    pub fn lpadmin_host_arg(&self) -> String {
        if self.host.starts_with('/') {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

pub(crate) struct CupsHttpConnection {
    pub(crate) lib: &'static LibCups,
    pub(crate) http: *mut c_void,
}

impl CupsHttpConnection {
    pub(crate) fn connect(endpoint: &CupsServerEndpoint) -> Result<Self, AppError> {
        if endpoint.host.starts_with('/') && !Path::new(&endpoint.host).exists() {
            return Err(AppError::queue_unavailable(format!(
                "cannot connect to the local CUPS service at '{}'",
                endpoint.lpadmin_host_arg()
            )));
        }
        let lib = libcups()?;
        let host_c = CString::new(endpoint.host.as_str())
            .map_err(|_| AppError::invalid_input("invalid CUPS server host"))?;
        // Safety: `host_c` is a valid NUL-terminated string.
        let http = unsafe {
            (lib.http_connect2)(
                host_c.as_ptr(),
                endpoint.port,
                std::ptr::null_mut(),
                0,
                HTTP_ENCRYPTION_IF_REQUESTED,
                1,
                5_000,
                std::ptr::null_mut(),
            )
        };
        if http.is_null() {
            return Err(AppError::queue_unavailable(format!(
                "cannot connect to the local CUPS service at '{}'",
                endpoint.lpadmin_host_arg()
            )));
        }
        Ok(Self { lib, http })
    }
}

impl Drop for CupsHttpConnection {
    fn drop(&mut self) {
        if !self.http.is_null() {
            // Safety: `self.http` was returned by `httpConnect2` and is owned by `self`.
            unsafe { (self.lib.http_close)(self.http) };
        }
    }
}

/// Detailed snapshot of one CUPS destination queried via `libcups`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CupsDestinationRecord {
    pub name: String,
    pub device_uri: String,
    pub printer_info: String,
    pub state: Option<i32>,
    pub accepting_jobs: bool,
    pub make_and_model: String,
    pub has_usable_conversion: bool,
    pub options: Vec<(String, String)>,
}

impl CupsDestinationRecord {
    pub fn is_raw(&self) -> bool {
        !self.has_usable_conversion
    }

    /// User-configured print defaults (excluding CUPS internal/system options).
    pub fn user_options(&self) -> Vec<(String, String)> {
        self.options
            .iter()
            .filter(|(k, _)| !is_cups_system_option(k))
            .cloned()
            .collect()
    }
}

/// Identifies internal / spooler-managed destination options in CUPS that should not be treated
/// as user print defaults.
pub fn is_cups_system_option(key: &str) -> bool {
    matches!(
        key.to_ascii_lowercase().as_str(),
        "device-uri"
            | "printer-name"
            | "printer-info"
            | "printer-is-accepting-jobs"
            | "printer-is-shared"
            | "printer-is-temporary"
            | "printer-location"
            | "printer-make-and-model"
            | "printer-state"
            | "printer-state-change-time"
            | "printer-state-reasons"
            | "printer-state-message"
            | "printer-type"
            | "printer-uri-supported"
            | "marker-change-time"
            | "job-cancel-after"
            | "job-hold-until"
            | "job-priority"
            | "job-sheets"
    )
}

/// Queries the number of active (pending, held, processing) jobs for `cups_name`.
pub fn query_cups_active_jobs(
    endpoint: &CupsServerEndpoint,
    cups_name: &str,
) -> Result<usize, AppError> {
    let conn = CupsHttpConnection::connect(endpoint)?;
    let c_name = CString::new(cups_name)
        .map_err(|_| AppError::invalid_input("invalid printer destination name"))?;
    let mut jobs_ptr: *mut CupsJob = std::ptr::null_mut();
    // Safety: `conn.http` is connected, `jobs_ptr` is an out-pointer.
    // myjobs = 0 (all users), whichjobs = 0 (active jobs).
    let count =
        unsafe { (conn.lib.cups_get_jobs2)(conn.http, &mut jobs_ptr, c_name.as_ptr(), 0, 0) };
    // cupsGetJobs2 can return zero both for an empty queue and for some IPP errors.
    // Check the status before treating a zero count as evidence that repair is safe.
    let status = unsafe { (conn.lib.cups_last_error)() };
    if count < 0 || status >= 0x0400 {
        if !jobs_ptr.is_null() && count > 0 {
            unsafe {
                (conn.lib.cups_free_jobs)(count, jobs_ptr);
            }
        }
        return Err(AppError::queue_unavailable(format!(
            "CUPS refused job listing ({})",
            last_cups_error_message(conn.lib)
        )));
    }
    if count > 0 && !jobs_ptr.is_null() {
        unsafe {
            (conn.lib.cups_free_jobs)(count, jobs_ptr);
        }
    }
    Ok(count as usize)
}

/// Queries all destinations from `endpoint` via `libcups` (`cupsGetDests2`).
pub fn query_cups_destinations(
    endpoint: &CupsServerEndpoint,
) -> Result<Vec<CupsDestinationRecord>, AppError> {
    let conn = CupsHttpConnection::connect(endpoint)?;
    let mut dests_ptr: *mut CupsDest = std::ptr::null_mut();
    // Safety: `conn.http` is a live `http_t*` and `dests_ptr` is an out-pointer.
    let count = unsafe { (conn.lib.cups_get_dests2)(conn.http, &mut dests_ptr) };
    if count <= 0 || dests_ptr.is_null() {
        let err_code = unsafe { (conn.lib.cups_last_error)() };
        if err_code >= 0x0400 && err_code != IPP_STATUS_ERROR_NOT_FOUND {
            return Err(AppError::queue_unavailable(format!(
                "CUPS refused destination listing ({})",
                last_cups_error_message(conn.lib)
            )));
        }
        return Ok(Vec::new());
    }

    let mut records = Vec::with_capacity(count as usize);
    // Safety: `dests_ptr` points to `count` valid `CupsDest` entries until `cupsFreeDests`.
    unsafe {
        let slice = std::slice::from_raw_parts(dests_ptr, count as usize);
        for dest in slice {
            if !dest.instance.is_null() {
                continue;
            }
            if dest.name.is_null() {
                continue;
            }
            let name = CStr::from_ptr(dest.name).to_string_lossy().into_owned();
            let device_uri = get_dest_option(conn.lib, dest, b"device-uri\0").unwrap_or_default();
            let printer_info =
                get_dest_option(conn.lib, dest, b"printer-info\0").unwrap_or_default();
            let state = get_dest_option(conn.lib, dest, b"printer-state\0")
                .and_then(|s| s.parse::<i32>().ok());
            let accepting_jobs = get_dest_option(conn.lib, dest, b"printer-is-accepting-jobs\0")
                .map(|v| !v.eq_ignore_ascii_case("false") && v != "0")
                .unwrap_or(true);
            let make_and_model =
                get_dest_option(conn.lib, dest, b"printer-make-and-model\0").unwrap_or_default();

            let mut options = Vec::new();
            if !dest.options.is_null() && dest.num_options > 0 {
                let slice_opts =
                    std::slice::from_raw_parts(dest.options, dest.num_options as usize);
                for opt in slice_opts {
                    if !opt.name.is_null() && !opt.value.is_null() {
                        let k = CStr::from_ptr(opt.name).to_string_lossy().into_owned();
                        let v = CStr::from_ptr(opt.value).to_string_lossy().into_owned();
                        options.push((k, v));
                    }
                }
            }

            let name_c = CString::new(name.as_str()).ok();
            let has_usable_conversion = if make_and_model.eq_ignore_ascii_case("Local Raw Printer")
                || make_and_model
                    .split(|c: char| !c.is_alphanumeric())
                    .any(|w| w.eq_ignore_ascii_case("raw"))
            {
                false
            } else if let Some(ref c_name) = name_c {
                let ppd_ptr = (conn.lib.cups_get_ppd2)(conn.http, c_name.as_ptr());
                if ppd_ptr.is_null() {
                    false
                } else {
                    let ppd_path = CStr::from_ptr(ppd_ptr).to_string_lossy();
                    let content = std::fs::read_to_string(ppd_path.as_ref()).unwrap_or_default();
                    let _ = std::fs::remove_file(ppd_path.as_ref());
                    content.contains("*cupsFilter2:") || content.contains("*cupsFilter:")
                }
            } else {
                false
            };

            records.push(CupsDestinationRecord {
                name,
                device_uri,
                printer_info,
                state,
                accepting_jobs,
                make_and_model,
                has_usable_conversion,
                options,
            });
        }
        (conn.lib.cups_free_dests)(count, dests_ptr);
    }

    Ok(records)
}

/// Queries the PPD file content for `cups_name` from `endpoint` via `libcups` (`cupsGetPPD2`).
pub fn query_cups_printer_ppd(
    endpoint: &CupsServerEndpoint,
    cups_name: &str,
) -> Result<String, AppError> {
    let conn = CupsHttpConnection::connect(endpoint)?;
    let c_name = CString::new(cups_name)
        .map_err(|_| AppError::invalid_input("invalid printer destination name"))?;
    let ppd_ptr = unsafe { (conn.lib.cups_get_ppd2)(conn.http, c_name.as_ptr()) };
    if ppd_ptr.is_null() {
        return Err(AppError::queue_unavailable(format!(
            "CUPS did not provide PPD for '{cups_name}' ({})",
            last_cups_error_message(conn.lib)
        )));
    }
    let ppd_path = unsafe { CStr::from_ptr(ppd_ptr) }.to_string_lossy();
    let path = std::path::Path::new(ppd_path.as_ref());
    let read_res = std::fs::read_to_string(path);
    let _ = std::fs::remove_file(path);
    let content = read_res.map_err(|e| AppError::internal(format!("cannot read PPD file: {e}")))?;
    Ok(content)
}

unsafe fn get_dest_option(
    lib: &LibCups,
    dest: &CupsDest,
    option_name_nul: &[u8],
) -> Option<String> {
    if dest.options.is_null() || dest.num_options <= 0 {
        return None;
    }
    let ptr = (lib.cups_get_option)(
        option_name_nul.as_ptr().cast::<c_char>(),
        dest.num_options,
        dest.options,
    );
    if ptr.is_null() {
        None
    } else {
        Some(CStr::from_ptr(ptr).to_string_lossy().into_owned())
    }
}

fn last_cups_error_message(lib: &LibCups) -> String {
    // Safety: `cupsLastErrorString` returns a thread-local NUL-terminated string or NULL.
    unsafe {
        let ptr = (lib.cups_last_error_string)();
        if ptr.is_null() {
            "unknown CUPS error".to_owned()
        } else {
            CStr::from_ptr(ptr).to_string_lossy().into_owned()
        }
    }
}

/// Reads raw records from CUPS using `libcups` so `DestinationAwarePrinterCatalog` can classify
/// local queues and exclude ShaPrint-owned Client queues.
#[derive(Debug, Clone)]
pub struct CupsSpoolerReader {
    endpoint: Option<CupsServerEndpoint>,
    proxy_authority: String,
}

impl Default for CupsSpoolerReader {
    fn default() -> Self {
        Self::new()
    }
}

impl CupsSpoolerReader {
    pub fn new() -> Self {
        Self {
            endpoint: None,
            proxy_authority: crate::adapters::queue_installation::proxy_authority(),
        }
    }

    pub fn with_authority(proxy_authority: impl Into<String>) -> Self {
        Self {
            endpoint: None,
            proxy_authority: proxy_authority.into(),
        }
    }

    pub fn with_endpoint(endpoint: CupsServerEndpoint) -> Self {
        Self {
            endpoint: Some(endpoint),
            proxy_authority: crate::adapters::queue_installation::proxy_authority(),
        }
    }

    pub fn with_endpoint_and_authority(
        endpoint: CupsServerEndpoint,
        proxy_authority: impl Into<String>,
    ) -> Self {
        Self {
            endpoint: Some(endpoint),
            proxy_authority: proxy_authority.into(),
        }
    }

    fn resolve_endpoint(&self) -> CupsServerEndpoint {
        CupsServerEndpoint::resolve(self.endpoint.as_ref())
    }
}

#[async_trait]
impl SpoolerReader for CupsSpoolerReader {
    async fn read_spooler_records(&self) -> Result<Vec<SpoolerRecord>, AppError> {
        let endpoint = self.resolve_endpoint();
        let proxy_authority = self.proxy_authority.clone();
        tokio::task::spawn_blocking(move || {
            let dests = query_cups_destinations(&endpoint)?;
            let mut records = Vec::with_capacity(dests.len());
            for dest in dests {
                // Stopped queues (`printer-state == 5`) or queues rejecting jobs are not working
                // local destinations unless they are ShaPrint Client queues being classified.
                let is_working = dest.accepting_jobs && dest.state != Some(IPP_PSTATE_STOPPED);
                let record = SpoolerRecord::new(dest.name, dest.device_uri);
                let is_recognised_client_queue = matches!(
                    record.classify(&proxy_authority),
                    crate::domain::SpoolerRecordClassification::RecognisedClientQueue(_)
                );
                if is_working || is_recognised_client_queue {
                    records.push(record);
                }
            }
            Ok(records)
        })
        .await
        .map_err(|error| AppError::internal(format!("CUPS enumeration did not finish: {error}")))?
    }
}

/// Lists working local CUPS destinations while excluding ShaPrint-owned Client queues.
pub struct CupsPrinterCatalog {
    inner: DestinationAwarePrinterCatalog,
    reader: CupsSpoolerReader,
}

impl Default for CupsPrinterCatalog {
    fn default() -> Self {
        Self::new()
    }
}

impl CupsPrinterCatalog {
    pub fn new() -> Self {
        Self::from_reader(CupsSpoolerReader::new())
    }

    pub fn with_authority(proxy_authority: String) -> Self {
        Self::from_reader(CupsSpoolerReader::with_authority(proxy_authority))
    }

    pub fn with_endpoint(endpoint: CupsServerEndpoint, proxy_authority: String) -> Self {
        Self::from_reader(CupsSpoolerReader::with_endpoint_and_authority(
            endpoint,
            proxy_authority,
        ))
    }

    fn from_reader(reader: CupsSpoolerReader) -> Self {
        Self {
            inner: DestinationAwarePrinterCatalog::with_authority(
                Arc::new(reader.clone()),
                reader.proxy_authority.clone(),
            ),
            reader,
        }
    }
}

#[async_trait]
impl LocalPrinterCatalog for CupsPrinterCatalog {
    async fn pending_client_jobs(&self, target: &RecognisedClientQueue) -> Result<u32, AppError> {
        let endpoint = self.reader.resolve_endpoint();
        let name = target.queue_name().to_string();
        tokio::task::spawn_blocking(move || {
            let count = query_cups_active_jobs(&endpoint, &name)?;
            u32::try_from(count)
                .map_err(|_| AppError::queue_unavailable("native job count is unavailable"))
        })
        .await
        .map_err(|_| AppError::queue_unavailable("native job inspection did not finish"))?
    }

    async fn inspect_client_queue(
        &self,
        target: &RecognisedClientQueue,
    ) -> Result<crate::application::LocalQueueInspection, AppError> {
        let endpoint = self.reader.resolve_endpoint();
        let authority = self.reader.proxy_authority.clone();
        let target = target.clone();
        tokio::task::spawn_blocking(move || {
            use crate::application::LocalQueueInspection;
            let dests = query_cups_destinations(&endpoint)?;
            let Some(dest) = dests
                .into_iter()
                .find(|dest| dest.name == target.queue_name().as_str())
            else {
                return Ok(LocalQueueInspection::Missing);
            };
            let record = SpoolerRecord::new(dest.name, dest.device_uri);
            let inspection = LocalQueueInspection::from_record(record, &target, &authority, None);
            Ok(inspection)
        })
        .await
        .map_err(|_| AppError::queue_unavailable("native CUPS inspection did not finish"))?
    }

    async fn local_printers(&self) -> Result<Vec<PrinterName>, AppError> {
        self.inner.local_printers().await
    }

    async fn recognised_client_queues(&self) -> Result<Vec<RecognisedClientQueue>, AppError> {
        self.inner.recognised_client_queues().await
    }
}

/// Maps ShaPrint `PrintSettings` into validated CUPS IPP job attribute `(name, value)` pairs.
pub fn build_cups_job_options(settings: &PrintSettings) -> Result<Vec<(String, String)>, AppError> {
    let mut options = Vec::new();
    if let Some(media) = &settings.media {
        let spec = media_spec(media)
            .ok_or_else(|| AppError::invalid_input(format!("unsupported media size '{media}'")))?;
        let value = if spec.keyword == "custom" {
            format!(
                "custom_{}x{}mm",
                format_tenths_mm(spec.short_tenths_mm),
                format_tenths_mm(spec.long_tenths_mm)
            )
        } else {
            spec.keyword.to_owned()
        };
        options.push(("media".to_owned(), value));
    }
    if let Some(color) = settings.color {
        let mode = if color { "color" } else { "monochrome" };
        options.push(("print-color-mode".to_owned(), mode.to_owned()));
    }
    if let Some(duplex) = settings.duplex {
        let sides = match duplex {
            DuplexMode::Simplex => "one-sided",
            DuplexMode::LongEdge => "two-sided-long-edge",
            DuplexMode::ShortEdge => "two-sided-short-edge",
        };
        options.push(("sides".to_owned(), sides.to_owned()));
    }
    if let Some(copies) = settings.copies {
        if !(1..=999).contains(&copies) {
            return Err(AppError::invalid_input(format!(
                "unsupported copies count '{copies}'"
            )));
        }
        options.push(("copies".to_owned(), copies.to_string()));
    }
    if let Some(orientation) = settings.orientation {
        let code = match orientation {
            PrintOrientation::Portrait => "3",
            PrintOrientation::Landscape => "4",
        };
        options.push(("orientation-requested".to_owned(), code.to_owned()));
    }
    Ok(options)
}

fn format_tenths_mm(tenths: i16) -> String {
    if tenths % 10 == 0 {
        format!("{}", tenths / 10)
    } else {
        format!("{}.{}", tenths / 10, (tenths % 10).abs())
    }
}

/// Validates that `document` is a complete, well-formed PWG Raster stream before any bytes are
/// sent to CUPS.
pub(crate) fn validate_pwg_raster_stream(document: &[u8]) -> Result<(), AppError> {
    let mut page_count = 0usize;
    for page in pwg_pages(document)? {
        let _decoded = page?;
        page_count += 1;
    }
    if page_count == 0 {
        return Err(AppError::invalid_input(
            "the PWG Raster document contains no pages",
        ));
    }
    Ok(())
}

/// Submits validated PWG Raster print jobs to a local CUPS destination via `libcups`.
#[derive(Debug, Clone)]
pub struct CupsPrintJobSubmitter {
    endpoint: Option<CupsServerEndpoint>,
}

impl Default for CupsPrintJobSubmitter {
    fn default() -> Self {
        Self::new()
    }
}

impl CupsPrintJobSubmitter {
    pub fn new() -> Self {
        Self { endpoint: None }
    }

    pub fn with_endpoint(endpoint: CupsServerEndpoint) -> Self {
        Self {
            endpoint: Some(endpoint),
        }
    }

    fn resolve_endpoint(&self) -> CupsServerEndpoint {
        CupsServerEndpoint::resolve(self.endpoint.as_ref())
    }
}

#[async_trait]
impl PrintJobSubmitter for CupsPrintJobSubmitter {
    fn is_available(&self) -> bool {
        libcups().is_ok()
    }

    async fn submit(&self, printer: &PrinterName, job: PrintJob) -> Result<u32, AppError> {
        let endpoint = self.resolve_endpoint();
        let printer_name = printer.as_str().to_owned();
        tokio::task::spawn_blocking(move || submit_cups_job(&endpoint, &printer_name, &job))
            .await
            .map_err(|error| {
                AppError::internal(format!("CUPS job submission did not finish: {error}"))
            })?
    }
}

fn submit_cups_job(
    endpoint: &CupsServerEndpoint,
    printer_name: &str,
    job: &PrintJob,
) -> Result<u32, AppError> {
    validate_pwg_raster_stream(job.document())?;
    let option_pairs = build_cups_job_options(job.settings())?;

    let dests = query_cups_destinations(endpoint)?;
    let Some(dest) = dests.iter().find(|d| d.name == printer_name) else {
        return Err(AppError::queue_unavailable(format!(
            "CUPS destination '{printer_name}' does not exist"
        )));
    };
    if !dest.accepting_jobs || dest.state == Some(IPP_PSTATE_STOPPED) {
        return Err(AppError::queue_unavailable(format!(
            "CUPS destination '{printer_name}' is not accepting print jobs"
        )));
    }

    let conn = CupsHttpConnection::connect(endpoint)?;
    let dest_c = CString::new(printer_name)
        .map_err(|_| AppError::invalid_input("printer name contains NUL byte"))?;
    let title_c = c"ShaPrint job";
    let docname_c = c"ShaPrint document";
    let format_c = c"image/pwg-raster";

    let mut cups_options: *mut CupsOption = std::ptr::null_mut();
    let mut num_options: c_int = 0;

    // Safety: `cupsAddOption` copies option strings into its managed array freed by `cupsFreeOptions`.
    unsafe {
        for (k, v) in &option_pairs {
            let k_c = CString::new(k.as_str())
                .map_err(|_| AppError::invalid_input("invalid option name"))?;
            let v_c = CString::new(v.as_str())
                .map_err(|_| AppError::invalid_input("invalid option value"))?;
            num_options = (conn.lib.cups_add_option)(
                k_c.as_ptr(),
                v_c.as_ptr(),
                num_options,
                &mut cups_options,
            );
        }
    }

    struct OptionsGuard {
        lib: &'static LibCups,
        num_options: c_int,
        options: *mut CupsOption,
    }
    impl Drop for OptionsGuard {
        fn drop(&mut self) {
            if !self.options.is_null() && self.num_options > 0 {
                unsafe { (self.lib.cups_free_options)(self.num_options, self.options) };
            }
        }
    }
    let _guard = OptionsGuard {
        lib: conn.lib,
        num_options,
        options: cups_options,
    };

    // Safety: `conn.http` is valid and `cups_options` has `num_options` elements.
    let job_id = unsafe {
        (conn.lib.cups_create_job)(
            conn.http,
            dest_c.as_ptr(),
            title_c.as_ptr(),
            num_options,
            cups_options,
        )
    };
    if job_id <= 0 {
        return Err(AppError::queue_unavailable(format!(
            "CUPS refused to create job for '{printer_name}': {}",
            last_cups_error_message(conn.lib)
        )));
    }

    // Safety: starts streaming the single document for `job_id`.
    let start_status = unsafe {
        (conn.lib.cups_start_document)(
            conn.http,
            dest_c.as_ptr(),
            job_id,
            docname_c.as_ptr(),
            format_c.as_ptr(),
            1,
        )
    };
    if start_status != HTTP_STATUS_CONTINUE {
        let message = last_cups_error_message(conn.lib);
        // Safety: `conn.http` and `dest_c` are valid; purges the incomplete `job_id`.
        unsafe {
            let _ = (conn.lib.cups_cancel_job2)(conn.http, dest_c.as_ptr(), job_id, 1);
        }
        return Err(AppError::queue_unavailable(format!(
            "CUPS refused to start PWG Raster document for '{printer_name}': {message}"
        )));
    }

    for chunk in job.document().chunks(STREAM_CHUNK_BYTES) {
        // Safety: `chunk` is a valid byte slice of `chunk.len()` bytes.
        let write_status = unsafe {
            (conn.lib.cups_write_request_data)(
                conn.http,
                chunk.as_ptr().cast::<c_char>(),
                chunk.len(),
            )
        };
        if write_status != HTTP_STATUS_CONTINUE {
            let message = last_cups_error_message(conn.lib);
            // Safety: `conn.http` and `dest_c` are valid; purges the incomplete `job_id`.
            unsafe {
                let _ = (conn.lib.cups_cancel_job2)(conn.http, dest_c.as_ptr(), job_id, 1);
            }
            return Err(AppError::queue_unavailable(format!(
                "CUPS interrupted PWG Raster stream for '{printer_name}': {message}"
            )));
        }
    }

    // Safety: completes the streamed document and returns the IPP status code.
    let finish_status = unsafe { (conn.lib.cups_finish_document)(conn.http, dest_c.as_ptr()) };
    if !matches!(
        finish_status,
        IPP_STATUS_OK | IPP_STATUS_OK_SUBST | IPP_STATUS_OK_CONFLICT
    ) {
        let message = last_cups_error_message(conn.lib);
        // Safety: `conn.http` and `dest_c` are valid; purges the failed `job_id`.
        unsafe {
            let _ = (conn.lib.cups_cancel_job2)(conn.http, dest_c.as_ptr(), job_id, 1);
        }
        return Err(AppError::queue_unavailable(format!(
            "CUPS rejected PWG Raster document for '{printer_name}': {message}"
        )));
    }

    Ok(job_id as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ErrorCode;

    #[test]
    fn maps_print_settings_into_cups_ipp_options() {
        let settings = PrintSettings {
            media: Some("iso_a4_210x297mm".to_owned()),
            color: Some(true),
            duplex: Some(DuplexMode::LongEdge),
            copies: Some(3),
            orientation: Some(PrintOrientation::Landscape),
        };
        let options = build_cups_job_options(&settings).expect("maps valid settings");
        assert_eq!(
            options,
            vec![
                ("media".to_owned(), "iso_a4_210x297mm".to_owned()),
                ("print-color-mode".to_owned(), "color".to_owned()),
                ("sides".to_owned(), "two-sided-long-edge".to_owned()),
                ("copies".to_owned(), "3".to_owned()),
                ("orientation-requested".to_owned(), "4".to_owned()),
            ]
        );
    }

    #[test]
    fn maps_monochrome_short_edge_portrait_and_custom_media() {
        let settings = PrintSettings {
            media: Some("custom_100x150mm".to_owned()),
            color: Some(false),
            duplex: Some(DuplexMode::ShortEdge),
            copies: Some(1),
            orientation: Some(PrintOrientation::Portrait),
        };
        let options = build_cups_job_options(&settings).expect("maps valid settings");
        assert_eq!(
            options,
            vec![
                ("media".to_owned(), "custom_100x150mm".to_owned()),
                ("print-color-mode".to_owned(), "monochrome".to_owned()),
                ("sides".to_owned(), "two-sided-short-edge".to_owned()),
                ("copies".to_owned(), "1".to_owned()),
                ("orientation-requested".to_owned(), "3".to_owned()),
            ]
        );
    }

    #[test]
    fn rejects_unsupported_media_or_invalid_copies_before_cups_submission() {
        let bad_media = PrintSettings {
            media: Some("invalid_paper_size".to_owned()),
            ..Default::default()
        };
        let err = build_cups_job_options(&bad_media).expect_err("rejects unknown media");
        assert_eq!(err.code(), ErrorCode::InvalidInput);

        let bad_copies = PrintSettings {
            copies: Some(0),
            ..Default::default()
        };
        let err = build_cups_job_options(&bad_copies).expect_err("rejects zero copies");
        assert_eq!(err.code(), ErrorCode::InvalidInput);
    }

    #[tokio::test]
    async fn cups_catalog_and_submitter_work_against_real_cupsd_and_exclude_shaprint_queues() {
        use std::process::{Child, Command};
        use std::time::{Duration, Instant};

        if !Path::new("/usr/sbin/cupsd").exists() || !Path::new("/usr/sbin/lpadmin").exists() {
            return;
        }

        struct IsolatedCupsd {
            dir: std::path::PathBuf,
            endpoint: CupsServerEndpoint,
            child: Child,
        }

        impl IsolatedCupsd {
            fn start() -> Self {
                let listener =
                    std::net::TcpListener::bind("127.0.0.1:0").expect("binds ephemeral port");
                let port = listener.local_addr().expect("reads addr").port();
                drop(listener);

                let dir = crate::test_support::temporary_directory("isolated-cupsd");
                for sub in [
                    "conf",
                    "spool",
                    "spool/temp",
                    "cache",
                    "state",
                    "state/certs",
                    "logs",
                    "ppd",
                    "serverbin/backend",
                    "serverbin/filter",
                    "serverbin/cgi-bin",
                    "serverbin/daemon",
                    "serverbin/driver",
                    "serverbin/monitor",
                    "serverbin/notifier",
                ] {
                    std::fs::create_dir_all(dir.join(sub)).expect("creates cups subdirs");
                }
                for sub in [
                    "filter", "cgi-bin", "daemon", "driver", "monitor", "notifier",
                ] {
                    let src = std::path::Path::new("/usr/lib/cups").join(sub);
                    if let Ok(entries) = std::fs::read_dir(&src) {
                        for entry in entries.flatten() {
                            let _ = std::os::unix::fs::symlink(
                                entry.path(),
                                dir.join("serverbin").join(sub).join(entry.file_name()),
                            );
                        }
                    }
                }
                for backend in ["ipp", "usb", "socket", "lpd"] {
                    let src = std::path::Path::new("/usr/lib/cups/backend").join(backend);
                    if src.exists() {
                        let dst = dir.join("serverbin/backend").join(backend);
                        std::fs::copy(&src, &dst).expect("copies backend");
                        use std::os::unix::fs::PermissionsExt;
                        std::fs::set_permissions(&dst, std::fs::Permissions::from_mode(0o755))
                            .expect("chmod 0755 backend");
                    }
                }
                let _ = std::os::unix::fs::symlink("ipp", dir.join("serverbin/backend/http"));
                let _ = std::os::unix::fs::symlink("ipp", dir.join("serverbin/backend/ipps"));

                let sock = dir.join("cups.sock");
                let cupsd_conf = dir.join("conf/cupsd.conf");
                let cups_files_conf = dir.join("conf/cups-files.conf");
                std::fs::write(
                    &cupsd_conf,
                    format!(
                        "LogLevel info\nListen 127.0.0.1:{port}\nListen {}\nBrowsing Off\nDefaultAuthType None\n<Location />\n  Order allow,deny\n  Allow all\n</Location>\n<Location /admin>\n  Order allow,deny\n  Allow all\n</Location>\n<Location /admin/conf>\n  Order allow,deny\n  Allow all\n</Location>\n<Policy default>\n  <Limit All>\n    Order allow,deny\n    Allow all\n  </Limit>\n</Policy>\n",
                        sock.display()
                    ),
                )
                .expect("writes cupsd.conf");
                std::fs::write(
                    &cups_files_conf,
                    format!(
                        "FileDevice Yes\nAccessLog {d}/logs/access_log\nErrorLog {d}/logs/error_log\nPageLog {d}/logs/page_log\nRequestRoot {d}/spool\nTempDir {d}/spool/temp\nCacheDir {d}/cache\nStateDir {d}/state\nServerRoot {d}/conf\nDataDir /usr/share/cups\nServerBin {d}/serverbin\n",
                        d = dir.display()
                    ),
                )
                .expect("writes cups-files.conf");

                let child = Command::new("/usr/sbin/cupsd")
                    .arg("-f")
                    .arg("-c")
                    .arg(&cupsd_conf)
                    .arg("-s")
                    .arg(&cups_files_conf)
                    .spawn()
                    .expect("spawns cupsd");

                let deadline = Instant::now() + Duration::from_secs(5);
                while Instant::now() < deadline {
                    if sock.exists() {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }

                Self {
                    dir,
                    endpoint: CupsServerEndpoint::parse(&format!("127.0.0.1:{port}")),
                    child,
                }
            }
        }

        impl Drop for IsolatedCupsd {
            fn drop(&mut self) {
                let _ = self.child.kill();
                let _ = self.child.wait();
                let _ = std::fs::remove_dir_all(&self.dir);
            }
        }

        let cupsd = IsolatedCupsd::start();
        let host_arg = cupsd.endpoint.lpadmin_host_arg();

        // Add one driverless IPP destination, one local USB/file destination, and one ShaPrint client queue.
        let req = crate::domain::ClientQueueRequest::new(
            "10.0.0.5:48631",
            PrinterName::parse("Office Printer").expect("valid"),
        )
        .expect("valid request");
        let shaprint_dest = req.cups_destination_name();

        for args in [
            vec![
                "-h",
                &host_arg,
                "-p",
                "Driverless_IPP",
                "-E",
                "-v",
                "ipp://192.0.2.10:631/ipp/print",
                "-D",
                "Driverless Network Printer",
            ],
            vec![
                "-h",
                &host_arg,
                "-p",
                "USB_LaserJet",
                "-E",
                "-v",
                "file:/dev/null",
                "-D",
                "USB LaserJet",
            ],
            vec![
                "-h",
                &host_arg,
                "-p",
                &shaprint_dest,
                "-E",
                "-v",
                "ipp://127.0.0.1:48632/ipp/print/10.0.0.5%3A48631/Office%20Printer",
                "-D",
                req.queue_name().as_str(),
            ],
        ] {
            let status = Command::new("/usr/sbin/lpadmin")
                .args(&args)
                .status()
                .expect("runs lpadmin");
            assert!(status.success(), "lpadmin failed for {args:?}");
        }

        let catalog =
            CupsPrinterCatalog::with_endpoint(cupsd.endpoint.clone(), "127.0.0.1:48632".to_owned());
        let local = catalog
            .local_printers()
            .await
            .expect("lists local printers");
        assert_eq!(
            local,
            vec![
                PrinterName::parse("Driverless_IPP").expect("valid"),
                PrinterName::parse("USB_LaserJet").expect("valid"),
            ]
        );

        let recognised = catalog
            .recognised_client_queues()
            .await
            .expect("lists recognised client queues");
        assert_eq!(recognised.len(), 1);
        assert_eq!(recognised[0].queue_name().as_str(), shaprint_dest);
        assert_eq!(recognised[0].server_address(), "10.0.0.5:48631");
        assert_eq!(recognised[0].printer_name().as_str(), "Office Printer");

        // Build a valid 1-page PWG Raster document and submit it to `USB_LaserJet` via `CupsPrintJobSubmitter`.
        let mut pwg_doc = b"RaS2".to_vec();
        let mut header = vec![0u8; 1796];
        for (offset, value) in [
            (276usize, 300u32),
            (280, 300),
            (372, 1),
            (376, 1),
            (384, 8),
            (388, 8),
            (392, 1),
            (400, 18),
            (420, 1),
        ] {
            header[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
        }
        pwg_doc.extend(header);
        pwg_doc.extend([0u8, 0u8, 255u8]);

        let submitter = CupsPrintJobSubmitter::with_endpoint(cupsd.endpoint.clone());
        let job = PrintJob::from_ipp_body(
            pwg_doc.clone(),
            0,
            PrintSettings {
                media: Some("iso_a4_210x297mm".to_owned()),
                color: Some(false),
                duplex: Some(DuplexMode::Simplex),
                copies: Some(2),
                orientation: Some(PrintOrientation::Landscape),
            },
        );
        let job_id = submitter
            .submit(&PrinterName::parse("USB_LaserJet").expect("valid"), job)
            .await
            .expect("submits PWG Raster job to CUPS");
        assert!(job_id >= 1);

        // Submitting a truncated PWG Raster payload returns InvalidInput.
        let truncated_job =
            PrintJob::from_ipp_body(vec![b'R', b'a', b'S', b'2'], 0, PrintSettings::default());
        let bad_format_err = submitter
            .submit(
                &PrinterName::parse("USB_LaserJet").expect("valid"),
                truncated_job,
            )
            .await
            .expect_err("rejects truncated PWG Raster");
        assert_eq!(bad_format_err.code(), ErrorCode::InvalidInput);

        // Submitting a valid PWG Raster job to a non-existent CUPS queue returns QueueUnavailable.
        let missing_queue_job = PrintJob::from_ipp_body(pwg_doc, 0, PrintSettings::default());
        let missing_err = submitter
            .submit(
                &PrinterName::parse("NonExistentQueue").expect("valid"),
                missing_queue_job,
            )
            .await
            .expect_err("non-existent CUPS queue must return QueueUnavailable");
        assert_eq!(missing_err.code(), ErrorCode::QueueUnavailable);
    }
}
