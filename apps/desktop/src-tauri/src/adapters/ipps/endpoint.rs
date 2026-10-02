//! The IPP operations the sharing endpoint answers, independent of how the request arrived.
//!
//! Query operations are public; Print-Job requires an authorized Network Channel and is submitted
//! only to a queue in the current shared selection.

use crate::application::SharedPrinterSource;
use crate::domain::{ErrorCode, PrinterName};

use super::protocol::{
    job_response, percent_decode, percent_encode, response, PrinterEntry, Request, Status,
    IPP_VERSION_1_1, OPERATION_GET_PRINTERS, OPERATION_GET_PRINTER_ATTRIBUTES, OPERATION_PRINT_JOB,
    OPERATION_VALIDATE_JOB,
};

use crate::adapters::ipps::NetworkChannel;
use crate::application::{DuplexMode, PrintJob, PrintJobSubmitter, PrintSettings};

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

/// Processes Print-Job with the authorization and queue-submission ports.
pub async fn answer_job(
    bytes: Vec<u8>,
    host: &str,
    shared: &dyn SharedPrinterSource,
    channel: &NetworkChannel,
    submitter: &dyn PrintJobSubmitter,
) -> Vec<u8> {
    let request = match Request::parse(&bytes) {
        Ok(request) => request,
        Err(_) => return response(0, IPP_VERSION_1_1, Status::BadRequest, &[]),
    };
    let request_id = request.request_id();
    let version = request.response_version();
    if !request.version_is_supported() {
        return response(request_id, version, Status::VersionNotSupported, &[]);
    }
    let operation = request.operation();
    if operation != OPERATION_PRINT_JOB && operation != OPERATION_VALIDATE_JOB {
        return answer_with_job_status(
            &bytes,
            host,
            shared,
            channel.is_configured() && submitter.is_available(),
        );
    }

    let Some(candidate) = request.value("network-channel") else {
        return response(request_id, version, Status::NotAuthorized, &[]);
    };
    if !channel.authorizes(candidate) {
        return response(request_id, version, Status::NotAuthorized, &[]);
    }
    if !submitter.is_available() {
        return response(request_id, version, Status::NotAcceptingJobs, &[]);
    }
    let shared_printers = shared.shared_printers();
    let selected = request
        .value("printer-uri")
        .and_then(|uri| find_by_uri(&shared_printers, host, uri))
        .cloned();
    let Some(printer) = selected else {
        return response(request_id, version, Status::NotFound, &[]);
    };

    if let Some(document_format) = request.value("document-format") {
        if !matches!(
            document_format.to_ascii_lowercase().as_str(),
            "application/octet-stream"
                | "image/pwg-raster"
                | "application/pdf"
                | "application/pclm"
                | "application/oxps"
        ) {
            return response(request_id, version, Status::DocumentFormatNotSupported, &[]);
        }
    } else if operation == OPERATION_PRINT_JOB {
        return response(request_id, version, Status::BadRequest, &[]);
    }

    let settings = match job_settings(&request) {
        Ok(settings) => settings,
        Err(status) => return response(request_id, version, status, &[]),
    };

    if operation == OPERATION_VALIDATE_JOB {
        return response(request_id, version, Status::Ok, &[]);
    }

    if request.document().is_empty() {
        return response(request_id, version, Status::BadRequest, &[]);
    }

    let document_start = request.document_start();
    drop(request);
    let job = PrintJob::from_ipp_body(bytes, document_start, settings);
    match submitter.submit(&printer, job).await {
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
            response(request_id, version, status, &[])
        }
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
    Ok(PrintSettings {
        media,
        color,
        duplex,
        copies,
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
    let Some((authority, path)) = ipps_authority_and_path(requested) else {
        trace_issue34("server-uri=malformed");
        return None;
    };
    if !authority.eq_ignore_ascii_case(host) {
        trace_issue34("server-uri-authority=mismatch");
        return None;
    }
    let Some(encoded_name) = path.strip_prefix("ipp/print/") else {
        trace_issue34("server-uri-path=mismatch");
        return None;
    };
    let decoded_name = percent_decode(encoded_name);
    let Some(printer) = find_by_name(shared, &decoded_name) else {
        trace_issue34("server-printer=not-shared");
        return None;
    };
    if decoded_name != printer.as_str() {
        trace_issue34("server-printer-case=normalized");
    }
    trace_issue34("server-lookup=matched");
    Some(printer)
}

fn trace_issue34(message: &str) {
    if std::env::var_os("SHAPRINT_ISSUE34_IPP_TRACE").is_some() {
        eprintln!("[DEBUG-34IPP] {message}");
    }
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
            vec![
                "image/pwg-raster",
                "application/oxps",
                "application/pdf",
                "application/PCLm",
                "application/octet-stream"
            ]
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
}
