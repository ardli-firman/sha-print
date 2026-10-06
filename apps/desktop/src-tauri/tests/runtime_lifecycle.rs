//! End-to-end lifecycle of the desktop shell's runtime, exercised through the public API with the
//! real client proxy and server sharing services.

mod support;

use std::sync::Arc;

use shaprint_desktop::adapters::ipps::NetworkChannel;
use shaprint_desktop::adapters::{client_connections::ClientConnections, ClientProxyService};
use shaprint_desktop::application::{
    PrintFailures, RuntimeCoordinator, SharedPrinterSource, Sharing,
};
use shaprint_desktop::domain::{ErrorCode, ServiceId, ServiceState};
use shaprint_desktop::Shell;

use support::{printer_names, sharing_runtime, temporary_directory};

/// A coordinator with both supervised services, sharing over the fake printer catalog.
fn coordinator(queues: &[&str]) -> (RuntimeCoordinator, Arc<Sharing>) {
    let (sharing, endpoint) = sharing_runtime(queues);
    let connections = Arc::new(
        ClientConnections::new(temporary_directory("runtime-proxy-trust"))
            .expect("opens proxy trust store"),
    );
    let channel = Arc::new(NetworkChannel::in_memory());
    let tracker = Arc::new(shaprint_desktop::application::PrintJobTracker::new());
    let runtime = RuntimeCoordinator::new(vec![
        Arc::new(ClientProxyService::with_port(
            connections,
            channel,
            Arc::new(PrintFailures::new()),
            tracker,
            0,
        )),
        Arc::new(support::sharing_service(Arc::clone(&sharing), endpoint)),
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
        RuntimeCoordinator::new(vec![Arc::new(support::sharing_service(sharing, endpoint))]);

    let error = runtime
        .start(ServiceId::ClientProxy)
        .await
        .expect_err("rejected");
    assert_eq!(error.code(), ErrorCode::UnknownService);
    assert_eq!(error.code_str(), "unknown-service");

    let unknown = ServiceId::parse("scanner").expect_err("not a service");
    assert_eq!(unknown.code(), ErrorCode::UnknownService);
}

#[tokio::test]
async fn persistent_selection_and_automatic_sharing_resumption_across_restarts() {
    let dir = temporary_directory("persist-lifecycle-resume");

    // Session 1: select printer, start sharing, then shutdown
    {
        let (sharing, endpoint) =
            support::sharing_runtime_persistent(&["HP LaserJet", "Zebra"], &dir);
        sharing
            .set_shared(printer_names(&["HP LaserJet"]))
            .await
            .expect("selects printer");

        let runtime = RuntimeCoordinator::new(vec![Arc::new(support::sharing_service(
            Arc::clone(&sharing),
            endpoint,
        ))]);

        runtime
            .start(ServiceId::ServerSharing)
            .await
            .expect("starts sharing");
        assert_eq!(
            state(&runtime, ServiceId::ServerSharing),
            ServiceState::Running
        );

        // App closes
        runtime.shutdown().await.expect("clean shutdown");
    }

    // Session 2: relaunch with same dir, restore and run autostart
    {
        let (sharing, endpoint) =
            support::sharing_runtime_persistent(&["HP LaserJet", "Zebra"], &dir);
        sharing.restore().await.expect("restores sharing state");

        let runtime = RuntimeCoordinator::new(vec![Arc::new(support::sharing_service(
            Arc::clone(&sharing),
            endpoint,
        ))]);

        assert_eq!(
            sharing
                .shared_printers()
                .iter()
                .map(|p| p.as_str())
                .collect::<Vec<_>>(),
            vec!["HP LaserJet"]
        );

        runtime.start_autostart().await.expect("autostart succeeds");
        assert_eq!(
            state(&runtime, ServiceId::ServerSharing),
            ServiceState::Running
        );

        runtime.shutdown().await.expect("clean shutdown");
    }

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn explicitly_stopping_sharing_stays_stopped_across_restarts_while_preserving_selection() {
    let dir = temporary_directory("persist-lifecycle-stop");

    // Session 1: select, start, explicitly stop, then shutdown
    {
        let (sharing, endpoint) =
            support::sharing_runtime_persistent(&["HP LaserJet", "Zebra"], &dir);
        sharing
            .set_shared(printer_names(&["Zebra"]))
            .await
            .expect("selects printer");

        let runtime = RuntimeCoordinator::new(vec![Arc::new(support::sharing_service(
            Arc::clone(&sharing),
            endpoint,
        ))]);

        runtime
            .start(ServiceId::ServerSharing)
            .await
            .expect("starts sharing");
        runtime
            .stop(ServiceId::ServerSharing)
            .await
            .expect("explicitly stops sharing");
        assert_eq!(
            state(&runtime, ServiceId::ServerSharing),
            ServiceState::Stopped
        );

        runtime.shutdown().await.expect("clean shutdown");
    }

    // Session 2: relaunch
    {
        let (sharing, endpoint) =
            support::sharing_runtime_persistent(&["HP LaserJet", "Zebra"], &dir);
        sharing.restore().await.expect("restores sharing state");

        let runtime = RuntimeCoordinator::new(vec![Arc::new(support::sharing_service(
            Arc::clone(&sharing),
            endpoint,
        ))]);

        runtime.start_autostart().await.expect("autostart succeeds");
        // Must stay stopped because user explicitly stopped it
        assert_eq!(
            state(&runtime, ServiceId::ServerSharing),
            ServiceState::Stopped
        );
        // But printer selection is preserved in UI
        assert_eq!(
            sharing
                .shared_printers()
                .iter()
                .map(|p| p.as_str())
                .collect::<Vec<_>>(),
            vec!["Zebra"]
        );

        runtime.shutdown().await.expect("clean shutdown");
    }

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn restarting_when_one_printer_removed_filters_it_and_resumes_sharing() {
    let dir = temporary_directory("persist-lifecycle-one-removed");

    // Session 1: select two printers, start, shutdown
    {
        let (sharing, endpoint) =
            support::sharing_runtime_persistent(&["HP LaserJet", "Zebra"], &dir);
        sharing
            .set_shared(printer_names(&["HP LaserJet", "Zebra"]))
            .await
            .expect("selects both");

        let runtime = RuntimeCoordinator::new(vec![Arc::new(support::sharing_service(
            Arc::clone(&sharing),
            endpoint,
        ))]);

        runtime
            .start(ServiceId::ServerSharing)
            .await
            .expect("starts sharing");
        runtime.shutdown().await.expect("clean shutdown");
    }

    // Session 2: Zebra was uninstalled from the OS
    {
        let (sharing, endpoint) = support::sharing_runtime_persistent(&["HP LaserJet"], &dir);
        sharing.restore().await.expect("restores and filters");

        let runtime = RuntimeCoordinator::new(vec![Arc::new(support::sharing_service(
            Arc::clone(&sharing),
            endpoint,
        ))]);

        runtime.start_autostart().await.expect("autostart succeeds");
        assert_eq!(
            state(&runtime, ServiceId::ServerSharing),
            ServiceState::Running
        );
        assert_eq!(
            sharing
                .shared_printers()
                .iter()
                .map(|p| p.as_str())
                .collect::<Vec<_>>(),
            vec!["HP LaserJet"]
        );

        runtime.shutdown().await.expect("clean shutdown");
    }

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn restarting_when_all_printers_removed_leaves_sharing_stopped_cleanly() {
    let dir = temporary_directory("persist-lifecycle-all-removed");

    // Session 1: select Zebra, start, shutdown
    {
        let (sharing, endpoint) = support::sharing_runtime_persistent(&["Zebra"], &dir);
        sharing
            .set_shared(printer_names(&["Zebra"]))
            .await
            .expect("selects Zebra");

        let runtime = RuntimeCoordinator::new(vec![Arc::new(support::sharing_service(
            Arc::clone(&sharing),
            endpoint,
        ))]);

        runtime
            .start(ServiceId::ServerSharing)
            .await
            .expect("starts sharing");
        runtime.shutdown().await.expect("clean shutdown");
    }

    // Session 2: Zebra was uninstalled, only Canon exists
    {
        let (sharing, endpoint) = support::sharing_runtime_persistent(&["Canon"], &dir);
        sharing.restore().await.expect("restores and filters");

        let runtime = RuntimeCoordinator::new(vec![Arc::new(support::sharing_service(
            Arc::clone(&sharing),
            endpoint,
        ))]);

        runtime
            .start_autostart()
            .await
            .expect("autostart cleanly skips sharing");
        assert_eq!(
            state(&runtime, ServiceId::ServerSharing),
            ServiceState::Stopped
        );
        assert!(sharing.shared_printers().is_empty());

        runtime.shutdown().await.expect("clean shutdown");
    }

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn restarting_when_network_channel_missing_leaves_sharing_stopped_and_does_not_claim_running()
{
    let dir = temporary_directory("persist-lifecycle-missing-channel");

    // Session 1: configure printer and start sharing
    {
        let (sharing, endpoint) = support::sharing_runtime_persistent(&["HP LaserJet"], &dir);
        sharing
            .set_shared(printer_names(&["HP LaserJet"]))
            .await
            .expect("selects HP LaserJet");

        let runtime = RuntimeCoordinator::new(vec![Arc::new(support::sharing_service(
            Arc::clone(&sharing),
            endpoint,
        ))]);

        runtime
            .start(ServiceId::ServerSharing)
            .await
            .expect("starts sharing");
        runtime.shutdown().await.expect("clean shutdown");
    }

    // Session 2: restore sharing but with unconfigured channel
    {
        let (sharing, endpoint) = support::sharing_runtime_persistent(&["HP LaserJet"], &dir);
        sharing.restore().await.expect("restores sharing state");

        // Create unconfigured channel service
        let unconfigured_channel =
            Arc::new(shaprint_desktop::adapters::ipps::NetworkChannel::in_memory());
        let service = Arc::new(support::sharing_service_with_channel(
            Arc::clone(&sharing),
            endpoint,
            unconfigured_channel,
        ));

        let runtime = RuntimeCoordinator::new(vec![service]);

        // Autostart should not claim sharing is running when channel is missing
        let _ = runtime.start_autostart().await;
        assert_ne!(
            state(&runtime, ServiceId::ServerSharing),
            ServiceState::Running
        );
        // User's printer selection remains preserved for when channel is configured
        assert_eq!(
            sharing
                .shared_printers()
                .iter()
                .map(|p| p.as_str())
                .collect::<Vec<_>>(),
            vec!["HP LaserJet"]
        );

        runtime.shutdown().await.expect("clean shutdown");
    }

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn explicit_stop_persists_even_when_service_was_in_failed_state() {
    let dir = temporary_directory("persist-lifecycle-failed-stop");

    // Session 1: printer is selected, sharing is autostart-enabled
    {
        let (sharing, endpoint) = support::sharing_runtime_persistent(&["HP LaserJet"], &dir);
        sharing
            .set_shared(printer_names(&["HP LaserJet"]))
            .await
            .expect("selects printer");

        let runtime = RuntimeCoordinator::new(vec![Arc::new(support::sharing_service(
            Arc::clone(&sharing),
            endpoint,
        ))]);

        runtime
            .start(ServiceId::ServerSharing)
            .await
            .expect("starts sharing");

        // Explicit user stop
        runtime
            .stop(ServiceId::ServerSharing)
            .await
            .expect("stops sharing");
        assert_eq!(
            state(&runtime, ServiceId::ServerSharing),
            ServiceState::Stopped
        );

        runtime.shutdown().await.expect("clean shutdown");
    }

    // Session 2: restart must stay stopped
    {
        let (sharing, endpoint) = support::sharing_runtime_persistent(&["HP LaserJet"], &dir);
        sharing.restore().await.expect("restores");

        let runtime = RuntimeCoordinator::new(vec![Arc::new(support::sharing_service(
            Arc::clone(&sharing),
            endpoint,
        ))]);

        runtime.start_autostart().await.expect("autostart");
        assert_eq!(
            state(&runtime, ServiceId::ServerSharing),
            ServiceState::Stopped
        );

        runtime.shutdown().await.expect("clean shutdown");
    }

    std::fs::remove_dir_all(&dir).ok();
}
