//! User-facing local diagnostics through the application/control boundary, with only the
//! native spooler replaced. No private helpers or print payloads are involved.
use std::sync::Arc;

use async_trait::async_trait;
use shaprint_desktop::application::{
    LocalPrinterCatalog, LocalQueueInspection, LocalTroubleshooting, RuntimeCoordinator,
};
use shaprint_desktop::domain::{AppError, ClientQueueName, PrinterName, RecognisedClientQueue};
use shaprint_desktop::ipc::dto::LocalTroubleshootingDto;

struct NativeSpooler;

#[async_trait]
impl LocalPrinterCatalog for NativeSpooler {
    async fn local_printers(&self) -> Result<Vec<PrinterName>, AppError> {
        unreachable!("passive troubleshooting must not enumerate shareable printers")
    }

    async fn inspect_client_queue(
        &self,
        _target: &RecognisedClientQueue,
    ) -> Result<LocalQueueInspection, AppError> {
        Ok(LocalQueueInspection::Matching {
            pending_jobs: Some(0),
        })
    }
}

fn target() -> RecognisedClientQueue {
    RecognisedClientQueue::new(
        ClientQueueName::parse("Office (ShaPrint 192.0.2.10-48631)").unwrap(),
        "192.0.2.10:48631".to_owned(),
        PrinterName::parse("Office").unwrap(),
    )
}

struct SlowJobs;

#[async_trait]
impl LocalPrinterCatalog for SlowJobs {
    async fn local_printers(&self) -> Result<Vec<PrinterName>, AppError> {
        unreachable!()
    }
    async fn inspect_client_queue(
        &self,
        _: &RecognisedClientQueue,
    ) -> Result<LocalQueueInspection, AppError> {
        Ok(LocalQueueInspection::Matching { pending_jobs: None })
    }
    async fn pending_client_jobs(&self, _: &RecognisedClientQueue) -> Result<u32, AppError> {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        Ok(0)
    }
}

#[tokio::test(start_paused = true)]
async fn deadline_keeps_completed_queue_checks_and_does_not_treat_unread_jobs_as_empty() {
    let started = tokio::time::Instant::now();
    let snapshot = LocalTroubleshootingDto::from(
        LocalTroubleshooting::new(
            Arc::new(SlowJobs),
            Arc::new(RuntimeCoordinator::new(Vec::new())),
        )
        .inspect(target())
        .await,
    );
    // Leave one second of the overall 15-second budget for local IPC delivery.
    assert_eq!(started.elapsed(), std::time::Duration::from_secs(14));
    assert_eq!(snapshot.checks[0].status, "passed");
    assert_eq!(snapshot.checks[1].status, "passed");
    assert_eq!(snapshot.checks[2].status, "inconclusive");
    assert!(!snapshot.queue_empty);
    assert!(!snapshot.repair_enabled);
}

#[tokio::test]
async fn selected_queue_diagnostics_identify_an_empty_native_queue_without_enabling_mutations() {
    let diagnostics = LocalTroubleshooting::new(
        Arc::new(NativeSpooler),
        Arc::new(RuntimeCoordinator::new(Vec::new())),
    );
    let snapshot = LocalTroubleshootingDto::from(diagnostics.inspect(target()).await);
    assert_eq!(
        snapshot.target.queue_name,
        "Office (ShaPrint 192.0.2.10-48631)"
    );
    assert_eq!(snapshot.target.server_address, "192.0.2.10:48631");
    for id in ["spooler", "client-queue", "pending-jobs"] {
        let check = snapshot
            .checks
            .iter()
            .find(|check| check.check == id)
            .unwrap();
        assert_eq!(check.status, "passed", "{id}");
    }
    assert!(!snapshot.test_print_enabled);
    assert!(!snapshot.repair_enabled);
    assert!(snapshot.attempted_actions.is_empty());
    assert!(!snapshot.version.is_empty());
}

#[cfg(unix)]
#[tokio::test]
async fn unavailable_native_cups_spooler_is_a_failure_not_an_empty_or_missing_queue() {
    use shaprint_desktop::adapters::printers::{cups::CupsServerEndpoint, CupsPrinterCatalog};
    let socket = format!("/tmp/shaprint-missing-cups-{}.sock", std::process::id());
    let catalog = CupsPrinterCatalog::with_endpoint(
        CupsServerEndpoint::parse(&socket),
        "127.0.0.1:48632".to_owned(),
    );
    let snapshot = LocalTroubleshootingDto::from(
        LocalTroubleshooting::new(
            Arc::new(catalog),
            Arc::new(RuntimeCoordinator::new(Vec::new())),
        )
        .inspect(target())
        .await,
    );
    let spooler = snapshot
        .checks
        .iter()
        .find(|check| check.check == "spooler")
        .unwrap();
    assert_eq!(spooler.status, "failed");
    assert_eq!(spooler.code.as_deref(), Some("queue-unavailable"));
    assert!(!snapshot.queue_empty);
    assert_eq!(snapshot.checks[1].status, "inconclusive");
}

struct UnreadableJobs;

#[async_trait]
impl LocalPrinterCatalog for UnreadableJobs {
    async fn local_printers(&self) -> Result<Vec<PrinterName>, AppError> {
        unreachable!()
    }
    async fn inspect_client_queue(
        &self,
        _: &RecognisedClientQueue,
    ) -> Result<LocalQueueInspection, AppError> {
        Ok(LocalQueueInspection::Matching { pending_jobs: None })
    }
    async fn pending_client_jobs(&self, _: &RecognisedClientQueue) -> Result<u32, AppError> {
        Err(AppError::not_authorized("untrusted native diagnostic text"))
    }
}

#[tokio::test]
async fn unreadable_pending_work_is_inconclusive_without_exposing_native_error_text() {
    let snapshot = LocalTroubleshootingDto::from(
        LocalTroubleshooting::new(
            Arc::new(UnreadableJobs),
            Arc::new(RuntimeCoordinator::new(Vec::new())),
        )
        .inspect(target())
        .await,
    );
    assert_eq!(snapshot.checks[1].status, "passed");
    assert_eq!(snapshot.checks[2].status, "inconclusive");
    assert!(!snapshot.queue_empty);
    assert!(!serde_json::to_string(&snapshot)
        .unwrap()
        .contains("untrusted native diagnostic text"));
}

struct MockServerInspector {
    inspection: shaprint_desktop::application::ServerInspection,
}

#[async_trait]
impl shaprint_desktop::application::ServerInspector for MockServerInspector {
    async fn inspect_server(
        &self,
        _server_address: &str,
        _printer_name: &str,
    ) -> Result<shaprint_desktop::application::ServerInspection, AppError> {
        Ok(self.inspection.clone())
    }
}

struct MockChannel {
    configured: bool,
}

impl shaprint_desktop::application::ChannelState for MockChannel {
    fn is_configured(&self) -> bool {
        self.configured
    }
}

struct MockAuthorization {
    rejected: bool,
}

impl shaprint_desktop::application::ObservedAuthorization for MockAuthorization {
    fn is_rejected(&self, _printer_name: &str) -> bool {
        self.rejected
    }
}

#[tokio::test]
async fn reachable_server_with_matching_identity_and_shared_printer_passes_checks() {
    use shaprint_desktop::application::{ServerIdentityInspection, ServerInspection};
    let diagnostics = LocalTroubleshooting::new(
        Arc::new(NativeSpooler),
        Arc::new(RuntimeCoordinator::new(Vec::new())),
    )
    .with_server_inspection(
        Arc::new(MockServerInspector {
            inspection: ServerInspection::Reachable {
                identity: ServerIdentityInspection::Matching,
                printer_shared: Some(true),
            },
        }),
        Arc::new(MockChannel { configured: true }),
        Arc::new(MockAuthorization { rejected: false }),
    );

    let snapshot = LocalTroubleshootingDto::from(diagnostics.inspect(target()).await);
    let check = |id: &str| {
        snapshot
            .checks
            .iter()
            .find(|c| c.check == id)
            .unwrap_or_else(|| panic!("missing check {id}"))
    };

    assert_eq!(check("server-reachability").status, "passed");
    assert_eq!(check("server-identity").status, "passed");
    assert_eq!(check("shared-printer").status, "passed");
    // Stored credentials alone must not claim authorized
    let channel = check("network-channel");
    assert_eq!(channel.status, "inconclusive");
    assert!(channel.code.is_none());
}

#[tokio::test]
async fn unreachable_server_reports_reachability_failure_without_firewall_diagnosis() {
    use shaprint_desktop::application::ServerInspection;
    let diagnostics = LocalTroubleshooting::new(
        Arc::new(NativeSpooler),
        Arc::new(RuntimeCoordinator::new(Vec::new())),
    )
    .with_server_inspection(
        Arc::new(MockServerInspector {
            inspection: ServerInspection::Unreachable,
        }),
        Arc::new(MockChannel { configured: true }),
        Arc::new(MockAuthorization { rejected: false }),
    );

    let snapshot = LocalTroubleshootingDto::from(diagnostics.inspect(target()).await);
    let check = |id: &str| snapshot.checks.iter().find(|c| c.check == id).unwrap();

    let reachability = check("server-reachability");
    assert_eq!(reachability.status, "failed");
    assert_eq!(reachability.code.as_deref(), Some("server-unavailable"));

    assert_eq!(check("server-identity").status, "not-applicable");
    assert_eq!(check("shared-printer").status, "not-applicable");

    let firewall = check("firewall");
    assert_ne!(firewall.status, "failed");
    assert_eq!(firewall.status, "not-applicable");
    assert!(firewall.code.is_none());
}

#[tokio::test]
async fn server_identity_changed_reports_identity_failure_and_blocks_shared_printer_check() {
    use shaprint_desktop::application::{ServerIdentityInspection, ServerInspection};
    let diagnostics = LocalTroubleshooting::new(
        Arc::new(NativeSpooler),
        Arc::new(RuntimeCoordinator::new(Vec::new())),
    )
    .with_server_inspection(
        Arc::new(MockServerInspector {
            inspection: ServerInspection::Reachable {
                identity: ServerIdentityInspection::Changed,
                printer_shared: None,
            },
        }),
        Arc::new(MockChannel { configured: true }),
        Arc::new(MockAuthorization { rejected: false }),
    );

    let snapshot = LocalTroubleshootingDto::from(diagnostics.inspect(target()).await);
    let check = |id: &str| snapshot.checks.iter().find(|c| c.check == id).unwrap();

    assert_eq!(check("server-reachability").status, "passed");
    let identity = check("server-identity");
    assert_eq!(identity.status, "failed");
    assert_eq!(identity.code.as_deref(), Some("server-identity-changed"));
    assert_eq!(check("shared-printer").status, "not-applicable");
}

#[tokio::test]
async fn unapproved_server_reports_untrusted_identity_failure() {
    use shaprint_desktop::application::{ServerIdentityInspection, ServerInspection};
    let diagnostics = LocalTroubleshooting::new(
        Arc::new(NativeSpooler),
        Arc::new(RuntimeCoordinator::new(Vec::new())),
    )
    .with_server_inspection(
        Arc::new(MockServerInspector {
            inspection: ServerInspection::Reachable {
                identity: ServerIdentityInspection::NotTrusted,
                printer_shared: None,
            },
        }),
        Arc::new(MockChannel { configured: false }),
        Arc::new(MockAuthorization { rejected: false }),
    );

    let snapshot = LocalTroubleshootingDto::from(diagnostics.inspect(target()).await);
    let check = |id: &str| snapshot.checks.iter().find(|c| c.check == id).unwrap();

    let identity = check("server-identity");
    assert_eq!(identity.status, "failed");
    assert_eq!(identity.code.as_deref(), Some("server-untrusted"));
    assert_eq!(check("shared-printer").status, "not-applicable");
}

#[tokio::test]
async fn unshared_printer_reports_printer_not_shared_failure() {
    use shaprint_desktop::application::{ServerIdentityInspection, ServerInspection};
    let diagnostics = LocalTroubleshooting::new(
        Arc::new(NativeSpooler),
        Arc::new(RuntimeCoordinator::new(Vec::new())),
    )
    .with_server_inspection(
        Arc::new(MockServerInspector {
            inspection: ServerInspection::Reachable {
                identity: ServerIdentityInspection::Matching,
                printer_shared: Some(false),
            },
        }),
        Arc::new(MockChannel { configured: true }),
        Arc::new(MockAuthorization { rejected: false }),
    );

    let snapshot = LocalTroubleshootingDto::from(diagnostics.inspect(target()).await);
    let check = |id: &str| snapshot.checks.iter().find(|c| c.check == id).unwrap();

    assert_eq!(check("server-reachability").status, "passed");
    assert_eq!(check("server-identity").status, "passed");
    let printer = check("shared-printer");
    assert_eq!(printer.status, "failed");
    assert_eq!(printer.code.as_deref(), Some("printer-not-shared"));
}

#[tokio::test]
async fn observed_authorization_rejection_reports_channel_failure() {
    use shaprint_desktop::application::{ServerIdentityInspection, ServerInspection};
    let diagnostics = LocalTroubleshooting::new(
        Arc::new(NativeSpooler),
        Arc::new(RuntimeCoordinator::new(Vec::new())),
    )
    .with_server_inspection(
        Arc::new(MockServerInspector {
            inspection: ServerInspection::Reachable {
                identity: ServerIdentityInspection::Matching,
                printer_shared: Some(true),
            },
        }),
        Arc::new(MockChannel { configured: true }),
        Arc::new(MockAuthorization { rejected: true }),
    );

    let snapshot = LocalTroubleshootingDto::from(diagnostics.inspect(target()).await);
    let check = |id: &str| snapshot.checks.iter().find(|c| c.check == id).unwrap();

    let channel = check("network-channel");
    assert_eq!(channel.status, "failed");
    assert_eq!(channel.code.as_deref(), Some("not-authorized"));
}

#[cfg(all(windows, feature = "desktop"))]
#[tokio::test]
#[ignore = "requires SHAPRINT_WINDOWS_DIAGNOSTICS_QUEUE naming an installed ShaPrint Client queue"]
async fn windows_native_queue_diagnostics_are_passive_and_return_job_state() {
    use shaprint_desktop::{ipc::control::ControlPlane, Shell};
    let name = std::env::var("SHAPRINT_WINDOWS_DIAGNOSTICS_QUEUE")
        .expect("set the installed Client queue name");
    let directory = std::env::temp_dir().join(format!(
        "shaprint-windows-diagnostics-{}",
        std::process::id()
    ));
    let shell = Arc::new(Shell::new(&directory).unwrap());
    let queue = shell
        .printer_catalog()
        .recognised_client_queues()
        .await
        .unwrap()
        .into_iter()
        .find(|queue| queue.queue_name().as_str() == name)
        .expect("Client queue must exist");
    let snapshot = ControlPlane::local(shell)
        .inspect_local_client_queue(queue.into())
        .await
        .unwrap();
    assert_eq!(snapshot.checks[0].status, "passed");
    assert_eq!(snapshot.checks[1].status, "passed");
    assert!(matches!(
        snapshot.checks[2].status.as_str(),
        "passed" | "failed"
    ));
    assert!(!snapshot.test_print_enabled && !snapshot.repair_enabled);
    std::fs::remove_dir_all(directory).unwrap();
}
