//! Observable HTTP/IPPS request seam for authorized Print-Job submissions.

mod support;

use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use shaprint_desktop::adapters::ipps::{answer_request_with_jobs, IppsServer, NetworkChannel};
use shaprint_desktop::adapters::ServerIdentity;
use shaprint_desktop::application::{
    DuplexMode, PrintFailures, PrintJob, PrintJobSubmitter, PrintSettings, RuntimeCoordinator,
    SharedPrinterSource, Sharing,
};
use shaprint_desktop::domain::{AppError, ErrorCode, JobPath, PrinterName, ServiceId};
use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt};

const OFFICE_PRINTER_URI: &str = "ipps://server:8631/ipp/print/Office%20Printer";

struct Shared(Vec<PrinterName>);

impl SharedPrinterSource for Shared {
    fn shared_printers(&self) -> Vec<PrinterName> {
        self.0.clone()
    }
}

#[derive(Default)]
struct FakeSubmitter(Mutex<Vec<(String, PrintJob)>>);

#[async_trait]
impl PrintJobSubmitter for FakeSubmitter {
    async fn submit(&self, printer: &PrinterName, job: PrintJob) -> Result<u32, AppError> {
        self.0
            .lock()
            .map_err(|_| AppError::internal("fake printer unavailable"))?
            .push((printer.as_str().to_owned(), job));
        Ok(17)
    }
}

/// A spooler that refuses jobs, so both server submission failure paths are observable.
struct RefusingSubmitter {
    code: ErrorCode,
    available: bool,
}

#[async_trait]
impl PrintJobSubmitter for RefusingSubmitter {
    fn is_available(&self) -> bool {
        self.available
    }

    async fn submit(&self, _printer: &PrinterName, _job: PrintJob) -> Result<u32, AppError> {
        Err(AppError::new(
            self.code,
            "the Windows spooler refused the job",
        ))
    }
}

fn channel_secret() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("{timestamp}")
}

fn text_attribute(body: &mut Vec<u8>, tag: u8, name: &str, value: &str) {
    body.push(tag);
    body.extend((name.len() as u16).to_be_bytes());
    body.extend(name.as_bytes());
    body.extend((value.len() as u16).to_be_bytes());
    body.extend(value.as_bytes());
}

fn integer_attribute(body: &mut Vec<u8>, name: &str, value: i32) {
    body.push(0x21);
    body.extend((name.len() as u16).to_be_bytes());
    body.extend(name.as_bytes());
    body.extend(4u16.to_be_bytes());
    body.extend(value.to_be_bytes());
}

fn print_job(channel: Option<&str>, printer: &str, sides: &str, document: &[u8]) -> Vec<u8> {
    print_job_with_format(channel, printer, "image/pwg-raster", sides, document)
}

fn print_job_with_format(
    channel: Option<&str>,
    printer: &str,
    document_format: &str,
    sides: &str,
    document: &[u8],
) -> Vec<u8> {
    let mut body = vec![2, 0, 0, 2, 0, 0, 0, 9, 1];
    text_attribute(&mut body, 0x47, "attributes-charset", "utf-8");
    text_attribute(&mut body, 0x48, "attributes-natural-language", "en");
    text_attribute(&mut body, 0x45, "printer-uri", printer);
    text_attribute(&mut body, 0x49, "document-format", document_format);
    if let Some(channel) = channel {
        text_attribute(&mut body, 0x41, "network-channel", channel);
    }
    body.push(0x02);
    text_attribute(&mut body, 0x44, "media", "iso_a4_210x297mm");
    text_attribute(&mut body, 0x44, "print-color-mode", "color");
    text_attribute(&mut body, 0x44, "sides", sides);
    integer_attribute(&mut body, "copies", 2);
    body.push(3);
    body.extend(document);
    body
}

fn print_job_with_options(
    channel: Option<&str>,
    printer: &str,
    color_mode: Option<&str>,
    sides: Option<&str>,
    copies: Option<i32>,
    document: &[u8],
) -> Vec<u8> {
    let mut body = vec![2, 0, 0, 2, 0, 0, 0, 9, 1];
    text_attribute(&mut body, 0x47, "attributes-charset", "utf-8");
    text_attribute(&mut body, 0x48, "attributes-natural-language", "en");
    text_attribute(&mut body, 0x45, "printer-uri", printer);
    text_attribute(&mut body, 0x49, "document-format", "image/pwg-raster");
    if let Some(channel) = channel {
        text_attribute(&mut body, 0x41, "network-channel", channel);
    }
    body.push(0x02);
    text_attribute(&mut body, 0x44, "media", "iso_a4_210x297mm");
    if let Some(mode) = color_mode {
        text_attribute(&mut body, 0x44, "print-color-mode", mode);
    }
    if let Some(s) = sides {
        text_attribute(&mut body, 0x44, "sides", s);
    }
    if let Some(c) = copies {
        integer_attribute(&mut body, "copies", c);
    }
    body.push(3);
    body.extend(document);
    body
}

fn validate_job(channel: Option<&str>, printer: &str) -> Vec<u8> {
    let mut body = vec![2, 0, 0, 4, 0, 0, 0, 9, 1];
    text_attribute(&mut body, 0x47, "attributes-charset", "utf-8");
    text_attribute(&mut body, 0x48, "attributes-natural-language", "en");
    text_attribute(&mut body, 0x45, "printer-uri", printer);
    if let Some(channel) = channel {
        text_attribute(&mut body, 0x41, "network-channel", channel);
    }
    body.push(0x02);
    text_attribute(&mut body, 0x44, "media", "iso_a4_210x297mm");
    text_attribute(&mut body, 0x44, "print-color-mode", "color");
    text_attribute(&mut body, 0x44, "sides", "one-sided");
    body.push(3);
    body
}
#[tokio::test]
async fn formats_without_a_renderer_are_rejected_before_queue_submission() {
    let shared = Shared(vec![
        PrinterName::parse("Office Printer").expect("valid name")
    ]);
    let submitter = FakeSubmitter::default();
    let channel = NetworkChannel::in_memory();
    let secret = channel_secret();
    channel
        .configure(&secret)
        .await
        .expect("configures channel");

    for format in [
        "application/pdf",
        "application/oxps",
        "application/pclm",
        "application/octet-stream",
    ] {
        let response = send(
            &print_job_with_format(Some(&secret), OFFICE_PRINTER_URI, format, "one-sided", &[0]),
            &shared,
            &channel,
            &submitter,
        )
        .await;
        assert_eq!(
            ipp_status(&response),
            0x040A,
            "unsupported format reached the RAW submission path: {format}"
        );
    }
    assert!(submitter.0.lock().expect("reads submissions").is_empty());
}

#[tokio::test]
async fn unsupported_document_format_is_rejected_before_queue_submission() {
    let shared = Shared(vec![
        PrinterName::parse("Office Printer").expect("valid name")
    ]);
    let submitter = FakeSubmitter::default();
    let channel = NetworkChannel::in_memory();
    let secret = channel_secret();
    channel
        .configure(&secret)
        .await
        .expect("configures channel");

    let response = send(
        &print_job_with_format(
            Some(&secret),
            OFFICE_PRINTER_URI,
            "text/html",
            "one-sided",
            b"document",
        ),
        &shared,
        &channel,
        &submitter,
    )
    .await;

    assert_eq!(ipp_status(&response), 0x040A);
    assert!(submitter.0.lock().expect("reads submissions").is_empty());
}

#[tokio::test]
async fn printer_uri_must_identify_a_queue_advertised_by_this_server() {
    let shared = Shared(vec![
        PrinterName::parse("Office Printer").expect("valid name")
    ]);
    let submitter = FakeSubmitter::default();
    let channel = NetworkChannel::in_memory();
    let secret = channel_secret();
    channel
        .configure(&secret)
        .await
        .expect("configures channel");

    for printer_uri in [
        "ipps://other-host/ipp/print/Office%20Printer",
        "ipps://server:8631/unshared/Office%20Printer",
        "ipp://server:8631/ipp/print/Office%20Printer",
    ] {
        let response = send(
            &print_job(Some(&secret), printer_uri, "one-sided", b"document"),
            &shared,
            &channel,
            &submitter,
        )
        .await;

        assert_eq!(ipp_status(&response), 0x0406, "{printer_uri}");
    }
    assert!(submitter.0.lock().expect("reads submissions").is_empty());
}

async fn send(
    body: &[u8],
    shared: &Shared,
    channel: &NetworkChannel,
    printer: &FakeSubmitter,
) -> Vec<u8> {
    let failures = PrintFailures::new();
    send_with_failures(body, shared, channel, printer, &failures).await
}

async fn send_with_failures(
    body: &[u8],
    shared: &Shared,
    channel: &NetworkChannel,
    printer: &dyn PrintJobSubmitter,
    failures: &PrintFailures,
) -> Vec<u8> {
    let (client, server) = duplex(16 * 1024);
    let answering = answer_request_with_jobs(server, shared, channel, printer, failures);
    let exchange = async move {
        let mut client = client;
        let head = format!("POST /ipp/print HTTP/1.1\r\nHost: server:8631\r\nContent-Type: application/ipp\r\nContent-Length: {}\r\n\r\n", body.len());
        client
            .write_all(head.as_bytes())
            .await
            .expect("writes request head");
        client.write_all(body).await.expect("writes IPP request");
        let mut response = Vec::new();
        client
            .read_to_end(&mut response)
            .await
            .expect("reads IPP response");
        response
    };
    let (response, answer_result) = tokio::join!(exchange, answering);
    answer_result.expect("request is answered");
    response
}

fn ipp_body(response: &[u8]) -> &[u8] {
    response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|position| &response[position + 4..])
        .unwrap_or(response)
}

fn ipp_status(response: &[u8]) -> u16 {
    let body = ipp_body(response);
    u16::from_be_bytes(
        body.get(2..4)
            .expect("contains the IPP response status")
            .try_into()
            .expect("IPP status has two bytes"),
    )
}

fn ipp_integer_attribute(response: &[u8], wanted: &str) -> Option<i32> {
    let body = ipp_body(response);
    if body.len() < 8 {
        return None;
    }
    let mut position = 8;
    while position < body.len() {
        let value_tag = *body.get(position)?;
        position += 1;
        if value_tag == 0x03 {
            break;
        }
        if (0x01..=0x05).contains(&value_tag) {
            continue;
        }
        let name_length = usize::from(u16::from_be_bytes(
            body.get(position..position.checked_add(2)?)?
                .try_into()
                .ok()?,
        ));
        position += 2;
        let name_end = position.checked_add(name_length)?;
        let name = std::str::from_utf8(body.get(position..name_end)?).ok()?;
        position = name_end;
        let value_length = usize::from(u16::from_be_bytes(
            body.get(position..position.checked_add(2)?)?
                .try_into()
                .ok()?,
        ));
        position += 2;
        let value_end = position.checked_add(value_length)?;
        let value = body.get(position..value_end)?;
        position = value_end;
        if name == wanted {
            return Some(i32::from_be_bytes(value.try_into().ok()?));
        }
    }
    None
}

struct LiveServer {
    runtime: RuntimeCoordinator,
    endpoint: Arc<IppsServer>,
    client: support::IppClient,
}

impl LiveServer {
    async fn start(channel: Arc<NetworkChannel>, submitter: Arc<FakeSubmitter>) -> Self {
        let sharing = Arc::new(Sharing::new(support::FakeCatalog::new(&["Office Printer"])));
        sharing
            .set_shared(vec![
                PrinterName::parse("Office Printer").expect("valid printer")
            ])
            .await
            .expect("selects printer");
        let identity = Arc::new(ServerIdentity::generate().expect("generates identity"));
        let endpoint = Arc::new(IppsServer::new(
            0,
            identity,
            channel,
            submitter,
            Arc::new(PrintFailures::new()),
        ));
        let runtime = RuntimeCoordinator::new(vec![Arc::new(support::sharing_service(
            Arc::clone(&sharing),
            Arc::clone(&endpoint),
        ))]);
        runtime
            .start(ServiceId::ServerSharing)
            .await
            .expect("starts IPPS sharing");
        let address = endpoint.bound_address().expect("binds IPPS listener");
        let client = support::IppClient::new(address.port(), endpoint.fingerprint().to_string());
        Self {
            runtime,
            endpoint,
            client,
        }
    }
}

#[tokio::test]
async fn authorized_print_job_reaches_the_selected_queue_with_document_and_settings() {
    let channel = Arc::new(NetworkChannel::in_memory());
    let secret = channel_secret();
    channel
        .configure(&secret)
        .await
        .expect("configures channel");
    let submitter = Arc::new(FakeSubmitter::default());
    let server = LiveServer::start(Arc::clone(&channel), Arc::clone(&submitter)).await;
    let document = b"opaque printer document bytes";
    let address = server.endpoint.bound_address().expect("listens");
    let printer_uri = format!(
        "ipps://127.0.0.1:{}/ipp/print/Office%20Printer",
        address.port()
    );
    let request = print_job(Some(&secret), &printer_uri, "two-sided-long-edge", document);

    let (http_status, response) = server
        .client
        .post(&request)
        .await
        .expect("submits over IPPS");

    assert_eq!(http_status, 200);
    assert_eq!(ipp_status(&response), 0x0000);
    assert_eq!(ipp_integer_attribute(&response, "job-id"), Some(17));
    {
        let submitted = submitter.0.lock().expect("reads fake submissions");
        assert_eq!(submitted.len(), 1);
        assert_eq!(submitted[0].0, "Office Printer");
        assert_eq!(submitted[0].1.document(), document);
        assert_eq!(
            submitted[0].1.settings(),
            &PrintSettings {
                media: Some("iso_a4_210x297mm".to_owned()),
                color: Some(true),
                duplex: Some(DuplexMode::LongEdge),
                copies: Some(2),
                orientation: None,
            },
        );
    }
    server.runtime.shutdown().await.expect("stops IPPS sharing");
}
#[tokio::test]
async fn short_edge_duplex_reaches_the_printer_adapter_unchanged() {
    let shared = Shared(vec![
        PrinterName::parse("Office Printer").expect("valid name")
    ]);
    let submitter = FakeSubmitter::default();
    let channel = NetworkChannel::in_memory();
    let secret = channel_secret();
    channel
        .configure(&secret)
        .await
        .expect("configures channel");

    let response = send(
        &print_job(
            Some(&secret),
            OFFICE_PRINTER_URI,
            "two-sided-short-edge",
            b"document",
        ),
        &shared,
        &channel,
        &submitter,
    )
    .await;

    assert_eq!(ipp_status(&response), 0x0000);
    assert_eq!(
        submitter.0.lock().expect("reads fake submissions")[0]
            .1
            .settings()
            .duplex,
        Some(DuplexMode::ShortEdge),
    );
}

#[tokio::test]
async fn landscape_orientation_and_legal_media_reach_the_printer_adapter_unchanged() {
    let shared = Shared(vec![
        PrinterName::parse("Office Printer").expect("valid name")
    ]);
    let submitter = FakeSubmitter::default();
    let channel = NetworkChannel::in_memory();
    let secret = channel_secret();
    channel
        .configure(&secret)
        .await
        .expect("configures channel");

    let mut body = vec![2, 0, 0, 2, 0, 0, 0, 9, 1];
    text_attribute(&mut body, 0x47, "attributes-charset", "utf-8");
    text_attribute(&mut body, 0x48, "attributes-natural-language", "en");
    text_attribute(&mut body, 0x45, "printer-uri", OFFICE_PRINTER_URI);
    text_attribute(&mut body, 0x49, "document-format", "image/pwg-raster");
    text_attribute(&mut body, 0x41, "network-channel", &secret);
    body.push(0x02);
    text_attribute(&mut body, 0x44, "media", "na_legal_8.5x14in");
    integer_attribute(&mut body, "orientation-requested", 4);
    body.push(3);
    body.extend(b"legal landscape document");

    let response = send(&body, &shared, &channel, &submitter).await;

    assert_eq!(ipp_status(&response), 0x0000);
    let submissions = submitter.0.lock().expect("reads fake submissions");
    assert_eq!(submissions.len(), 1);
    assert_eq!(
        submissions[0].1.settings().media.as_deref(),
        Some("na_legal_8.5x14in")
    );
    assert_eq!(
        submissions[0].1.settings().orientation,
        Some(shaprint_desktop::application::PrintOrientation::Landscape)
    );
}

#[tokio::test]
async fn monochrome_and_bilevel_reach_the_printer_adapter_as_non_color() {
    let shared = Shared(vec![
        PrinterName::parse("Office Printer").expect("valid name")
    ]);
    let submitter = FakeSubmitter::default();
    let channel = NetworkChannel::in_memory();
    let secret = channel_secret();
    channel
        .configure(&secret)
        .await
        .expect("configures channel");

    for (color_mode, duplex_mode, copies) in [
        ("monochrome", "one-sided", 5),
        ("bi-level", "two-sided-long-edge", 1),
    ] {
        let req = print_job_with_options(
            Some(&secret),
            OFFICE_PRINTER_URI,
            Some(color_mode),
            Some(duplex_mode),
            Some(copies),
            b"document",
        );
        let response = send(&req, &shared, &channel, &submitter).await;
        assert_eq!(ipp_status(&response), 0x0000, "color_mode: {color_mode}");
    }

    let submissions = submitter.0.lock().expect("reads submissions");
    assert_eq!(submissions.len(), 2);
    assert_eq!(submissions[0].1.settings().color, Some(false));
    assert_eq!(
        submissions[0].1.settings().duplex,
        Some(DuplexMode::Simplex)
    );
    assert_eq!(submissions[0].1.settings().copies, Some(5));

    assert_eq!(submissions[1].1.settings().color, Some(false));
    assert_eq!(
        submissions[1].1.settings().duplex,
        Some(DuplexMode::LongEdge)
    );
    assert_eq!(submissions[1].1.settings().copies, Some(1));
}

#[tokio::test]
async fn unsupported_color_mode_is_rejected_without_submitting_job() {
    let shared = Shared(vec![
        PrinterName::parse("Office Printer").expect("valid name")
    ]);
    let submitter = FakeSubmitter::default();
    let channel = NetworkChannel::in_memory();
    let secret = channel_secret();
    channel
        .configure(&secret)
        .await
        .expect("configures channel");

    let req = print_job_with_options(
        Some(&secret),
        OFFICE_PRINTER_URI,
        Some("sepia"),
        Some("one-sided"),
        Some(1),
        b"document",
    );
    let response = send(&req, &shared, &channel, &submitter).await;

    // AttributesOrValuesNotSupported = 0x040B
    assert_eq!(ipp_status(&response), 0x040B);
    assert!(submitter.0.lock().expect("reads submissions").is_empty());
}

#[tokio::test]
async fn unsupported_duplex_mode_is_rejected_without_submitting_job() {
    let shared = Shared(vec![
        PrinterName::parse("Office Printer").expect("valid name")
    ]);
    let submitter = FakeSubmitter::default();
    let channel = NetworkChannel::in_memory();
    let secret = channel_secret();
    channel
        .configure(&secret)
        .await
        .expect("configures channel");

    let req = print_job_with_options(
        Some(&secret),
        OFFICE_PRINTER_URI,
        Some("color"),
        Some("two-sided-tumble"),
        Some(1),
        b"document",
    );
    let response = send(&req, &shared, &channel, &submitter).await;

    assert_eq!(ipp_status(&response), 0x040B);
    assert!(submitter.0.lock().expect("reads submissions").is_empty());
}

#[tokio::test]
async fn unsupported_copies_count_is_rejected_without_submitting_job() {
    let shared = Shared(vec![
        PrinterName::parse("Office Printer").expect("valid name")
    ]);
    let submitter = FakeSubmitter::default();
    let channel = NetworkChannel::in_memory();
    let secret = channel_secret();
    channel
        .configure(&secret)
        .await
        .expect("configures channel");

    for invalid in [0, 1000] {
        let req = print_job_with_options(
            Some(&secret),
            OFFICE_PRINTER_URI,
            Some("color"),
            Some("one-sided"),
            Some(invalid),
            b"document",
        );
        let response = send(&req, &shared, &channel, &submitter).await;

        assert_eq!(ipp_status(&response), 0x040B, "copies: {invalid}");
        assert!(submitter.0.lock().expect("reads submissions").is_empty());
    }
}

#[tokio::test]
async fn missing_or_wrong_channel_never_reaches_a_queue() {
    let shared = Shared(vec![
        PrinterName::parse("Office Printer").expect("valid name")
    ]);
    let submitter = FakeSubmitter::default();
    let channel = NetworkChannel::in_memory();
    let secret = channel_secret();
    channel
        .configure(&secret)
        .await
        .expect("configures channel");

    let wrong_secret = channel_secret();
    for supplied in [None, Some(wrong_secret.as_str())] {
        let response = send(
            &print_job(supplied, OFFICE_PRINTER_URI, "one-sided", b"document"),
            &shared,
            &channel,
            &submitter,
        )
        .await;
        assert_eq!(ipp_status(&response), 0x0403);
    }
    let unconfigured = NetworkChannel::in_memory();
    let attempted_secret = channel_secret();
    let response = send(
        &print_job(
            Some(&attempted_secret),
            OFFICE_PRINTER_URI,
            "one-sided",
            b"document",
        ),
        &shared,
        &unconfigured,
        &submitter,
    )
    .await;
    assert_eq!(ipp_status(&response), 0x0403);
    assert!(submitter.0.lock().expect("reads submissions").is_empty());
}

#[tokio::test]
async fn unshared_queue_never_reaches_a_queue_submitter() {
    let shared = Shared(vec![
        PrinterName::parse("Office Printer").expect("valid name")
    ]);
    let submitter = FakeSubmitter::default();
    let channel = NetworkChannel::in_memory();
    let secret = channel_secret();
    channel
        .configure(&secret)
        .await
        .expect("configures channel");

    let response = send(
        &print_job(
            Some(&secret),
            "ipps://server:8631/ipp/print/Unshared%20Queue",
            "one-sided",
            b"document",
        ),
        &shared,
        &channel,
        &submitter,
    )
    .await;

    assert_eq!(ipp_status(&response), 0x0406);
    assert!(submitter.0.lock().expect("reads submissions").is_empty());
}

#[tokio::test]
async fn channel_verifier_survives_reopen_without_persisting_plaintext() {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let directory = std::env::temp_dir().join(format!("shaprint-channel-{timestamp}"));
    let second_directory = directory.with_extension("second");
    let secret = channel_secret();

    {
        let channel = NetworkChannel::open(directory.clone()).expect("opens verifier store");
        assert!(channel.configure(&secret).await.expect("persists verifier"));
    }
    let reopened = NetworkChannel::open(directory.clone()).expect("reopens verifier store");
    assert!(reopened.is_configured());
    assert!(reopened.authorizes(&secret));
    #[cfg(windows)]
    assert_eq!(
        reopened.client_credential().as_deref(),
        Some(secret.as_str())
    );
    let verifier =
        std::fs::read(directory.join("network-channel-verifier.json")).expect("reads verifier");
    assert!(!verifier
        .windows(secret.len())
        .any(|window| window == secret.as_bytes()));
    let record: serde_json::Value =
        serde_json::from_slice(&verifier).expect("verifier record is JSON");
    let salt = record["salt"]
        .as_str()
        .expect("verifier has a per-configuration salt");
    assert_eq!(hex::decode(salt).expect("salt is hex").len(), 16);
    let digest = record["sha256"]
        .as_str()
        .expect("verifier has a digest")
        .to_owned();

    let second = NetworkChannel::open(second_directory.clone()).expect("opens second store");
    assert!(second
        .configure(&secret)
        .await
        .expect("persists second verifier"));
    let second_record: serde_json::Value = serde_json::from_slice(
        &std::fs::read(second_directory.join("network-channel-verifier.json"))
            .expect("reads second verifier"),
    )
    .expect("second verifier record is JSON");
    assert_ne!(
        salt,
        second_record["salt"]
            .as_str()
            .expect("second verifier has a salt")
    );
    assert_ne!(
        digest,
        second_record["sha256"]
            .as_str()
            .expect("second verifier has a digest")
    );
    let _ = std::fs::remove_dir_all(directory);
    let _ = std::fs::remove_dir_all(second_directory);
}

#[tokio::test]
async fn legacy_unsalted_channel_verifier_requires_reconfiguration() {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let directory = std::env::temp_dir().join(format!("shaprint-channel-legacy-{timestamp}"));
    std::fs::create_dir_all(&directory).expect("creates legacy verifier directory");
    std::fs::write(
        directory.join("network-channel-verifier.json"),
        r#"{"sha256":"0000000000000000000000000000000000000000000000000000000000000000"}"#,
    )
    .expect("writes legacy verifier");

    let channel = NetworkChannel::open(directory.clone())
        .expect("loads legacy verifier as requiring configuration");
    assert!(!channel.is_configured());
    assert!(!channel.authorizes("legacy-channel"));
    let replacement = channel_secret();
    assert!(channel
        .configure(&replacement)
        .await
        .expect("replaces legacy verifier"));
    let reopened = NetworkChannel::open(directory.clone()).expect("reopens salted verifier");
    assert!(reopened.is_configured());
    assert!(reopened.authorizes(&replacement));
    let _ = std::fs::remove_dir_all(directory);
}

#[tokio::test]
async fn stopping_sharing_refuses_new_jobs_before_the_printer_adapter() {
    let channel = Arc::new(NetworkChannel::in_memory());
    let secret = channel_secret();
    channel
        .configure(&secret)
        .await
        .expect("configures channel");
    let submitter = Arc::new(FakeSubmitter::default());
    let server = LiveServer::start(Arc::clone(&channel), Arc::clone(&submitter)).await;
    let address = server.endpoint.bound_address().expect("listens");
    let printer_uri = format!(
        "ipps://127.0.0.1:{}/ipp/print/Office%20Printer",
        address.port()
    );
    let request = print_job(Some(&secret), &printer_uri, "one-sided", b"document");

    let (status, response) = server.client.post(&request).await.expect("reaches server");
    assert_eq!(status, 200);
    assert_eq!(ipp_status(&response), 0x0000);
    assert_eq!(submitter.0.lock().expect("reads submissions").len(), 1);

    server
        .runtime
        .stop(ServiceId::ServerSharing)
        .await
        .expect("stops sharing");
    assert!(server.client.post(&request).await.is_err());
    assert_eq!(submitter.0.lock().expect("reads submissions").len(), 1);
    server.runtime.shutdown().await.expect("shuts down runtime");
}

#[tokio::test]
async fn a_refused_queue_submission_is_reported_with_a_stable_code_and_no_channel_value() {
    let channel = Arc::new(NetworkChannel::in_memory());
    let secret = channel_secret();
    channel
        .configure(&secret)
        .await
        .expect("configures channel");
    let shared = Shared(vec![
        PrinterName::parse("Office Printer").expect("valid printer")
    ]);
    let failures = PrintFailures::new();
    let submitter = RefusingSubmitter {
        code: ErrorCode::Internal,
        available: true,
    };

    let response = send_with_failures(
        &print_job(Some(&secret), OFFICE_PRINTER_URI, "one-sided", b"document"),
        &shared,
        &channel,
        &submitter,
        &failures,
    )
    .await;

    assert_eq!(ipp_status(&response), 0x0500);
    let failure = failures.latest().expect("reports the failure");
    assert_eq!(failure.path(), JobPath::ServerSubmission);
    assert_eq!(failure.code(), ErrorCode::QueueUnavailable);
    assert_eq!(failure.code().as_str(), "queue-unavailable");
    assert_eq!(
        failure.message(),
        "The job for 'Office Printer' could not be submitted to the Windows printer queue."
    );
    assert!(failure.recovery().contains("printer"));

    // Neither the Network Channel nor the document may reach the user-facing record.
    assert!(!failure.message().contains(&secret));
    assert!(!failure.recovery().contains(&secret));
    assert!(!failure.message().contains("document"));
}

#[tokio::test]
async fn printer_settings_the_queue_rejects_are_reported_as_the_users_to_change() {
    let channel = Arc::new(NetworkChannel::in_memory());
    let secret = channel_secret();
    channel
        .configure(&secret)
        .await
        .expect("configures channel");
    let shared = Shared(vec![
        PrinterName::parse("Office Printer").expect("valid printer")
    ]);
    let failures = PrintFailures::new();
    let submitter = RefusingSubmitter {
        code: ErrorCode::InvalidInput,
        available: true,
    };

    let response = send_with_failures(
        &print_job(Some(&secret), OFFICE_PRINTER_URI, "one-sided", b"document"),
        &shared,
        &channel,
        &submitter,
        &failures,
    )
    .await;

    assert_eq!(ipp_status(&response), 0x040b);
    let failure = failures.latest().expect("reports the failure");
    assert_eq!(failure.code(), ErrorCode::InvalidInput);
    assert!(failure.recovery().contains("media size"));
}

#[tokio::test]
async fn a_server_with_no_working_spooler_reports_it_after_authorization() {
    let channel = Arc::new(NetworkChannel::in_memory());
    let secret = channel_secret();
    channel
        .configure(&secret)
        .await
        .expect("configures channel");
    let shared = Shared(vec![
        PrinterName::parse("Office Printer").expect("valid printer")
    ]);
    let failures = PrintFailures::new();
    let submitter = RefusingSubmitter {
        code: ErrorCode::Unsupported,
        available: false,
    };

    let response = send_with_failures(
        &print_job(Some(&secret), OFFICE_PRINTER_URI, "one-sided", b"document"),
        &shared,
        &channel,
        &submitter,
        &failures,
    )
    .await;

    assert_eq!(ipp_status(&response), 0x0508);
    let failure = failures.latest().expect("reports the failure");
    assert_eq!(failure.path(), JobPath::ServerSubmission);
    assert_eq!(failure.code(), ErrorCode::QueueUnavailable);
    assert!(failure.recovery().contains("printer"));
}

#[tokio::test]
async fn an_unauthorized_job_never_reaches_the_server_users_failure_report() {
    let channel = Arc::new(NetworkChannel::in_memory());
    channel
        .configure(&channel_secret())
        .await
        .expect("configures channel");
    let shared = Shared(vec![
        PrinterName::parse("Office Printer").expect("valid printer")
    ]);
    let failures = PrintFailures::new();
    let submitter = RefusingSubmitter {
        code: ErrorCode::Internal,
        available: true,
    };

    let response = send_with_failures(
        &print_job(
            Some("an-attackers-guess"),
            OFFICE_PRINTER_URI,
            "one-sided",
            b"document",
        ),
        &shared,
        &channel,
        &submitter,
        &failures,
    )
    .await;

    assert_eq!(ipp_status(&response), 0x0403);
    // Anyone can reach the endpoint, so a rejected channel must not be able to fill the server
    // user's screen with failures they cannot act on.
    assert_eq!(failures.latest(), None);
}

#[tokio::test]
async fn validate_job_with_correct_channel_succeeds_without_submitting_job() {
    let channel = Arc::new(NetworkChannel::in_memory());
    let secret = channel_secret();
    channel
        .configure(&secret)
        .await
        .expect("configures channel");
    let submitter = Arc::new(FakeSubmitter::default());
    let server = LiveServer::start(Arc::clone(&channel), Arc::clone(&submitter)).await;
    let address = server.endpoint.bound_address().expect("listens");
    let printer_uri = format!(
        "ipps://127.0.0.1:{}/ipp/print/Office%20Printer",
        address.port()
    );
    let request = validate_job(Some(&secret), &printer_uri);

    let (status, response) = server.client.post(&request).await.expect("reaches server");
    assert_eq!(status, 200);
    assert_eq!(ipp_status(&response), 0x0000);
    assert!(
        submitter.0.lock().expect("reads submissions").is_empty(),
        "Validate-Job must not submit a print job"
    );

    server.runtime.shutdown().await.expect("shuts down runtime");
}

#[tokio::test]
async fn validate_job_with_incorrect_channel_is_rejected() {
    let channel = Arc::new(NetworkChannel::in_memory());
    let secret = channel_secret();
    channel
        .configure(&secret)
        .await
        .expect("configures channel");
    let submitter = Arc::new(FakeSubmitter::default());
    let server = LiveServer::start(Arc::clone(&channel), Arc::clone(&submitter)).await;
    let address = server.endpoint.bound_address().expect("listens");
    let printer_uri = format!(
        "ipps://127.0.0.1:{}/ipp/print/Office%20Printer",
        address.port()
    );
    let request = validate_job(Some("wrong-secret"), &printer_uri);

    let (status, response) = server.client.post(&request).await.expect("reaches server");
    assert_eq!(status, 200);
    assert_eq!(ipp_status(&response), 0x0403); // NotAuthorized (RFC 8011 client-error-not-authorized)
    assert!(submitter.0.lock().expect("reads submissions").is_empty());

    server.runtime.shutdown().await.expect("shuts down runtime");
}
