//! Integration tests for in-flight print job tracking and safe restart drain guard (#55).
//!
//! Acceptance criteria:
//! 1. In-flight Print job requests on the Client proxy and on the Server IPPS endpoint are tracked
//!    atomically from request arrival until completion or connection drop.
//! 2. The application reports that a restart is unsafe while one or more Print jobs are in flight,
//!    even if a connection fails or is cancelled mid-stream (no leaked counters).
//! 3. After the last in-flight Print job finishes, the drain guard holds restart readiness for the
//!    cooldown buffer (8 seconds) so the operating system spooler can finish dispatching the job.
//! 4. The tracker also exposes whether the application has been idle of Print jobs for the background
//!    auto-update quiet window (15 minutes).
//! 5. Automated integration tests verify that an active Print job blocks restart readiness, that
//!    completing the job and waiting out the cooldown unblocks readiness, and that aborted jobs
//!    always release their lease.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use shaprint_desktop::adapters::client_connections::ClientConnections;
use shaprint_desktop::adapters::ipps::{IppsServer, NetworkChannel};
use shaprint_desktop::adapters::ClientProxyService;
use shaprint_desktop::application::{
    PrintFailures, PrintJob, PrintJobSubmitter, PrintJobTracker, RuntimeCoordinator, Sharing,
};
use shaprint_desktop::domain::{AppError, PrinterName, ServiceId};

mod support;
use support::FakeCatalog;

const TEST_QUEUE: &str = "Tracked-Printer";

fn print_job(tracker: &PrintJobTracker) -> shaprint_desktop::application::RequestLease {
    let mut lease = tracker
        .try_acquire_request()
        .expect("tracker accepts requests");
    assert!(lease.mark_print_job());
    lease
}

/// A submitter whose completion can be delayed to hold a job in-flight deterministically.
struct BlockingSubmitter {
    started_notify: Arc<tokio::sync::Notify>,
    continue_notify: Arc<tokio::sync::Notify>,
    received: Arc<AtomicBool>,
}

impl BlockingSubmitter {
    fn new() -> (
        Self,
        Arc<tokio::sync::Notify>,
        Arc<tokio::sync::Notify>,
        Arc<AtomicBool>,
    ) {
        let started = Arc::new(tokio::sync::Notify::new());
        let cont = Arc::new(tokio::sync::Notify::new());
        let received = Arc::new(AtomicBool::new(false));
        (
            Self {
                started_notify: Arc::clone(&started),
                continue_notify: Arc::clone(&cont),
                received: Arc::clone(&received),
            },
            started,
            cont,
            received,
        )
    }
}

#[async_trait]
impl PrintJobSubmitter for BlockingSubmitter {
    async fn submit(&self, _printer: &PrinterName, _job: PrintJob) -> Result<u32, AppError> {
        self.received.store(true, Ordering::SeqCst);
        self.started_notify.notify_waiters();
        self.continue_notify.notified().await;
        Ok(101)
    }

    fn is_available(&self) -> bool {
        true
    }
}

fn text_attribute(body: &mut Vec<u8>, tag: u8, name: &str, value: &str) {
    body.push(tag);
    body.extend((name.len() as u16).to_be_bytes());
    body.extend(name.as_bytes());
    body.extend((value.len() as u16).to_be_bytes());
    body.extend(value.as_bytes());
}

fn sample_print_job_request(printer_uri: &str, channel_secret: &str) -> Vec<u8> {
    let mut body = vec![2, 0, 0, 2, 0, 0, 0, 9, 1];
    text_attribute(&mut body, 0x47, "attributes-charset", "utf-8");
    text_attribute(&mut body, 0x48, "attributes-natural-language", "en");
    text_attribute(&mut body, 0x45, "printer-uri", printer_uri);
    text_attribute(&mut body, 0x49, "document-format", "image/pwg-raster");
    text_attribute(&mut body, 0x41, "network-channel", channel_secret);
    body.push(0x02);
    text_attribute(&mut body, 0x44, "media", "iso_a4_210x297mm");
    body.push(3);
    body.extend(b"opaque printer document bytes");
    body
}

#[tokio::test]
async fn server_tracks_in_flight_print_job_and_holds_drain_cooldown() {
    let tracker = Arc::new(PrintJobTracker::new());
    let (submitter, started, cont, received) = BlockingSubmitter::new();

    let channel = Arc::new(NetworkChannel::in_memory());
    channel
        .configure("secret123")
        .await
        .expect("configures channel");

    let sharing = Arc::new(Sharing::new(FakeCatalog::new(&[TEST_QUEUE])));
    sharing
        .set_shared(vec![PrinterName::parse(TEST_QUEUE).expect("valid queue")])
        .await
        .expect("selects queue");

    let identity =
        Arc::new(shaprint_desktop::adapters::identity::ServerIdentity::generate().unwrap());
    let endpoint = Arc::new(IppsServer::new(
        0,
        identity,
        Arc::clone(&channel),
        Arc::new(submitter),
        Arc::new(PrintFailures::new()),
        Arc::clone(&tracker),
    ));

    let server_runtime = Arc::new(RuntimeCoordinator::new(vec![Arc::new(
        support::sharing_service(sharing, Arc::clone(&endpoint)),
    )]));
    server_runtime
        .start(ServiceId::ServerSharing)
        .await
        .expect("starts server");

    let address = endpoint.bound_address().expect("server listening");

    // Initially tracker has 0 active jobs and restart is safe
    assert_eq!(tracker.active_count(), 0);
    assert!(tracker.is_restart_safe());

    let printer_uri = format!(
        "ipps://127.0.0.1:{}/ipp/print/{}",
        address.port(),
        TEST_QUEUE
    );
    let ipp_payload = sample_print_job_request(&printer_uri, "secret123");
    let client = support::IppClient::new(address.port(), endpoint.fingerprint().to_string());

    // Spawn client sending print job in background
    let send_task = tokio::spawn(async move { client.post(&ipp_payload).await });

    // Wait until submitter starts processing the job
    started.notified().await;
    assert!(received.load(Ordering::SeqCst));

    // While job is in flight, active count is 1 and restart is unsafe!
    assert_eq!(tracker.active_count(), 1);
    assert!(!tracker.is_restart_safe());
    assert!(!tracker.is_restart_safe_with_cooldown(Duration::from_millis(50)));

    // Let the job complete
    cont.notify_waiters();
    let (status, body) = send_task
        .await
        .expect("task completes")
        .expect("request succeeds");
    assert_eq!(status, 200);
    assert!(!body.is_empty());

    // Once complete, active count drops back to 0 immediately
    assert_eq!(tracker.active_count(), 0);

    // But cooldown (50ms for test) holds restart readiness!
    let test_cooldown = Duration::from_millis(60);
    assert!(!tracker.is_restart_safe_with_cooldown(test_cooldown));

    // After cooldown expires, restart is unblocked
    tokio::time::sleep(test_cooldown + Duration::from_millis(15)).await;
    assert!(tracker.is_restart_safe_with_cooldown(test_cooldown));

    server_runtime.shutdown().await.expect("shuts down server");
}

#[tokio::test]
async fn aborted_or_dropped_jobs_release_lease_without_leaking_counters() {
    let tracker = Arc::new(PrintJobTracker::new());
    assert_eq!(tracker.active_count(), 0);

    {
        let _lease = print_job(&tracker);
        assert_eq!(tracker.active_count(), 1);
        assert!(!tracker.is_restart_safe());
        // Simulating aborted / dropped connection: lease drops here
    }

    assert_eq!(tracker.active_count(), 0);
    // Restart is safe once cooldown elapses
    let test_cooldown = Duration::from_millis(20);
    tokio::time::sleep(test_cooldown + Duration::from_millis(5)).await;
    assert!(tracker.is_restart_safe_with_cooldown(test_cooldown));
}

#[tokio::test]
async fn quiet_window_exposes_continuous_idle_duration() {
    let tracker = Arc::new(PrintJobTracker::new());
    let short_quiet_window = Duration::from_millis(40);

    tokio::time::sleep(short_quiet_window + Duration::from_millis(5)).await;
    assert!(tracker.is_idle_for(short_quiet_window));

    // Active print job resets idle state
    let lease = print_job(&tracker);
    assert!(!tracker.is_idle_for(short_quiet_window));

    drop(lease);
    // Immediately after completing, idle window is reset to 0
    assert!(!tracker.is_idle_for(short_quiet_window));

    // After waiting short_quiet_window again
    tokio::time::sleep(short_quiet_window + Duration::from_millis(5)).await;
    assert!(tracker.is_idle_for(short_quiet_window));
}

#[tokio::test]
async fn client_proxy_tracks_in_flight_print_job_and_releases_counter() {
    let server_tracker = Arc::new(PrintJobTracker::new());
    let proxy_tracker = Arc::new(PrintJobTracker::new());

    let (submitter, started, cont, received) = BlockingSubmitter::new();

    let channel = Arc::new(NetworkChannel::in_memory());
    channel
        .configure("proxy_secret")
        .await
        .expect("configures server channel");

    let sharing = Arc::new(Sharing::new(FakeCatalog::new(&[TEST_QUEUE])));
    sharing
        .set_shared(vec![PrinterName::parse(TEST_QUEUE).expect("valid queue")])
        .await
        .expect("selects queue");

    let identity =
        Arc::new(shaprint_desktop::adapters::identity::ServerIdentity::generate().unwrap());
    let endpoint = Arc::new(IppsServer::new(
        0,
        identity,
        Arc::clone(&channel),
        Arc::new(submitter),
        Arc::new(PrintFailures::new()),
        Arc::clone(&server_tracker),
    ));

    let server_runtime = Arc::new(RuntimeCoordinator::new(vec![Arc::new(
        support::sharing_service(sharing, Arc::clone(&endpoint)),
    )]));
    server_runtime
        .start(ServiceId::ServerSharing)
        .await
        .expect("starts server");
    let server_addr = endpoint.bound_address().expect("server listening");

    let client_connections = Arc::new(
        ClientConnections::new(support::temporary_directory("drain-client-trust"))
            .expect("opens client trust store"),
    );
    let review = client_connections
        .inspect(&format!("127.0.0.1:{}", server_addr.port()))
        .await
        .expect("inspects server identity");
    client_connections
        .approve(&review.address, &review.current_fingerprint)
        .await
        .expect("approves server identity");

    let client_channel = Arc::new(NetworkChannel::in_memory());
    client_channel
        .configure("proxy_secret")
        .await
        .expect("configures client channel");

    let proxy = Arc::new(ClientProxyService::with_port(
        Arc::clone(&client_connections),
        client_channel,
        Arc::new(PrintFailures::new()),
        Arc::clone(&proxy_tracker),
        0,
    ));

    let client_runtime = Arc::new(RuntimeCoordinator::new(vec![proxy.clone()]));
    client_runtime
        .start_autostart()
        .await
        .expect("starts client proxy");
    let proxy_addr = proxy.bound_address().expect("proxy listening");

    assert_eq!(proxy_tracker.active_count(), 0);
    assert!(proxy_tracker.is_restart_safe());

    let queue_uri = shaprint_desktop::adapters::client_queue_uri(
        &proxy_addr.to_string(),
        &format!("127.0.0.1:{}", server_addr.port()),
        TEST_QUEUE,
    )
    .expect("constructs queue uri");

    let ipp_payload = sample_print_job_request(&queue_uri, "proxy_secret");

    // Client sends plain HTTP POST to the local client proxy
    let proxy_port = proxy_addr.port();
    let send_task = tokio::spawn(async move {
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", proxy_port))
            .await
            .expect("connects to proxy");
        let head = format!(
            "POST /ipp/print HTTP/1.1\r\nHost: 127.0.0.1:{proxy_port}\r\nContent-Type: application/ipp\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            ipp_payload.len()
        );
        tokio::io::AsyncWriteExt::write_all(&mut stream, head.as_bytes())
            .await
            .unwrap();
        tokio::io::AsyncWriteExt::write_all(&mut stream, &ipp_payload)
            .await
            .unwrap();
        tokio::io::AsyncWriteExt::flush(&mut stream).await.unwrap();

        let mut res = Vec::new();
        tokio::io::AsyncReadExt::read_to_end(&mut stream, &mut res)
            .await
            .unwrap();
        res
    });

    // Submitter should receive the forwarded job
    started.notified().await;
    assert!(received.load(Ordering::SeqCst));

    // Both client proxy and server tracker count the active job!
    assert_eq!(proxy_tracker.active_count(), 1);
    assert_eq!(server_tracker.active_count(), 1);
    assert!(!proxy_tracker.is_restart_safe());
    assert!(!server_tracker.is_restart_safe());

    // Unblock the submitter
    cont.notify_waiters();
    let response = send_task.await.expect("task completes");
    assert!(!response.is_empty());

    // Once finished, active counts drop to 0
    assert_eq!(proxy_tracker.active_count(), 0);
    assert_eq!(server_tracker.active_count(), 0);

    let test_cooldown = Duration::from_millis(50);
    assert!(!proxy_tracker.is_restart_safe_with_cooldown(test_cooldown));
    assert!(!server_tracker.is_restart_safe_with_cooldown(test_cooldown));

    tokio::time::sleep(test_cooldown + Duration::from_millis(15)).await;
    assert!(proxy_tracker.is_restart_safe_with_cooldown(test_cooldown));
    assert!(server_tracker.is_restart_safe_with_cooldown(test_cooldown));

    client_runtime
        .shutdown()
        .await
        .expect("stops client runtime");
    server_runtime
        .shutdown()
        .await
        .expect("stops server runtime");
}
