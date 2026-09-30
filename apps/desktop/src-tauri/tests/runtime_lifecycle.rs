//! End-to-end lifecycle of the desktop shell's runtime, exercised through the public API with the
//! real client proxy and server sharing services.

mod support;

use std::sync::Arc;

use shaprint_desktop::adapters::ServerSharingService;
use shaprint_desktop::application::{RuntimeCoordinator, Sharing};
use shaprint_desktop::domain::{ErrorCode, ServiceId, ServiceState};
use shaprint_desktop::Shell;

use support::{printer_names, sharing_runtime, temporary_directory};

/// A coordinator with both supervised services, sharing over the fake printer catalog.
fn coordinator(queues: &[&str]) -> (RuntimeCoordinator, Arc<Sharing>) {
    let (sharing, endpoint) = sharing_runtime(queues);
    let runtime = RuntimeCoordinator::new(vec![
        Arc::new(shaprint_desktop::adapters::ClientProxyService::new()),
        Arc::new(ServerSharingService::new(Arc::clone(&sharing), endpoint)),
    ]);
    (runtime, sharing)
}

fn state(runtime: &RuntimeCoordinator, id: ServiceId) -> ServiceState {
    runtime
        .status()
        .service(id)
        .map(|status| status.state())
        .unwrap_or_else(|| panic!("no status for {}", id.as_str()))
}

#[tokio::test]
async fn launching_the_shell_runs_the_proxy_and_leaves_sharing_stopped() {
    let directory = temporary_directory("lifecycle");
    let shell = Shell::new(&directory).expect("builds the shell");
    let runtime = shell.runtime();

    runtime.start_autostart().await.expect("autostart succeeds");

    assert_eq!(
        state(&runtime, ServiceId::ClientProxy),
        ServiceState::Running
    );
    assert_eq!(
        state(&runtime, ServiceId::ServerSharing),
        ServiceState::Stopped
    );
    runtime.shutdown().await.expect("shutdown succeeds");
    std::fs::remove_dir_all(&directory).ok();
}

#[tokio::test]
async fn the_user_starts_and_stops_sharing() {
    let (runtime, sharing) = coordinator(&["HP LaserJet"]);
    sharing
        .set_shared(printer_names(&["HP LaserJet"]))
        .await
        .expect("selects a printer to share");

    runtime
        .start(ServiceId::ServerSharing)
        .await
        .expect("sharing starts");
    assert_eq!(
        state(&runtime, ServiceId::ServerSharing),
        ServiceState::Running
    );

    runtime
        .stop(ServiceId::ServerSharing)
        .await
        .expect("sharing stops");
    assert_eq!(
        state(&runtime, ServiceId::ServerSharing),
        ServiceState::Stopped
    );

    runtime.shutdown().await.expect("shutdown succeeds");
}

#[tokio::test]
async fn sharing_cannot_start_before_a_printer_is_selected() {
    let (runtime, _sharing) = coordinator(&["HP LaserJet", "Zebra"]);

    let error = runtime
        .start(ServiceId::ServerSharing)
        .await
        .expect_err("rejected");

    assert_eq!(error.code(), ErrorCode::InvalidState);
    assert_eq!(
        error.message(),
        "select at least one printer to share before starting"
    );
    // The user's configuration gap is not a service failure: sharing stays startable.
    assert_eq!(
        state(&runtime, ServiceId::ServerSharing),
        ServiceState::Stopped
    );
    runtime.shutdown().await.expect("shutdown succeeds");
}

#[tokio::test]
async fn closing_the_runtime_stops_every_background_task() {
    let (runtime, sharing) = coordinator(&["HP LaserJet"]);
    sharing
        .set_shared(printer_names(&["HP LaserJet"]))
        .await
        .expect("selects a printer to share");
    runtime.start_autostart().await.expect("autostart succeeds");
    runtime
        .start(ServiceId::ServerSharing)
        .await
        .expect("sharing starts");

    runtime.shutdown().await.expect("shutdown succeeds");

    let status = runtime.status();
    assert_eq!(status.services().len(), 2);
    for service in status.services() {
        assert_eq!(
            service.state(),
            ServiceState::Stopped,
            "{} still has a live task",
            service.id().as_str()
        );
    }
    // A second shutdown finds nothing left to stop.
    runtime.shutdown().await.expect("shutdown is idempotent");
}

#[tokio::test]
async fn services_the_shell_does_not_own_are_rejected_with_a_stable_code() {
    let (sharing, endpoint) = sharing_runtime(&["HP LaserJet"]);
    let runtime =
        RuntimeCoordinator::new(vec![Arc::new(ServerSharingService::new(sharing, endpoint))]);

    let error = runtime
        .start(ServiceId::ClientProxy)
        .await
        .expect_err("rejected");
    assert_eq!(error.code(), ErrorCode::UnknownService);
    assert_eq!(error.code_str(), "unknown-service");

    let unknown = ServiceId::parse("scanner").expect_err("not a service");
    assert_eq!(unknown.code(), ErrorCode::UnknownService);
}
