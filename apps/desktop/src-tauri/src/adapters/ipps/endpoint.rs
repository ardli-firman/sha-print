//! The IPP operations the sharing endpoint answers, independent of how the request arrived.
//!
//! Query operations are public; Print-Job requires an authorized Network Channel and is submitted
//! only to a queue in the current shared selection.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::application::SharedPrinterSource;
use crate::domain::{ErrorCode, PrintFailure, PrinterName};

use super::protocol::{
    job_attributes_response, job_response, jobs_response, managed_job_response, percent_decode,
    percent_encode, response, JobEntry, PrinterEntry, Request, Status, IPP_VERSION_1_1,
    OPERATION_CANCEL_JOB, OPERATION_CREATE_JOB, OPERATION_GET_JOBS, OPERATION_GET_JOB_ATTRIBUTES,
    OPERATION_GET_PRINTERS, OPERATION_GET_PRINTER_ATTRIBUTES, OPERATION_PRINT_JOB,
    OPERATION_SEND_DOCUMENT, OPERATION_VALIDATE_JOB,
};

use crate::adapters::ipps::NetworkChannel;
use crate::application::{
    DuplexMode, PrintFailures, PrintJob, PrintJobSubmitter, PrintOrientation, PrintSettings,
    RequestLease,
};

const PENDING_JOB_LIFETIME: Duration = Duration::from_secs(5 * 60);
const JOB_HISTORY_LIFETIME: Duration = Duration::from_secs(60 * 60);
const MAX_TRACKED_JOBS: usize = 256;
const MAX_JOB_DOCUMENT_BYTES: usize = 64 * 1024 * 1024;

/// Server-lifetime state for RFC 8011 multi-step print operations.
#[derive(Debug, Default)]
pub(crate) struct JobStore {
    inner: Mutex<JobStoreState>,
}

#[derive(Debug)]
struct JobStoreState {
    next_id: i32,
    jobs: BTreeMap<i32, StoredJob>,
}

impl Default for JobStoreState {
    fn default() -> Self {
        Self {
            next_id: 1,
            jobs: BTreeMap::new(),
        }
    }
}

#[derive(Debug)]
struct StoredJob {
    entry: JobEntry,
    printer: PrinterName,
    settings: PrintSettings,
    document: Vec<u8>,
    created_at: Instant,
    _pending_lease: Option<RequestLease>,
}

struct JobSubmission {
    entry: JobEntry,
    printer: PrinterName,
    settings: PrintSettings,
    document: Vec<u8>,
}

impl JobStore {
    fn create(
        &self,
        host: &str,
        printer: PrinterName,
        name: Option<&str>,
        settings: PrintSettings,
        lease: RequestLease,
    ) -> Result<JobEntry, Status> {
        let mut state = self.inner.lock().map_err(|_| Status::InternalError)?;
        Self::expire_locked(&mut state);
        if state.jobs.len() >= MAX_TRACKED_JOBS {
            return Err(Status::NotAcceptingJobs);
        }
        let id = Self::next_id(&mut state)?;
        let printer_uri = entry(host, &printer, true).uri;
        let job = JobEntry {
            id,
            uri: format!("{printer_uri}/jobs/{id}"),
            printer_uri,
            printer_name: printer.as_str().to_owned(),
            name: name.unwrap_or("ShaPrint job").to_owned(),
            document_count: 0,
            state: 3,
        };
        state.jobs.insert(
            id,
            StoredJob {
                entry: job.clone(),
                printer,
                settings,
                document: Vec::new(),
                created_at: Instant::now(),
                _pending_lease: Some(lease),
            },
        );
        Ok(job)
    }

    fn next_id(state: &mut JobStoreState) -> Result<i32, Status> {
        for _ in 0..=MAX_TRACKED_JOBS {
            let id = state.next_id.max(1);
            state.next_id = if id == i32::MAX { 1 } else { id + 1 };
            if !state.jobs.contains_key(&id) {
                return Ok(id);
            }
        }
        Err(Status::NotAcceptingJobs)
    }

    fn get(&self, id: i32) -> Result<JobEntry, Status> {
        let mut state = self.inner.lock().map_err(|_| Status::InternalError)?;
        Self::expire_locked(&mut state);
        state
            .jobs
            .get(&id)
            .map(|job| job.entry.clone())
            .ok_or(Status::NotFound)
    }

    fn list(
        &self,
        printer: &PrinterName,
        which_jobs: Option<&str>,
    ) -> Result<Vec<JobEntry>, Status> {
        let which_jobs = which_jobs.unwrap_or("not-completed");
        if !["all", "completed", "not-completed", "canceled", "aborted"]
            .iter()
            .any(|value| which_jobs.eq_ignore_ascii_case(value))
        {
            return Err(Status::AttributesOrValuesNotSupported);
        }
        let mut state = self.inner.lock().map_err(|_| Status::InternalError)?;
        Self::expire_locked(&mut state);
        Ok(state
            .jobs
            .values()
            .filter(|job| {
                job.printer.as_str().eq_ignore_ascii_case(printer.as_str())
                    && match which_jobs.to_ascii_lowercase().as_str() {
                        "all" => true,
                        "completed" => matches!(job.entry.state, 7..=9),
                        "not-completed" => matches!(job.entry.state, 3..=6),
                        "canceled" => job.entry.state == 7,
                        "aborted" => job.entry.state == 8,
                        _ => false,
                    }
            })
            .map(|job| job.entry.clone())
            .collect())
    }

    fn append_document(
        &self,
        id: i32,
        printer: &PrinterName,
        document: &[u8],
        last_document: bool,
    ) -> Result<Option<JobSubmission>, Status> {
        let mut state = self.inner.lock().map_err(|_| Status::InternalError)?;
        Self::expire_locked(&mut state);
        let job = state.jobs.get_mut(&id).ok_or(Status::NotFound)?;
        if !job.printer.as_str().eq_ignore_ascii_case(printer.as_str()) {
            return Err(Status::NotFound);
        }
        if job.entry.state != 3 {
            return Err(Status::NotPossible);
        }
        if document.len() > MAX_JOB_DOCUMENT_BYTES.saturating_sub(job.document.len()) {
            return Err(Status::DocumentTooLarge);
        }
        job.document.extend_from_slice(document);
        job.entry.document_count = job.entry.document_count.saturating_add(1);
        if !last_document {
            return Ok(None);
        }
        job.entry.state = 5;
        Ok(Some(JobSubmission {
            entry: job.entry.clone(),
            printer: job.printer.clone(),
            settings: job.settings.clone(),
            document: std::mem::take(&mut job.document),
        }))
    }

    fn finish(&self, id: i32, state_value: i32) -> Option<JobEntry> {
        let mut state = self.inner.lock().ok()?;
        let job = state.jobs.get_mut(&id)?;
        job.entry.state = state_value;
        job.document.clear();
        job._pending_lease.take();
        Some(job.entry.clone())
    }

    fn cancel(&self, id: i32, printer: &PrinterName) -> Result<JobEntry, Status> {
        let mut state = self.inner.lock().map_err(|_| Status::InternalError)?;
        Self::expire_locked(&mut state);
        let job = state.jobs.get_mut(&id).ok_or(Status::NotFound)?;
        if !job.printer.as_str().eq_ignore_ascii_case(printer.as_str()) {
            return Err(Status::NotFound);
        }
        if job.entry.state != 3 {
            return Err(Status::NotPossible);
        }
        job.entry.state = 7;
        job.document.clear();
        job._pending_lease.take();
        Ok(job.entry.clone())
    }

    pub(crate) fn expire(&self) {
        if let Ok(mut state) = self.inner.lock() {
            Self::expire_locked(&mut state);
        }
    }

    pub(crate) fn abort_incomplete(&self) {
        if let Ok(mut state) = self.inner.lock() {
            for job in state.jobs.values_mut() {
                if matches!(job.entry.state, 3 | 5) {
                    job.entry.state = 8;
                    job.document.clear();
                    job._pending_lease.take();
                }
            }
        }
    }

    fn expire_locked(state: &mut JobStoreState) {
        state.jobs.retain(|_, job| {
            let lifetime = if job.entry.state == 3 {
                PENDING_JOB_LIFETIME
            } else {
                JOB_HISTORY_LIFETIME
            };
            job.created_at.elapsed() < lifetime
        });
    }
}

#[cfg(test)]
fn answer(request: &[u8], host: &str, shared: &dyn SharedPrinterSource) -> Vec<u8> {
    answer_with_job_status(request, host, shared, false)
}

fn answer_with_job_status(
    request: &[u8],
    host: &str,
    shared: &dyn SharedPrinterSource,
    accepting_jobs: bool,
) -> Vec<u8> {
    let request = match Request::parse(request) {
        Ok(request) => request,
        // The header was unreadable, so there is no request id to echo.
        Err(_) => return response(0, IPP_VERSION_1_1, Status::BadRequest, &[]),
    };

    let operation = request.operation();
    let request_id = request.request_id();
    let version = request.response_version();
    if !request.version_is_supported() {
        return response(request_id, version, Status::VersionNotSupported, &[]);
    }

    match operation {
        OPERATION_GET_PRINTERS => {
            let printers: Vec<PrinterEntry> = shared
                .shared_printers()
                .iter()
                .map(|name| entry(host, name, accepting_jobs))
                .collect();
            response(request_id, version, Status::Ok, &printers)
        }
        OPERATION_GET_PRINTER_ATTRIBUTES => {
            let shared = shared.shared_printers();
            let selected = match request.value("printer-uri") {
                Some(uri) => find_by_uri(&shared, host, uri),
                None => request
                    .value("printer-name")
                    .and_then(|name| find_by_name(&shared, name)),
            };
            match selected {
                Some(name) => response(
                    request_id,
                    version,
                    Status::Ok,
                    &[entry(host, name, accepting_jobs)],
                ),
                None => response(request_id, version, Status::NotFound, &[]),
            }
        }
        _ => response(request_id, version, Status::UnsupportedOperation, &[]),
    }
}

/// Dependencies shared by the operations one IPPS request may use.
#[derive(Clone, Copy)]
pub(super) struct EndpointContext<'a> {
    pub(super) host: &'a str,
    pub(super) shared: &'a dyn SharedPrinterSource,
    pub(super) channel: &'a NetworkChannel,
    pub(super) submitter: &'a dyn PrintJobSubmitter,
    pub(super) failures: &'a PrintFailures,
    pub(super) jobs: &'a JobStore,
}

/// Processes Print-Job with the authorization and queue-submission ports.
pub(super) async fn answer_job(
    bytes: Vec<u8>,
    context: EndpointContext<'_>,
    mut request_lease: RequestLease,
) -> (Vec<u8>, Option<RequestLease>) {
    let EndpointContext {
        host,
        shared,
        channel,
        submitter,
        failures,
        ..
    } = context;
    let request = match Request::parse(&bytes) {
        Ok(request) => request,
        Err(_) => return (response(0, IPP_VERSION_1_1, Status::BadRequest, &[]), None),
    };
    let request_id = request.request_id();
    let version = request.response_version();
    if !request.version_is_supported() {
        return (
            response(request_id, version, Status::VersionNotSupported, &[]),
            None,
        );
    }
    let operation = request.operation();
    if matches!(
        operation,
        OPERATION_CREATE_JOB
            | OPERATION_SEND_DOCUMENT
            | OPERATION_CANCEL_JOB
            | OPERATION_GET_JOB_ATTRIBUTES
            | OPERATION_GET_JOBS
    ) {
        drop(request);
        return answer_multi_step(bytes, context, request_lease).await;
    }
    if operation != OPERATION_PRINT_JOB && operation != OPERATION_VALIDATE_JOB {
        return (
            answer_with_job_status(
                &bytes,
                host,
                shared,
                channel.is_configured() && submitter.is_available(),
            ),
            None,
        );
    }

    let Some(candidate) = request.value("network-channel") else {
        return (
            response(request_id, version, Status::NotAuthorized, &[]),
            None,
        );
    };
    if !channel.authorizes(candidate) {
        return (
            response(request_id, version, Status::NotAuthorized, &[]),
            None,
        );
    }

    let active_job_guard = if operation == OPERATION_PRINT_JOB {
        if !request_lease.mark_print_job() {
            return (
                response(request_id, version, Status::NotAcceptingJobs, &[]),
                None,
            );
        }
        Some(request_lease)
    } else {
        None
    };
    if !submitter.is_available() {
        // Reported only after authorization, so an anonymous caller cannot fill the server user's
        // screen with failures it has no way to act on.
        failures.report(PrintFailure::server(ErrorCode::QueueUnavailable, None));
        return (
            response(request_id, version, Status::NotAcceptingJobs, &[]),
            active_job_guard,
        );
    }
    let shared_printers = shared.shared_printers();
    let selected = request
        .value("printer-uri")
        .and_then(|uri| find_by_uri(&shared_printers, host, uri))
        .cloned();
    let Some(printer) = selected else {
        return (
            response(request_id, version, Status::NotFound, &[]),
            active_job_guard,
        );
    };

    if let Some(document_format) = request.value("document-format") {
        if !matches!(
            document_format.to_ascii_lowercase().as_str(),
            "image/pwg-raster"
        ) {
            return (
                response(request_id, version, Status::DocumentFormatNotSupported, &[]),
                active_job_guard,
            );
        }
    } else if operation == OPERATION_PRINT_JOB {
        return (
            response(request_id, version, Status::BadRequest, &[]),
            active_job_guard,
        );
    }

    let settings = match job_settings(&request) {
        Ok(settings) => settings,
        Err(status) => return (response(request_id, version, status, &[]), active_job_guard),
    };

    if operation == OPERATION_VALIDATE_JOB {
        return (
            response(request_id, version, Status::Ok, &[]),
            active_job_guard,
        );
    }

    if request.document().is_empty() {
        return (
            response(request_id, version, Status::BadRequest, &[]),
            active_job_guard,
        );
    }

    let document_start = request.document_start();
    drop(request);
    let job = PrintJob::from_ipp_body(bytes, document_start, settings);
    let answer = match submitter.submit(&printer, job).await {
        Ok(job_id) => {
            let printer_uri = entry(host, &printer, true).uri;
            let job_uri = format!("{printer_uri}/jobs/{job_id}");
            job_response(request_id, version, job_id, &job_uri)
        }
        Err(error) => {
            let status = match error.code() {
                ErrorCode::InvalidInput => Status::AttributesOrValuesNotSupported,
                ErrorCode::Unsupported => Status::NotAcceptingJobs,
                _ => Status::InternalError,
            };
            failures.report(PrintFailure::server(
                submission_failure(error.code()),
                Some(&printer),
            ));
            response(request_id, version, status, &[])
        }
    };
    (answer, active_job_guard)
}

async fn answer_multi_step(
    bytes: Vec<u8>,
    context: EndpointContext<'_>,
    mut request_lease: RequestLease,
) -> (Vec<u8>, Option<RequestLease>) {
    let EndpointContext {
        host,
        shared,
        channel,
        submitter,
        failures,
        jobs,
    } = context;
    let request = match Request::parse(&bytes) {
        Ok(request) => request,
        Err(_) => return (response(0, IPP_VERSION_1_1, Status::BadRequest, &[]), None),
    };
    let request_id = request.request_id();
    let version = request.response_version();
    let operation = request.operation();
    if !request.version_is_supported() {
        return (
            response(request_id, version, Status::VersionNotSupported, &[]),
            None,
        );
    }

    if matches!(
        operation,
        OPERATION_CREATE_JOB
            | OPERATION_SEND_DOCUMENT
            | OPERATION_CANCEL_JOB
            | OPERATION_GET_JOB_ATTRIBUTES
            | OPERATION_GET_JOBS
    ) {
        let Some(candidate) = request.value("network-channel") else {
            return (
                response(request_id, version, Status::NotAuthorized, &[]),
                None,
            );
        };
        if !channel.authorizes(candidate) {
            return (
                response(request_id, version, Status::NotAuthorized, &[]),
                None,
            );
        }
    }

    let shared_printers = shared.shared_printers();
    let selected = request
        .value("printer-uri")
        .and_then(|uri| find_by_uri(&shared_printers, host, uri))
        .cloned();

    match operation {
        OPERATION_CREATE_JOB => {
            let Some(printer) = selected else {
                return (response(request_id, version, Status::NotFound, &[]), None);
            };
            if !request.document().is_empty() {
                return (response(request_id, version, Status::BadRequest, &[]), None);
            }
            if request
                .value("document-format")
                .is_some_and(|format| !format.eq_ignore_ascii_case("image/pwg-raster"))
            {
                return (
                    response(request_id, version, Status::DocumentFormatNotSupported, &[]),
                    None,
                );
            }
            if !submitter.is_available() {
                failures.report(PrintFailure::server(ErrorCode::QueueUnavailable, None));
                return (
                    response(request_id, version, Status::NotAcceptingJobs, &[]),
                    None,
                );
            }
            let settings = match job_settings(&request) {
                Ok(settings) => settings,
                Err(status) => return (response(request_id, version, status, &[]), None),
            };
            if !request_lease.mark_print_job() {
                return (
                    response(request_id, version, Status::NotAcceptingJobs, &[]),
                    None,
                );
            }
            match jobs.create(
                host,
                printer,
                request.value("job-name"),
                settings,
                request_lease,
            ) {
                Ok(job) => (managed_job_response(request_id, version, &job), None),
                Err(status) => (response(request_id, version, status, &[]), None),
            }
        }
        OPERATION_SEND_DOCUMENT => {
            let Some(printer) = selected else {
                return (response(request_id, version, Status::NotFound, &[]), None);
            };
            let Some(job_id) = request.integer("job-id").filter(|id| *id > 0) else {
                return (response(request_id, version, Status::BadRequest, &[]), None);
            };
            if request
                .value("document-format")
                .is_some_and(|format| !format.eq_ignore_ascii_case("image/pwg-raster"))
            {
                return (
                    response(request_id, version, Status::DocumentFormatNotSupported, &[]),
                    None,
                );
            }
            let Some(last_document) = request.boolean("last-document") else {
                return (response(request_id, version, Status::BadRequest, &[]), None);
            };
            if request.document().is_empty() {
                return (response(request_id, version, Status::BadRequest, &[]), None);
            }
            if !request_lease.mark_print_job() {
                return (
                    response(request_id, version, Status::NotAcceptingJobs, &[]),
                    None,
                );
            }
            if !submitter.is_available() {
                failures.report(PrintFailure::server(
                    ErrorCode::QueueUnavailable,
                    Some(&printer),
                ));
                return (
                    response(request_id, version, Status::NotAcceptingJobs, &[]),
                    Some(request_lease),
                );
            }
            let submission =
                match jobs.append_document(job_id, &printer, request.document(), last_document) {
                    Ok(submission) => submission,
                    Err(status) => {
                        return (
                            response(request_id, version, status, &[]),
                            Some(request_lease),
                        )
                    }
                };
            let Some(submission) = submission else {
                return match jobs.get(job_id) {
                    Ok(job) => (
                        managed_job_response(request_id, version, &job),
                        Some(request_lease),
                    ),
                    Err(status) => (
                        response(request_id, version, status, &[]),
                        Some(request_lease),
                    ),
                };
            };

            let document_start = request.document_start();
            drop(request);
            let mut body = bytes[..document_start].to_vec();
            body.extend_from_slice(&submission.document);
            let print_job = PrintJob::from_ipp_body(body, document_start, submission.settings);
            let result = submitter.submit(&submission.printer, print_job).await;
            match result {
                Ok(_) => {
                    let job = jobs.finish(job_id, 9).unwrap_or(submission.entry);
                    (
                        managed_job_response(request_id, version, &job),
                        Some(request_lease),
                    )
                }
                Err(error) => {
                    let status = match error.code() {
                        ErrorCode::InvalidInput => Status::AttributesOrValuesNotSupported,
                        ErrorCode::Unsupported => Status::NotAcceptingJobs,
                        _ => Status::InternalError,
                    };
                    failures.report(PrintFailure::server(
                        submission_failure(error.code()),
                        Some(&submission.printer),
                    ));
                    jobs.finish(job_id, 8);
                    (
                        response(request_id, version, status, &[]),
                        Some(request_lease),
                    )
                }
            }
        }
        OPERATION_GET_JOB_ATTRIBUTES => {
            let Some(job_id) = request.integer("job-id").filter(|id| *id > 0) else {
                return (response(request_id, version, Status::BadRequest, &[]), None);
            };
            let job = match jobs.get(job_id) {
                Ok(job) => job,
                Err(status) => return (response(request_id, version, status, &[]), None),
            };
            let Some(printer) = selected else {
                return (response(request_id, version, Status::NotFound, &[]), None);
            };
            if !printer.as_str().eq_ignore_ascii_case(&job.printer_name) {
                return (response(request_id, version, Status::NotFound, &[]), None);
            }
            let requested = request.text_values("requested-attributes");
            (
                job_attributes_response(request_id, version, &job, &requested),
                None,
            )
        }
        OPERATION_GET_JOBS => {
            let Some(printer) = selected else {
                return (response(request_id, version, Status::NotFound, &[]), None);
            };
            match jobs.list(&printer, request.value("which-jobs")) {
                Ok(jobs) => {
                    let requested = request.text_values("requested-attributes");
                    (jobs_response(request_id, version, &jobs, &requested), None)
                }
                Err(status) => (response(request_id, version, status, &[]), None),
            }
        }
        OPERATION_CANCEL_JOB => {
            let Some(printer) = selected else {
                return (response(request_id, version, Status::NotFound, &[]), None);
            };
            let Some(job_id) = request.integer("job-id").filter(|id| *id > 0) else {
                return (response(request_id, version, Status::BadRequest, &[]), None);
            };
            if !request_lease.mark_print_job() {
                return (
                    response(request_id, version, Status::NotAcceptingJobs, &[]),
                    None,
                );
            }
            match jobs.cancel(job_id, &printer) {
                Ok(job) => (
                    managed_job_response(request_id, version, &job),
                    Some(request_lease),
                ),
                Err(status) => (
                    response(request_id, version, status, &[]),
                    Some(request_lease),
                ),
            }
        }
        _ => (
            response(request_id, version, Status::UnsupportedOperation, &[]),
            None,
        ),
    }
}

/// The code a user should see when the spooler refused a job.
///
/// Settings the printer cannot accept are the user's to change; anything else means the queue
/// itself is not usable from this server.
fn submission_failure(code: ErrorCode) -> ErrorCode {
    if code == ErrorCode::InvalidInput {
        code
    } else {
        ErrorCode::QueueUnavailable
    }
}

fn job_settings(request: &Request<'_>) -> Result<PrintSettings, Status> {
    let media = request.value("media").map(str::to_owned);
    let color = match request.value("print-color-mode") {
        Some("color") => Some(true),
        Some("monochrome") | Some("bi-level") => Some(false),
        Some(_) => return Err(Status::AttributesOrValuesNotSupported),
        None => None,
    };
    let duplex = match request.value("sides") {
        Some("one-sided") => Some(DuplexMode::Simplex),
        Some("two-sided-long-edge") => Some(DuplexMode::LongEdge),
        Some("two-sided-short-edge") => Some(DuplexMode::ShortEdge),
        Some(_) => return Err(Status::AttributesOrValuesNotSupported),
        None => None,
    };
    let copies = match request.integer("copies") {
        Some(value) if (1..=999).contains(&value) => Some(value as u16),
        Some(_) => return Err(Status::AttributesOrValuesNotSupported),
        None => None,
    };
    let orientation = match request.integer("orientation-requested") {
        Some(3) => Some(PrintOrientation::Portrait),
        Some(4) => Some(PrintOrientation::Landscape),
        Some(_) => return Err(Status::AttributesOrValuesNotSupported),
        None => None,
    };
    Ok(PrintSettings {
        media,
        color,
        duplex,
        copies,
        orientation,
    })
}

/// The advertisement for one shared queue.
fn entry(host: &str, name: &PrinterName, accepting_jobs: bool) -> PrinterEntry {
    PrinterEntry {
        name: name.as_str().to_owned(),
        uri: format!("ipps://{host}/ipp/print/{}", percent_encode(name.as_str())),
        accepting_jobs,
    }
}

/// Finds a queue only when the request URI identifies its advertised IPPS authority and path.
fn find_by_uri<'a>(
    shared: &'a [PrinterName],
    host: &str,
    requested: &str,
) -> Option<&'a PrinterName> {
    let (authority, path) = ipps_authority_and_path(requested)?;
    if !authority.eq_ignore_ascii_case(host) {
        return None;
    }
    let encoded_name = path.strip_prefix("ipp/print/")?;
    let decoded_name = percent_decode(encoded_name);
    let printer = find_by_name(shared, &decoded_name)?;
    Some(printer)
}

fn ipps_authority_and_path(uri: &str) -> Option<(&str, &str)> {
    let (scheme, remainder) = uri.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("ipps") {
        return None;
    }
    let (authority, path) = remainder.split_once('/')?;
    (!authority.is_empty() && !path.is_empty()).then_some((authority, path))
}

fn find_by_name<'a>(shared: &'a [PrinterName], requested: &str) -> Option<&'a PrinterName> {
    shared
        .iter()
        .find(|printer| printer.as_str().eq_ignore_ascii_case(requested))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::SharedPrinterSource;
    use crate::domain::PrinterName;

    /// Shares a fixed set of queues.
    struct FakeShared {
        names: Vec<PrinterName>,
    }

    impl FakeShared {
        fn new(names: &[&str]) -> Self {
            Self {
                names: names
                    .iter()
                    .map(|name| PrinterName::parse(name).expect("valid name"))
                    .collect(),
            }
        }
    }

    impl SharedPrinterSource for FakeShared {
        fn shared_printers(&self) -> Vec<PrinterName> {
            self.names.clone()
        }
    }

    /// Builds a client request.
    fn request(operation: u16, attributes: &[(&str, &str)]) -> Vec<u8> {
        let mut out = vec![2, 0];
        out.extend(operation.to_be_bytes());
        out.extend(5u32.to_be_bytes());
        out.push(0x01);
        push(&mut out, 0x47, "attributes-charset", "utf-8");
        push(&mut out, 0x48, "attributes-natural-language", "en");
        for (name, value) in attributes {
            push(&mut out, 0x45, name, value);
        }
        out.push(0x03);
        out
    }

    /// Appends one name/value attribute.
    fn push(out: &mut Vec<u8>, value_tag: u8, name: &str, value: &str) {
        out.push(value_tag);
        out.extend((name.len() as u16).to_be_bytes());
        out.extend(name.as_bytes());
        out.extend((value.len() as u16).to_be_bytes());
        out.extend(value.as_bytes());
    }

    /// The `printer-name` values an answer advertises.
    fn advertised(answer: &[u8]) -> Vec<String> {
        values_of(answer, b"printer-name")
    }

    /// The values an answer carries for one attribute.
    fn values_of(answer: &[u8], wanted: &[u8]) -> Vec<String> {
        let mut values = Vec::new();
        let mut position = 8;
        let mut matching = false;
        while position + 1 < answer.len() {
            let value_tag = answer[position];
            position += 1;
            if value_tag == 0x03 {
                break;
            }
            if (0x01..=0x05).contains(&value_tag) {
                matching = false;
                continue;
            }
            let name = read(answer, &mut position);
            let value = read(answer, &mut position);
            if !name.is_empty() {
                matching = name == wanted;
            }
            if matching {
                values.push(String::from_utf8_lossy(&value).into_owned());
            }
        }
        values
    }

    fn read(bytes: &[u8], position: &mut usize) -> Vec<u8> {
        let length = usize::from(u16::from_be_bytes([bytes[*position], bytes[*position + 1]]));
        *position += 2;
        let value = bytes[*position..*position + length].to_vec();
        *position += length;
        value
    }

    /// The status code of a response header (RFC 8010 §3.4.3).
    fn status(answer: &[u8]) -> u16 {
        u16::from_be_bytes([answer[2], answer[3]])
    }

    #[test]
    fn get_printers_lists_exactly_the_shared_queues() {
        let shared = FakeShared::new(&["HP LaserJet", "Zebra"]);

        let answer = answer(
            &request(OPERATION_GET_PRINTERS, &[]),
            "server:8631",
            &shared,
        );

        assert_eq!(status(&answer), 0x0000);
        assert_eq!(advertised(&answer), vec!["HP LaserJet", "Zebra"]);
        assert!(
            String::from_utf8_lossy(&answer).contains("ipps://server:8631/ipp/print/HP%20LaserJet")
        );
    }

    #[test]
    fn get_printers_advertises_supported_document_formats() {
        let shared = FakeShared::new(&["Zebra"]);

        let answer = answer(
            &request(OPERATION_GET_PRINTERS, &[]),
            "server:8631",
            &shared,
        );

        assert_eq!(
            values_of(&answer, b"document-format-supported"),
            vec!["image/pwg-raster"]
        );
    }

    #[test]
    fn get_printers_with_nothing_shared_is_a_successful_empty_answer() {
        let shared = FakeShared::new(&[]);

        let answer = answer(&request(OPERATION_GET_PRINTERS, &[]), "server", &shared);

        assert_eq!(status(&answer), 0x0000);
        assert!(advertised(&answer).is_empty());
    }

    #[test]
    fn get_printer_attributes_finds_a_shared_queue_by_its_advertised_uri() {
        let shared = FakeShared::new(&["HP LaserJet", "Zebra"]);

        let answer = answer(
            &request(
                OPERATION_GET_PRINTER_ATTRIBUTES,
                &[("printer-uri", "ipps://server:8631/ipp/print/Zebra")],
            ),
            "server:8631",
            &shared,
        );

        assert_eq!(status(&answer), 0x0000);
        assert_eq!(advertised(&answer), vec!["Zebra"]);
    }

    #[test]
    fn get_printer_attributes_accepts_a_case_normalized_queue_uri() {
        let shared = FakeShared::new(&["Office Printer"]);

        let answer = answer(
            &request(
                OPERATION_GET_PRINTER_ATTRIBUTES,
                &[(
                    "printer-uri",
                    "ipps://server:8631/ipp/print/office%20printer",
                )],
            ),
            "server:8631",
            &shared,
        );

        assert_eq!(status(&answer), 0x0000);
        assert_eq!(advertised(&answer), vec!["Office Printer"]);
    }

    #[test]
    fn get_printer_attributes_also_accepts_a_bare_printer_name() {
        let shared = FakeShared::new(&["HP LaserJet"]);

        let answer = answer(
            &request(
                OPERATION_GET_PRINTER_ATTRIBUTES,
                &[("printer-name", "hp laserjet")],
            ),
            "server",
            &shared,
        );

        assert_eq!(status(&answer), 0x0000);
        assert_eq!(advertised(&answer), vec!["HP LaserJet"]);
    }

    #[test]
    fn an_unshared_queue_is_not_found() {
        let shared = FakeShared::new(&["HP LaserJet"]);

        let answer = answer(
            &request(
                OPERATION_GET_PRINTER_ATTRIBUTES,
                &[("printer-uri", "ipps://server:8631/ipp/print/Canon")],
            ),
            "server:8631",
            &shared,
        );

        assert_eq!(status(&answer), 0x0406);
        assert!(advertised(&answer).is_empty());
    }

    #[test]
    fn a_request_without_a_printer_is_not_found() {
        let shared = FakeShared::new(&["HP LaserJet"]);

        let answer = answer(
            &request(OPERATION_GET_PRINTER_ATTRIBUTES, &[]),
            "server",
            &shared,
        );

        assert_eq!(status(&answer), 0x0406);
    }

    #[test]
    fn an_unreadable_request_is_a_client_error() {
        let shared = FakeShared::new(&["HP LaserJet"]);

        let answer = answer(&[2, 0, 0x00], "server", &shared);

        assert_eq!(status(&answer), 0x0400);
        assert_eq!(&answer[4..8], &0u32.to_be_bytes());
    }

    #[test]
    fn a_request_in_an_unknown_version_is_rejected_with_a_known_version() {
        let shared = FakeShared::new(&["HP LaserJet"]);
        let mut bytes = request(OPERATION_GET_PRINTERS, &[]);
        bytes[0] = 3;

        let answer = answer(&bytes, "server", &shared);

        assert_eq!(status(&answer), 0x0503);
        assert_eq!(&answer[0..2], &[1, 1]);
    }

    #[test]
    fn the_advertised_uri_round_trips_through_the_lookup() {
        let shared = FakeShared::new(&["Queue#1/2"]);

        let listing = answer(&request(OPERATION_GET_PRINTERS, &[]), "server", &shared);
        let uri = values_of(&listing, b"printer-uri-supported")
            .into_iter()
            .next()
            .expect("a printer uri");

        let answer = answer(
            &request(OPERATION_GET_PRINTER_ATTRIBUTES, &[("printer-uri", &uri)]),
            "server",
            &shared,
        );

        assert_eq!(status(&answer), 0x0000);
    }

    #[test]
    fn a_printer_name_with_slashes_is_looked_up_by_name() {
        let shared = FakeShared::new(&["Office/Floor2/Printer"]);
        let answer = answer(
            &request(
                OPERATION_GET_PRINTER_ATTRIBUTES,
                &[("printer-name", "Office/Floor2/Printer")],
            ),
            "server",
            &shared,
        );

        assert_eq!(status(&answer), 0x0000);
    }

    #[test]
    fn job_settings_parses_orientation_requested() {
        use crate::application::PrintOrientation;

        let mut out = vec![2, 0, 0, 2, 0, 0, 0, 1, 0x01];
        push(&mut out, 0x47, "attributes-charset", "utf-8");
        push(&mut out, 0x48, "attributes-natural-language", "en");
        out.push(0x02);
        // Add orientation-requested: enum tag 0x23, value 3 (Portrait)
        out.push(0x23);
        out.extend((b"orientation-requested".len() as u16).to_be_bytes());
        out.extend(b"orientation-requested");
        out.extend(4u16.to_be_bytes());
        out.extend(3i32.to_be_bytes());
        out.push(0x03);

        let req = Request::parse(&out).expect("valid request");
        let settings = job_settings(&req).expect("valid settings");
        assert_eq!(settings.orientation, Some(PrintOrientation::Portrait));

        // Test landscape (4)
        let mut out = vec![2, 0, 0, 2, 0, 0, 0, 1, 0x01];
        push(&mut out, 0x47, "attributes-charset", "utf-8");
        push(&mut out, 0x48, "attributes-natural-language", "en");
        out.push(0x02);
        out.push(0x23);
        out.extend((b"orientation-requested".len() as u16).to_be_bytes());
        out.extend(b"orientation-requested");
        out.extend(4u16.to_be_bytes());
        out.extend(4i32.to_be_bytes());
        out.push(0x03);

        let req = Request::parse(&out).expect("valid request");
        let settings = job_settings(&req).expect("valid settings");
        assert_eq!(settings.orientation, Some(PrintOrientation::Landscape));

        // Test unsupported orientation (e.g. 5)
        let mut out = vec![2, 0, 0, 2, 0, 0, 0, 1, 0x01];
        push(&mut out, 0x47, "attributes-charset", "utf-8");
        push(&mut out, 0x48, "attributes-natural-language", "en");
        out.push(0x02);
        out.push(0x23);
        out.extend((b"orientation-requested".len() as u16).to_be_bytes());
        out.extend(b"orientation-requested");
        out.extend(4u16.to_be_bytes());
        out.extend(5i32.to_be_bytes());
        out.push(0x03);

        let req = Request::parse(&out).expect("valid request");
        assert_eq!(
            job_settings(&req),
            Err(Status::AttributesOrValuesNotSupported)
        );
    }

    fn push_integer(out: &mut Vec<u8>, tag: u8, name: &str, value: i32) {
        out.push(tag);
        out.extend((name.len() as u16).to_be_bytes());
        out.extend(name.as_bytes());
        out.extend(4u16.to_be_bytes());
        out.extend(value.to_be_bytes());
    }

    fn job_request_with(text_attrs: &[(&str, &str)], int_attrs: &[(&str, u8, i32)]) -> Vec<u8> {
        let mut out = vec![2, 0, 0, 2, 0, 0, 0, 1, 0x01];
        push(&mut out, 0x47, "attributes-charset", "utf-8");
        push(&mut out, 0x48, "attributes-natural-language", "en");
        out.push(0x02);
        for (name, val) in text_attrs {
            push(&mut out, 0x44, name, val);
        }
        for (name, tag, val) in int_attrs {
            push_integer(&mut out, *tag, name, *val);
        }
        out.push(0x03);
        out
    }

    #[test]
    fn job_settings_parses_color_modes() {
        for (mode_str, expected) in [
            ("color", Some(true)),
            ("monochrome", Some(false)),
            ("bi-level", Some(false)),
        ] {
            let bytes = job_request_with(&[("print-color-mode", mode_str)], &[]);
            let req = Request::parse(&bytes).expect("valid request");
            let settings = job_settings(&req).expect("valid settings");
            assert_eq!(settings.color, expected, "mode: {mode_str}");
        }

        let bytes = job_request_with(&[("print-color-mode", "sepia")], &[]);
        let req = Request::parse(&bytes).expect("valid request");
        assert_eq!(
            job_settings(&req),
            Err(Status::AttributesOrValuesNotSupported)
        );
    }

    #[test]
    fn job_settings_parses_duplex_modes() {
        for (sides_str, expected) in [
            ("one-sided", Some(DuplexMode::Simplex)),
            ("two-sided-long-edge", Some(DuplexMode::LongEdge)),
            ("two-sided-short-edge", Some(DuplexMode::ShortEdge)),
        ] {
            let bytes = job_request_with(&[("sides", sides_str)], &[]);
            let req = Request::parse(&bytes).expect("valid request");
            let settings = job_settings(&req).expect("valid settings");
            assert_eq!(settings.duplex, expected, "sides: {sides_str}");
        }

        let bytes = job_request_with(&[("sides", "two-sided-tumble")], &[]);
        let req = Request::parse(&bytes).expect("valid request");
        assert_eq!(
            job_settings(&req),
            Err(Status::AttributesOrValuesNotSupported)
        );
    }

    #[test]
    fn job_settings_parses_copies_and_rejects_out_of_range() {
        for (count, expected) in [(1, Some(1)), (5, Some(5)), (999, Some(999))] {
            let bytes = job_request_with(&[], &[("copies", 0x21, count)]);
            let req = Request::parse(&bytes).expect("valid request");
            let settings = job_settings(&req).expect("valid settings");
            assert_eq!(settings.copies, expected, "copies: {count}");
        }

        for invalid in [0, 1000, -1] {
            let bytes = job_request_with(&[], &[("copies", 0x21, invalid)]);
            let req = Request::parse(&bytes).expect("valid request");
            assert_eq!(
                job_settings(&req),
                Err(Status::AttributesOrValuesNotSupported),
                "invalid copies: {invalid}"
            );
        }
    }
}
