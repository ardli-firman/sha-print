//! The client print path: a local Windows IPP queue sends a job through the authenticated proxy,
//! which forwards it over approved IPPS to the server's selected shared printer.

mod support;

// `Command`, the two ports, and the tokio time helpers are only used by the Windows-only smoke
// test below, so they are gated with it instead of warning on every other platform.
#[cfg(windows)]
use std::process::Command;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use shaprint_desktop::adapters::ipps::{protocol, NetworkChannel};
use shaprint_desktop::adapters::{
    client_connections::ClientConnections, client_queue_uri, ClientProxyService, IppsServer,
    ServerIdentity,
};
use shaprint_desktop::application::{
    DuplexMode, PrintFailures, PrintJob, PrintJobSubmitter, RuntimeCoordinator, Sharing,
};
use shaprint_desktop::domain::{AppError, ErrorCode, JobPath, PrinterName, ServiceId};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
#[cfg(windows)]
use tokio::time::{timeout, Duration};

use support::{get_printer_attributes, temporary_directory, FakeCatalog};

const SERVER_QUEUE: &str = "Office Printer";
#[cfg(windows)]
const NATIVE_SMOKE_SERVER_PORT: u16 = 8631;
#[cfg(windows)]
const NATIVE_SMOKE_PROXY_PORT: u16 = 8632;

fn channel_secret() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is after the Unix epoch")
        .as_nanos()
        .to_string()
        + &NEXT.fetch_add(1, Ordering::Relaxed).to_string()
}

fn document_bytes() -> Vec<u8> {
    (0..256).map(|value| (value % 251) as u8).collect()
}

#[derive(Default)]
struct FakeSubmitter {
    jobs: Mutex<Vec<(String, PrintJob)>>,
    submitted: tokio::sync::Notify,
}

#[async_trait]
impl PrintJobSubmitter for FakeSubmitter {
    async fn submit(&self, printer: &PrinterName, job: PrintJob) -> Result<u32, AppError> {
        self.jobs
            .lock()
            .map_err(|_| AppError::internal("fake printer unavailable"))?
            .push((printer.as_str().to_owned(), job));
        self.submitted.notify_one();
        Ok(23)
    }
}

struct RunningPair {
    client: Arc<RuntimeCoordinator>,
    server: Arc<RuntimeCoordinator>,
    proxy: Arc<ClientProxyService>,
    failures: Arc<PrintFailures>,
    server_address: String,
    submitter: Arc<FakeSubmitter>,
}

impl RunningPair {
    async fn start(client_channel_matches: bool) -> Self {
        Self::start_on_ports(client_channel_matches, 0, 0).await
    }

    async fn start_on_ports(
        client_channel_matches: bool,
        server_port: u16,
        proxy_port: u16,
    ) -> Self {
        let server_channel_value = channel_secret();
        let server_channel = Arc::new(NetworkChannel::in_memory());
        server_channel
            .configure(&server_channel_value)
            .await
            .expect("configures server Network Channel");
        let sharing = Arc::new(Sharing::new(FakeCatalog::new(&[SERVER_QUEUE])));
        sharing
            .set_shared(vec![
                PrinterName::parse(SERVER_QUEUE).expect("valid queue name")
            ])
            .await
            .expect("selects server queue");
        let submitter = Arc::new(FakeSubmitter::default());
        let endpoint = Arc::new(IppsServer::new(
            server_port,
            Arc::new(ServerIdentity::generate().expect("creates server identity")),
            server_channel,
            submitter.clone(),
            Arc::new(PrintFailures::new()),
        ));
        let server = Arc::new(RuntimeCoordinator::new(vec![Arc::new(
            support::sharing_service(sharing, endpoint.clone()),
        )]));
        server
            .start(ServiceId::ServerSharing)
            .await
            .expect("starts server IPPS endpoint");
        let server_address = endpoint.bound_address().expect("server is listening");

        let client_connections = Arc::new(
            ClientConnections::new(temporary_directory("client-proxy-trust"))
                .expect("opens client trust store"),
        );
        let review = client_connections
            .inspect(&format!("127.0.0.1:{}", server_address.port()))
            .await
            .expect("inspects server identity");
        client_connections
            .approve(&review.address, &review.current_fingerprint)
            .await
            .expect("approves server identity");

        let client_channel = Arc::new(NetworkChannel::in_memory());
        let client_channel_value = if client_channel_matches {
            server_channel_value
        } else {
            channel_secret()
        };
        client_channel
            .configure(&client_channel_value)
            .await
            .expect("configures client Network Channel");
        // The proxy reports what it could not forward here, so a test can read the same surface the
        // window shows (#39).
        let failures = Arc::new(PrintFailures::new());
        let proxy = Arc::new(ClientProxyService::with_port(
            client_connections.clone(),
            client_channel.clone(),
            Arc::clone(&failures),
            proxy_port,
        ));
        let client = Arc::new(RuntimeCoordinator::new(vec![proxy.clone()]));
        client
            .start_autostart()
            .await
            .expect("starts local client proxy");
        Self {
            client,
            server,
            proxy,
            failures,
            server_address: format!("127.0.0.1:{}", server_address.port()),
            submitter,
        }
    }

    fn proxy_address(&self) -> String {
        self.proxy
            .bound_address()
            .expect("proxy is listening")
            .to_string()
    }

    async fn stop(self) {
        self.client.shutdown().await.expect("stops client proxy");
        self.server
            .shutdown()
            .await
            .expect("stops server IPPS endpoint");
    }
}

fn print_job(printer_uri: &str, document: &[u8]) -> Vec<u8> {
    let mut body = vec![2, 0, 0, 2, 0, 0, 0, 41, 1];
    text_attribute(&mut body, 0x47, "attributes-charset", "utf-8");
    text_attribute(&mut body, 0x48, "attributes-natural-language", "en");
    text_attribute(&mut body, 0x45, "printer-uri", printer_uri);
    text_attribute(
        &mut body,
        0x49,
        "document-format",
        "application/octet-stream",
    );
    text_attribute(&mut body, 0x44, "media", "iso_a4_210x297mm");
    text_attribute(&mut body, 0x44, "print-color-mode", "color");
    text_attribute(&mut body, 0x44, "sides", "two-sided-long-edge");
    body.push(0x21);
    body.extend(6u16.to_be_bytes());
    body.extend(b"copies");
    body.extend(4u16.to_be_bytes());
    body.extend(3i32.to_be_bytes());
    body.push(3);
    body.extend(document);
    body
}

fn validate_job(printer_uri: &str) -> Vec<u8> {
    let mut body = vec![2, 0, 0, 4, 0, 0, 0, 42, 1];
    text_attribute(&mut body, 0x47, "attributes-charset", "utf-8");
    text_attribute(&mut body, 0x48, "attributes-natural-language", "en");
    text_attribute(&mut body, 0x45, "printer-uri", printer_uri);
    text_attribute(&mut body, 0x44, "media", "iso_a4_210x297mm");
    text_attribute(&mut body, 0x44, "print-color-mode", "color");
    text_attribute(&mut body, 0x44, "sides", "one-sided");
    body.push(3);
    body
}

fn text_attribute(body: &mut Vec<u8>, tag: u8, name: &str, value: &str) {
    body.push(tag);
    body.extend((name.len() as u16).to_be_bytes());
    body.extend(name.as_bytes());
    body.extend((value.len() as u16).to_be_bytes());
    body.extend(value.as_bytes());
}

async fn submit_to_local_queue(
    address: &str,
    printer_uri: &str,
    document: &[u8],
) -> (u16, Vec<u8>) {
    submit_request_to_local_queue(address, print_job(printer_uri, document)).await
}

async fn submit_request_to_local_queue(address: &str, body: Vec<u8>) -> (u16, Vec<u8>) {
    let mut stream = TcpStream::connect(address)
        .await
        .expect("connects to local proxy");
    let request = format!(
        "POST /ipp/print HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/ipp\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("writes local IPP request");
    stream.write_all(&body).await.expect("writes print job");
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .await
        .expect("reads local IPP response");
    let boundary = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("contains HTTP response head");
    let status = String::from_utf8_lossy(&response[..boundary])
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse().ok())
        .expect("contains HTTP status");
    (status, response[boundary + 4..].to_vec())
}

fn ipp_status(body: &[u8]) -> u16 {
    u16::from_be_bytes([body[2], body[3]])
}

fn local_printer_uri(proxy_address: &str, server_address: &str) -> String {
    client_queue_uri(proxy_address, server_address, SERVER_QUEUE).expect("builds local queue URI")
}

#[tokio::test]
async fn printer_attributes_advertise_the_local_queue_uri() {
    let pair = RunningPair::start(true).await;
    let proxy_address = pair.proxy_address();
    let local_uri = local_printer_uri(&proxy_address, &pair.server_address);
    let request = get_printer_attributes(17, &local_uri);

    let (http_status, response) = submit_request_to_local_queue(&proxy_address, request).await;

    assert_eq!(http_status, 200);
    assert_eq!(ipp_status(&response), 0x0000);
    let attributes = protocol::Request::parse(&response).expect("valid IPP response");
    assert_eq!(
        attributes.value("printer-uri-supported"),
        Some(local_uri.as_str())
    );
    assert_eq!(attributes.value("uri-security-supported"), Some("none"));
    assert!(attributes.value("printer-uuid").is_some());
    pair.stop().await;
}

#[tokio::test]
async fn printer_attributes_keep_a_case_normalized_local_queue_uri() {
    let pair = RunningPair::start(true).await;
    let proxy_address = pair.proxy_address();
    let local_uri = local_printer_uri(&proxy_address, &pair.server_address)
        .replace("Office%20Printer", "office%20printer");
    let request = get_printer_attributes(18, &local_uri);

    let (http_status, response) = submit_request_to_local_queue(&proxy_address, request).await;

    assert_eq!(http_status, 200);
    assert_eq!(ipp_status(&response), 0x0000);
    let attributes = protocol::Request::parse(&response).expect("valid IPP response");
    assert_eq!(
        attributes.value("printer-uri-supported"),
        Some(local_uri.as_str())
    );
    pair.stop().await;
}

#[tokio::test]
async fn print_job_from_a_native_queue_reaches_the_selected_server_printer() {
    let pair = RunningPair::start(true).await;
    let proxy_address = pair.proxy_address();
    let uri = local_printer_uri(&proxy_address, &pair.server_address);
    let document = document_bytes();
    let (http_status, response) = submit_to_local_queue(&proxy_address, &uri, &document).await;

    assert_eq!(http_status, 200);
    assert_eq!(ipp_status(&response), 0x0000);
    {
        let submissions = pair.submitter.jobs.lock().expect("reads submitted jobs");
        assert_eq!(submissions.len(), 1);
        assert_eq!(submissions[0].0, SERVER_QUEUE);
        assert_eq!(submissions[0].1.document(), document);
        assert_eq!(
            submissions[0].1.settings().media.as_deref(),
            Some("iso_a4_210x297mm")
        );
        assert_eq!(submissions[0].1.settings().color, Some(true));
        assert_eq!(
            submissions[0].1.settings().duplex,
            Some(DuplexMode::LongEdge)
        );
        assert_eq!(submissions[0].1.settings().copies, Some(3));
    }
    // A job that reached the queue is not a failure the user has to act on.
    assert_eq!(pair.failures.latest(), None);
    pair.stop().await;
}

#[tokio::test]
async fn print_job_with_monochrome_simplex_and_custom_copies_reaches_server_printer() {
    let pair = RunningPair::start(true).await;
    let proxy_address = pair.proxy_address();
    let uri = local_printer_uri(&proxy_address, &pair.server_address);
    let document = document_bytes();

    let mut body = vec![2, 0, 0, 2, 0, 0, 0, 43, 1];
    text_attribute(&mut body, 0x47, "attributes-charset", "utf-8");
    text_attribute(&mut body, 0x48, "attributes-natural-language", "en");
    text_attribute(&mut body, 0x45, "printer-uri", &uri);
    text_attribute(
        &mut body,
        0x49,
        "document-format",
        "application/octet-stream",
    );
    text_attribute(&mut body, 0x44, "media", "iso_a4_210x297mm");
    text_attribute(&mut body, 0x44, "print-color-mode", "monochrome");
    text_attribute(&mut body, 0x44, "sides", "one-sided");
    body.push(0x21);
    body.extend(6u16.to_be_bytes());
    body.extend(b"copies");
    body.extend(4u16.to_be_bytes());
    body.extend(4i32.to_be_bytes());
    body.push(3);
    body.extend(&document);

    let (http_status, response) = submit_request_to_local_queue(&proxy_address, body).await;

    assert_eq!(http_status, 200);
    assert_eq!(ipp_status(&response), 0x0000);
    {
        let submissions = pair.submitter.jobs.lock().expect("reads submitted jobs");
        assert_eq!(submissions.len(), 1);
        assert_eq!(submissions[0].0, SERVER_QUEUE);
        assert_eq!(submissions[0].1.document(), document);
        assert_eq!(submissions[0].1.settings().color, Some(false));
        assert_eq!(
            submissions[0].1.settings().duplex,
            Some(DuplexMode::Simplex)
        );
        assert_eq!(submissions[0].1.settings().copies, Some(4));
    }
    assert_eq!(pair.failures.latest(), None);
    pair.stop().await;
}

#[tokio::test]
async fn proxy_forwards_a_wrong_network_channel_as_an_authorization_failure() {
    let pair = RunningPair::start(false).await;
    let proxy_address = pair.proxy_address();
    let uri = local_printer_uri(&proxy_address, &pair.server_address);

    let (http_status, response) =
        submit_to_local_queue(&proxy_address, &uri, &document_bytes()).await;

    assert_eq!(http_status, 200);
    assert_eq!(ipp_status(&response), 0x0403);
    assert!(pair
        .submitter
        .jobs
        .lock()
        .expect("reads submitted jobs")
        .is_empty());

    let failure = pair.failures.latest().expect("reports the failure");
    assert_eq!(failure.path(), JobPath::ClientForwarding);
    assert_eq!(failure.code(), ErrorCode::NotAuthorized);
    assert_eq!(failure.code().as_str(), "not-authorized");
    assert_eq!(
        failure.message(),
        "The job for 'Office Printer' was rejected because the Network Channel is missing or incorrect."
    );
    assert!(failure.recovery().contains("Network Channel"));
    pair.stop().await;
}

#[tokio::test]
async fn a_print_job_whose_server_is_unreachable_is_reported_with_a_recovery_action() {
    let pair = RunningPair::start(true).await;
    pair.server
        .stop(ServiceId::ServerSharing)
        .await
        .expect("stops the server IPPS endpoint");
    let proxy_address = pair.proxy_address();
    let uri = local_printer_uri(&proxy_address, &pair.server_address);

    let (http_status, _response) =
        submit_to_local_queue(&proxy_address, &uri, &document_bytes()).await;

    assert_eq!(http_status, 200);
    let failure = pair.failures.latest().expect("reports the failure");
    assert_eq!(failure.path(), JobPath::ClientForwarding);
    assert_eq!(failure.code(), ErrorCode::ServerUnavailable);
    assert!(failure.message().contains(SERVER_QUEUE));
    assert!(failure
        .recovery()
        .contains("Check that ShaPrint is running on the server"));
    pair.stop().await;
}

#[tokio::test]
async fn proxy_replaces_a_network_channel_supplied_by_the_local_driver() {
    let pair = RunningPair::start(true).await;
    let proxy_address = pair.proxy_address();
    let local_uri = local_printer_uri(&proxy_address, &pair.server_address);
    let mut request = print_job(&local_uri, &document_bytes());
    let end_attributes = protocol::Request::parse(&request)
        .expect("valid IPP request")
        .document_start()
        - 1;
    let mut forged = Vec::new();
    text_attribute(&mut forged, 0x41, "network-channel", &channel_secret());
    request.splice(end_attributes..end_attributes, forged);

    let (http_status, response) = submit_request_to_local_queue(&proxy_address, request).await;

    assert_eq!(http_status, 200);
    assert_eq!(ipp_status(&response), 0x0000);
    assert_eq!(pair.submitter.jobs.lock().expect("reads jobs").len(), 1);
    pair.stop().await;
}

#[tokio::test]
async fn proxy_forwards_validate_job_with_the_configured_network_channel() {
    let pair = RunningPair::start(true).await;
    let proxy_address = pair.proxy_address();
    let local_uri = local_printer_uri(&proxy_address, &pair.server_address);
    let request = validate_job(&local_uri);

    let (http_status, response) = submit_request_to_local_queue(&proxy_address, request).await;

    assert_eq!(http_status, 200);
    assert_eq!(ipp_status(&response), 0x0000);
    assert!(pair.submitter.jobs.lock().expect("reads jobs").is_empty());
    pair.stop().await;
}

/// Creates a temporary Windows IPP queue, sends a generated page through the real spooler and
/// removes the queue. The Rust fake printer adapter receives the job, so this produces no paper.
#[cfg(windows)]
#[tokio::test]
#[ignore = "requires elevated Windows PrintManagement access; uses a temporary queue and fake server adapter"]
async fn windows_native_queue_prints_through_the_local_proxy_to_the_fake_adapter() {
    struct TraceEnvironment;
    impl Drop for TraceEnvironment {
        fn drop(&mut self) {
            std::env::remove_var("SHAPRINT_ISSUE34_IPP_TRACE");
        }
    }

    std::env::set_var("SHAPRINT_ISSUE34_IPP_TRACE", "1");
    let _trace_environment = TraceEnvironment;
    let pair =
        RunningPair::start_on_ports(true, NATIVE_SMOKE_SERVER_PORT, NATIVE_SMOKE_PROXY_PORT).await;
    let queue_name = format!("ShaPrint Issue34 Smoke {}", std::process::id());
    let proxy_authority = format!("127.0.0.1:{NATIVE_SMOKE_PROXY_PORT}");
    let ipp_url = client_queue_uri(
        &proxy_authority,
        &format!("127.0.0.1:{NATIVE_SMOKE_SERVER_PORT}"),
        SERVER_QUEUE,
    )
    .expect("builds native queue URI");
    let script =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/client_proxy_smoke.ps1");
    let queue_argument = queue_name.clone();
    let url_argument = ipp_url.clone();
    let status = tokio::task::spawn_blocking(move || {
        Command::new("powershell.exe")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(script)
            .arg(queue_argument)
            .arg(url_argument)
            .status()
    })
    .await
    .expect("joins Windows spooler smoke command")
    .expect("starts Windows PowerShell");
    assert!(
        status.success(),
        "temporary Windows IPP queue setup or print failed: {status}"
    );

    timeout(Duration::from_secs(30), pair.submitter.submitted.notified())
        .await
        .expect("native print reaches the fake adapter");
    {
        let jobs = pair.submitter.jobs.lock().expect("reads submitted jobs");
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].0, SERVER_QUEUE);
        assert!(!jobs[0].1.document().is_empty());
    }
    pair.stop().await;
}
