//! Native runtime smoke for #172. Requires an isolated elevated Windows runner; no printer,
//! existing queue, credentials, or interactive window is needed. Diagnostics use ControlPlane.
#![cfg(all(windows, feature = "desktop"))]

mod support;

use std::{
    process::{Command, Output},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

use async_trait::async_trait;
use ring::rand::{SecureRandom, SystemRandom};
use shaprint_desktop::{
    adapters::{ipps::NetworkChannel, IppsServer, ServerIdentity},
    application::{
        PrintFailures, PrintJob, PrintJobSubmitter, PrintJobTracker, RuntimeCoordinator, Sharing,
    },
    domain::{AppError, ClientQueueRequest, PrinterName, ServiceId},
    ipc::{
        control::ControlPlane,
        dto::{LocalTroubleshootingDto, RecognisedClientQueueDto},
    },
    RuntimePorts, Shell,
};

struct NoPhysicalPrinter(AtomicUsize);

#[async_trait]
impl PrintJobSubmitter for NoPhysicalPrinter {
    async fn submit(&self, _: &PrinterName, _: PrintJob) -> Result<u32, AppError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(AppError::unsupported(
            "this diagnostic fixture never prints",
        ))
    }
}

fn powershell(script: &str, queue: &str) -> Output {
    Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("SHAPRINT_DIAGNOSTICS_QUEUE", queue)
        .output()
        .expect("runs fixture PowerShell")
}

fn require_success(output: Output) {
    assert!(
        output.status.success(),
        "fixture setup failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Owns only the uniquely named queue created below; cleanup also runs on assertion failure.
struct QueueCleanup(String);

impl QueueCleanup {
    fn remove(&self) {
        require_success(powershell(
            r#"
$ErrorActionPreference = 'Stop'
$name = $env:SHAPRINT_DIAGNOSTICS_QUEUE
if (Get-Printer -Name $name -ErrorAction SilentlyContinue) {
    Get-PrintJob -PrinterName $name -ErrorAction SilentlyContinue | Remove-PrintJob -ErrorAction Stop
    Remove-Printer -Name $name -ErrorAction Stop
}
if (Get-Printer -Name $name -ErrorAction SilentlyContinue) { throw 'fixture queue was not removed' }
"#,
            &self.0,
        ));
    }
}

impl Drop for QueueCleanup {
    fn drop(&mut self) {
        // Restart only this disposable runner's spooler if an assertion interrupted its check.
        let _ = powershell("Start-Service Spooler -ErrorAction SilentlyContinue", "");
        let _ = powershell(
            r#"
$name = $env:SHAPRINT_DIAGNOSTICS_QUEUE
if (Get-Printer -Name $name -ErrorAction SilentlyContinue) {
    Get-PrintJob -PrinterName $name -ErrorAction SilentlyContinue | Remove-PrintJob -ErrorAction SilentlyContinue
    Remove-Printer -Name $name -ErrorAction SilentlyContinue
}
"#,
            &self.0,
        );
    }
}

fn outcome<'a>(snapshot: &'a LocalTroubleshootingDto, check: &str) -> &'a str {
    snapshot
        .checks
        .iter()
        .find(|row| row.check == check)
        .expect("check exists")
        .status
        .as_str()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires an isolated elevated Windows runner; creates a native queue and briefly stops its spooler"]
async fn windows_native_diagnostics_are_self_contained() {
    let tag = std::env::var("SHAPRINT_DIAGNOSTICS_RUN_ID")
        .unwrap_or_else(|_| std::process::id().to_string());
    assert!(!tag.is_empty() && tag.chars().all(|c| c.is_ascii_digit() || c == '-'));
    let printer_name = format!("ShaPrint Diagnostics {tag}");
    let root = support::temporary_directory("windows-native-diagnostics");
    let shell = Arc::new(
        Shell::with_ports(
            &root,
            RuntimePorts {
                proxy_port: 48632,
                server_port: 0,
                discovery_ports: vec![0],
            },
        )
        .expect("creates isolated desktop runtime"),
    );
    let mut bytes = [0u8; 32];
    SystemRandom::new()
        .fill(&mut bytes)
        .expect("generates runtime-only credentials");
    let channel = hex::encode(bytes);
    shell.network_channel().configure(&channel).await.unwrap();
    let server_channel = Arc::new(NetworkChannel::in_memory());
    server_channel.configure(&channel).await.unwrap();
    let sharing = Arc::new(Sharing::new(support::FakeCatalog::new(&[&printer_name])));
    sharing
        .set_shared(support::printer_names(&[&printer_name]))
        .await
        .unwrap();
    let physical = Arc::new(NoPhysicalPrinter(AtomicUsize::new(0)));
    let endpoint = Arc::new(IppsServer::new(
        0,
        Arc::new(ServerIdentity::generate().unwrap()),
        Arc::clone(&server_channel),
        physical.clone(),
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
    let request =
        ClientQueueRequest::new(&address, PrinterName::parse(&printer_name).unwrap()).unwrap();
    let queue_name = request.queue_name().to_string();
    let check_name = queue_name.clone();
    require_success(
        tokio::task::spawn_blocking(move || {
            powershell(
                r#"
$ErrorActionPreference = 'Stop'
$name = $env:SHAPRINT_DIAGNOSTICS_QUEUE
$plainName = $name -replace ' \(ShaPrint [^)]+\)$', ''
if ((Get-Printer -Name $name -ErrorAction SilentlyContinue) -or
    (Get-Printer -Name $plainName -ErrorAction SilentlyContinue)) {
    throw 'refusing to take over an existing queue'
}
"#,
                &check_name,
            )
        })
        .await
        .unwrap(),
    );
    let cleanup = QueueCleanup(queue_name.clone());
    tokio::task::spawn_blocking(move || {
        shaprint_desktop::adapters::queue_installation::platform_installer().install(&request)
    })
    .await
    .unwrap()
    .expect("installs a real native IPP Client queue");
    let control = ControlPlane::local(Arc::clone(&shell));
    let target = RecognisedClientQueueDto {
        queue_name: queue_name.clone(),
        server_address: address,
        printer_name,
    };
    let healthy = control
        .inspect_local_client_queue(target.clone())
        .await
        .unwrap();
    assert_eq!(outcome(&healthy, "spooler"), "passed");
    assert_eq!(outcome(&healthy, "client-queue"), "passed");
    assert_eq!(outcome(&healthy, "pending-jobs"), "passed");
    assert_eq!(outcome(&healthy, "client-proxy"), "passed");
    assert_eq!(outcome(&healthy, "server-reachability"), "passed");
    assert_eq!(outcome(&healthy, "server-identity"), "passed");
    assert_eq!(outcome(&healthy, "shared-printer"), "passed");
    assert_eq!(outcome(&healthy, "network-channel"), "inconclusive");
    assert_eq!(outcome(&healthy, "firewall"), "not-applicable");
    assert!(
        healthy.checks.iter().all(|check| check.status != "failed"),
        "{:?}",
        healthy.checks
    );
    assert!(healthy.queue_empty && !healthy.test_print_enabled && healthy.repair_enabled);
    assert!(healthy.attempted_actions.is_empty());
    println!("[native-diagnostics] matching empty queue, spooler and proxy: passed");

    let held_queue = queue_name.clone();
    require_success(
        tokio::task::spawn_blocking(move || {
            Command::new("powershell.exe")
                .args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-File",
                ])
                .arg(
                    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("tests/windows_diagnostics_hold_job.ps1"),
                )
                .env("SHAPRINT_DIAGNOSTICS_QUEUE", held_queue)
                .output()
                .unwrap()
        })
        .await
        .unwrap(),
    );
    for _ in 0..2 {
        let pending = control
            .inspect_local_client_queue(target.clone())
            .await
            .unwrap();
        assert_eq!(outcome(&pending, "pending-jobs"), "failed");
        assert!(!pending.queue_empty && !pending.test_print_enabled && !pending.repair_enabled);
    }
    assert_eq!(
        physical.0.load(Ordering::SeqCst),
        0,
        "diagnostics must not forward or submit print work"
    );
    println!("[native-diagnostics] repeated inspection preserves held work: passed");
    shell.runtime().stop(ServiceId::ClientProxy).await.unwrap();
    assert_eq!(
        outcome(
            &control
                .inspect_local_client_queue(target.clone())
                .await
                .unwrap(),
            "client-proxy"
        ),
        "failed"
    );
    println!("[native-diagnostics] stopped proxy: passed");

    let queue = queue_name.clone();
    require_success(
        tokio::task::spawn_blocking(move || {
            powershell(
                r#"
$ErrorActionPreference = 'Stop'
$name = $env:SHAPRINT_DIAGNOSTICS_QUEUE
Get-PrintJob -PrinterName $name | Remove-PrintJob -ErrorAction Stop
Set-Printer -Name $name -PortName 'FILE:' -ErrorAction Stop
"#,
                &queue,
            )
        })
        .await
        .unwrap(),
    );
    let mismatch = control
        .inspect_local_client_queue(target.clone())
        .await
        .unwrap();
    assert_eq!(outcome(&mismatch, "client-queue"), "failed");
    assert_eq!(outcome(&mismatch, "pending-jobs"), "not-applicable");
    println!("[native-diagnostics] mismatched destination: passed");
    cleanup.remove();
    assert_eq!(
        outcome(
            &control
                .inspect_local_client_queue(target.clone())
                .await
                .unwrap(),
            "client-queue"
        ),
        "failed"
    );
    println!("[native-diagnostics] missing queue and explicit cleanup: passed");

    require_success(
        tokio::task::spawn_blocking(|| {
            powershell(
                "$ErrorActionPreference = 'Stop'; Stop-Service Spooler -Force",
                "",
            )
        })
        .await
        .unwrap(),
    );
    let unavailable = control.inspect_local_client_queue(target).await.unwrap();
    require_success(
        tokio::task::spawn_blocking(|| {
            powershell("$ErrorActionPreference = 'Stop'; Start-Service Spooler", "")
        })
        .await
        .unwrap(),
    );
    assert_eq!(outcome(&unavailable, "spooler"), "failed");
    assert!(!unavailable.queue_empty);
    assert_eq!(physical.0.load(Ordering::SeqCst), 0);
    println!("[native-diagnostics] unavailable spooler and passive-only diagnostics: passed");
    shell.runtime().shutdown().await.unwrap();
    server.shutdown().await.unwrap();
    drop(cleanup);
    drop(control);
    drop(shell);
    std::fs::remove_dir_all(root).unwrap();
}
