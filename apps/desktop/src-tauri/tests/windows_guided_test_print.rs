//! Native route qualification fixture. It creates a disposable Windows Client queue and
//! submits through GDI, the real spooler, local proxy and a TLS Server with no physical output.
#![cfg(all(windows, feature = "desktop"))]
mod support;
use async_trait::async_trait;
use ring::rand::{SecureRandom, SystemRandom};
use shaprint_desktop::{
    adapters::{ipps::NetworkChannel, IppsServer, ServerIdentity},
    application::{
        PrintFailures, PrintJob, PrintJobSubmitter, PrintJobTracker, PrintSettings,
        RuntimeCoordinator, Sharing,
    },
    domain::{AppError, ClientQueueRequest, PrinterName, ServiceId},
    ipc::control::ControlPlane,
    RuntimePorts, Shell,
};
use std::{
    process::Command,
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Default)]
struct Received(Mutex<Vec<(String, PrintSettings, usize)>>);
#[async_trait]
impl PrintJobSubmitter for Received {
    async fn submit(&self, printer: &PrinterName, job: PrintJob) -> Result<u32, AppError> {
        // Decode the actual native driver's output without exporting or recording page bytes.
        let pages = shaprint_desktop::adapters::printers::raster::pwg_pages(job.document())?
            .collect::<Result<Vec<_>, _>>()?;
        self.0.lock().unwrap().push((
            printer.as_str().to_owned(),
            job.settings().clone(),
            pages.len(),
        ));
        Ok(1)
    }
}
fn config(queue: &str) -> Vec<u8> {
    let output = Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command", "$ErrorActionPreference='Stop'; Get-PrintConfiguration -PrinterName $env:SHAPRINT_TEST_QUEUE | Select-Object PaperSize,Color,DuplexingMode,Collate | ConvertTo-Json -Compress"])
        .env("SHAPRINT_TEST_QUEUE", queue).output().unwrap();
    assert!(output.status.success(), "reads disposable queue defaults");
    output.stdout
}
struct QueueCleanup(String);
impl Drop for QueueCleanup {
    fn drop(&mut self) {
        let _ = Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command", "Get-Printer -Name $env:SHAPRINT_TEST_QUEUE -ErrorAction SilentlyContinue | Remove-Printer -ErrorAction SilentlyContinue"])
            .env("SHAPRINT_TEST_QUEUE", &self.0).output();
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires isolated elevated Windows with Microsoft IPP Class Driver; installs a disposable queue, no physical printer"]
async fn native_client_queue_reaches_selected_server_without_changing_defaults() {
    let root = support::temporary_directory("windows-guided-test");
    let shell = Arc::new(
        Shell::with_ports(
            &root,
            RuntimePorts {
                proxy_port: 48632,
                server_port: 0,
                discovery_ports: vec![0],
            },
        )
        .unwrap(),
    );
    let printer = format!("Guided test {}", std::process::id());
    let mut secret = [0u8; 32];
    SystemRandom::new().fill(&mut secret).unwrap();
    let channel = hex::encode(secret);
    shell.network_channel().configure(&channel).await.unwrap();
    let server_channel = Arc::new(NetworkChannel::in_memory());
    server_channel.configure(&channel).await.unwrap();
    let sharing = Arc::new(Sharing::new(support::FakeCatalog::new(&[&printer])));
    sharing
        .set_shared(support::printer_names(&[&printer]))
        .await
        .unwrap();
    let received = Arc::new(Received::default());
    let endpoint = Arc::new(IppsServer::new(
        0,
        Arc::new(ServerIdentity::generate().unwrap()),
        server_channel.clone(),
        received.clone(),
        Arc::new(PrintFailures::new()),
        Arc::new(PrintJobTracker::new()),
    ));
    let server = Arc::new(RuntimeCoordinator::new(vec![Arc::new(
        support::sharing_service_with_channel(sharing, endpoint.clone(), server_channel),
    )]));
    server.start(ServiceId::ServerSharing).await.unwrap();
    let address = format!("127.0.0.1:{}", endpoint.bound_address().unwrap().port());
    let review = shell.client_connections().inspect(&address).await.unwrap();
    shell
        .client_connections()
        .approve(&review.address, &review.current_fingerprint)
        .await
        .unwrap();
    shell.runtime().start(ServiceId::ClientProxy).await.unwrap();
    let request = ClientQueueRequest::new(&address, PrinterName::parse(&printer).unwrap()).unwrap();
    let queue = request.queue_name().to_string();
    // A process-unique queue must not already exist, and installer conflict protections apply.
    assert!(!shell
        .printer_catalog()
        .recognised_client_queues()
        .await
        .unwrap()
        .iter()
        .any(|q| q.queue_name().as_str() == queue));
    let unused_queue = queue.clone();
    let unused_printer = printer.clone();
    assert!(tokio::task::spawn_blocking(move || Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", "if ((Get-Printer -Name $env:SHAPRINT_TEST_QUEUE -ErrorAction SilentlyContinue) -or (Get-Printer -Name $env:SHAPRINT_TEST_PRINTER -ErrorAction SilentlyContinue)) { exit 1 }"])
        .env("SHAPRINT_TEST_QUEUE", unused_queue).env("SHAPRINT_TEST_PRINTER", unused_printer).status()).await.unwrap().unwrap().success(), "refuse an existing queue of any kind");
    let cleanup = QueueCleanup(queue.clone());
    tokio::task::spawn_blocking(move || {
        shaprint_desktop::adapters::queue_installation::platform_installer().install(&request)
    })
    .await
    .unwrap()
    .unwrap();
    let before_queue = queue.clone();
    let before = tokio::task::spawn_blocking(move || config(&before_queue))
        .await
        .unwrap();
    let target = shell
        .printer_catalog()
        .recognised_client_queues()
        .await
        .unwrap()
        .into_iter()
        .find(|q| q.queue_name().as_str() == queue)
        .unwrap();
    let control = ControlPlane::local(shell.clone());
    let ready = control
        .inspect_local_client_queue(target.into())
        .await
        .unwrap();
    assert!(
        ready.test_print_enabled,
        "native queue must have usable default media and healthy prerequisites"
    );
    let id = ready.session_id.unwrap();
    let sent = control.send_test_print(id).await.unwrap();
    assert_eq!(sent.submission.as_deref(), Some("accepted"));
    assert_eq!(sent.physical_result.as_deref(), Some("unconfirmed"));
    tokio::time::timeout(Duration::from_secs(90), async {
        loop {
            if !received.0.lock().unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("native spooler traverses proxy and selected TLS Server");
    {
        let jobs = received.0.lock().unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].0, printer);
        assert_eq!(jobs[0].2, 1);
        assert_eq!(jobs[0].1.copies, Some(1));
        assert_eq!(jobs[0].1.color, Some(false));
        assert_eq!(
            jobs[0].1.duplex,
            Some(shaprint_desktop::application::DuplexMode::Simplex)
        );
    }
    let after_queue = queue.clone();
    let after = tokio::task::spawn_blocking(move || config(&after_queue))
        .await
        .unwrap();
    assert_eq!(before, after, "saved queue defaults must not change");
    control.close_test_print_session(id).await.unwrap();
    shell.runtime().shutdown().await.unwrap();
    server.shutdown().await.unwrap();
    drop(cleanup);
    std::fs::remove_dir_all(root).unwrap();
}
