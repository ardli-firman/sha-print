//! The client print path: a local Windows IPP queue sends a job through the authenticated proxy,
//! which forwards it over approved IPPS to the server's selected shared printer.

mod support;

// `Command`, the two ports, and the tokio time helpers are only used by the Windows-only smoke
// test below, so they are gated with it instead of warning on every other platform.
use std::path::PathBuf;
#[cfg(windows)]
use std::process::Command;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use shaprint_desktop::adapters::ipps::{protocol, NetworkChannel};
use shaprint_desktop::adapters::printers::raster::{decode_pwg, RasterPage};
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
const NATIVE_SMOKE_SERVER_PORT: u16 = 48631;
#[cfg(windows)]
const NATIVE_SMOKE_PROXY_PORT: u16 = 48632;

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
    render: AtomicBool,
    pages: Mutex<Vec<RasterPage>>,
    submitted: tokio::sync::Notify,
}

#[async_trait]
impl PrintJobSubmitter for FakeSubmitter {
    async fn submit(&self, printer: &PrinterName, job: PrintJob) -> Result<u32, AppError> {
        if self.render.load(Ordering::Relaxed) {
            // Same decoder the real Windows adapter runs before invoking the installed driver.
            let pages = decode_pwg(job.document())?;
            self.pages
                .lock()
                .expect("records rendered pages")
                .extend(pages);
        }
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
    trust_directory: PathBuf,
    server_tracker: Arc<shaprint_desktop::application::PrintJobTracker>,
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
        let server_tracker = Arc::new(shaprint_desktop::application::PrintJobTracker::new());
        let endpoint = Arc::new(IppsServer::new(
            server_port,
            Arc::new(ServerIdentity::generate().expect("creates server identity")),
            server_channel,
            submitter.clone(),
            Arc::new(PrintFailures::new()),
            server_tracker.clone(),
        ));
        let server = Arc::new(RuntimeCoordinator::new(vec![Arc::new(
            support::sharing_service(sharing, endpoint.clone()),
        )]));
        server
            .start(ServiceId::ServerSharing)
            .await
            .expect("starts server IPPS endpoint");
        let server_address = endpoint.bound_address().expect("server is listening");

        let trust_directory = temporary_directory("client-proxy-trust");
        let client_connections =
            Arc::new(ClientConnections::new(&trust_directory).expect("opens client trust store"));
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
        let client_tracker = Arc::new(shaprint_desktop::application::PrintJobTracker::new());
        let proxy = Arc::new(ClientProxyService::with_port(
            client_connections.clone(),
            client_channel.clone(),
            Arc::clone(&failures),
            client_tracker,
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
            trust_directory,
            server_tracker,
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
    text_attribute(&mut body, 0x49, "document-format", "image/pwg-raster");
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

fn operation_request(operation: u16, request_id: u32, printer_uri: &str) -> Vec<u8> {
    let mut body = vec![2, 0];
    body.extend(operation.to_be_bytes());
    body.extend(request_id.to_be_bytes());
    body.push(1);
    text_attribute(&mut body, 0x47, "attributes-charset", "utf-8");
    text_attribute(&mut body, 0x48, "attributes-natural-language", "en");
    text_attribute(&mut body, 0x45, "printer-uri", printer_uri);
    body
}

fn integer_attribute(body: &mut Vec<u8>, name: &str, value: i32) {
    body.push(0x21);
    body.extend((name.len() as u16).to_be_bytes());
    body.extend(name.as_bytes());
    body.extend(4u16.to_be_bytes());
    body.extend(value.to_be_bytes());
}

fn boolean_attribute(body: &mut Vec<u8>, name: &str, value: bool) {
    body.push(0x22);
    body.extend((name.len() as u16).to_be_bytes());
    body.extend(name.as_bytes());
    body.extend(1u16.to_be_bytes());
    body.push(u8::from(value));
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
    submit_request_with_headers(address, "/ipp/print", "application/ipp", body).await
}

async fn submit_request_with_headers(
    address: &str,
    path: &str,
    content_type: &str,
    body: Vec<u8>,
) -> (u16, Vec<u8>) {
    let mut stream = TcpStream::connect(address)
        .await
        .expect("connects to local proxy");
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {address}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("writes local IPP request");
    stream.write_all(&body).await.expect("writes print job");
    stream
        .shutdown()
        .await
        .expect("half-closes the request after its declared body");
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
async fn client_raster_job_reaches_server_as_pixels_with_page_order_and_colors_preserved() {
    let pair = RunningPair::start(true).await;
    pair.submitter.render.store(true, Ordering::Relaxed);
    let proxy_address = pair.proxy_address();
    let uri = local_printer_uri(&proxy_address, &pair.server_address);
    let mut document = b"RaS2".to_vec();
    for colors in [3u32, 1] {
        let mut header = vec![0; 1796];
        for (offset, value) in [
            (276, 300u32),
            (280, 300),
            (372, 2),
            (376, 2),
            (384, 8),
            (388, colors * 8),
            (392, 2 * colors),
            (400, if colors == 3 { 19 } else { 18 }),
            (420, colors),
        ] {
            header[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
        }
        document.extend(header);
        if colors == 3 {
            document.extend([1, 255, 255, 0, 0, 0, 0, 255]);
        } else {
            document.extend([1, 1, 64]);
        }
    }
    let (http, response) = submit_to_local_queue(&proxy_address, &uri, &document).await;
    assert_eq!((http, ipp_status(&response)), (200, 0));
    {
        let pages = pair.submitter.pages.lock().expect("reads rendered pages");
        assert_eq!(pages.len(), 2);
        assert_eq!(
            (pages[0].width, pages[0].height, pages[0].dpi),
            (2, 2, [300, 300])
        );
        assert_eq!(pages[0].pixels, [0, 0, 255, 0, 255, 0, 0, 0].repeat(2));
        assert_eq!(pages[1].pixels, [64, 64, 64, 0].repeat(4));
    }
    // A truncated second page must not be accepted as a partially printable job.
    document.pop();
    let (_, response) = submit_to_local_queue(&proxy_address, &uri, &document).await;
    assert_ne!(ipp_status(&response), 0);
    assert_eq!(
        pair.submitter.jobs.lock().expect("reads submissions").len(),
        1
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
async fn http_localhost_uri_and_parameterized_ipp_content_type_print_successfully() {
    let pair = RunningPair::start(true).await;
    let proxy_address = pair.proxy_address();
    let uri = local_printer_uri(&proxy_address, &pair.server_address)
        .replace("ipp://127.0.0.1", "http://localhost");

    let (http_status, response) = submit_request_with_headers(
        &proxy_address,
        "/ipp/print",
        "Application/IPP; charset=utf-8",
        print_job(&uri, &document_bytes()),
    )
    .await;

    assert_eq!(http_status, 200);
    assert_eq!(ipp_status(&response), 0x0000);
    assert_eq!(pair.submitter.jobs.lock().expect("reads jobs").len(), 1);
    pair.stop().await;
}

#[tokio::test]
async fn multi_megabyte_raster_payload_reaches_the_server_intact() {
    let pair = RunningPair::start(true).await;
    let proxy_address = pair.proxy_address();
    let uri = local_printer_uri(&proxy_address, &pair.server_address);
    let document = vec![0x5a; 2 * 1024 * 1024];

    let (http_status, response) = submit_to_local_queue(&proxy_address, &uri, &document).await;

    assert_eq!(http_status, 200);
    let proxy_failure = pair
        .failures
        .latest()
        .map(|failure| format!("{}: {}", failure.message(), failure.recovery()))
        .unwrap_or_else(|| "none".to_owned());
    let submitted_jobs = pair
        .submitter
        .jobs
        .lock()
        .expect("reads submitted jobs")
        .len();
    assert_eq!(
        ipp_status(&response),
        0x0000,
        "unexpected IPP response; submitted_jobs={submitted_jobs}; proxy_failure={proxy_failure}"
    );
    {
        let jobs = pair.submitter.jobs.lock().expect("reads submitted jobs");
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].1.document(), document);
    }
    pair.stop().await;
}

#[tokio::test]
async fn request_path_routes_print_job_when_printer_uri_is_missing_or_rewritten() {
    let pair = RunningPair::start(true).await;
    let proxy_address = pair.proxy_address();
    let path = format!(
        "/ipp/print/{}/{}",
        protocol::percent_encode(&pair.server_address),
        protocol::percent_encode(SERVER_QUEUE),
    );
    let mut request = vec![2, 0, 0, 2, 0, 0, 0, 47, 1];
    text_attribute(&mut request, 0x47, "attributes-charset", "utf-8");
    text_attribute(&mut request, 0x48, "attributes-natural-language", "en");
    text_attribute(&mut request, 0x49, "document-format", "image/pwg-raster");
    request.push(3);
    request.extend(document_bytes());

    let (http_status, response) =
        submit_request_with_headers(&proxy_address, &path, "application/ipp", request).await;

    assert_eq!(http_status, 200);
    assert_eq!(ipp_status(&response), 0x0000);

    let rewritten_uri = "http://spooler-rewrote-authority.invalid/ipp/print/wrong/queue";
    let (http_status, response) = submit_request_with_headers(
        &proxy_address,
        &path,
        "application/ipp",
        print_job(rewritten_uri, &document_bytes()),
    )
    .await;
    assert_eq!(http_status, 200);
    assert_eq!(ipp_status(&response), 0x0000);
    assert_eq!(pair.submitter.jobs.lock().expect("reads jobs").len(), 2);
    pair.stop().await;
}

#[tokio::test]
async fn create_send_and_job_queries_complete_over_the_local_proxy() {
    let pair = RunningPair::start(true).await;
    let proxy_address = pair.proxy_address();
    let printer_uri = local_printer_uri(&proxy_address, &pair.server_address);

    let mut create = operation_request(0x0005, 51, &printer_uri);
    text_attribute(&mut create, 0x44, "media", "iso_a4_210x297mm");
    integer_attribute(&mut create, "copies", 2);
    create.push(3);
    let (http_status, response) = submit_request_to_local_queue(&proxy_address, create).await;
    assert_eq!(http_status, 200);
    assert_eq!(ipp_status(&response), 0x0000);
    let created = protocol::Request::parse(&response).expect("valid Create-Job response");
    let job_id = created.integer("job-id").expect("server assigned a job id");
    assert!(job_id > 0);
    assert!(created.value("job-uri").is_some());

    let first_document = document_bytes();
    let final_document = b"second PWG Raster document".to_vec();
    let mut send_first = operation_request(0x0006, 52, &printer_uri);
    integer_attribute(&mut send_first, "job-id", job_id);
    text_attribute(&mut send_first, 0x49, "document-format", "image/pwg-raster");
    boolean_attribute(&mut send_first, "last-document", false);
    send_first.push(3);
    send_first.extend(&first_document);
    let (http_status, response) = submit_request_to_local_queue(&proxy_address, send_first).await;
    assert_eq!(http_status, 200);
    assert_eq!(ipp_status(&response), 0x0000);
    let pending =
        protocol::Request::parse(&response).expect("valid pending Send-Document response");
    assert_eq!(pending.integer("job-state"), Some(3));
    assert!(pair.submitter.jobs.lock().expect("reads jobs").is_empty());

    let mut send_final = operation_request(0x0006, 53, &printer_uri);
    integer_attribute(&mut send_final, "job-id", job_id);
    boolean_attribute(&mut send_final, "last-document", true);
    send_final.push(3);
    send_final.extend(&final_document);
    let (http_status, response) = submit_request_to_local_queue(&proxy_address, send_final).await;
    assert_eq!(http_status, 200);
    assert_eq!(ipp_status(&response), 0x0000);
    let mut document = first_document;
    document.extend(final_document);

    let mut get_job = operation_request(0x0009, 54, &printer_uri);
    integer_attribute(&mut get_job, "job-id", job_id);
    text_attribute(&mut get_job, 0x44, "requested-attributes", "job-id");
    text_attribute(&mut get_job, 0x44, "", "job-state");
    get_job.push(3);
    let (http_status, response) = submit_request_to_local_queue(&proxy_address, get_job).await;
    assert_eq!(http_status, 200);
    assert_eq!(ipp_status(&response), 0x0000);
    let attributes =
        protocol::Request::parse(&response).expect("valid Get-Job-Attributes response");
    assert_eq!(attributes.integer("job-id"), Some(job_id));
    assert_eq!(attributes.integer("job-state"), Some(9));

    let mut get_jobs = operation_request(0x000a, 55, &printer_uri);
    text_attribute(&mut get_jobs, 0x44, "which-jobs", "completed");
    get_jobs.push(3);
    let (http_status, response) = submit_request_to_local_queue(&proxy_address, get_jobs).await;
    assert_eq!(http_status, 200);
    assert_eq!(ipp_status(&response), 0x0000);
    let jobs = protocol::Request::parse(&response).expect("valid Get-Jobs response");
    assert_eq!(jobs.integer("job-id"), Some(job_id));
    assert_eq!(jobs.integer("job-state"), Some(9));
    assert_eq!(jobs.integer("number-of-documents"), Some(2));

    let mut cancel = operation_request(0x0008, 56, &printer_uri);
    integer_attribute(&mut cancel, "job-id", job_id);
    cancel.push(3);
    let (http_status, response) = submit_request_to_local_queue(&proxy_address, cancel).await;
    assert_eq!(http_status, 200);
    assert_eq!(ipp_status(&response), 0x0408);
    {
        let submitted = pair.submitter.jobs.lock().expect("reads submitted jobs");
        assert_eq!(submitted.len(), 1);
        assert_eq!(submitted[0].1.document(), document);
        assert_eq!(submitted[0].1.settings().copies, Some(2));
    }
    pair.stop().await;
}

#[tokio::test]
async fn canceling_a_pending_job_releases_its_restart_guard() {
    let pair = RunningPair::start(true).await;
    let proxy_address = pair.proxy_address();
    let printer_uri = local_printer_uri(&proxy_address, &pair.server_address);

    let mut create = operation_request(0x0005, 61, &printer_uri);
    create.push(3);
    let (http_status, response) = submit_request_to_local_queue(&proxy_address, create).await;
    assert_eq!(http_status, 200);
    assert_eq!(ipp_status(&response), 0x0000);
    let created = protocol::Request::parse(&response).expect("valid Create-Job response");
    let job_id = created.integer("job-id").expect("server assigned a job id");
    assert_eq!(pair.server_tracker.active_count(), 1);

    let mut cancel = operation_request(0x0008, 62, &printer_uri);
    integer_attribute(&mut cancel, "job-id", job_id);
    cancel.push(3);
    let (http_status, response) = submit_request_to_local_queue(&proxy_address, cancel).await;
    assert_eq!(http_status, 200);
    assert_eq!(ipp_status(&response), 0x0000);
    let canceled = protocol::Request::parse(&response).expect("valid Cancel-Job response");
    assert_eq!(canceled.integer("job-id"), Some(job_id));
    assert_eq!(canceled.integer("job-state"), Some(7));
    assert_eq!(pair.server_tracker.active_count(), 0);

    let mut get_jobs = operation_request(0x000a, 63, &printer_uri);
    text_attribute(&mut get_jobs, 0x44, "which-jobs", "canceled");
    get_jobs.push(3);
    let (http_status, response) = submit_request_to_local_queue(&proxy_address, get_jobs).await;
    assert_eq!(http_status, 200);
    assert_eq!(ipp_status(&response), 0x0000);
    let jobs = protocol::Request::parse(&response).expect("valid canceled Get-Jobs response");
    assert_eq!(jobs.integer("job-id"), Some(job_id));
    pair.stop().await;
}

#[tokio::test]
async fn stopping_server_sharing_releases_pending_job_guard() {
    let pair = RunningPair::start(true).await;
    let proxy_address = pair.proxy_address();
    let printer_uri = local_printer_uri(&proxy_address, &pair.server_address);
    let mut create = operation_request(0x0005, 66, &printer_uri);
    create.push(3);
    let (http_status, response) = submit_request_to_local_queue(&proxy_address, create).await;
    assert_eq!(http_status, 200);
    assert_eq!(ipp_status(&response), 0x0000);
    assert_eq!(pair.server_tracker.active_count(), 1);
    let tracker = Arc::clone(&pair.server_tracker);

    pair.stop().await;

    assert_eq!(tracker.active_count(), 0);
}

#[tokio::test]
async fn create_job_requires_authorization_and_a_shared_printer() {
    let pair = RunningPair::start(false).await;
    let proxy_address = pair.proxy_address();
    let uri = local_printer_uri(&proxy_address, &pair.server_address);
    let mut create = operation_request(0x0005, 71, &uri);
    create.push(3);
    let (http_status, response) = submit_request_to_local_queue(&proxy_address, create).await;
    assert_eq!(http_status, 200);
    assert_eq!(ipp_status(&response), 0x0403);
    assert_eq!(pair.server_tracker.active_count(), 0);
    pair.stop().await;

    let pair = RunningPair::start(true).await;
    let proxy_address = pair.proxy_address();
    let unshared_uri = client_queue_uri(&proxy_address, &pair.server_address, "NotShared")
        .expect("builds an unshared local queue URI");
    let mut create = operation_request(0x0005, 72, &unshared_uri);
    create.push(3);
    let (http_status, response) = submit_request_to_local_queue(&proxy_address, create).await;
    assert_eq!(http_status, 200);
    assert_eq!(ipp_status(&response), 0x0406);
    assert_eq!(pair.server_tracker.active_count(), 0);
    pair.stop().await;
}

#[tokio::test]
async fn running_proxy_observes_trusted_server_removal_from_disk() {
    let pair = RunningPair::start(true).await;
    let proxy_address = pair.proxy_address();
    let uri = local_printer_uri(&proxy_address, &pair.server_address);
    let trust_store = pair.trust_directory.join("client-server-trust.json");
    std::fs::write(&trust_store, r#"{"servers":{}}"#)
        .expect("replaces the persisted Trusted server approvals");

    let (http_status, response) =
        submit_to_local_queue(&proxy_address, &uri, &document_bytes()).await;

    assert_eq!(http_status, 200);
    assert_ne!(ipp_status(&response), 0x0000);
    assert!(pair.submitter.jobs.lock().expect("reads jobs").is_empty());
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
    text_attribute(&mut body, 0x49, "document-format", "image/pwg-raster");
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
