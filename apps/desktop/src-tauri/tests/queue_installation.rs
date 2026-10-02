//! The observable seam between installing a native client queue (#35) and the local proxy that
//! serves it (#34): a queue installed for a trusted server's shared printer must send its jobs to
//! the loopback proxy and reach that printer on the server.

mod support;

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use shaprint_desktop::adapters::client_connections::ClientConnections;
use shaprint_desktop::adapters::ipps::NetworkChannel;
use shaprint_desktop::adapters::{
    client_queue_uri, ClientProxyService, IppsServer, ServerIdentity,
};
use shaprint_desktop::application::{
    ClientProxyState, ElevationBroker, PrintFailures, PrintJob, PrintJobSubmitter,
    QueueInstallation, QueueInstaller, RuntimeCoordinator, Setup, Sharing, TrustedServerPrinters,
};
use shaprint_desktop::domain::{
    AppError, ClientQueueRequest, ErrorCode, PrinterName, ServiceId, SetupAction, SetupFailure,
    SetupFailureKind,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use support::{printer_names, temporary_directory, FakeCatalog};

const SERVER_QUEUE: &str = "Office Printer";

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

/// Records the jobs the server submits to its Windows queue, standing in for the spooler.
#[derive(Default)]
struct RecordingSubmitter {
    jobs: Mutex<Vec<(String, Vec<u8>)>>,
}

impl RecordingSubmitter {
    fn jobs(&self) -> Vec<(String, Vec<u8>)> {
        self.jobs
            .lock()
            .map(|jobs| jobs.clone())
            .unwrap_or_default()
    }
}

#[async_trait]
impl PrintJobSubmitter for RecordingSubmitter {
    async fn submit(&self, printer: &PrinterName, job: PrintJob) -> Result<u32, AppError> {
        self.jobs
            .lock()
            .map_err(|_| AppError::internal("fake printer unavailable"))?
            .push((printer.as_str().to_owned(), job.document().to_vec()));
        Ok(23)
    }
}

/// Records the queue installs the app asked for elevated, standing in for the UAC handoff.
#[derive(Default)]
struct RecordingBroker {
    installed: Mutex<Vec<ClientQueueRequest>>,
    refusals: Mutex<Vec<SetupFailureKind>>,
}

impl RecordingBroker {
    fn refusing(kind: SetupFailureKind) -> Arc<Self> {
        let broker = Self::default();
        broker
            .refusals
            .lock()
            .expect("records the refusal")
            .push(kind);
        Arc::new(broker)
    }

    fn installed(&self) -> Vec<ClientQueueRequest> {
        self.installed
            .lock()
            .map(|requests| requests.clone())
            .unwrap_or_default()
    }
}

impl ElevationBroker for RecordingBroker {
    fn elevate(&self, _action: SetupAction) -> Result<(), SetupFailure> {
        Err(SetupFailure::new(
            SetupFailureKind::Other,
            "no parameterless action is part of this flow",
        ))
    }

    fn install_queue(&self, request: &ClientQueueRequest) -> Result<(), SetupFailure> {
        if let Some(kind) = self
            .refusals
            .lock()
            .ok()
            .and_then(|kinds| kinds.first().copied())
        {
            return Err(SetupFailure::new(kind, "recorded refusal"));
        }
        self.installed
            .lock()
            .map_err(|_| SetupFailure::new(SetupFailureKind::Other, "lock is poisoned"))?
            .push(request.clone());
        Ok(())
    }
}

/// Builds the destination the way the platform installers do; installing itself stays behind the
/// elevated helper, so this never runs in the app process.
struct TrackingInstaller {
    authority: String,
}

impl QueueInstaller for TrackingInstaller {
    fn queue_uri(&self, request: &ClientQueueRequest) -> Result<String, AppError> {
        client_queue_uri(
            &self.authority,
            request.server_address(),
            request.printer().as_str(),
        )
    }

    fn install(&self, _request: &ClientQueueRequest) -> Result<(), SetupFailure> {
        Err(SetupFailure::new(
            SetupFailureKind::Other,
            "the elevated helper performs the install",
        ))
    }
}

/// A running server, a running local proxy, and the trust the two need.
struct RunningClient {
    server: Arc<RuntimeCoordinator>,
    coordinator: Arc<RuntimeCoordinator>,
    connections: Arc<ClientConnections>,
    proxy_address: String,
    server_port: u16,
    submitter: Arc<RecordingSubmitter>,
}

impl RunningClient {
    async fn start(approve_server: bool) -> Self {
        Self::start_on_ports(approve_server, 0, 0).await
    }

    /// Starts the pair on fixed ports, which the Windows smoke test needs: a real installed queue
    /// points at the product's proxy port.
    async fn start_on_ports(approve_server: bool, server_port: u16, proxy_port: u16) -> Self {
        let channel_value = channel_secret();
        let server_channel = Arc::new(NetworkChannel::in_memory());
        server_channel
            .configure(&channel_value)
            .await
            .expect("configures the server Network Channel");
        let sharing = Arc::new(Sharing::new(FakeCatalog::new(&[SERVER_QUEUE])));
        sharing
            .set_shared(printer_names(&[SERVER_QUEUE]))
            .await
            .expect("selects the shared queue");
        let submitter = Arc::new(RecordingSubmitter::default());
        let endpoint = Arc::new(IppsServer::new(
            server_port,
            Arc::new(ServerIdentity::generate().expect("creates a server identity")),
            server_channel,
            submitter.clone(),
            Arc::new(PrintFailures::new()),
        ));
        // The shared helper advertises on an ephemeral discovery port, so this test never competes
        // for the machine's multicast DNS port.
        let server = Arc::new(RuntimeCoordinator::new(vec![Arc::new(
            support::sharing_service(sharing, endpoint.clone()),
        )]));
        server
            .start(ServiceId::ServerSharing)
            .await
            .expect("starts the server IPPS endpoint");
        let server_port = endpoint
            .bound_address()
            .expect("the server is listening")
            .port();

        let connections = Arc::new(
            ClientConnections::new(temporary_directory("queue-install-trust"))
                .expect("opens the client trust store"),
        );
        let review = connections
            .inspect(&format!("127.0.0.1:{server_port}"))
            .await
            .expect("inspects the server identity");
        if approve_server {
            connections
                .approve(&review.address, &review.current_fingerprint)
                .await
                .expect("approves the server identity");
        }

        let client_channel = Arc::new(NetworkChannel::in_memory());
        client_channel
            .configure(&channel_value)
            .await
            .expect("configures the client Network Channel");
        let proxy = Arc::new(ClientProxyService::with_port(
            Arc::clone(&connections),
            client_channel,
            Arc::new(PrintFailures::new()),
            proxy_port,
        ));
        let coordinator = Arc::new(RuntimeCoordinator::new(vec![proxy.clone()]));
        coordinator
            .start_autostart()
            .await
            .expect("starts the local client proxy");
        let proxy_address = proxy
            .bound_address()
            .expect("the proxy is listening")
            .to_string();

        Self {
            server,
            coordinator,
            connections,
            proxy_address,
            server_port,
            submitter,
        }
    }

    fn queue_installation(&self, broker: Arc<RecordingBroker>) -> QueueInstallation {
        QueueInstallation::new(
            Arc::new(Setup::new(broker as Arc<dyn ElevationBroker>)),
            Arc::clone(&self.connections) as Arc<dyn TrustedServerPrinters>,
            Arc::new(TrackingInstaller {
                authority: self.proxy_address.clone(),
            }) as Arc<dyn QueueInstaller>,
            Arc::clone(&self.coordinator) as Arc<dyn ClientProxyState>,
        )
    }

    async fn stop(self) {
        self.coordinator
            .shutdown()
            .await
            .expect("stops the client proxy");
        self.server
            .shutdown()
            .await
            .expect("stops the server IPPS endpoint");
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
    body.push(3);
    body.extend(document);
    body
}

fn text_attribute(body: &mut Vec<u8>, tag: u8, name: &str, value: &str) {
    body.push(tag);
    body.extend((name.len() as u16).to_be_bytes());
    body.extend(name.as_bytes());
    body.extend((value.len() as u16).to_be_bytes());
    body.extend(value.as_bytes());
}

fn ipp_status(body: &[u8]) -> u16 {
    u16::from_be_bytes([body[2], body[3]])
}

/// Sends one IPP request to the installed queue's URI through the local proxy.
async fn submit_through_proxy(
    proxy_address: &str,
    printer_uri: &str,
    document: &[u8],
) -> (u16, Vec<u8>) {
    let body = print_job(printer_uri, document);
    let mut stream = TcpStream::connect(proxy_address)
        .await
        .expect("connects to the local proxy");
    let request = format!(
        "POST /ipp/print HTTP/1.1\r\nHost: {proxy_address}\r\nContent-Type: application/ipp\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("writes the local IPP request");
    stream.write_all(&body).await.expect("writes the print job");
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .await
        .expect("reads the local IPP response");
    let boundary = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("contains an HTTP response head");
    let status = String::from_utf8_lossy(&response[..boundary])
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse().ok())
        .expect("contains an HTTP status");
    (status, response[boundary + 4..].to_vec())
}

#[tokio::test]
async fn an_installed_queue_prints_through_the_local_proxy_to_the_shared_printer() {
    let client = RunningClient::start(true).await;
    let broker = Arc::new(RecordingBroker::default());
    let installation = client.queue_installation(Arc::clone(&broker));

    // The queue name is derived from the printer and the normalized server address.
    let queue = installation
        .install(&format!(" 127.0.0.1:{} ", client.server_port), SERVER_QUEUE)
        .await
        .expect("installs the queue");
    assert_eq!(
        queue.name().as_str(),
        format!("{SERVER_QUEUE} (ShaPrint 127.0.0.1-{})", client.server_port)
    );
    assert_eq!(
        queue.request().server_address(),
        format!("127.0.0.1:{}", client.server_port)
    );

    // The app asked for elevation with exactly the queue the user is told about.
    assert_eq!(broker.installed(), vec![queue.request().clone()]);

    // The destination the installer gives Windows is the proxy, and it resolves to the shared
    // printer: this is what "the installed queue prints" means without a Windows spooler.
    let document = document_bytes();
    let (http_status, response) =
        submit_through_proxy(&client.proxy_address, queue.uri(), &document).await;
    assert_eq!(http_status, 200);
    assert_eq!(ipp_status(&response), 0x0000);
    assert_eq!(
        client.submitter.jobs(),
        vec![(SERVER_QUEUE.to_owned(), document)]
    );

    client.stop().await;
}

#[tokio::test]
async fn an_unapproved_server_installs_nothing() {
    let client = RunningClient::start(false).await;
    let broker = Arc::new(RecordingBroker::default());
    let installation = client.queue_installation(Arc::clone(&broker));

    let error = installation
        .install(&format!("127.0.0.1:{}", client.server_port), SERVER_QUEUE)
        .await
        .expect_err("refuses an unapproved server");

    // #48 made this refusal specific: an unapproved server is reported as `server-untrusted`
    // instead of the generic `invalid-state` the print path used before.
    assert_eq!(error.code(), ErrorCode::ServerNotTrusted);
    assert!(broker.installed().is_empty());
    client.stop().await;
}

#[tokio::test]
async fn a_printer_the_server_does_not_share_installs_nothing() {
    let client = RunningClient::start(true).await;
    let broker = Arc::new(RecordingBroker::default());
    let installation = client.queue_installation(Arc::clone(&broker));

    let error = installation
        .install(&format!("127.0.0.1:{}", client.server_port), "Zebra")
        .await
        .expect_err("refuses an unshared printer");

    assert_eq!(error.code(), ErrorCode::InvalidInput);
    assert!(error.message().contains("Zebra"));
    assert!(broker.installed().is_empty());
    client.stop().await;
}

#[tokio::test]
async fn a_refused_prompt_reports_the_action_the_user_can_take() {
    let client = RunningClient::start(true).await;
    let broker = RecordingBroker::refusing(SetupFailureKind::PermissionDenied);
    let installation = client.queue_installation(Arc::clone(&broker));

    let error = installation
        .install(&format!("127.0.0.1:{}", client.server_port), SERVER_QUEUE)
        .await
        .expect_err("refuses when the prompt is dismissed");

    assert_eq!(error.code(), ErrorCode::Unsupported);
    assert!(error.message().contains("administrator permission"));
    // The queue name is in the message, and the raw helper detail is not.
    assert!(error.message().contains("ShaPrint 127.0.0.1-"));
    assert!(!error.message().contains("recorded refusal"));
    client.stop().await;
}

/// Runs a PowerShell snippet with the smoke queue name in the environment, off the async runtime so
/// the server and proxy tasks keep running while the spooler is queried.
async fn smoke_powershell(script: &'static str, queue: &str) -> std::process::Output {
    let queue = queue.to_owned();
    tokio::task::spawn_blocking(move || {
        std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .env("SHAPRINT_SMOKE_QUEUE", queue)
            .output()
    })
    .await
    .expect("joins the Windows PowerShell task")
    .expect("runs Windows PowerShell")
}

/// Reports whether the spooler has `queue`. The name travels through the environment, so it is
/// never interpolated into the script text.
async fn spooler_has_queue(queue: &str) -> bool {
    let output = smoke_powershell(
        "if (Get-Printer -Name $env:SHAPRINT_SMOKE_QUEUE -ErrorAction SilentlyContinue) { 'installed' } else { 'missing' }",
        queue,
    )
    .await;
    String::from_utf8_lossy(&output.stdout).contains("installed")
}

/// Removes a smoke queue the test created.
async fn remove_queue(queue: &str) {
    let _ = smoke_powershell(
        "Remove-Printer -Name $env:SHAPRINT_SMOKE_QUEUE -ErrorAction SilentlyContinue",
        queue,
    )
    .await;
}

/// Confirms the installed queue is in the spooler and prints one page through it, the way the
/// standard Windows print dialog would (`tests/queue_install_smoke.ps1`).
async fn print_through_installed_queue(queue: &str) {
    let script =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/queue_install_smoke.ps1");
    let queue = queue.to_owned();
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(script)
            .arg(queue)
            .output()
    })
    .await
    .expect("joins the Windows spooler smoke task")
    .expect("starts Windows PowerShell");
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.is_empty() {
        eprintln!("{stdout}");
    }
    assert!(
        output.status.success(),
        "the installed queue did not accept a printed page: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Waits until the server's printer adapter has received the page the queue printed.
async fn wait_for_submission(client: &RunningClient) {
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(30);
    while client.submitter.jobs().is_empty() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the installed queue never printed through the local proxy"
        );
        tokio::time::sleep(tokio::time::Duration::from_millis(250)).await;
    }

    let jobs = client.submitter.jobs();
    assert_eq!(jobs[0].0, SERVER_QUEUE);
    assert!(
        !jobs[0].1.is_empty(),
        "the server's printer adapter received an empty document"
    );
}

/// Installs a real Windows queue for a loopback server, prints through it, and removes it.
///
/// This is the only check that exercises the elevated helper's own install path against a real
/// spooler: everything else stops at the URI and the classification, which run everywhere.
///
/// Ignored by default: it needs an elevated session and binds the product's own ports, which means
/// a running ShaPrint instance must be closed first. Run it on Windows with
/// `cargo test --no-default-features --test queue_installation -- --ignored`.
#[tokio::test]
#[ignore = "creates and prints through a real Windows printer queue; needs an elevated session"]
async fn windows_installs_a_native_queue_that_prints_through_the_proxy() {
    if !cfg!(windows) {
        return;
    }
    let installer = shaprint_desktop::adapters::queue_installation::platform_installer();
    if !installer.is_available() {
        return;
    }

    struct TraceEnvironment;
    impl Drop for TraceEnvironment {
        fn drop(&mut self) {
            std::env::remove_var("SHAPRINT_ISSUE34_IPP_TRACE");
        }
    }
    let _trace_environment = TraceEnvironment;
    std::env::set_var("SHAPRINT_ISSUE34_IPP_TRACE", "1");

    const NATIVE_SERVER_PORT: u16 = 8631;
    const NATIVE_PROXY_PORT: u16 = 8632;
    let client = RunningClient::start_on_ports(true, NATIVE_SERVER_PORT, NATIVE_PROXY_PORT).await;
    let request = ClientQueueRequest::new(
        &format!("127.0.0.1:{NATIVE_SERVER_PORT}"),
        PrinterName::parse(SERVER_QUEUE).expect("valid printer name"),
    )
    .expect("valid request");
    let queue_name = request.queue_name().as_str().to_owned();
    remove_queue(&queue_name).await;

    struct QueueCleanup(String);
    impl Drop for QueueCleanup {
        fn drop(&mut self) {
            let queue = self.0.clone();
            let _ = std::process::Command::new("powershell.exe")
                .args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    "Remove-Printer -Name $env:SHAPRINT_SMOKE_QUEUE -ErrorAction SilentlyContinue",
                ])
                .env("SHAPRINT_SMOKE_QUEUE", queue)
                .output();
        }
    }
    let _cleanup = QueueCleanup(queue_name.clone());

    let request_to_install = request.clone();
    let installer_to_run = Arc::clone(&installer);
    tokio::task::spawn_blocking(move || installer_to_run.install(&request_to_install))
        .await
        .expect("joins installer task")
        .expect("installs the queue in the elevated session");
    // The queue is in the spooler, and Windows renders a page through it to the local proxy.
    print_through_installed_queue(&queue_name).await;
    wait_for_submission(&client).await;

    remove_queue(&queue_name).await;
    assert!(
        !spooler_has_queue(&queue_name).await,
        "the smoke queue was not removed"
    );
    client.stop().await;
}
