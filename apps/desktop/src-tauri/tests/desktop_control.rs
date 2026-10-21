//! The desktop's machine control plane (issue #130) against a real `shaprintd` and the real
//! `shaprintctl` helper wrapped by a fake `pkexec`.
//!
//! The window reaches the machine exactly as production does: unprivileged reads over the daemon's
//! Unix control socket, and mutations as `pkexec /usr/bin/shaprintctl <command> …` — here `pkexec`
//! is a script that either authorizes (runs the helper) or declines (exit 126), because a test
//! cannot drive polkit itself. The control plane only ever knows the socket path and the helper
//! path: no daemon state file is named anywhere in its API, and the unreadable-state-directory
//! test proves the read path never needs one.

#![cfg(unix)]

mod support;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use shaprint_desktop::adapters::daemon::{
    DaemonClient, DaemonConfig, DaemonServer, PeerAccessPolicy,
};
use shaprint_desktop::adapters::elevated_ctl::ElevatedCtl;
use shaprint_desktop::application::RuntimeCoordinator;
use shaprint_desktop::domain::{ErrorCode, PrinterName, ServiceId};
use shaprint_desktop::ipc::client_connections::TrustedServerStatusDto;
use shaprint_desktop::ipc::control::{ControlMode, ControlPlane};
use shaprint_desktop::RuntimePorts;
use support::temporary_directory;

/// The machine state a window controls: daemon, state directory, socket, and the control plane.
struct Machine {
    root: PathBuf,
    state_dir: PathBuf,
    socket: PathBuf,
    daemon: DaemonServer,
    control: ControlPlane,
}

impl Machine {
    /// Launches a daemon with isolated state and a control plane wired to `pkexec_body` as the
    /// polkit helper and the real `shaprintctl` as the program it runs. `{{root}}` in the body
    /// expands to this machine's directory.
    async fn launch(name: &str, pkexec_body: &str) -> Self {
        Self::launch_with_policy(name, pkexec_body, PeerAccessPolicy::RootOrDaemonOwner).await
    }

    async fn launch_with_policy(name: &str, pkexec_body: &str, policy: PeerAccessPolicy) -> Self {
        let root = temporary_directory(name);
        std::fs::create_dir_all(&root).expect("creates test root");
        let state_dir = root.join("var/lib/shaprint");
        let socket = root.join("run/shaprint/shaprintd.sock");
        let daemon = DaemonServer::launch(
            DaemonConfig::new(&state_dir, &socket)
                .with_config_dir(root.join("etc/shaprint"))
                .with_access_policy(policy)
                .with_ports(RuntimePorts {
                    proxy_port: 0,
                    server_port: 0,
                    discovery_ports: vec![0],
                }),
        )
        .await
        .expect("launches daemon");

        let helper_dir = root.join("helper");
        std::fs::create_dir_all(&helper_dir).expect("creates helper dir");
        let body = pkexec_body.replace("{{root}}", &root.display().to_string());
        let pkexec = script(&helper_dir, "pkexec", &body);
        let control = ControlPlane::machine_with(
            DaemonClient::new(&socket),
            ElevatedCtl::new(pkexec, env!("CARGO_BIN_EXE_shaprintctl"), &socket)
                .with_authorization_program(script(&helper_dir, "pkcheck", "exit 0")),
        );

        Self {
            root,
            state_dir,
            socket,
            daemon,
            control,
        }
    }

    fn client(&self) -> DaemonClient {
        DaemonClient::new(&self.socket)
    }

    /// The fingerprint of the daemon's own Server identity, read through the control plane —
    /// never from the certificate files in `state_dir`.
    fn server_fingerprint(&self) -> String {
        self.daemon
            .shell()
            .expect("daemon launched with shell")
            .endpoint()
            .fingerprint()
            .to_string()
    }

    async fn stop(self) {
        self.daemon
            .shutdown()
            .await
            .expect("shuts daemon down cleanly");
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Writes an executable shell script and returns its path.
fn script(directory: &Path, name: &str, body: &str) -> PathBuf {
    std::fs::create_dir_all(directory).expect("creates script dir");
    let path = directory.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("writes script");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
        .expect("makes script executable");
    path
}

/// A fake `pkexec` that authorizes: it records every argument it was asked to run (proof the
/// mutation went through the polkit path) and then runs the helper exactly like polkit does.
fn authorizing_pkexec(marker: &Path) -> String {
    format!("echo \"$@\" >> '{}'; exec \"$@\"", marker.display())
}

/// A per-test marker file inside `shaprint-<name>-marker-<pid>/`, created and ready to append to.
fn marker_file(name: &str) -> PathBuf {
    let directory =
        std::env::temp_dir().join(format!("shaprint-{name}-marker-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("creates marker dir");
    directory.join("pkexec-args.log")
}

fn remove_marker(name: &str) {
    let directory =
        std::env::temp_dir().join(format!("shaprint-{name}-marker-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(directory);
}

#[tokio::test]
async fn selected_queue_diagnostics_are_passive_machine_reads_with_a_bounded_snapshot() {
    let machine = Machine::launch(
        "desktop-local-troubleshooting",
        "echo invoked >> {{root}}/pkexec-was-run; exit 99",
    )
    .await;
    let snapshot = machine
        .control
        .inspect_local_client_queue(shaprint_desktop::ipc::dto::RecognisedClientQueueDto {
            queue_name: "shaprint_missing_queue".to_owned(),
            server_address: "192.0.2.10:48631".to_owned(),
            printer_name: "Office".to_owned(),
        })
        .await
        .expect("returns classified local checks over the daemon socket");
    assert_eq!(snapshot.target.queue_name, "shaprint_missing_queue");
    assert_eq!(snapshot.checks.len(), 4);
    assert!(!snapshot.queue_empty);
    assert!(!snapshot.test_print_enabled);
    assert!(!snapshot.repair_enabled);
    assert!(snapshot.attempted_actions.is_empty());
    assert!(!machine.root.join("pkexec-was-run").exists());
    machine.stop().await;
}

fn service_state(status: &shaprint_desktop::ipc::dto::RuntimeStatusDto, id: &str) -> String {
    status
        .services
        .iter()
        .find(|service| service.id == id)
        .map(|service| service.state.clone())
        .unwrap_or_else(|| panic!("status must report service {id}"))
}

/// Reads over the control socket need neither elevation nor the daemon's state files, and the
/// control plane reports which plane it administers.
#[tokio::test]
async fn gui_reads_the_machine_daemon_without_elevation_or_state_files() {
    let machine = Machine::launch(
        "desktop-control-reads",
        // Any invocation of the helper is a failure here: reads must never elevate.
        "echo invoked >> {{root}}/pkexec-was-run; exit 99",
    )
    .await;

    assert_eq!(machine.control.mode(), ControlMode::Machine);

    let status = machine
        .control
        .status()
        .await
        .expect("reads status over the socket");
    assert_eq!(service_state(&status, "client-proxy"), "running");
    assert_eq!(service_state(&status, "server-sharing"), "stopped");

    let identity = machine
        .control
        .server_identity()
        .await
        .expect("reads the daemon's server identity");
    assert_eq!(identity.fingerprint, machine.server_fingerprint());

    // Lock the daemon's state directory: identity and status are answered from memory over the
    // socket, so the GUI's read path never needs (or opens) a daemon state file.
    let state_dir = machine.state_dir.clone();
    std::fs::set_permissions(&state_dir, std::fs::Permissions::from_mode(0o000))
        .expect("locks state dir");
    let locked_status = machine.control.status().await;
    let locked_identity = machine.control.server_identity().await;
    std::fs::set_permissions(&state_dir, std::fs::Permissions::from_mode(0o700))
        .expect("unlocks state dir");
    locked_status.expect("status works with an unreadable state dir");
    locked_identity.expect("identity works with an unreadable state dir");

    assert!(
        machine
            .control
            .list_trusted_servers()
            .await
            .expect("lists machine trusted servers")
            .is_empty(),
        "a fresh machine approves no servers"
    );
    assert!(!machine
        .control
        .network_channel_configured()
        .await
        .expect("reads configured status"));

    assert!(
        !machine.root.join("pkexec-was-run").exists(),
        "reads must never run the elevated helper"
    );

    machine.stop().await;
}

#[tokio::test]
async fn unavailable_daemon_reports_recovery_without_asking_for_setup_authorization() {
    let root = temporary_directory("desktop-control-offline-setup");
    std::fs::create_dir_all(&root).unwrap();
    let socket = root.join("missing.sock");
    let marker = root.join("authorization-invoked");
    let checker = script(
        &root,
        "pkcheck",
        &format!("touch '{}'; exit 0", marker.display()),
    );
    let pkexec = script(&root, "pkexec", r#"exec "$@""#);
    let control = ControlPlane::machine_with(
        DaemonClient::new(&socket),
        ElevatedCtl::new(pkexec, env!("CARGO_BIN_EXE_shaprintctl"), &socket)
            .with_authorization_program(checker),
    );
    let error = control
        .start_service(ServiceId::ClientProxy)
        .await
        .unwrap_err();
    assert_eq!(error.code(), ErrorCode::ServerUnavailable);
    assert!(
        !marker.exists(),
        "a missing daemon must be reported before requesting a password"
    );
    std::fs::remove_dir_all(root).unwrap();
}

/// Every administrative mutation runs the same `shaprintctl` CLI the headless operator uses,
/// wrapped by polkit, and the daemon's state changes accordingly (AC2, AC3).
#[tokio::test]
async fn mutations_run_the_packaged_cli_through_the_polkit_path() {
    let marker = marker_file("desktop-control-mutations");
    let machine = Machine::launch("desktop-control-mutations", &authorizing_pkexec(&marker)).await;

    // A real remote ShaPrint Server to approve, probe, and forget.
    let (remote_sharing, remote_endpoint) = support::sharing_runtime(&["Office LaserJet"]);
    remote_sharing
        .set_shared(support::printer_names(&["Office LaserJet"]))
        .await
        .expect("selects the remote shared printer");
    let remote_runtime = Arc::new(RuntimeCoordinator::new(vec![Arc::new(
        support::sharing_service(remote_sharing, Arc::clone(&remote_endpoint)),
    )]));
    remote_runtime
        .start(ServiceId::ServerSharing)
        .await
        .expect("starts remote server sharing");
    let remote_port = remote_endpoint
        .bound_address()
        .expect("remote server bound")
        .port();
    let remote_address = format!("127.0.0.1:{remote_port}");
    let fingerprint = remote_endpoint.fingerprint().to_string();

    // Lifecycle: stop and start a supervised service, exactly like `shaprintctl stop/start`.
    let stopped = machine
        .control
        .stop_service(ServiceId::ClientProxy)
        .await
        .expect("stops the client proxy");
    assert_eq!(service_state(&stopped, "client-proxy"), "stopped");
    let started = machine
        .control
        .start_service(ServiceId::ClientProxy)
        .await
        .expect("starts the client proxy");
    assert_eq!(service_state(&started, "client-proxy"), "running");

    // Network Channel: the value travels on the helper's stdin; only configured status comes back.
    let secret = "channel-secret-that-must-not-leak";
    let configured = machine
        .control
        .configure_network_channel(secret)
        .await
        .expect("sets the machine Network Channel");
    assert!(configured);
    assert!(machine
        .control
        .network_channel_configured()
        .await
        .expect("reads configured status"));
    let status = machine.control.status().await.expect("reads status");
    assert!(
        !serde_json::to_string(&status)
            .expect("serializes status")
            .contains(secret),
        "status payload must never expose the Network Channel value"
    );

    // Machine trust: approve, review, probe, and forget through the elevated helper.
    let review = machine
        .control
        .approve_server(&remote_address, &fingerprint)
        .await
        .expect("approves the remote server");
    assert!(review.trusted);
    let inspected = machine
        .control
        .inspect_server(&remote_address)
        .await
        .expect("inspects the trusted server");
    assert!(inspected.trusted);
    let probe = machine
        .control
        .probe_server(&remote_address)
        .await
        .expect("probes the trusted server");
    assert_eq!(probe.status, TrustedServerStatusDto::Online);
    assert_eq!(probe.printers, vec!["Office LaserJet".to_owned()]);
    let listed = machine
        .control
        .list_trusted_servers()
        .await
        .expect("lists machine trusted servers");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].address, remote_address);
    machine
        .control
        .forget_server(&remote_address)
        .await
        .expect("forgets the remote server");
    assert!(
        machine
            .control
            .list_trusted_servers()
            .await
            .expect("lists machine trusted servers")
            .is_empty(),
        "forget removes the approval from the machine trust store"
    );

    // Sharing selection: the machine mutation the Share page runs. It needs a reachable spooler,
    // so the round trip runs wherever the daemon can list local queues.
    if let Ok(current) = machine.control.local_printers().await {
        let cleared = machine
            .control
            .set_shared_printers(Vec::new())
            .await
            .expect("clears the shared printer selection");
        assert!(cleared.printers.iter().all(|printer| !printer.shared));
        if let Some(first) = current.printers.first() {
            let queue = PrinterName::parse(&first.name).expect("valid printer name");
            let selected = machine
                .control
                .set_shared_printers(vec![queue])
                .await
                .expect("selects a local queue for sharing");
            assert!(selected
                .printers
                .iter()
                .any(|printer| printer.name == first.name && printer.shared));
            machine
                .control
                .set_shared_printers(Vec::new())
                .await
                .expect("clears the selection again");
        }
    }

    // Proof the mutations went through `pkexec shaprintctl` — and that the secret was not among
    // the arguments they were passed.
    let invoked = std::fs::read_to_string(&marker).expect("pkexec path recorded its arguments");
    assert!(invoked.contains("trust approve"), "{invoked}");
    assert!(invoked.contains("--socket"), "{invoked}");
    assert!(
        !invoked.contains(secret),
        "the Network Channel must travel on stdin, never in arguments: {invoked}"
    );

    remote_runtime
        .shutdown()
        .await
        .expect("stops remote server sharing");
    machine.stop().await;
    remove_marker("desktop-control-mutations");
}

/// Declining polkit authorization changes nothing and names the action required to retry (AC4).
#[tokio::test]
async fn declined_authorization_leaves_the_machine_unchanged_and_names_the_retry() {
    let machine = Machine::launch("desktop-control-declined", "exit 126").await;

    let error = machine
        .control
        .configure_network_channel("never-written")
        .await
        .expect_err("declined authorization must fail");
    assert_eq!(error.code(), ErrorCode::NotAuthorized);
    assert!(
        error
            .message()
            .contains("org.shaprint.control.network-channel"),
        "retry message must name the polkit action: {}",
        error.message()
    );
    assert!(error.message().contains("retry"), "{}", error.message());
    assert!(
        error.message().contains("No changes were made"),
        "{}",
        error.message()
    );
    assert!(
        !machine
            .control
            .network_channel_configured()
            .await
            .expect("reads configured status"),
        "a declined prompt must leave the Network Channel untouched"
    );

    let error = machine
        .control
        .stop_service(ServiceId::ClientProxy)
        .await
        .expect_err("declined authorization must fail");
    assert_eq!(error.code(), ErrorCode::NotAuthorized);
    assert!(
        error.message().contains("org.shaprint.control.stop"),
        "{}",
        error.message()
    );
    let status = machine.control.status().await.expect("reads status");
    assert_eq!(
        service_state(&status, "client-proxy"),
        "running",
        "a declined prompt must leave the service running"
    );

    let error = machine
        .control
        .forget_server("192.0.2.10:48631")
        .await
        .expect_err("declined authorization must fail");
    assert_eq!(error.code(), ErrorCode::NotAuthorized);
    assert!(
        error.message().contains("org.shaprint.control.trust"),
        "{}",
        error.message()
    );
    assert!(
        machine
            .control
            .list_trusted_servers()
            .await
            .expect("lists machine trusted servers")
            .is_empty(),
        "a declined prompt must not touch the trust store"
    );

    machine.stop().await;
}

/// A daemon that only authorizes another uid answers reads but rejects mutations with the same
/// stable `not-authorized` code `sudo shaprintctl` would surface — state unchanged (AC1, AC4).
#[tokio::test]
async fn daemon_peer_policy_denies_the_helper_with_the_cli_stable_error() {
    let marker = marker_file("desktop-control-denied");
    let machine = Machine::launch_with_policy(
        "desktop-control-denied",
        &authorizing_pkexec(&marker),
        PeerAccessPolicy::AllowUid(u32::MAX - 1),
    )
    .await;

    // Reads stay available to every verified local peer.
    let status = machine.control.status().await.expect("reads status");
    assert_eq!(service_state(&status, "client-proxy"), "running");

    let error = machine
        .control
        .configure_network_channel("never-written")
        .await
        .expect_err("the daemon must refuse the mutation");
    assert_eq!(error.code(), ErrorCode::NotAuthorized);
    assert!(
        error.message().contains("root privileges"),
        "the CLI's stable message must survive the round trip: {}",
        error.message()
    );
    assert!(
        !machine
            .control
            .network_channel_configured()
            .await
            .expect("reads configured status"),
        "the refused mutation must leave the machine unchanged"
    );

    let error = machine
        .control
        .stop_service(ServiceId::ClientProxy)
        .await
        .expect_err("the daemon must refuse the mutation");
    assert_eq!(error.code(), ErrorCode::NotAuthorized);
    let status = machine.control.status().await.expect("reads status");
    assert_eq!(service_state(&status, "client-proxy"), "running");

    machine.stop().await;
    remove_marker("desktop-control-denied");
}

/// `stop` — the daemon shutdown every CLI operator has — is available in the GUI control plane
/// and actually stops the machine (AC2); the window's own exit path never reaches these services
/// (AC5).
#[tokio::test]
async fn gui_stops_the_machine_daemon_and_the_socket_goes_away() {
    let marker = marker_file("desktop-control-shutdown");
    let machine = Machine::launch("desktop-control-shutdown", &authorizing_pkexec(&marker)).await;
    let socket_path = machine.socket.clone();

    let stopped = machine
        .control
        .shutdown_daemon()
        .await
        .expect("stops the machine daemon through the helper");
    assert_eq!(
        service_state(&stopped, "client-proxy"),
        "stopped",
        "the returned status reflects the stopped services"
    );

    // The daemon stops its listener and services; wait for it to stop answering.
    let client = machine.client();
    let mut refused = false;
    for _ in 0..100 {
        if client.status().await.is_err() {
            refused = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(refused, "the daemon must stop answering after shutdown");

    // The in-process handle still completes the wait that removes the socket file.
    machine.stop().await;
    assert!(
        !socket_path.exists(),
        "the control socket file is removed: {}",
        socket_path.display()
    );
    remove_marker("desktop-control-shutdown");
}
