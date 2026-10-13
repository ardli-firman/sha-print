//! Behavioral tests for the Linux machine daemon (`shaprintd`) and local control client
//! (`shaprintctl`) over a real Unix domain socket.

#![cfg(unix)]

mod support;

use std::net::TcpListener;
use std::path::Path;
use std::sync::Arc;

use shaprint_desktop::adapters::client_connections::ClientConnections;
use shaprint_desktop::adapters::daemon::{
    DaemonClient, DaemonConfig, DaemonServer, PeerAccessPolicy,
};
use shaprint_desktop::adapters::discovery::MdnsAdvertiser;
use shaprint_desktop::adapters::ipps::NetworkChannel;
use shaprint_desktop::adapters::{ClientProxyService, IppsServer, ServerSharingService};
use shaprint_desktop::application::{PrintFailures, PrintJobTracker, RuntimeCoordinator, Sharing};
use shaprint_desktop::domain::ErrorCode;
use shaprint_desktop::RuntimePorts;
use support::{printer_names, sharing_runtime, temporary_directory};

struct TestDaemonHarness {
    runtime: Arc<RuntimeCoordinator>,
    sharing: Arc<Sharing>,
    proxy: Arc<ClientProxyService>,
    endpoint: Arc<IppsServer>,
    mdns: Arc<MdnsAdvertiser>,
}

async fn test_daemon_harness(dir: &Path, queues: &[&str]) -> TestDaemonHarness {
    let (sharing, endpoint) = sharing_runtime(queues);
    let connections =
        Arc::new(ClientConnections::new(dir.join("trust")).expect("opens proxy trust store"));
    let channel = Arc::new(NetworkChannel::in_memory());
    channel
        .configure_sync("test-channel-secret")
        .expect("configures test Network Channel");
    let tracker = Arc::new(PrintJobTracker::new());
    let proxy = Arc::new(ClientProxyService::with_port(
        connections,
        Arc::clone(&channel),
        Arc::new(PrintFailures::new()),
        tracker,
        0,
    ));
    let mdns = Arc::new(MdnsAdvertiser::on(vec![0]));
    let advertiser: Arc<dyn shaprint_desktop::application::ServerAdvertiser> = mdns.clone();
    let sharing_service = Arc::new(ServerSharingService::new(
        Arc::clone(&sharing),
        Arc::clone(&endpoint),
        advertiser,
        support::allowed_inbound_setup(),
        channel as Arc<dyn shaprint_desktop::application::ChannelState>,
    ));
    let runtime = Arc::new(RuntimeCoordinator::new(vec![
        proxy.clone(),
        sharing_service,
    ]));
    runtime
        .start_autostart()
        .await
        .expect("starts autostart services");

    TestDaemonHarness {
        runtime,
        sharing,
        proxy,
        endpoint,
        mdns,
    }
}

#[tokio::test]
async fn daemon_launches_without_desktop_and_reports_stable_runtime_status_over_unix_socket() {
    let dir = temporary_directory("linux-daemon-status");
    std::fs::create_dir_all(&dir).expect("creates state dir");
    let socket_path = dir.join("shaprintd.sock");
    let secret = "super-secret-network-channel-value";

    let server = DaemonServer::launch(DaemonConfig::new(&dir, &socket_path).with_ports(
        RuntimePorts {
            proxy_port: 0,
            server_port: 0,
            discovery_ports: vec![0],
        },
    ))
    .await
    .expect("launches daemon without desktop session");
    server
        .shell()
        .expect("daemon launched with shell")
        .network_channel()
        .configure_sync(secret)
        .expect("configures Network Channel");

    let client = DaemonClient::new(&socket_path);
    let status = client
        .status()
        .await
        .expect("queries status over unix socket");

    let proxy = status
        .services
        .iter()
        .find(|service| service.id == "client-proxy")
        .expect("reports client-proxy status");
    assert_eq!(proxy.state, "running");

    let sharing = status
        .services
        .iter()
        .find(|service| service.id == "server-sharing")
        .expect("reports server-sharing status");
    assert_eq!(sharing.state, "stopped");

    let raw = serde_json::to_string(&status).expect("serializes status");
    assert!(
        !raw.contains(secret),
        "status payload must never expose the Network Channel value"
    );

    server.shutdown().await.expect("shuts down daemon cleanly");
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn daemon_controls_runtimes_through_coordinator_and_shutdown_releases_all_listeners() {
    let dir = temporary_directory("linux-daemon-lifecycle");
    std::fs::create_dir_all(&dir).expect("creates test dir");
    let socket_path = dir.join("shaprintd.sock");
    let harness = test_daemon_harness(&dir, &["HP LaserJet"]).await;

    let server = DaemonServer::bind(&socket_path, Arc::clone(&harness.runtime))
        .await
        .expect("binds daemon socket");
    let client = DaemonClient::new(&socket_path);

    // Preconditions come from RuntimeCoordinator rather than duplicated daemon rules.
    let err = client
        .start_service("server-sharing")
        .await
        .expect_err("rejects server-sharing before a printer is selected");
    assert_eq!(err.code(), ErrorCode::InvalidState);
    assert_eq!(
        err.message(),
        "select at least one printer to share before starting"
    );

    harness
        .sharing
        .set_shared(printer_names(&["HP LaserJet"]))
        .await
        .expect("selects shared printer");

    let started = client
        .start_service("server-sharing")
        .await
        .expect("starts server-sharing over unix socket");
    assert!(started
        .services
        .iter()
        .any(|s| s.id == "server-sharing" && s.state == "running"));

    let proxy_addr = harness
        .proxy
        .bound_address()
        .expect("proxy bound a TCP port");
    let ipps_addr = harness
        .endpoint
        .bound_address()
        .expect("IPPS bound a TCP port");
    let udp_port = harness
        .mdns
        .bound_port()
        .expect("discovery bound a UDP port");

    let stopped = client
        .shutdown()
        .await
        .expect("requests daemon shutdown over unix socket");
    assert!(stopped.services.iter().all(|s| s.state == "stopped"));

    server
        .wait()
        .await
        .expect("daemon finishes shutdown cleanly");

    assert!(!socket_path.exists(), "control socket file is removed");
    let rebound_proxy = TcpListener::bind(proxy_addr).expect("proxy TCP listener was released");
    let rebound_ipps = TcpListener::bind(ipps_addr).expect("IPPS TCP listener was released");
    let rebound_udp = std::net::UdpSocket::bind(("0.0.0.0", udp_port))
        .expect("discovery UDP listener was released");
    drop((rebound_proxy, rebound_ipps, rebound_udp));
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn unprivileged_peer_can_read_status_but_mutations_fail_with_not_authorized() {
    let dir = temporary_directory("linux-daemon-peer-auth");
    std::fs::create_dir_all(&dir).expect("creates test dir");
    let socket_path = dir.join("shaprintd.sock");
    let harness = test_daemon_harness(&dir, &["HP LaserJet"]).await;
    harness
        .sharing
        .set_shared(printer_names(&["HP LaserJet"]))
        .await
        .expect("selects shared printer");

    // Require a different UID for mutations so our real kernel SO_PEERCRED is treated as an
    // unprivileged local user even if tests run as root or a regular user.
    let server = DaemonServer::bind_with_policy(
        &socket_path,
        Arc::clone(&harness.runtime),
        PeerAccessPolicy::AllowUid(u32::MAX - 1),
    )
    .await
    .expect("binds daemon socket with peer policy");

    let client = DaemonClient::new(&socket_path);

    // Read-only status succeeds for any verified local peer.
    let status = client
        .status()
        .await
        .expect("unprivileged peer can read status");
    assert!(status
        .services
        .iter()
        .any(|s| s.id == "client-proxy" && s.state == "running"));

    // Mutating operations are rejected with the stable `not-authorized` error code.
    let start_err = client
        .start_service("server-sharing")
        .await
        .expect_err("unprivileged start must be rejected");
    assert_eq!(start_err.code(), ErrorCode::NotAuthorized);
    assert_eq!(start_err.code_str(), "not-authorized");

    let stop_err = client
        .stop_service("client-proxy")
        .await
        .expect_err("unprivileged stop must be rejected");
    assert_eq!(stop_err.code(), ErrorCode::NotAuthorized);

    let shutdown_err = client
        .shutdown()
        .await
        .expect_err("unprivileged shutdown must be rejected");
    assert_eq!(shutdown_err.code(), ErrorCode::NotAuthorized);

    // State was not mutated by any rejected request.
    let after = client
        .status()
        .await
        .expect("status remains readable after rejected mutations");
    assert!(after
        .services
        .iter()
        .any(|s| s.id == "client-proxy" && s.state == "running"));
    assert!(after
        .services
        .iter()
        .any(|s| s.id == "server-sharing" && s.state == "stopped"));

    server.shutdown().await.expect("clean shutdown");
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn smoke_launches_shaprintd_observes_status_via_shaprintctl_and_stops_cleanly() {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let dir = temporary_directory("linux-smoke-cli");
    std::fs::create_dir_all(&dir).expect("creates smoke dir");
    let socket_path = dir.join("shaprintd.sock");
    let proxy_port = support::free_port();
    let server_port = support::free_port();
    let discovery_port = support::free_udp_port();

    let shaprintd_bin = env!("CARGO_BIN_EXE_shaprintd");
    let shaprintctl_bin = env!("CARGO_BIN_EXE_shaprintctl");

    let mut daemon = Command::new(shaprintd_bin)
        .args([
            "--state-dir",
            &dir.to_string_lossy(),
            "--socket",
            &socket_path.to_string_lossy(),
            "--proxy-port",
            &proxy_port.to_string(),
            "--server-port",
            &server_port.to_string(),
            "--discovery-port",
            &discovery_port.to_string(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawns shaprintd binary");

    // Wait until `shaprintctl status` succeeds against the live daemon socket.
    let deadline = Instant::now() + Duration::from_secs(10);
    let status_stdout = loop {
        if let Some(exit) = daemon.try_wait().expect("checks daemon process") {
            panic!("shaprintd exited prematurely with {exit}");
        }
        let output = Command::new(shaprintctl_bin)
            .args(["--socket", &socket_path.to_string_lossy(), "status"])
            .output()
            .expect("runs shaprintctl status");
        if output.status.success() {
            break String::from_utf8_lossy(&output.stdout).into_owned();
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for shaprintctl status; last stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    };

    assert!(
        status_stdout.contains("client-proxy") && status_stdout.contains("running"),
        "expected client-proxy running in status output: {status_stdout}"
    );
    assert!(
        status_stdout.contains("server-sharing") && status_stdout.contains("stopped"),
        "expected server-sharing stopped in status output: {status_stdout}"
    );

    // While running, the daemon holds the proxy listener port.
    assert!(
        TcpListener::bind(("127.0.0.1", proxy_port)).is_err(),
        "proxy port should be held by shaprintd while running"
    );

    // Individual supervised services can be stopped and restarted through `shaprintctl`.
    let stop_proxy = Command::new(shaprintctl_bin)
        .args([
            "--socket",
            &socket_path.to_string_lossy(),
            "stop",
            "client-proxy",
        ])
        .output()
        .expect("runs shaprintctl stop client-proxy");
    assert!(stop_proxy.status.success());
    assert!(
        String::from_utf8_lossy(&stop_proxy.stdout).contains("client-proxy: stopped"),
        "expected client-proxy stopped after stop command"
    );

    let start_proxy = Command::new(shaprintctl_bin)
        .args([
            "--socket",
            &socket_path.to_string_lossy(),
            "start",
            "client-proxy",
        ])
        .output()
        .expect("runs shaprintctl start client-proxy");
    assert!(start_proxy.status.success());
    assert!(
        String::from_utf8_lossy(&start_proxy.stdout).contains("client-proxy: running"),
        "expected client-proxy running after start command"
    );

    // Stop the daemon through `shaprintctl stop`.
    let stop_output = Command::new(shaprintctl_bin)
        .args(["--socket", &socket_path.to_string_lossy(), "stop"])
        .output()
        .expect("runs shaprintctl stop");
    assert!(
        stop_output.status.success(),
        "shaprintctl stop failed: {}",
        String::from_utf8_lossy(&stop_output.stderr)
    );

    // Wait for `shaprintd` to exit cleanly.
    let exit_status = loop {
        if let Some(status) = daemon.try_wait().expect("polls shaprintd exit") {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for shaprintd to exit after stop"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    assert!(exit_status.success(), "shaprintd exited with {exit_status}");

    assert!(
        !socket_path.exists(),
        "shaprintd must remove its Unix socket on clean shutdown"
    );
    let rebound = TcpListener::bind(("127.0.0.1", proxy_port))
        .expect("proxy port is released after shaprintd stops");
    drop(rebound);
    std::fs::remove_dir_all(&dir).ok();
}
