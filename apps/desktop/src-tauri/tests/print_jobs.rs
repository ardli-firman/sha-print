//! Observable HTTP/IPPS request seam for authorized Print-Job submissions.

mod support;

use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use shaprint_desktop::adapters::ipps::{answer_request_with_jobs, IppsServer, NetworkChannel};
use shaprint_desktop::adapters::{ServerIdentity, ServerSharingService};
use shaprint_desktop::application::{
    DuplexMode, PrintJob, PrintJobSubmitter, PrintSettings, RuntimeCoordinator,
    SharedPrinterSource, Sharing,
};
use shaprint_desktop::domain::{AppError, PrinterName, ServiceId};
use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt};

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
    let mut body = vec![2, 0, 0, 2, 0, 0, 0, 9, 1];
    text_attribute(&mut body, 0x47, "attributes-charset", "utf-8");
    text_attribute(&mut body, 0x48, "attributes-natural-language", "en");
    text_attribute(&mut body, 0x45, "printer-uri", printer);
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

async fn send(
    body: &[u8],
    shared: &Shared,
    channel: &NetworkChannel,
    printer: &FakeSubmitter,
) -> Vec<u8> {
    let (client, server) = duplex(16 * 1024);
    let answering = answer_request_with_jobs(server, shared, channel, printer);
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
        let endpoint = Arc::new(IppsServer::new(0, identity, channel, submitter));
        let runtime = RuntimeCoordinator::new(vec![Arc::new(ServerSharingService::new(
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
    let printer_uri = format!("ipps://{address}/ipp/print/Office%20Printer");
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
            "Office Printer",
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
            &print_job(supplied, "Office Printer", "one-sided", b"document"),
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
            "Office Printer",
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
        &print_job(Some(&secret), "Unshared Queue", "one-sided", b"document"),
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
    let secret = channel_secret();

    {
        let channel = NetworkChannel::open(directory.clone()).expect("opens verifier store");
        assert!(channel.configure(&secret).await.expect("persists verifier"));
    }
    let reopened = NetworkChannel::open(directory.clone()).expect("reopens verifier store");
    assert!(reopened.is_configured());
    assert!(reopened.authorizes(&secret));
    let verifier =
        std::fs::read(directory.join("network-channel-verifier.json")).expect("reads verifier");
    assert!(!verifier
        .windows(secret.len())
        .any(|window| window == secret.as_bytes()));
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
    let printer_uri = format!("ipps://{address}/ipp/print/Office%20Printer");
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
