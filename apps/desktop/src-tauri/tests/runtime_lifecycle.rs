//! End-to-end lifecycle of the desktop shell's runtime, exercised through the public API with the
//! real client proxy and server sharing services.

use std::sync::Arc;

use shaprint_desktop::adapters::{ClientProxyService, ServerSharingService};
use shaprint_desktop::application::RuntimeCoordinator;
use shaprint_desktop::domain::{ErrorCode, ServiceId, ServiceState};

fn coordinator() -> RuntimeCoordinator {
    RuntimeCoordinator::new(vec![
        Arc::new(ClientProxyService::new()),
        Arc::new(ServerSharingService::new()),
    ])
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
    let runtime = shaprint_desktop::runtime();

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
}

#[tokio::test]
async fn the_user_starts_and_stops_sharing() {
    let runtime = coordinator();

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
async fn closing_the_runtime_stops_every_background_task() {
    let runtime = coordinator();
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
    let runtime = RuntimeCoordinator::new(vec![Arc::new(ServerSharingService::new())]);

    let error = runtime
        .start(ServiceId::ClientProxy)
        .await
        .expect_err("rejected");
    assert_eq!(error.code(), ErrorCode::UnknownService);
    assert_eq!(error.code_str(), "unknown-service");

    let unknown = ServiceId::parse("scanner").expect_err("not a service");
    assert_eq!(unknown.code(), ErrorCode::UnknownService);
}
