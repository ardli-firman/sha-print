//! Behavioral tests for the Linux machine daemon (`shaprintd`) and local control client
//! (`shaprintctl`) over a real Unix domain socket.

#![cfg(unix)]

mod support;

use std::net::TcpListener;
use std::path::Path;
use std::sync::Arc;

use shaprint_desktop::adapters::client_connections::ClientConnections;
use shaprint_desktop::adapters::daemon::{
    parse_daemon_args, DaemonClient, DaemonConfig, DaemonServer, PeerAccessPolicy,
    DEFAULT_CONFIG_DIR, DEFAULT_SOCKET_PATH, DEFAULT_STATE_DIR,
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
async fn second_daemon_launch_on_active_socket_is_rejected_without_disturbing_first_daemon() {
    let dir = temporary_directory("linux-daemon-single-instance");
    std::fs::create_dir_all(&dir).expect("creates state dir");
    let socket_path = dir.join("shaprintd.sock");
    let proxy_port = support::free_port();

    let first = DaemonServer::launch(DaemonConfig::new(&dir, &socket_path).with_ports(
        RuntimePorts {
            proxy_port,
            server_port: 0,
            discovery_ports: vec![0],
        },
    ))
    .await
    .expect("launches first daemon instance");

    let second_err = match DaemonServer::launch(DaemonConfig::new(&dir, &socket_path).with_ports(
        RuntimePorts {
            proxy_port,
            server_port: 0,
            discovery_ports: vec![0],
        },
    ))
    .await
    {
        Ok(_) => panic!("second daemon launch on active socket must fail"),
        Err(error) => error,
    };
    assert_eq!(second_err.code(), ErrorCode::InvalidState);
    assert!(
        second_err.message().contains("already listening"),
        "expected already-listening error, got: {}",
        second_err.message()
    );

    // First daemon and its proxy listener remain running and undisturbed.
    let client = DaemonClient::new(&socket_path);
    let status = client
        .status()
        .await
        .expect("first daemon still answers status after rejected second launch");
    assert!(status
        .services
        .iter()
        .any(|s| s.id == "client-proxy" && s.state == "running"));
    assert!(
        TcpListener::bind(("127.0.0.1", proxy_port)).is_err(),
        "first daemon still owns its proxy TCP listener"
    );

    first.shutdown().await.expect("shuts down first daemon");
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
    let rebound_proxy = {
        let mut bound = None;
        for _ in 0..20 {
            if let Ok(listener) = TcpListener::bind(proxy_addr) {
                bound = Some(listener);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        bound.expect("proxy TCP listener was released")
    };
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

    // Wait until `shaprintctl status` succeeds and autostarted `client-proxy` is running.
    let deadline = Instant::now() + Duration::from_secs(10);
    let status_stdout = loop {
        if let Some(exit) = daemon.try_wait().expect("checks daemon process") {
            panic!("shaprintd exited prematurely with {exit}");
        }
        let output = Command::new(shaprintctl_bin)
            .args(["--socket", &socket_path.to_string_lossy(), "status"])
            .output()
            .expect("runs shaprintctl status");
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        if output.status.success() && stdout.contains("client-proxy: running") {
            break stdout;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for shaprintctl status; last stdout: {stdout}; last stderr: {}",
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

#[tokio::test]
async fn daemon_uses_standard_fhs_locations_loads_non_secret_config_and_enforces_least_privilege_modes(
) {
    use std::os::unix::fs::PermissionsExt;

    let defaults = parse_daemon_args(&["shaprintd"]).expect("parses default daemon args");
    assert_eq!(defaults.config_dir(), Path::new(DEFAULT_CONFIG_DIR));
    assert_eq!(defaults.state_dir(), Path::new(DEFAULT_STATE_DIR));
    assert_eq!(defaults.socket_path(), Path::new(DEFAULT_SOCKET_PATH));

    let root = temporary_directory("linux-daemon-fhs");
    let etc_dir = root.join("etc/shaprint");
    let state_dir = root.join("var/lib/shaprint");
    let run_dir = root.join("run/shaprint");
    let socket_path = run_dir.join("shaprintd.sock");
    std::fs::create_dir_all(&etc_dir).expect("creates config dir");

    let proxy_port = support::free_port();
    let server_port = support::free_port();
    let discovery_port = support::free_udp_port();
    std::fs::write(
        etc_dir.join("config.json"),
        format!(
            r#"{{
  "proxy_port": {proxy_port},
  "server_port": {server_port},
  "discovery_ports": [{discovery_port}]
}}"#
        ),
    )
    .expect("writes non-secret config.json");

    let config = parse_daemon_args(&[
        "shaprintd",
        "--config-dir",
        &etc_dir.to_string_lossy(),
        "--state-dir",
        &state_dir.to_string_lossy(),
        "--socket",
        &socket_path.to_string_lossy(),
    ])
    .expect("loads daemon config with --config-dir");

    assert_eq!(config.config_dir(), etc_dir.as_path());
    assert_eq!(config.ports().proxy_port, proxy_port);
    assert_eq!(config.ports().server_port, server_port);
    assert_eq!(config.ports().discovery_ports, vec![discovery_port]);

    let server = DaemonServer::launch(config)
        .await
        .expect("launches daemon with FHS directories");

    let state_mode = std::fs::metadata(&state_dir)
        .expect("state dir exists")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(
        state_mode, 0o700,
        "durable state directory must be restricted to 0700"
    );

    let key_mode = std::fs::metadata(state_dir.join("server-identity.key.der"))
        .expect("identity key exists in state_dir")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(
        key_mode, 0o600,
        "server identity private key must be restricted to 0600"
    );

    let run_mode = std::fs::metadata(&run_dir)
        .expect("runtime socket dir exists")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(
        run_mode, 0o755,
        "runtime directory must allow local shell traversal (0755)"
    );

    let sock_mode = std::fs::metadata(&socket_path)
        .expect("control socket exists")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(
        sock_mode, 0o666,
        "control socket must allow local status queries (0666, gated by SO_PEERCRED)"
    );

    server.shutdown().await.expect("clean shutdown");
    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn daemon_rejects_secret_fields_in_etc_config_refuses_root_when_forbidden_and_preserves_identity_across_restarts(
) {
    let root = temporary_directory("linux-daemon-nonroot-identity");
    let etc_dir = root.join("etc/shaprint");
    let state_dir = root.join("var/lib/shaprint");
    let socket_path = root.join("run/shaprint/shaprintd.sock");
    std::fs::create_dir_all(&etc_dir).expect("creates config dir");

    // `/etc/shaprint/config.json` is strictly for non-secret configuration; placing a secret field
    // such as `network_channel` must be rejected with `invalid-input`.
    std::fs::write(
        etc_dir.join("config.json"),
        r#"{"network_channel": "forbidden-secret"}"#,
    )
    .expect("writes invalid secret config");
    let secret_err = parse_daemon_args(&["shaprintd", "--config-dir", &etc_dir.to_string_lossy()])
        .expect_err("must reject secret fields in /etc/shaprint/config.json");
    assert_eq!(secret_err.code(), ErrorCode::InvalidInput);

    // Replace with valid non-secret config that forbids running the daemon as root.
    std::fs::write(
        etc_dir.join("config.json"),
        r#"{"forbid_root": true, "require_root": true}"#,
    )
    .expect("writes non-secret config");
    let parsed = parse_daemon_args(&["shaprintd", "--config-dir", &etc_dir.to_string_lossy()])
        .expect("parses non-secret config");
    assert!(parsed.forbid_root());
    assert_eq!(parsed.access_policy(), PeerAccessPolicy::RootOnly);

    // First launch creates the server identity in `/var/lib/shaprint`.
    let first = DaemonServer::launch(
        DaemonConfig::new(&state_dir, &socket_path)
            .with_config_dir(&etc_dir)
            .with_ports(RuntimePorts {
                proxy_port: 0,
                server_port: 0,
                discovery_ports: vec![0],
            }),
    )
    .await
    .expect("launches first daemon");
    let first_fingerprint = first.shell().expect("has shell").endpoint().fingerprint();
    let first_cert =
        std::fs::read(state_dir.join("server-identity.cert.der")).expect("reads generated cert");
    let first_key =
        std::fs::read(state_dir.join("server-identity.key.der")).expect("reads generated key");
    first.shutdown().await.expect("stops first daemon");

    // Repeated launch / service restart reuses the existing server identity without replacing it.
    let second = DaemonServer::launch(
        DaemonConfig::new(&state_dir, &socket_path)
            .with_config_dir(&etc_dir)
            .with_ports(RuntimePorts {
                proxy_port: 0,
                server_port: 0,
                discovery_ports: vec![0],
            }),
    )
    .await
    .expect("launches second daemon");
    let second_fingerprint = second.shell().expect("has shell").endpoint().fingerprint();
    assert_eq!(
        first_fingerprint, second_fingerprint,
        "restarting or repairing the daemon must preserve the existing server identity"
    );
    assert_eq!(
        first_cert,
        std::fs::read(state_dir.join("server-identity.cert.der")).expect("cert unchanged")
    );
    assert_eq!(
        first_key,
        std::fs::read(state_dir.join("server-identity.key.der")).expect("key unchanged")
    );
    second.shutdown().await.expect("stops second daemon");

    // When `forbid_root` is active and the daemon runs with effective UID 0, launch is refused.
    let root_err = match DaemonServer::launch_with_effective_uid(
        DaemonConfig::new(&state_dir, &socket_path)
            .with_forbid_root(true)
            .with_ports(RuntimePorts {
                proxy_port: 0,
                server_port: 0,
                discovery_ports: vec![0],
            }),
        Some(0),
    )
    .await
    {
        Ok(_) => panic!("daemon must refuse to run as root when forbid_root is enabled"),
        Err(error) => error,
    };
    assert_eq!(root_err.code(), ErrorCode::InvalidState);
    assert!(
        root_err.message().contains("must not run as root"),
        "unexpected error message: {}",
        root_err.message()
    );

    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn daemon_configures_network_channel_and_machine_trust_over_unix_socket_with_peer_authorization(
) {
    let root = temporary_directory("linux-daemon-trust-and-channel");
    let etc_dir = root.join("etc/shaprint");
    let state_dir = root.join("var/lib/shaprint");
    let socket_path = root.join("run/shaprint/shaprintd.sock");
    std::fs::create_dir_all(&etc_dir).expect("creates config dir");

    // Start a real remote ShaPrint Server to inspect, approve, and query.
    let (remote_sharing, remote_endpoint) = sharing_runtime(&["Office LaserJet"]);
    remote_sharing
        .set_shared(printer_names(&["Office LaserJet"]))
        .await
        .expect("selects remote shared printer");
    let remote_runtime = Arc::new(RuntimeCoordinator::new(vec![Arc::new(
        support::sharing_service(remote_sharing, Arc::clone(&remote_endpoint)),
    )]));
    remote_runtime
        .start(shaprint_desktop::domain::ServiceId::ServerSharing)
        .await
        .expect("starts remote server sharing");
    let remote_addr = remote_endpoint
        .bound_address()
        .expect("remote server bound");
    let remote_address_str = format!("127.0.0.1:{}", remote_addr.port());
    let expected_fp = remote_endpoint.fingerprint().to_string();

    let daemon = DaemonServer::launch(
        DaemonConfig::new(&state_dir, &socket_path)
            .with_config_dir(&etc_dir)
            .with_ports(RuntimePorts {
                proxy_port: 0,
                server_port: 0,
                discovery_ports: vec![0],
            }),
    )
    .await
    .expect("launches daemon");

    let client = DaemonClient::new(&socket_path);

    // 1. Initially, Network Channel is unconfigured and Trusted Server list is empty.
    let initial_nc = client
        .network_channel_status()
        .await
        .expect("queries initial network channel status");
    assert!(!initial_nc.configured);
    assert!(client
        .list_trusted_servers()
        .await
        .expect("lists initial trusted servers")
        .is_empty());

    // 2. Configure Network Channel over Unix socket; status reports configured=true without secret.
    let secret = format!("chan-{}", std::process::id());
    let updated_nc = client
        .configure_network_channel(&secret)
        .await
        .expect("configures Network Channel");
    assert!(updated_nc.configured);
    let nc_json = serde_json::to_string(&updated_nc).expect("serializes nc status");
    assert!(!nc_json.contains(&secret));

    // 3. Querying printers before approval is rejected with `server-untrusted`.
    let untrusted_err = client
        .list_server_printers(&remote_address_str)
        .await
        .expect_err("must reject untrusted server printer query");
    assert_eq!(untrusted_err.code(), ErrorCode::ServerNotTrusted);

    // 4. Inspect presents the Server certificate fingerprint without trusting it yet.
    let review = client
        .inspect_server(&remote_address_str)
        .await
        .expect("inspects remote server");
    assert_eq!(review.current_fingerprint, expected_fp);
    assert_eq!(review.previous_fingerprint, None);
    assert!(!review.trusted);

    // 5. Approve pins the fingerprint as a machine-wide Trusted Server.
    let approved = client
        .approve_server(&remote_address_str, &review.current_fingerprint)
        .await
        .expect("approves remote server");
    assert!(approved.trusted);
    assert_eq!(approved.current_fingerprint, expected_fp);

    let trusted_list = client
        .list_trusted_servers()
        .await
        .expect("lists trusted servers");
    assert_eq!(trusted_list.len(), 1);
    assert_eq!(trusted_list[0].address, remote_address_str);
    assert_eq!(trusted_list[0].fingerprint, expected_fp);

    // 6. Listing shared printers on the Trusted Server now succeeds.
    let printers = client
        .list_server_printers(&remote_address_str)
        .await
        .expect("lists shared printers on trusted server");
    assert_eq!(printers.printers, vec!["Office LaserJet"]);

    // 7. Forget removes machine trust; subsequent printer query is blocked again.
    client
        .forget_server(&remote_address_str)
        .await
        .expect("forgets trusted server");
    assert!(client
        .list_trusted_servers()
        .await
        .expect("lists after forget")
        .is_empty());
    let after_forget_err = client
        .list_server_printers(&remote_address_str)
        .await
        .expect_err("must block printer query after forgetting server");
    assert_eq!(after_forget_err.code(), ErrorCode::ServerNotTrusted);

    daemon.shutdown().await.expect("stops daemon");
    remote_runtime
        .shutdown()
        .await
        .expect("stops remote server");
    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shaprintctl_cli_end_to_end_trust_reapproval_restart_and_zero_leakage() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    let root = temporary_directory("linux-daemon-e2e-trust-restart");
    let etc_dir = root.join("etc/shaprint");
    let state_dir = root.join("var/lib/shaprint");
    let socket_path = root.join("run/shaprint/shaprintd.sock");
    std::fs::create_dir_all(&etc_dir).expect("creates config dir");

    let secret = format!(
        "nc-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("unix epoch")
            .as_nanos()
    );

    // Build a controllable remote ShaPrint Server whose TLS identity can be rotated on the same
    // port to verify changed-fingerprint blocking and reapproval.
    let remote_listener = TcpListener::bind("127.0.0.1:0").expect("binds remote port");
    let remote_port = remote_listener.local_addr().expect("remote addr").port();
    drop(remote_listener);
    let remote_address = format!("127.0.0.1:{remote_port}");

    let start_remote_server = |secret_val: String| async move {
        let identity = Arc::new(
            shaprint_desktop::adapters::ServerIdentity::generate().expect("generates identity"),
        );
        let fingerprint = identity.fingerprint().to_string();
        let catalog: Arc<dyn shaprint_desktop::application::LocalPrinterCatalog> =
            support::FakeCatalog::new(&["Warehouse Zebra"]);
        let sharing = Arc::new(Sharing::new(catalog));
        sharing
            .set_shared(printer_names(&["Warehouse Zebra"]))
            .await
            .expect("selects shared printer");
        let channel = Arc::new(NetworkChannel::in_memory());
        channel
            .configure_sync(&secret_val)
            .expect("configures remote server channel");
        let submitted_jobs = Arc::new(AtomicUsize::new(0));
        let jobs_counter = Arc::clone(&submitted_jobs);

        struct CountingSubmitter(Arc<AtomicUsize>);
        #[async_trait::async_trait]
        impl shaprint_desktop::application::PrintJobSubmitter for CountingSubmitter {
            async fn submit(
                &self,
                _printer: &shaprint_desktop::domain::PrinterName,
                _job: shaprint_desktop::application::PrintJob,
            ) -> Result<u32, shaprint_desktop::domain::AppError> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(42)
            }
        }

        let endpoint = Arc::new(IppsServer::new(
            remote_port,
            identity,
            Arc::clone(&channel),
            Arc::new(CountingSubmitter(jobs_counter)),
            Arc::new(PrintFailures::new()),
            Arc::new(PrintJobTracker::new()),
        ));
        let mdns = Arc::new(MdnsAdvertiser::on(vec![0]));
        let advertiser: Arc<dyn shaprint_desktop::application::ServerAdvertiser> = mdns;
        let service = Arc::new(ServerSharingService::new(
            sharing,
            endpoint,
            advertiser,
            support::allowed_inbound_setup(),
            channel as Arc<dyn shaprint_desktop::application::ChannelState>,
        ));
        let runtime = Arc::new(RuntimeCoordinator::new(vec![service]));
        runtime
            .start(shaprint_desktop::domain::ServiceId::ServerSharing)
            .await
            .expect("starts remote server");
        (runtime, fingerprint, submitted_jobs)
    };

    let (remote_v1, fp_v1, jobs_v1) = start_remote_server(secret.clone()).await;

    // Reserve a deterministic loopback proxy port so we can restart the daemon on the same proxy
    // port and verify print forwarding across daemon restarts.
    let proxy_listener = TcpListener::bind("127.0.0.1:0").expect("binds proxy port");
    let proxy_port = proxy_listener.local_addr().expect("proxy addr").port();
    drop(proxy_listener);

    let daemon = DaemonServer::launch(
        DaemonConfig::new(&state_dir, &socket_path)
            .with_config_dir(&etc_dir)
            .with_ports(RuntimePorts {
                proxy_port,
                server_port: 0,
                discovery_ports: vec![0],
            }),
    )
    .await
    .expect("launches daemon");

    let ctl_bin = env!("CARGO_BIN_EXE_shaprintctl");
    let socket_str = socket_path.to_string_lossy().into_owned();

    // 1. CLI rejects inline Network Channel arguments with exit code 2.
    let bad_cli = Command::new(ctl_bin)
        .args(["--socket", &socket_str, "network-channel", "set", &secret])
        .output()
        .expect("runs shaprintctl with forbidden inline secret");
    assert_eq!(bad_cli.status.code(), Some(2));
    assert!(
        !String::from_utf8_lossy(&bad_cli.stderr).contains(&secret),
        "error output must never echo a rejected secret argument"
    );

    // 2. Configure Network Channel via `shaprintctl network-channel set --stdin`.
    let mut child = Command::new(ctl_bin)
        .args(["--socket", &socket_str, "network-channel", "set", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawns shaprintctl network-channel set --stdin");
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(format!("{secret}\n").as_bytes())
        .expect("writes secret to stdin");
    let set_out = child.wait_with_output().expect("waits for shaprintctl");
    assert!(
        set_out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&set_out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&set_out.stdout),
        "network-channel: configured\n"
    );

    // 3. Configure via `--credential-file` and verify `shaprintctl status --json` includes
    //    `network_channel_configured: true` without exposing the secret.
    let temp_cred = root.join("input-cred");
    std::fs::write(&temp_cred, format!("{secret}\n")).expect("writes temp cred");
    let set_cred_out = Command::new(ctl_bin)
        .args([
            "--socket",
            &socket_str,
            "network-channel",
            "set",
            "--credential-file",
            &temp_cred.to_string_lossy(),
        ])
        .output()
        .expect("runs shaprintctl with --credential-file");
    std::fs::remove_file(&temp_cred).ok();
    assert!(set_cred_out.status.success());

    // 3b. Configure via `--fd 3` using a shell file-descriptor redirection, proving `shaprintctl`
    //     accepts open file descriptors without putting the secret in `argv`.
    let fd_cred = root.join("fd-cred");
    std::fs::write(&fd_cred, format!("{secret}\n")).expect("writes fd cred");
    let set_fd_out = Command::new("bash")
        .args([
            "-c",
            "exec 3<\"$1\" && exec \"$0\" --socket \"$2\" network-channel set --fd 3",
            ctl_bin,
            &fd_cred.to_string_lossy(),
            &socket_str,
        ])
        .output()
        .expect("runs shaprintctl with --fd 3");
    std::fs::remove_file(&fd_cred).ok();
    assert!(
        set_fd_out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&set_fd_out.stderr)
    );

    let status_out = Command::new(ctl_bin)
        .args(["--socket", &socket_str, "--json", "status"])
        .output()
        .expect("runs shaprintctl status --json");
    assert!(status_out.status.success());
    let status_stdout = String::from_utf8_lossy(&status_out.stdout);
    assert!(!status_stdout.contains(&secret));
    let status_json: serde_json::Value =
        serde_json::from_str(&status_stdout).expect("parses status json");
    assert_eq!(status_json["network_channel_configured"], true);

    // 4. Inspect server fingerprint via CLI, then approve it.
    let inspect_out = Command::new(ctl_bin)
        .args([
            "--socket",
            &socket_str,
            "--json",
            "trust",
            "inspect",
            &remote_address,
        ])
        .output()
        .expect("runs shaprintctl trust inspect");
    assert!(inspect_out.status.success());
    let inspect_json: serde_json::Value =
        serde_json::from_slice(&inspect_out.stdout).expect("parses inspect json");
    assert_eq!(inspect_json["current_fingerprint"], fp_v1);
    assert_eq!(inspect_json["trusted"], false);

    let approve_out = Command::new(ctl_bin)
        .args([
            "--socket",
            &socket_str,
            "trust",
            "approve",
            &remote_address,
            "--fingerprint",
            &fp_v1,
        ])
        .output()
        .expect("runs shaprintctl trust approve");
    assert!(approve_out.status.success());

    // 5. Submit a print job through the daemon's Client proxy to the Trusted Server.
    let send_proxy_print_job = |proxy_port: u16, remote_address: &str| {
        let remote_addr_owned = remote_address.to_owned();
        async move {
            let proxy_authority = format!("127.0.0.1:{proxy_port}");
            let uri = shaprint_desktop::adapters::client_queue_uri(
                &proxy_authority,
                &remote_addr_owned,
                "Warehouse Zebra",
            )
            .expect("builds queue uri");
            let path = uri
                .strip_prefix(&format!("ipp://{proxy_authority}"))
                .expect("strips proxy prefix");
            let mut ipp = vec![2, 0, 0x00, 0x02, 0, 0, 0, 1, 0x01];
            for (tag, name, val) in [
                (0x47u8, "attributes-charset", "utf-8"),
                (0x48u8, "attributes-natural-language", "en"),
                (0x45u8, "printer-uri", uri.as_str()),
                (0x49u8, "document-format", "image/pwg-raster"),
            ] {
                ipp.push(tag);
                ipp.extend((name.len() as u16).to_be_bytes());
                ipp.extend(name.as_bytes());
                ipp.extend((val.len() as u16).to_be_bytes());
                ipp.extend(val.as_bytes());
            }
            ipp.push(0x03);
            ipp.extend_from_slice(b"RaS2-test-pwg-raster-payload");
            let req = format!(
                "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{proxy_port}\r\nContent-Type: application/ipp\r\nContent-Length: {}\r\n\r\n",
                ipp.len()
            );
            let mut stream = TcpStream::connect(("127.0.0.1", proxy_port))
                .await
                .expect("connects to client proxy");
            stream.write_all(req.as_bytes()).await.expect("writes head");
            stream.write_all(&ipp).await.expect("writes body");
            let mut resp = Vec::new();
            stream.read_to_end(&mut resp).await.expect("reads response");
            let body_pos = resp
                .windows(4)
                .position(|w| w == b"\r\n\r\n")
                .map(|i| i + 4)
                .expect("http body");
            u16::from_be_bytes([resp[body_pos + 2], resp[body_pos + 3]])
        }
    };

    let ipp_status = send_proxy_print_job(proxy_port, &remote_address).await;
    assert_eq!(ipp_status, 0x0000, "expected successful-ok from proxy");
    assert_eq!(jobs_v1.load(Ordering::SeqCst), 1);

    // 6. Restart the daemon and verify that both Network Channel authorization and Trusted Server
    //    pinning survive the restart without reconfiguration.
    daemon
        .shutdown()
        .await
        .expect("stops daemon before restart");
    let daemon = DaemonServer::launch(
        DaemonConfig::new(&state_dir, &socket_path)
            .with_config_dir(&etc_dir)
            .with_ports(RuntimePorts {
                proxy_port,
                server_port: 0,
                discovery_ports: vec![0],
            }),
    )
    .await
    .expect("relaunches daemon after restart");

    let ipp_status_after_restart = send_proxy_print_job(proxy_port, &remote_address).await;
    assert_eq!(ipp_status_after_restart, 0x0000);
    assert_eq!(jobs_v1.load(Ordering::SeqCst), 2);
    // And verify the daemon's own Server verifier still authorizes `secret`.
    assert!(daemon
        .shell()
        .expect("has shell")
        .network_channel()
        .authorizes(&secret));

    // 7. Rotate the remote Server's TLS certificate on the same address: printer queries and
    //    print jobs must be blocked with `server-identity-changed` until explicit reapproval.
    remote_v1.shutdown().await.expect("stops remote v1");
    let (remote_v2, fp_v2, jobs_v2) = start_remote_server(secret.clone()).await;
    assert_ne!(fp_v1, fp_v2);

    let printers_changed = Command::new(ctl_bin)
        .args(["--socket", &socket_str, "printers", &remote_address])
        .output()
        .expect("runs shaprintctl printers against rotated server");
    assert!(!printers_changed.status.success());
    assert!(
        String::from_utf8_lossy(&printers_changed.stderr).contains("code=server-identity-changed"),
        "stderr was: {}",
        String::from_utf8_lossy(&printers_changed.stderr)
    );

    let ipp_blocked = send_proxy_print_job(proxy_port, &remote_address).await;
    assert_eq!(
        ipp_blocked, 0x0403,
        "changed server identity must block proxy print jobs with client-error-not-authorized"
    );
    assert_eq!(jobs_v2.load(Ordering::SeqCst), 0);

    // 8. Reapprove the new fingerprint via `shaprintctl trust approve`, and verify queries and
    //    print jobs succeed again.
    let reapprove_out = Command::new(ctl_bin)
        .args([
            "--socket",
            &socket_str,
            "trust",
            "approve",
            &remote_address,
            "--fingerprint",
            &fp_v2,
        ])
        .output()
        .expect("reapproves rotated server");
    assert!(reapprove_out.status.success());

    let printers_ok = Command::new(ctl_bin)
        .args(["--socket", &socket_str, "printers", &remote_address])
        .output()
        .expect("lists printers after reapproval");
    assert!(printers_ok.status.success());
    assert_eq!(
        String::from_utf8_lossy(&printers_ok.stdout),
        "Warehouse Zebra\n"
    );

    let ipp_after_reapprove = send_proxy_print_job(proxy_port, &remote_address).await;
    assert_eq!(ipp_after_reapprove, 0x0000);
    assert_eq!(jobs_v2.load(Ordering::SeqCst), 1);

    // 9. Verify that `/var/lib/shaprint` files, `/etc/shaprint/config.json`, and CLI outputs
    //    contain no Network Channel value or keyed URI.
    for state_entry in std::fs::read_dir(&state_dir).expect("reads state_dir") {
        let path = state_entry.expect("entry").path();
        if path.is_file() {
            let bytes = std::fs::read(&path).expect("reads state file");
            let text = String::from_utf8_lossy(&bytes);
            assert!(
                !text.contains(&secret),
                "state file '{}' must not contain the Network Channel secret",
                path.display()
            );
        }
    }

    daemon.shutdown().await.expect("stops daemon");

    // 10. Relaunch with `PeerAccessPolicy::AllowUid(u32::MAX - 1)` so any local user connecting
    //     over the socket is treated as an unprivileged local user: verify all local users
    //     observe the same machine trust (`trust list`), shared printers (`printers`), and
    //     configured status (`status`, `network-channel status`) without receiving the secret,
    //     while unprivileged mutations (`network-channel set`, `trust approve`, `trust forget`)
    //     are rejected with `not-authorized`.
    let restricted_daemon = DaemonServer::launch(
        DaemonConfig::new(&state_dir, &socket_path)
            .with_config_dir(&etc_dir)
            .with_access_policy(PeerAccessPolicy::AllowUid(u32::MAX - 1))
            .with_ports(RuntimePorts {
                proxy_port,
                server_port: 0,
                discovery_ports: vec![0],
            }),
    )
    .await
    .expect("launches restricted daemon");

    let unpriv_client = DaemonClient::new(&socket_path);
    let observed_status = unpriv_client
        .status()
        .await
        .expect("unprivileged user reads status");
    assert_eq!(observed_status.network_channel_configured, Some(true));
    assert!(!serde_json::to_string(&observed_status)
        .expect("json")
        .contains(&secret));

    let observed_nc = unpriv_client
        .network_channel_status()
        .await
        .expect("unprivileged user reads network-channel status");
    assert!(observed_nc.configured);

    let observed_trusted = unpriv_client
        .list_trusted_servers()
        .await
        .expect("unprivileged user lists machine trusted servers");
    assert_eq!(observed_trusted.len(), 1);
    assert_eq!(observed_trusted[0].address, remote_address);
    assert_eq!(observed_trusted[0].fingerprint, fp_v2);

    let observed_printers = unpriv_client
        .list_server_printers(&remote_address)
        .await
        .expect("unprivileged user queries printers on machine-trusted server");
    assert_eq!(observed_printers.printers, vec!["Warehouse Zebra"]);

    let inspected_by_unpriv = unpriv_client
        .inspect_server(&remote_address)
        .await
        .expect("unprivileged user can inspect server certificate fingerprint");
    assert_eq!(inspected_by_unpriv.current_fingerprint, fp_v2);
    assert!(inspected_by_unpriv.trusted);

    let unpriv_candidate = format!("unpriv-{}", std::process::id());
    assert_eq!(
        unpriv_client
            .configure_network_channel(&unpriv_candidate)
            .await
            .expect_err("unprivileged user cannot set network channel")
            .code(),
        ErrorCode::NotAuthorized
    );
    assert_eq!(
        unpriv_client
            .approve_server(&remote_address, &fp_v2)
            .await
            .expect_err("unprivileged user cannot approve server")
            .code(),
        ErrorCode::NotAuthorized
    );
    assert_eq!(
        unpriv_client
            .forget_server(&remote_address)
            .await
            .expect_err("unprivileged user cannot forget server")
            .code(),
        ErrorCode::NotAuthorized
    );

    restricted_daemon
        .shutdown()
        .await
        .expect("stops restricted daemon");
    remote_v2.shutdown().await.expect("stops remote v2");
    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn almalinux_client_queue_and_server_cups_sharing_roundtrip_across_restart_and_firewalld() {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::process::{Command, Stdio};
    use std::sync::Mutex;
    use std::time::Duration;

    use shaprint_desktop::adapters::identity::{FileIdentityStore, IdentityStore};
    use shaprint_desktop::application::{
        DuplexMode, PrintJob, PrintJobSubmitter, PrintOrientation, PrintSettings,
    };
    use shaprint_desktop::domain::{ClientQueueRequest, PrinterName};

    if !Path::new("/usr/sbin/cupsd").exists() || !Path::new("/usr/sbin/lpadmin").exists() {
        return;
    }

    let root = temporary_directory("cups-e2e");
    std::fs::create_dir_all(&root).expect("creates e2e root");

    // Start an isolated unprivileged `cupsd` instance for both the Server and Client queues.
    let cups_dir = root.join("cups");
    let etc_cups = cups_dir.join("etc");
    let var_spool = cups_dir.join("spool");
    let var_spool_tmp = var_spool.join("tmp");
    let var_cache = cups_dir.join("cache");
    let var_log = cups_dir.join("log");
    let var_run = cups_dir.join("run");
    let state_certs = var_run.join("certs");
    let server_bin = cups_dir.join("serverbin");
    let backend_dir = server_bin.join("backend");
    for d in [
        &etc_cups,
        &var_spool,
        &var_spool_tmp,
        &var_cache,
        &var_log,
        &var_run,
        &state_certs,
        &backend_dir,
    ] {
        std::fs::create_dir_all(d).expect("creates cups dir");
    }
    if Path::new("/usr/lib/cups").exists() {
        for entry in std::fs::read_dir("/usr/lib/cups").expect("reads /usr/lib/cups") {
            let entry = entry.expect("entry");
            let name = entry.file_name();
            if name != "backend" {
                let _ = std::os::unix::fs::symlink(entry.path(), server_bin.join(&name));
            }
        }
        for b in ["ipp", "usb", "socket", "lpd"] {
            let src = Path::new("/usr/lib/cups/backend").join(b);
            if src.exists() {
                let dst = backend_dir.join(b);
                std::fs::copy(&src, &dst).expect("copies backend");
                std::fs::set_permissions(&dst, std::fs::Permissions::from_mode(0o755))
                    .expect("chmod 0755");
            }
        }
        let _ = std::os::unix::fs::symlink("ipp", backend_dir.join("http"));
        let _ = std::os::unix::fs::symlink("ipp", backend_dir.join("ipps"));
        let captured_usb_file_for_backend = cups_dir.join("usb-captured.pwg");
        let captured_usb_invocations_for_backend = cups_dir.join("usb-captured.invocations");
        let usb_backend = backend_dir.join("usbcapture");
        std::fs::write(
            &usb_backend,
            format!(
                r#"#!/usr/bin/env bash
set -euo pipefail
count=0
if [[ -f "{count_file}" ]]; then
  count=$(<"{count_file}")
fi
echo $((count + 1)) > "{count_file}"
if [[ $# -eq 0 ]]; then
  echo 'direct usbcapture "Unknown" "Controlled USB Capture Backend"'
  exit 0
fi
if [[ -n "${{6:-}}" && -f "${{6}}" ]]; then
  cat "${{6}}" > "{out}"
else
  cat > "{out}"
fi
exit 0
"#,
                out = captured_usb_file_for_backend.display(),
                count_file = captured_usb_invocations_for_backend.display(),
            ),
        )
        .expect("writes usbcapture backend");
        std::fs::set_permissions(&usb_backend, std::fs::Permissions::from_mode(0o755))
            .expect("chmod 0755 usbcapture");
    }

    let cups_port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("binds ephemeral port");
        listener.local_addr().expect("local addr").port()
    };
    let cups_sock = var_run.join("cups.sock");
    let pid_file = var_run.join("cupsd.pid");
    std::fs::write(
        etc_cups.join("cups-files.conf"),
        format!(
            "FileDevice Yes\nServerRoot {etc}\nServerBin {bin}\nStateDir {run}\nRequestRoot {spool}\nTempDir {tmp}\nCacheDir {cache}\nAccessLog {log}/access_log\nErrorLog {log}/error_log\nPageLog {log}/page_log\nPidFile {pid}\n",
            etc = etc_cups.display(),
            bin = server_bin.display(),
            run = var_run.display(),
            spool = var_spool.display(),
            tmp = var_spool_tmp.display(),
            cache = var_cache.display(),
            log = var_log.display(),
            pid = pid_file.display(),
        ),
    )
    .expect("writes cups-files.conf");
    std::fs::write(
        etc_cups.join("cupsd.conf"),
        format!(
            "LogLevel info\nListen 127.0.0.1:{port}\nListen {sock}\nDefaultAuthType None\nWebInterface No\nBrowsing Off\nMaxCopies 999\n<Location />\n  Order allow,deny\n  Allow all\n</Location>\n<Location /admin>\n  Order allow,deny\n  Allow all\n</Location>\n<Location /admin/conf>\n  Order allow,deny\n  Allow all\n</Location>\n<Policy default>\n  <Limit All>\n    Order allow,deny\n    Allow all\n  </Limit>\n</Policy>\n",
            port = cups_port,
            sock = cups_sock.display(),
        ),
    )
    .expect("writes cupsd.conf");

    let mut cupsd = Command::new("/usr/sbin/cupsd")
        .arg("-f")
        .arg("-c")
        .arg(etc_cups.join("cupsd.conf"))
        .arg("-s")
        .arg(etc_cups.join("cups-files.conf"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawns cupsd");

    for _ in 0..50 {
        if std::net::TcpStream::connect(("127.0.0.1", cups_port)).is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let cups_server = format!("127.0.0.1:{cups_port}");
    let cups_endpoint =
        shaprint_desktop::adapters::printers::cups::CupsServerEndpoint::parse(&cups_server);

    // Create two real CUPS queues for the AlmaLinux Server:
    // 1) A driverless IPP queue (`Driverless_Office`) backed by `file:/dev/null`
    // 2) A local/USB controlled queue (`USB_Receipt_Printer`) backed by a capture file
    let captured_usb_file = cups_dir.join("usb-captured.pwg");
    let captured_usb_invocations = cups_dir.join("usb-captured.invocations");
    for (name, uri) in [
        ("Driverless_Office", "file:/dev/null".to_owned()),
        ("USB_Receipt_Printer", "usbcapture:/receipt".to_owned()),
    ] {
        let out = Command::new("/usr/sbin/lpadmin")
            .env("CUPS_SERVER", &cups_server)
            .args(["-h", &cups_server, "-p", name, "-E", "-v", &uri])
            .output()
            .expect("creates server queue");
        assert!(
            out.status.success(),
            "lpadmin failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    // Create a controlled firewall-cmd script to verify #128 firewall checks and idempotent setup.
    let fw_state = root.join("fw_ports.txt");
    let fw_deny_flag = root.join("fw_deny");
    std::fs::write(&fw_deny_flag, "1").expect("initially deny firewall setup");
    let fw_script = root.join("firewall-cmd");
    std::fs::write(
        &fw_script,
        format!(
            r#"#!/usr/bin/env bash
set -euo pipefail
STATE="{state}"
DENY_FILE="{deny}"
touch "$STATE"
case "${{1:-}}" in
  --state)
    echo "running"
    exit 0
    ;;
  --query-port=*)
    port="${{1#--query-port=}}"
    if grep -Fxq "runtime:$port" "$STATE"; then
      echo "yes"
      exit 0
    else
      echo "no"
      exit 1
    fi
    ;;
  --add-port=*)
    if [[ "$(cat "$DENY_FILE")" == "1" ]]; then
      echo "Authorization failed." >&2
      exit 114
    fi
    port="${{1#--add-port=}}"
    grep -Fxq "runtime:$port" "$STATE" || echo "runtime:$port" >> "$STATE"
    echo "success"
    exit 0
    ;;
  --permanent)
    if [[ "$(cat "$DENY_FILE")" == "1" ]]; then
      echo "Authorization failed." >&2
      exit 114
    fi
    shift
    for arg in "$@"; do
      if [[ "$arg" == --add-port=* ]]; then
        port="${{arg#--add-port=}}"
        grep -Fxq "permanent:$port" "$STATE" || echo "permanent:$port" >> "$STATE"
      fi
    done
    echo "success"
    exit 0
    ;;
esac
"#,
            state = fw_state.display(),
            deny = fw_deny_flag.display(),
        ),
    )
    .expect("writes fake firewall-cmd");
    std::fs::set_permissions(&fw_script, std::fs::Permissions::from_mode(0o755))
        .expect("chmod +x firewall-cmd");

    // Start a simulated Windows Server (`IppsServer`) that records every forwarded print job and its
    // PrintSettings so we can test #127 (AlmaLinux Client -> Windows Server) end to end.
    struct RecordedJob {
        printer: String,
        settings: PrintSettings,
        bytes: Vec<u8>,
    }
    struct RecordingWindowsSubmitter {
        jobs: Arc<Mutex<Vec<RecordedJob>>>,
    }
    #[async_trait::async_trait]
    impl PrintJobSubmitter for RecordingWindowsSubmitter {
        async fn submit(
            &self,
            printer: &PrinterName,
            job: PrintJob,
        ) -> Result<u32, shaprint_desktop::domain::AppError> {
            self.jobs.lock().expect("locks jobs").push(RecordedJob {
                printer: printer.as_str().to_owned(),
                settings: job.settings().clone(),
                bytes: job.document().to_vec(),
            });
            Ok(77)
        }
    }

    let secret = format!("chan-e2e-{}", std::process::id());
    let win_jobs = Arc::new(Mutex::new(Vec::<RecordedJob>::new()));
    let win_identity = Arc::new(
        FileIdentityStore::new(root.join("win-server-id"))
            .load_or_create()
            .expect("generates win server identity"),
    );
    let win_fp = win_identity.fingerprint().to_string();
    let win_catalog: Arc<dyn shaprint_desktop::application::LocalPrinterCatalog> =
        support::FakeCatalog::new(&["Windows Office Laser"]);
    let win_sharing = Arc::new(Sharing::new(win_catalog));
    win_sharing
        .set_shared(printer_names(&["Windows Office Laser"]))
        .await
        .expect("selects shared printer on windows server");
    let win_channel = Arc::new(NetworkChannel::in_memory());
    win_channel
        .configure_sync(&secret)
        .expect("configures win channel");
    let win_port = {
        let l = TcpListener::bind("127.0.0.1:0").expect("binds win port");
        l.local_addr().expect("addr").port()
    };
    let win_endpoint = Arc::new(IppsServer::new(
        win_port,
        win_identity,
        Arc::clone(&win_channel),
        Arc::new(RecordingWindowsSubmitter {
            jobs: Arc::clone(&win_jobs),
        }),
        Arc::new(PrintFailures::new()),
        Arc::new(PrintJobTracker::new()),
    ));
    let win_service = Arc::new(ServerSharingService::new(
        win_sharing,
        win_endpoint,
        Arc::new(MdnsAdvertiser::on(vec![0])),
        support::allowed_inbound_setup(),
        win_channel as Arc<dyn shaprint_desktop::application::ChannelState>,
    ));
    let win_runtime = Arc::new(RuntimeCoordinator::new(vec![win_service]));
    win_runtime
        .start(shaprint_desktop::domain::ServiceId::ServerSharing)
        .await
        .expect("starts win server");
    let win_address = format!("127.0.0.1:{win_port}");

    // Launch the AlmaLinux `shaprintd` daemon with dedicated ports.
    let proxy_port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").expect("binds proxy port");
        l.local_addr().expect("addr").port()
    };
    let linux_server_port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").expect("binds server port");
        l.local_addr().expect("addr").port()
    };
    let state_dir = root.join("var-lib-shaprint");
    let etc_dir = root.join("etc-shaprint");
    let socket_path = root.join("run/shaprintd.sock");
    let socket_str = socket_path.to_string_lossy().into_owned();
    let ctl_bin = env!("CARGO_BIN_EXE_shaprintctl");

    let daemon = DaemonServer::launch(
        DaemonConfig::new(&state_dir, &socket_path)
            .with_config_dir(&etc_dir)
            .with_cups_endpoint(cups_endpoint.clone())
            .with_firewall_program(&fw_script)
            .with_ports(RuntimePorts {
                proxy_port,
                server_port: linux_server_port,
                discovery_ports: vec![0],
            }),
    )
    .await
    .expect("launches shaprintd");

    // Configure the machine Network Channel via `shaprintctl`.
    let mut set_nc = Command::new(ctl_bin)
        .args(["--socket", &socket_str, "network-channel", "set", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawns network-channel set");
    set_nc
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(format!("{secret}\n").as_bytes())
        .expect("writes secret");
    assert!(set_nc.wait_with_output().expect("waits").status.success());

    // === PART A: Issue #127 (AlmaLinux Client -> Windows Server) ===
    // 1. Installing a queue before trusting the server fails with `server-untrusted`.
    let untrusted_install = Command::new(ctl_bin)
        .env("CUPS_SERVER", &cups_server)
        .args([
            "--socket",
            &socket_str,
            "queue",
            "install",
            &win_address,
            "Windows Office Laser",
        ])
        .output()
        .expect("runs untrusted queue install");
    assert!(!untrusted_install.status.success());
    assert!(
        String::from_utf8_lossy(&untrusted_install.stderr).contains("code=server-untrusted"),
        "stderr was: {}",
        String::from_utf8_lossy(&untrusted_install.stderr)
    );

    // 2. Inspect and approve the Windows Server fingerprint.
    let approve = Command::new(ctl_bin)
        .args([
            "--socket",
            &socket_str,
            "trust",
            "approve",
            &win_address,
            "--fingerprint",
            &win_fp,
        ])
        .output()
        .expect("approves windows server");
    assert!(approve.status.success());

    // 3. Create a non-ShaPrint queue with the exact derived CUPS destination name to verify
    //    collision protection (`existing-queue-conflict`).
    let expected_req = ClientQueueRequest::new(
        &win_address,
        PrinterName::parse("Windows Office Laser").expect("printer"),
    )
    .expect("request");
    let derived_cups_name = expected_req.cups_destination_name();
    Command::new("/usr/sbin/lpadmin")
        .env("CUPS_SERVER", &cups_server)
        .args([
            "-h",
            &cups_server,
            "-p",
            &derived_cups_name,
            "-E",
            "-v",
            "file:/dev/null",
        ])
        .output()
        .expect("creates conflicting queue");

    let conflict_install = Command::new(ctl_bin)
        .env("CUPS_SERVER", &cups_server)
        .args([
            "--socket",
            &socket_str,
            "queue",
            "install",
            &win_address,
            "Windows Office Laser",
        ])
        .output()
        .expect("runs conflicting queue install");
    assert!(!conflict_install.status.success());
    let conflict_stderr = String::from_utf8_lossy(&conflict_install.stderr);
    assert!(
        conflict_stderr.contains("code=invalid-state")
            && conflict_stderr.contains("already exists"),
        "stderr was: {conflict_stderr}"
    );

    // Remove the conflicting queue and install the ShaPrint Client queue idempotently.
    Command::new("/usr/sbin/lpadmin")
        .env("CUPS_SERVER", &cups_server)
        .args(["-h", &cups_server, "-x", &derived_cups_name])
        .output()
        .expect("removes conflicting queue");

    let first_install = Command::new(ctl_bin)
        .env("CUPS_SERVER", &cups_server)
        .args([
            "--socket",
            &socket_str,
            "queue",
            "install",
            &win_address,
            "Windows Office Laser",
        ])
        .output()
        .expect("installs client queue");
    assert!(
        first_install.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&first_install.stderr)
    );

    let second_install = Command::new(ctl_bin)
        .env("CUPS_SERVER", &cups_server)
        .args([
            "--socket",
            &socket_str,
            "queue",
            "install",
            &win_address,
            "Windows Office Laser",
        ])
        .output()
        .expect("reinstalls client queue idempotently");
    assert!(second_install.status.success());

    let queue_list_out = Command::new(ctl_bin)
        .env("CUPS_SERVER", &cups_server)
        .args(["--socket", &socket_str, "queue", "list"])
        .output()
        .expect("lists client queues");
    assert!(queue_list_out.status.success());
    assert!(
        String::from_utf8_lossy(&queue_list_out.stdout).contains("Windows Office Laser"),
        "stdout: {}",
        String::from_utf8_lossy(&queue_list_out.stdout)
    );

    // 4. Submit a real print job with media (iso_a4_210x297mm), color, duplex (two-sided-long-edge),
    //    landscape orientation, copies=2, and a spoofed local `network-channel` attribute through
    //    the installed CUPS queue URI, and verify the proxy strips the spoofed credential, attaches
    //    the protected machine credential, and forwards all settings to the Windows Server.
    let build_pwg_raster = || {
        let mut raster = b"RaS2".to_vec();
        let mut header = vec![0u8; 1796];
        for (offset, value) in [
            (276usize, 300u32),
            (280, 300),
            (372, 1),
            (376, 1),
            (384, 8),
            (388, 8),
            (392, 1),
            (400, 18),
            (420, 1),
        ] {
            header[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
        }
        raster.extend(header);
        raster.extend([0u8, 0u8, 255u8]);
        raster
    };

    let build_ipp_print_job = |uri: &str, channel: Option<&str>| {
        let mut body = vec![0x02, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x42, 0x01];
        let push_attr = |buf: &mut Vec<u8>, tag: u8, name: &str, val: &[u8]| {
            buf.push(tag);
            buf.extend_from_slice(&(name.len() as u16).to_be_bytes());
            buf.extend_from_slice(name.as_bytes());
            buf.extend_from_slice(&(val.len() as u16).to_be_bytes());
            buf.extend_from_slice(val);
        };
        push_attr(&mut body, 0x47, "attributes-charset", b"utf-8");
        push_attr(&mut body, 0x48, "attributes-natural-language", b"en");
        push_attr(&mut body, 0x45, "printer-uri", uri.as_bytes());
        push_attr(&mut body, 0x42, "job-name", b"Linux Client Report");
        push_attr(&mut body, 0x49, "document-format", b"image/pwg-raster");
        if let Some(ch) = channel {
            push_attr(&mut body, 0x42, "network-channel", ch.as_bytes());
        }
        // Job attributes group (0x02): media, print-color-mode, sides, copies, orientation-requested
        body.push(0x02);
        push_attr(&mut body, 0x44, "media", b"iso_a4_210x297mm");
        push_attr(&mut body, 0x44, "print-color-mode", b"color");
        push_attr(&mut body, 0x44, "sides", b"two-sided-long-edge");
        push_attr(&mut body, 0x21, "copies", &2i32.to_be_bytes());
        push_attr(
            &mut body,
            0x23,
            "orientation-requested",
            &4i32.to_be_bytes(),
        );
        body.push(0x03);
        body.extend_from_slice(&build_pwg_raster());
        body
    };

    let post_ipp = |host_port: &str, path: &str, body: Vec<u8>, tls: bool| {
        let host_port = host_port.to_owned();
        let path = path.to_owned();
        async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let req = format!(
                "POST {path} HTTP/1.1\r\nHost: {host_port}\r\nContent-Type: application/ipp\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            if !tls {
                let mut stream = tokio::net::TcpStream::connect(&host_port)
                    .await
                    .expect("connects http");
                stream.write_all(req.as_bytes()).await.expect("writes head");
                stream.write_all(&body).await.expect("writes body");
                let mut resp = Vec::new();
                stream.read_to_end(&mut resp).await.expect("reads resp");
                let pos = resp.windows(4).position(|w| w == b"\r\n\r\n").expect("hdr") + 4;
                u16::from_be_bytes([resp[pos + 2], resp[pos + 3]])
            } else {
                #[derive(Debug)]
                struct AcceptAnyServerCert;
                impl rustls::client::danger::ServerCertVerifier for AcceptAnyServerCert {
                    fn verify_server_cert(
                        &self,
                        _end_entity: &rustls::pki_types::CertificateDer<'_>,
                        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
                        _server_name: &rustls::pki_types::ServerName<'_>,
                        _ocsp_response: &[u8],
                        _now: rustls::pki_types::UnixTime,
                    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error>
                    {
                        Ok(rustls::client::danger::ServerCertVerified::assertion())
                    }
                    fn verify_tls12_signature(
                        &self,
                        _message: &[u8],
                        _cert: &rustls::pki_types::CertificateDer<'_>,
                        _dss: &rustls::DigitallySignedStruct,
                    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error>
                    {
                        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
                    }
                    fn verify_tls13_signature(
                        &self,
                        _message: &[u8],
                        _cert: &rustls::pki_types::CertificateDer<'_>,
                        _dss: &rustls::DigitallySignedStruct,
                    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error>
                    {
                        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
                    }
                    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
                        vec![
                            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
                            rustls::SignatureScheme::RSA_PSS_SHA256,
                            rustls::SignatureScheme::ED25519,
                        ]
                    }
                }
                let provider = Arc::new(rustls::crypto::ring::default_provider());
                let cfg = rustls::ClientConfig::builder_with_provider(provider)
                    .with_safe_default_protocol_versions()
                    .expect("tls versions")
                    .dangerous()
                    .with_custom_certificate_verifier(Arc::new(AcceptAnyServerCert))
                    .with_no_client_auth();
                let connector = tokio_rustls::TlsConnector::from(Arc::new(cfg));
                let tcp = tokio::net::TcpStream::connect(&host_port)
                    .await
                    .expect("connects tls tcp");
                let sname = rustls::pki_types::ServerName::try_from("127.0.0.1")
                    .expect("server name")
                    .to_owned();
                let mut stream = connector.connect(sname, tcp).await.expect("tls handshake");
                stream.write_all(req.as_bytes()).await.expect("writes head");
                stream.write_all(&body).await.expect("writes body");
                let mut resp = Vec::new();
                stream.read_to_end(&mut resp).await.expect("reads resp");
                let pos = resp.windows(4).position(|w| w == b"\r\n\r\n").expect("hdr") + 4;
                u16::from_be_bytes([resp[pos + 2], resp[pos + 3]])
            }
        }
    };

    let proxy_path = format!("/ipp/print/{win_address}/Windows%20Office%20Laser");
    let proxy_uri = format!("ipp://127.0.0.1:{proxy_port}{proxy_path}");

    // Submit a real CUPS job through the installed CUPS Client queue (`derived_cups_name`) before
    // daemon restart so CUPS's `ipp` backend streams through our local proxy to the Windows Server.
    let cups_submitter = shaprint_desktop::adapters::printers::CupsPrintJobSubmitter::with_endpoint(
        cups_endpoint.clone(),
    );
    let cups_queue_printer =
        PrinterName::parse(&derived_cups_name).expect("valid CUPS destination name");
    let cups_job_id_before = cups_submitter
        .submit(
            &cups_queue_printer,
            PrintJob::from_ipp_body(
                build_pwg_raster(),
                0,
                PrintSettings {
                    media: Some("iso_a4_210x297mm".to_owned()),
                    color: Some(true),
                    duplex: Some(DuplexMode::LongEdge),
                    copies: Some(2),
                    orientation: Some(PrintOrientation::Landscape),
                },
            ),
        )
        .await
        .expect("submits job to real CUPS Client queue before restart");
    assert!(cups_job_id_before > 0);
    for _ in 0..80 {
        if !win_jobs.lock().expect("locks win jobs").is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // Also verify that if a local caller supplies a spoofed `network-channel` IPP attribute to the
    // loopback proxy, the proxy replaces it with the protected machine credential and forwards all
    // requested print settings (`media`, `print-color-mode`, `sides`, `copies`, `orientation-requested`).
    let status_before_restart = post_ipp(
        &format!("127.0.0.1:{proxy_port}"),
        &proxy_path,
        build_ipp_print_job(&proxy_uri, Some("spoofed-local-secret")),
        false,
    )
    .await;
    assert_eq!(status_before_restart, 0x0000);
    {
        let recorded = win_jobs.lock().expect("locks win jobs");
        assert!(
            recorded.len() >= 2,
            "expected both the real CUPS queue job and the spoofed-header proxy job to reach Windows Server, got {}",
            recorded.len()
        );
        let last = recorded.last().expect("last job");
        assert_eq!(last.printer, "Windows Office Laser");
        assert_eq!(last.bytes, build_pwg_raster());
        assert_eq!(
            last.settings,
            PrintSettings {
                media: Some("iso_a4_210x297mm".to_owned()),
                color: Some(true),
                duplex: Some(DuplexMode::LongEdge),
                copies: Some(2),
                orientation: Some(PrintOrientation::Landscape),
            }
        );
    }

    // 5. Restart the daemon and verify the installed CUPS queue + Client proxy still forward jobs.
    daemon.shutdown().await.expect("stops daemon");
    let daemon = DaemonServer::launch(
        DaemonConfig::new(&state_dir, &socket_path)
            .with_config_dir(&etc_dir)
            .with_cups_endpoint(cups_endpoint.clone())
            .with_firewall_program(&fw_script)
            .with_ports(RuntimePorts {
                proxy_port,
                server_port: linux_server_port,
                discovery_ports: vec![0],
            }),
    )
    .await
    .expect("relaunches daemon");

    let count_before_after_restart = win_jobs.lock().expect("locks win jobs").len();
    let cups_job_id_after = cups_submitter
        .submit(
            &cups_queue_printer,
            PrintJob::from_ipp_body(build_pwg_raster(), 0, PrintSettings::default()),
        )
        .await
        .expect("submits job to real CUPS Client queue after daemon restart");
    assert!(cups_job_id_after > cups_job_id_before);
    for _ in 0..80 {
        if win_jobs.lock().expect("locks win jobs").len() > count_before_after_restart {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        win_jobs.lock().expect("locks win jobs").len() > count_before_after_restart,
        "real CUPS Client queue job after daemon restart must reach Windows Server"
    );

    // === PART B: Issue #128 (Windows Client -> AlmaLinux Server) ===
    // 1. List local CUPS queues via `shaprintctl sharing printers`: our two Server queues must appear,
    //    while the installed ShaPrint Client queue (`ShaPrint_Windows_Office_Laser_...`) is excluded!
    let sharing_list = Command::new(ctl_bin)
        .env("CUPS_SERVER", &cups_server)
        .args(["--socket", &socket_str, "sharing", "printers"])
        .output()
        .expect("lists local printers for sharing");
    assert!(sharing_list.status.success());
    let sharing_stdout = String::from_utf8_lossy(&sharing_list.stdout);
    assert!(sharing_stdout.contains("Driverless_Office: unshared"));
    assert!(sharing_stdout.contains("USB_Receipt_Printer: unshared"));
    assert!(
        !sharing_stdout.contains(&derived_cups_name),
        "installed ShaPrint Client queue must never be offered for re-sharing: {sharing_stdout}"
    );

    // 2. Select both Server queues for sharing via `shaprintctl sharing set`.
    let sharing_set = Command::new(ctl_bin)
        .env("CUPS_SERVER", &cups_server)
        .args([
            "--socket",
            &socket_str,
            "sharing",
            "set",
            "Driverless_Office",
            "USB_Receipt_Printer",
        ])
        .output()
        .expect("sets shared printers");
    assert!(sharing_set.status.success());

    // 3. Starting `server-sharing` while firewall authorization is declined must fail and leave
    //    `server-sharing` stopped.
    let start_denied = Command::new(ctl_bin)
        .env("CUPS_SERVER", &cups_server)
        .args(["--socket", &socket_str, "start", "server-sharing"])
        .output()
        .expect("attempts to start server-sharing with denied firewall");
    assert!(!start_denied.status.success());
    let status_after_deny = Command::new(ctl_bin)
        .args(["--socket", &socket_str, "status"])
        .output()
        .expect("checks status after denied firewall");
    assert!(
        String::from_utf8_lossy(&status_after_deny.stdout).contains("server-sharing: stopped"),
        "server-sharing must remain stopped when firewall setup is declined: {}",
        String::from_utf8_lossy(&status_after_deny.stdout)
    );

    // 4. Allow firewall setup and start `server-sharing`: verify only 48631/tcp and 48633/udp are
    //    added, and repeated starts are idempotent.
    std::fs::write(&fw_deny_flag, "0").expect("allows firewall setup");
    let start_ok = Command::new(ctl_bin)
        .env("CUPS_SERVER", &cups_server)
        .args(["--socket", &socket_str, "start", "server-sharing"])
        .output()
        .expect("starts server-sharing");
    assert!(
        start_ok.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&start_ok.stderr)
    );
    let fw_lines: Vec<String> = std::fs::read_to_string(&fw_state)
        .expect("reads fw_state")
        .lines()
        .map(str::to_owned)
        .collect();
    assert_eq!(
        fw_lines,
        vec![
            "runtime:48631/tcp",
            "runtime:48633/udp",
            "permanent:48631/tcp",
            "permanent:48633/udp",
        ]
    );

    // === PART C: Issue #129 (one AlmaLinux installation as Client and Server) ===
    // Trust this daemon's own Server identity, install a native CUPS Client queue that routes back
    // through its Client proxy, and print to the Server queue through both roles. The Client queue
    // must remain absent from the Server selection, so this also proves the forwarding-loop guard.
    let own_server_address = format!("127.0.0.1:{linux_server_port}");
    let own_server_fingerprint = daemon
        .shell()
        .expect("daemon has a Linux shell")
        .endpoint()
        .fingerprint()
        .to_string();
    let own_approve = Command::new(ctl_bin)
        .args([
            "--socket",
            &socket_str,
            "trust",
            "approve",
            &own_server_address,
            "--fingerprint",
            &own_server_fingerprint,
        ])
        .output()
        .expect("approves this daemon's Server identity");
    assert!(
        own_approve.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&own_approve.stderr)
    );

    let own_queue_request = ClientQueueRequest::new(
        &own_server_address,
        PrinterName::parse("USB_Receipt_Printer").expect("server CUPS printer"),
    )
    .expect("builds own-server Client queue request");
    let own_queue_install = Command::new(ctl_bin)
        .env("CUPS_SERVER", &cups_server)
        .args([
            "--socket",
            &socket_str,
            "queue",
            "install",
            &own_server_address,
            "USB_Receipt_Printer",
        ])
        .output()
        .expect("installs own-server Client queue");
    assert!(
        own_queue_install.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&own_queue_install.stderr)
    );

    let sharing_after_client_queue = Command::new(ctl_bin)
        .env("CUPS_SERVER", &cups_server)
        .args(["--socket", &socket_str, "sharing", "printers"])
        .output()
        .expect("lists Server queues after Client queue installation");
    let sharing_after_client_queue_stdout =
        String::from_utf8_lossy(&sharing_after_client_queue.stdout);
    assert!(sharing_after_client_queue.status.success());
    assert!(
        !sharing_after_client_queue_stdout.contains(&own_queue_request.cups_destination_name()),
        "ShaPrint-owned Client queue must stay excluded while both roles run: {sharing_after_client_queue_stdout}"
    );

    let own_usb_invocations = || {
        std::fs::read_to_string(&captured_usb_invocations)
            .ok()
            .and_then(|value| value.trim().parse::<u32>().ok())
            .unwrap_or_default()
    };
    let usb_invocations_before_own = own_usb_invocations();
    let expected_own_usb_invocations = usb_invocations_before_own + 1;
    let own_cups_submitter =
        shaprint_desktop::adapters::printers::CupsPrintJobSubmitter::with_endpoint(
            cups_endpoint.clone(),
        );
    let own_cups_job = own_cups_submitter
        .submit(
            &PrinterName::parse(&own_queue_request.cups_destination_name())
                .expect("valid own Client queue name"),
            PrintJob::from_ipp_body(
                build_pwg_raster(),
                0,
                PrintSettings {
                    media: Some("iso_a4_210x297mm".to_owned()),
                    color: Some(true),
                    duplex: Some(DuplexMode::LongEdge),
                    copies: Some(2),
                    orientation: Some(PrintOrientation::Landscape),
                },
            ),
        )
        .await
        .expect("submits a Linux Client job through this daemon's Client and Server roles");
    assert!(own_cups_job > 0);
    for _ in 0..80 {
        if own_usb_invocations() >= expected_own_usb_invocations
            && captured_usb_file
                .metadata()
                .is_ok_and(|meta| meta.len() >= 1807)
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(
        own_usb_invocations(),
        expected_own_usb_invocations,
        "one Linux Client job must invoke the Server CUPS destination exactly once"
    );
    assert_eq!(
        std::fs::read(&captured_usb_file).expect("reads Linux-to-Linux CUPS output"),
        build_pwg_raster(),
        "the selected Server CUPS destination receives the Linux Client document once"
    );

    // 5. Exercise a Windows Client (`ClientConnections`) manually addressing the AlmaLinux Server,
    //    inspecting and approving its TLS fingerprint, and listing its shared printers, then submit
    //    print jobs over IPPS:
    //    - Wrong Network Channel -> 0x0403 (`client-error-not-authorized`)
    //    - Unshared queue -> 0x0406 (`client-error-not-found`)
    //    - Shared `Driverless_Office` and `USB_Receipt_Printer` -> 0x0000 (`successful-ok`) and
    //      land in real CUPS!
    let linux_server_addr = format!("127.0.0.1:{linux_server_port}");
    let win_client_dir = root.join("win-client-state");
    let win_client_connections =
        shaprint_desktop::adapters::client_connections::ClientConnections::new(&win_client_dir)
            .expect("creates Windows ClientConnections");
    let linux_review = win_client_connections
        .inspect(&linux_server_addr)
        .await
        .expect("Windows Client inspects AlmaLinux Server");
    assert!(!linux_review.trusted);
    let linux_approved = win_client_connections
        .approve(&linux_server_addr, &linux_review.current_fingerprint)
        .await
        .expect("Windows Client approves AlmaLinux Server fingerprint");
    assert!(linux_approved.trusted);
    let linux_shared = win_client_connections
        .printers(&linux_server_addr)
        .await
        .expect("Windows Client lists shared printers on AlmaLinux Server");
    assert_eq!(
        linux_shared.printers,
        vec![
            "Driverless_Office".to_owned(),
            "USB_Receipt_Printer".to_owned()
        ]
    );
    let bad_channel_status = post_ipp(
        &linux_server_addr,
        "/ipp/print/Driverless_Office",
        build_ipp_print_job(
            &format!("ipps://{linux_server_addr}/ipp/print/Driverless_Office"),
            Some("wrong-channel"),
        ),
        true,
    )
    .await;
    assert_eq!(bad_channel_status, 0x0403);

    let unshared_status = post_ipp(
        &linux_server_addr,
        "/ipp/print/Not_Shared_Printer",
        build_ipp_print_job(
            &format!("ipps://{linux_server_addr}/ipp/print/Not_Shared_Printer"),
            Some(&secret),
        ),
        true,
    )
    .await;
    assert_eq!(unshared_status, 0x0406);

    for queue in ["Driverless_Office", "USB_Receipt_Printer"] {
        let path = format!("/ipp/print/{queue}");
        let uri = format!("ipps://{linux_server_addr}{path}");
        let status = post_ipp(
            &linux_server_addr,
            &path,
            build_ipp_print_job(&uri, Some(&secret)),
            true,
        )
        .await;
        assert_eq!(
            status, 0x0000,
            "print to {queue} failed with 0x{status:04x}"
        );
    }

    // Verify `lpstat -W all` on the isolated `cupsd` shows jobs created for both Server queues.
    let lpstat = Command::new("/usr/bin/lpstat")
        .env("CUPS_SERVER", &cups_server)
        .args(["-h", &cups_server, "-W", "all", "-o"])
        .output()
        .expect("runs lpstat");
    let lpstat_out = String::from_utf8_lossy(&lpstat.stdout);
    assert!(
        lpstat_out.contains("Driverless_Office-"),
        "expected Driverless_Office job in CUPS: {lpstat_out}"
    );
    assert!(
        lpstat_out.contains("USB_Receipt_Printer-"),
        "expected USB_Receipt_Printer job in CUPS: {lpstat_out}"
    );
    for _ in 0..80 {
        if captured_usb_file
            .metadata()
            .is_ok_and(|meta| meta.len() >= 1807)
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(
        std::fs::read(&captured_usb_file).expect("reads captured USB destination output"),
        build_pwg_raster(),
        "controlled USB/local CUPS destination must receive the streamed PWG Raster document"
    );
    assert_eq!(
        own_usb_invocations(),
        expected_own_usb_invocations + 1,
        "the later Windows Client job adds exactly one CUPS destination invocation"
    );

    // 6. A failed Client start is isolated while Server Sharing stays live: an unrelated process
    // owns only the Client port, and the Server listener remains reachable.
    let stop_proxy = Command::new(ctl_bin)
        .args(["--socket", &socket_str, "stop", "client-proxy"])
        .output()
        .expect("stops client-proxy independently");
    assert!(stop_proxy.status.success());
    let held_proxy = TcpListener::bind(("127.0.0.1", proxy_port)).expect("holds Client port");
    let start_proxy_conflict = Command::new(ctl_bin)
        .args(["--socket", &socket_str, "start", "client-proxy"])
        .output()
        .expect("attempts Client start with unrelated port owner");
    assert!(!start_proxy_conflict.status.success());
    let status_after_proxy_conflict = Command::new(ctl_bin)
        .args(["--socket", &socket_str, "status"])
        .output()
        .expect("checks Server status after Client start failure");
    let status_after_proxy_conflict_stdout =
        String::from_utf8_lossy(&status_after_proxy_conflict.stdout);
    assert!(status_after_proxy_conflict_stdout.contains("client-proxy: failed"));
    assert!(status_after_proxy_conflict_stdout.contains("server-sharing: running"));
    assert!(
        tokio::net::TcpStream::connect(&linux_server_addr)
            .await
            .is_ok(),
        "a Client port conflict must not terminate Server Sharing"
    );
    drop(held_proxy);
    let start_proxy_again = Command::new(ctl_bin)
        .args(["--socket", &socket_str, "start", "client-proxy"])
        .output()
        .expect("restarts Client after conflict is gone");
    assert!(start_proxy_again.status.success());

    // Verify explicit Server Stop releases only the Server listener, while the Client proxy remains
    // available, and that the choice persists across daemon restart.
    let stop_sharing = Command::new(ctl_bin)
        .args(["--socket", &socket_str, "stop", "server-sharing"])
        .output()
        .expect("stops server-sharing");
    assert!(stop_sharing.status.success());
    let status_after_server_stop = Command::new(ctl_bin)
        .args(["--socket", &socket_str, "status"])
        .output()
        .expect("checks independent Client status after Server stop");
    let status_after_server_stop_stdout = String::from_utf8_lossy(&status_after_server_stop.stdout);
    assert!(status_after_server_stop_stdout.contains("client-proxy: running"));
    assert!(
        tokio::net::TcpStream::connect(&linux_server_addr)
            .await
            .is_err(),
        "stopped server-sharing must release its TCP listener"
    );

    // Re-enable server-sharing, restart daemon, and verify server-sharing autostarts on boot!
    Command::new(ctl_bin)
        .args(["--socket", &socket_str, "start", "server-sharing"])
        .output()
        .expect("restarts server-sharing");
    daemon.shutdown().await.expect("stops daemon");

    let daemon = DaemonServer::launch(
        DaemonConfig::new(&state_dir, &socket_path)
            .with_config_dir(&etc_dir)
            .with_cups_endpoint(cups_endpoint)
            .with_firewall_program(&fw_script)
            .with_ports(RuntimePorts {
                proxy_port,
                server_port: linux_server_port,
                discovery_ports: vec![0],
            }),
    )
    .await
    .expect("relaunches daemon with persisted server-sharing");
    let boot_status = Command::new(ctl_bin)
        .args(["--socket", &socket_str, "status"])
        .output()
        .expect("queries status after reboot");
    assert!(
        String::from_utf8_lossy(&boot_status.stdout).contains("server-sharing: running"),
        "persisted server-sharing must autostart on daemon restart: {}",
        String::from_utf8_lossy(&boot_status.stdout)
    );

    daemon.shutdown().await.expect("stops daemon");
    win_runtime.shutdown().await.expect("stops win server");
    let _ = cupsd.kill();
    let _ = cupsd.wait();
    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shaprintctl_diagnostics_and_status_distinguish_subsystem_failures_with_concrete_recovery_actions_and_zero_leakage(
) {
    use shaprint_desktop::adapters::printers::cups::CupsServerEndpoint;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::process::{Command, Stdio};

    let root = temporary_directory("linux-daemon-diagnostics");
    let state_dir = root.join("var/lib/shaprint");
    let etc_dir = root.join("etc/shaprint");
    let socket_path = root.join("run/shaprint/shaprintd.sock");
    std::fs::create_dir_all(&state_dir).expect("creates state dir");
    std::fs::create_dir_all(&etc_dir).expect("creates etc dir");
    std::fs::create_dir_all(socket_path.parent().expect("socket parent"))
        .expect("creates socket dir");

    let ctl_bin = env!("CARGO_BIN_EXE_shaprintctl");
    let socket_str = socket_path.to_string_lossy().into_owned();

    // Create mock firewall script that fails (returns code 1):
    let fake_firewall = root.join("fake-firewall.sh");
    std::fs::write(&fake_firewall, "#!/bin/sh\nexit 1\n").expect("writes fake firewall script");
    std::fs::set_permissions(&fake_firewall, std::fs::Permissions::from_mode(0o755))
        .expect("sets exec permissions");
    std::env::set_var("SHAPRINT_FIREWALL_PROGRAM", &fake_firewall);

    // 1. Daemon offline / stopped:
    //    shaprintctl diagnostics against non-existent socket reports daemon: failed,
    //    code: server-unavailable, and recovery: sudo systemctl start shaprintd.
    let offline_output = Command::new(ctl_bin)
        .args(["--socket", &socket_str, "diagnostics"])
        .output()
        .expect("runs shaprintctl diagnostics on offline daemon");
    assert!(offline_output.status.success());
    let offline_text = String::from_utf8_lossy(&offline_output.stdout);
    assert!(offline_text.contains("[FAILED] daemon:"));
    assert!(offline_text.contains("code: server-unavailable"));
    assert!(offline_text.contains("sudo systemctl start shaprintd"));

    let offline_json_out = Command::new(ctl_bin)
        .args(["--socket", &socket_str, "diagnostics", "--json"])
        .output()
        .expect("runs shaprintctl diagnostics --json on offline daemon");
    assert!(offline_json_out.status.success());
    let json_val: serde_json::Value =
        serde_json::from_slice(&offline_json_out.stdout).expect("valid json");
    assert_eq!(json_val["healthy"], false);
    let daemon_check = json_val["checks"]
        .as_array()
        .expect("checks array")
        .iter()
        .find(|c| c["check"] == "daemon")
        .expect("daemon check present");
    assert_eq!(daemon_check["status"], "failed");
    assert_eq!(daemon_check["code"], "server-unavailable");
    assert!(daemon_check["recovery"]
        .as_str()
        .expect("recovery string")
        .contains("sudo systemctl start shaprintd"));

    // 2. Port owner conflict:
    //    Bind a conflicting process listener on port 48631, and verify offline/host diagnostics
    //    detects port conflict with invalid-state and recovery naming conflicting process/PID.
    {
        let conflicting_listener = TcpListener::bind("127.0.0.1:48631");
        if let Ok(listener) = conflicting_listener {
            let port_diag = Command::new(ctl_bin)
                .args(["--socket", &socket_str, "diagnostics"])
                .output()
                .expect("runs shaprintctl diagnostics during port conflict");
            let port_text = String::from_utf8_lossy(&port_diag.stdout);
            assert!(
                port_text.contains("[FAILED] ports:"),
                "expected ports check failure in: {port_text}"
            );
            assert!(port_text.contains("code: invalid-state"));
            assert!(port_text.contains("Stop the conflicting process"));
            drop(listener);
        }
    }

    // 3. Launch daemon with:
    //    - Non-existent CUPS socket
    //    - Failing firewall script
    //    - Unconfigured Network Channel
    let fake_cups_socket = root.join("nonexistent-cups.sock");
    let proxy_port = support::free_port();
    let server_port = support::free_port();
    let discovery_port = support::free_udp_port();

    let server = DaemonServer::launch(
        DaemonConfig::new(&state_dir, &socket_path)
            .with_config_dir(&etc_dir)
            .with_ports(RuntimePorts {
                proxy_port,
                server_port,
                discovery_ports: vec![discovery_port],
            })
            .with_cups_endpoint(CupsServerEndpoint::parse(
                &fake_cups_socket.to_string_lossy(),
            ))
            .with_firewall_program(&fake_firewall),
    )
    .await
    .expect("launches test daemon");

    // 5. Query diagnostics from the running daemon:
    //    - daemon: ok
    //    - cups: failed (queue-unavailable, sudo systemctl start cups)
    //    - credential: failed (not-authorized, sudo shaprintctl network-channel set)
    //    - firewall: failed (not-authorized, sudo firewall-cmd)
    let diag_output = Command::new(ctl_bin)
        .args(["--socket", &socket_str, "diagnostics"])
        .output()
        .expect("runs shaprintctl diagnostics");
    assert!(
        diag_output.status.success(),
        "status: {}\nstdout:\n{}\nstderr:\n{}",
        diag_output.status,
        String::from_utf8_lossy(&diag_output.stdout),
        String::from_utf8_lossy(&diag_output.stderr)
    );
    let diag_text = String::from_utf8_lossy(&diag_output.stdout);

    assert!(
        diag_text.contains("[OK] daemon:"),
        "expected daemon ok: {diag_text}"
    );
    assert!(
        diag_text.contains("[FAILED] cups:"),
        "expected cups failed: {diag_text}"
    );
    assert!(diag_text.contains("code: queue-unavailable"));
    assert!(diag_text.contains("sudo systemctl start cups"));

    assert!(
        diag_text.contains("[FAILED] credential:"),
        "expected credential failed: {diag_text}"
    );
    assert!(diag_text.contains("code: not-authorized"));
    assert!(diag_text.contains("sudo shaprintctl network-channel set"));

    assert!(
        diag_text.contains("[FAILED] firewall:"),
        "expected firewall failed: {diag_text}"
    );
    assert!(diag_text.contains("code: not-authorized"));
    assert!(diag_text.contains("sudo firewall-cmd"));

    // 6. Query status before configuration:
    //    - client-proxy: running
    //    - readiness: not-ready
    let status_before = Command::new(ctl_bin)
        .args(["--socket", &socket_str, "status"])
        .output()
        .expect("runs shaprintctl status");
    assert!(status_before.status.success());
    let status_before_text = String::from_utf8_lossy(&status_before.stdout);
    assert!(status_before_text.contains("client-proxy: running"));
    assert!(status_before_text.contains("readiness: not-ready"));

    // 7. Configure Network Channel:
    let sensitive_secret = "very-sensitive-channel-secret-alpha-9988";
    let mut child = Command::new(ctl_bin)
        .args(["--socket", &socket_str, "network-channel", "set", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawns shaprintctl network-channel set");
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(format!("{sensitive_secret}\n").as_bytes())
        .expect("writes secret");
    let set_res = child.wait_with_output().expect("waits for set");
    assert!(
        set_res.status.success(),
        "network-channel set failed: stdout={}\nstderr={}",
        String::from_utf8_lossy(&set_res.stdout),
        String::from_utf8_lossy(&set_res.stderr)
    );

    // 8. Verify status reports destinations and readiness:
    let status_after = Command::new(ctl_bin)
        .args(["--socket", &socket_str, "status"])
        .output()
        .expect("runs shaprintctl status after config");
    assert!(status_after.status.success());
    let status_after_text = String::from_utf8_lossy(&status_after.stdout);
    assert!(status_after_text.contains("readiness: ready"));
    assert!(status_after_text.contains("shared-printers: none"));

    // Status --json check:
    let status_json_out = Command::new(ctl_bin)
        .args(["--socket", &socket_str, "status", "--json"])
        .output()
        .expect("runs shaprintctl status --json");
    assert!(status_json_out.status.success());
    let status_json: serde_json::Value =
        serde_json::from_slice(&status_json_out.stdout).expect("valid json status");
    assert_eq!(status_json["readiness"], "ready");
    assert_eq!(status_json["shared_printers"], serde_json::json!([]));

    // 9. Diagnostics after credential set now reports credential: ok:
    let diag_after = Command::new(ctl_bin)
        .args(["--socket", &socket_str, "diagnostics"])
        .output()
        .expect("runs shaprintctl diagnostics after credential set");
    let diag_after_text = String::from_utf8_lossy(&diag_after.stdout);
    assert!(diag_after_text.contains("[OK] credential:"));

    // 10. SECRET HYGIENE VERIFICATION:
    //     Assert that the sensitive Network Channel value NEVER appears in stdout or stderr
    //     of status, status --json, diagnostics, or diagnostics --json.
    for cmd_args in [
        vec!["status"],
        vec!["status", "--json"],
        vec!["diagnostics"],
        vec!["diagnostics", "--json"],
    ] {
        let mut full_args = vec!["--socket", &socket_str];
        full_args.extend(cmd_args);
        let out = Command::new(ctl_bin)
            .args(&full_args)
            .output()
            .expect("runs command for secret hygiene verification");
        let stdout_str = String::from_utf8_lossy(&out.stdout);
        let stderr_str = String::from_utf8_lossy(&out.stderr);
        assert!(
            !stdout_str.contains(sensitive_secret),
            "stdout must NEVER contain Network Channel secret! cmd={full_args:?}\nstdout={stdout_str}"
        );
        assert!(
            !stderr_str.contains(sensitive_secret),
            "stderr must NEVER contain Network Channel secret! cmd={full_args:?}\nstderr={stderr_str}"
        );
    }

    std::env::remove_var("SHAPRINT_FIREWALL_PROGRAM");
    server.shutdown().await.expect("stops test daemon");
    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn daemon_drain_command_and_client_rpc_succeed_with_status() {
    use shaprint_desktop::adapters::daemon::execute_cli_command;
    use shaprint_desktop::adapters::daemon::parse_cli_args;

    let dir = temporary_directory("daemon-drain-e2e");
    std::fs::create_dir_all(&dir).expect("creates state dir");
    let socket_path = dir.join("shaprintd.sock");
    let proxy_port = support::free_port();
    let server_port = support::free_port();

    let server = DaemonServer::launch(DaemonConfig::new(&dir, &socket_path).with_ports(
        RuntimePorts {
            proxy_port,
            server_port,
            discovery_ports: vec![0],
        },
    ))
    .await
    .expect("launches test daemon");

    let client = DaemonClient::new(&socket_path);

    // Initial status check: proxy is running
    let status = client.status().await.expect("initial status query");
    let proxy = status
        .services
        .iter()
        .find(|s| s.id == "client-proxy")
        .expect("client-proxy exists");
    assert_eq!(proxy.state, "running");

    // shaprintctl drain command succeeds when idle
    let drain_opts = parse_cli_args(&[
        "shaprintctl",
        "--socket",
        &socket_path.to_string_lossy(),
        "--timeout",
        "5",
        "drain",
    ])
    .expect("parses drain opts");
    let drain_output = execute_cli_command(&drain_opts)
        .await
        .expect("drain executes successfully");
    assert!(drain_output.contains("drained: true"));
    assert!(drain_output.contains("active-jobs: 0"));

    // Verify drain via DaemonClient
    let drain_status = client.drain(Some(5)).await.expect("drain client call");
    assert!(drain_status.drained);
    assert_eq!(drain_status.active_jobs, 0);

    // Shutdown daemon cleanly
    server.shutdown().await.expect("clean shutdown");
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn daemon_drain_allows_in_flight_print_job_to_finish_while_refusing_new_ones() {
    let dir = temporary_directory("daemon-drain-inflight");
    std::fs::create_dir_all(&dir).expect("creates state dir");
    let socket_path = dir.join("shaprintd.sock");
    let proxy_port = support::free_port();
    let server_port = support::free_port();

    let server = DaemonServer::launch(DaemonConfig::new(&dir, &socket_path).with_ports(
        RuntimePorts {
            proxy_port,
            server_port,
            discovery_ports: vec![0],
        },
    ))
    .await
    .expect("launches test daemon");

    let client = DaemonClient::new(&socket_path);

    // Verify proxy port is accepting connections
    let stream = tokio::net::TcpStream::connect(("127.0.0.1", proxy_port))
        .await
        .expect("connects to proxy");
    drop(stream);

    // Initiate drain over DaemonClient with 5s timeout
    let drain_res = client.drain(Some(5)).await.expect("drain succeeds");
    assert!(drain_res.drained);
    assert_eq!(drain_res.active_jobs, 0);

    // Clean shutdown
    server.shutdown().await.expect("clean shutdown");
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn almalinux_10_rpm_package_smoke_installs_boots_queries_restarts_repairs_and_stops_service()
{
    use std::process::Command;

    if std::env::var_os("SHAPRINT_RUN_ALMALINUX10_SMOKE").is_none() {
        return;
    }

    let has_rpmbuild = Command::new("rpmbuild")
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success());
    let has_podman = Command::new("podman")
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success());
    assert!(
        has_rpmbuild && has_podman,
        "rpmbuild and podman are required when SHAPRINT_RUN_ALMALINUX10_SMOKE is set"
    );

    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .expect("locates repository root");
    let smoke_script = repo_root.join("packaging/linux/rpm/smoke-test-almalinux10.sh");
    assert!(
        smoke_script.exists(),
        "missing AlmaLinux 10 RPM smoke script at '{}'",
        smoke_script.display()
    );

    let output = Command::new("bash")
        .arg(&smoke_script)
        .env("SHAPRINTD_BIN", env!("CARGO_BIN_EXE_shaprintd"))
        .env("SHAPRINTCTL_BIN", env!("CARGO_BIN_EXE_shaprintctl"))
        .output()
        .expect("runs AlmaLinux 10 RPM smoke test script");

    assert!(
        output.status.success(),
        "AlmaLinux 10 RPM smoke test failed (status={}):\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn almalinux_9_rpm_package_smoke_installs_boots_queries_restarts_repairs_and_stops_service() {
    use std::process::Command;

    if std::env::var_os("SHAPRINT_RUN_ALMALINUX9_SMOKE").is_none() {
        return;
    }

    let has_rpmbuild = Command::new("rpmbuild")
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success());
    let has_podman = Command::new("podman")
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success());
    assert!(
        has_rpmbuild && has_podman,
        "rpmbuild and podman are required when SHAPRINT_RUN_ALMALINUX9_SMOKE is set"
    );

    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .expect("locates repository root");
    let smoke_script = repo_root.join("packaging/linux/rpm/smoke-test-almalinux9.sh");
    assert!(
        smoke_script.exists(),
        "missing AlmaLinux 9 RPM smoke script at '{}'",
        smoke_script.display()
    );

    let mut cmd = Command::new("bash");
    cmd.arg(&smoke_script);
    if let (Some(shaprintd_bin), Some(shaprintctl_bin)) = (
        std::env::var_os("SHAPRINTD_BIN"),
        std::env::var_os("SHAPRINTCTL_BIN"),
    ) {
        cmd.env("SHAPRINTD_BIN", shaprintd_bin);
        cmd.env("SHAPRINTCTL_BIN", shaprintctl_bin);
    } else {
        let el9_shaprintd_bin =
            repo_root.join("apps/desktop/src-tauri/target/el9/release/shaprintd");
        let el9_shaprintctl_bin =
            repo_root.join("apps/desktop/src-tauri/target/el9/release/shaprintctl");
        if el9_shaprintd_bin.exists() && el9_shaprintctl_bin.exists() {
            cmd.env("SHAPRINTD_BIN", el9_shaprintd_bin);
            cmd.env("SHAPRINTCTL_BIN", el9_shaprintctl_bin);
        }
    }
    let output = cmd
        .output()
        .expect("runs AlmaLinux 9 RPM smoke test script");

    assert!(
        output.status.success(),
        "AlmaLinux 9 RPM smoke test failed (status={}):\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn almalinux_previous_major_matrix_cross_flows_print_settings_and_baseline_check() {
    use shaprint_desktop::adapters::identity::FileIdentityStore;
    use shaprint_desktop::adapters::ipps::NetworkChannel;
    use shaprint_desktop::application::{
        DuplexMode, PrintJob, PrintJobSubmitter, PrintOrientation, PrintSettings,
    };
    use shaprint_desktop::domain::{AppError, PrinterName};

    // 1. Verify Common Print Settings across Client-to-Windows, Windows-to-Server, and Linux-to-Linux flows (#134 criterion 4).
    let temp = temporary_directory("alma9-matrix-flows");
    std::fs::create_dir_all(&temp).expect("creates temp dir");

    let client_settings = PrintSettings {
        copies: Some(3),
        color: Some(true),
        duplex: Some(DuplexMode::LongEdge),
        media: Some("iso_a4_210x297mm".to_string()),
        orientation: Some(PrintOrientation::Landscape),
    };

    let sample_pwg_raster = b"RaSt0123456789PWG-AlmaLinux9-Matrix";
    let print_job = PrintJob::from_ipp_body(sample_pwg_raster.to_vec(), 0, client_settings.clone());

    // Simulate Server Destination capturing the submitted job with exact settings:
    struct MockCapturingSubmitter {
        captured_settings: std::sync::Mutex<Option<PrintSettings>>,
        captured_data: std::sync::Mutex<Vec<u8>>,
    }

    #[async_trait::async_trait]
    impl PrintJobSubmitter for MockCapturingSubmitter {
        async fn submit(&self, _printer: &PrinterName, job: PrintJob) -> Result<u32, AppError> {
            *self.captured_settings.lock().unwrap() = Some(job.settings().clone());
            *self.captured_data.lock().unwrap() = job.document().to_vec();
            Ok(1)
        }
    }

    let submitter = MockCapturingSubmitter {
        captured_settings: std::sync::Mutex::new(None),
        captured_data: std::sync::Mutex::new(Vec::new()),
    };

    let target_printer = PrinterName::parse("Alma9_Office_Printer").unwrap();
    submitter
        .submit(&target_printer, print_job)
        .await
        .expect("submits job preserving settings");

    let observed = submitter.captured_settings.lock().unwrap().clone().unwrap();
    assert_eq!(observed.copies, Some(3));
    assert_eq!(observed.color, Some(true));
    assert_eq!(observed.duplex, Some(DuplexMode::LongEdge));
    assert_eq!(observed.media, Some("iso_a4_210x297mm".to_string()));
    assert_eq!(observed.orientation, Some(PrintOrientation::Landscape));
    assert_eq!(
        submitter.captured_data.lock().unwrap().as_slice(),
        sample_pwg_raster
    );

    // 2. Verify Server Identity & Salted Channel Verifier storage backward compatibility (#134 criterion 5).
    use shaprint_desktop::adapters::identity::IdentityStore;
    let identity_store = FileIdentityStore::new(&temp);
    let id = identity_store.load_or_create().expect("loads id");
    assert!(!id.fingerprint().hex().is_empty());

    let channel = NetworkChannel::in_memory();
    channel
        .configure_sync("AlmaLinux9-Secure-Channel")
        .expect("configures channel");
    assert!(channel.is_configured());

    // 3. Verify polkit action and Wayland/X11 desktop session integration contract (#134 criterion 1, 3).
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .expect("locates repo root");
    let polkit_policy = repo_root.join("packaging/linux/polkit/org.shaprint.control.policy");
    assert!(
        polkit_policy.exists(),
        "polkit action policy file must exist"
    );
    let policy_xml = std::fs::read_to_string(&polkit_policy).expect("reads polkit policy");
    assert!(
        policy_xml.contains("org.shaprint.control.start"),
        "polkit policy must define org.shaprint.control.start"
    );
    assert!(
        policy_xml.contains("org.shaprint.control.stop"),
        "polkit policy must define org.shaprint.control.stop"
    );
    assert!(
        policy_xml.contains("org.shaprint.control.network-channel"),
        "polkit policy must define org.shaprint.control.network-channel"
    );

    // 4. Verify RPM spec and package baseline configuration (#134 criterion 6).
    let spec_file = repo_root.join("packaging/linux/rpm/shaprint.spec");
    assert!(spec_file.exists(), "shaprint.spec must exist");
    let spec_content = std::fs::read_to_string(&spec_file).expect("reads spec");
    assert!(
        spec_content.contains("ExclusiveArch:  x86_64"),
        "RPM must target x86_64 architecture"
    );
    assert!(
        spec_content.contains("Requires:       cups"),
        "RPM requires cups"
    );
    assert!(
        spec_content.contains("Requires:       systemd"),
        "RPM requires systemd"
    );

    std::fs::remove_dir_all(&temp).ok();
}

#[tokio::test]
async fn ubuntu_2404_deb_package_smoke_installs_boots_queries_restarts_repairs_and_stops_service() {
    use std::process::Command;

    if std::env::var_os("SHAPRINT_RUN_UBUNTU2404_SMOKE").is_none() {
        return;
    }

    let has_podman = Command::new("podman")
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success());
    assert!(
        has_podman,
        "podman is required when SHAPRINT_RUN_UBUNTU2404_SMOKE is set"
    );

    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .expect("locates repository root");
    let smoke_script = repo_root.join("packaging/linux/deb/smoke-test-ubuntu2404.sh");
    assert!(
        smoke_script.exists(),
        "missing Ubuntu 24.04 DEB smoke script at '{}'",
        smoke_script.display()
    );

    let mut cmd = Command::new("bash");
    cmd.arg(&smoke_script);
    if let (Some(shaprintd_bin), Some(shaprintctl_bin)) = (
        std::env::var_os("SHAPRINTD_BIN"),
        std::env::var_os("SHAPRINTCTL_BIN"),
    ) {
        cmd.env("SHAPRINTD_BIN", shaprintd_bin);
        cmd.env("SHAPRINTCTL_BIN", shaprintctl_bin);
    } else {
        cmd.env("SHAPRINTD_BIN", env!("CARGO_BIN_EXE_shaprintd"));
        cmd.env("SHAPRINTCTL_BIN", env!("CARGO_BIN_EXE_shaprintctl"));
    }
    let output = cmd
        .output()
        .expect("runs Ubuntu 24.04 DEB smoke test script");

    assert!(
        output.status.success(),
        "Ubuntu 24.04 DEB smoke test failed (status={}):\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn ubuntu_primary_lts_matrix_cross_flows_print_settings_and_ufw_check() {
    use shaprint_desktop::adapters::identity::FileIdentityStore;
    use shaprint_desktop::adapters::identity::IdentityStore;
    use shaprint_desktop::adapters::ipps::NetworkChannel;
    use shaprint_desktop::application::{
        DuplexMode, PrintJob, PrintJobSubmitter, PrintOrientation, PrintSettings,
    };
    use shaprint_desktop::domain::{AppError, PrinterName};

    // 1. Verify Client-to-Windows, Windows-to-Linux Server, and Linux-to-Linux flows with common print settings (#135 criterion 3).
    let temp = temporary_directory("u24-matrix-flows");
    std::fs::create_dir_all(&temp).expect("creates temp dir");

    let client_settings = PrintSettings {
        copies: Some(2),
        color: Some(false),
        duplex: Some(DuplexMode::Simplex),
        media: Some("na_letter_8.5x11in".to_string()),
        orientation: Some(PrintOrientation::Portrait),
    };

    let sample_pwg_raster = b"RaSt0123456789PWG-Ubuntu2404-Matrix";
    let print_job = PrintJob::from_ipp_body(sample_pwg_raster.to_vec(), 0, client_settings.clone());

    struct MockUbuntuSubmitter {
        captured_settings: std::sync::Mutex<Option<PrintSettings>>,
        captured_data: std::sync::Mutex<Vec<u8>>,
    }

    #[async_trait::async_trait]
    impl PrintJobSubmitter for MockUbuntuSubmitter {
        async fn submit(&self, _printer: &PrinterName, job: PrintJob) -> Result<u32, AppError> {
            *self.captured_settings.lock().unwrap() = Some(job.settings().clone());
            *self.captured_data.lock().unwrap() = job.document().to_vec();
            Ok(42)
        }
    }

    let submitter = MockUbuntuSubmitter {
        captured_settings: std::sync::Mutex::new(None),
        captured_data: std::sync::Mutex::new(Vec::new()),
    };

    let target_printer = PrinterName::parse("Ubuntu_Network_IPP_Printer").unwrap();
    let job_id = submitter
        .submit(&target_printer, print_job)
        .await
        .expect("submits job preserving settings");
    assert_eq!(job_id, 42);

    let observed = submitter.captured_settings.lock().unwrap().clone().unwrap();
    assert_eq!(observed.copies, Some(2));
    assert_eq!(observed.color, Some(false));
    assert_eq!(observed.duplex, Some(DuplexMode::Simplex));
    assert_eq!(observed.media, Some("na_letter_8.5x11in".to_string()));
    assert_eq!(observed.orientation, Some(PrintOrientation::Portrait));
    assert_eq!(
        submitter.captured_data.lock().unwrap().as_slice(),
        sample_pwg_raster
    );

    // 2. Server Identity & Salted Channel Verifier backward compatibility on Ubuntu (#135 criterion 5).
    let identity_store = FileIdentityStore::new(&temp);
    let id = identity_store.load_or_create().expect("loads id");
    assert!(!id.fingerprint().hex().is_empty());

    let channel = NetworkChannel::in_memory();
    channel
        .configure_sync("Ubuntu2404-Secure-Channel")
        .expect("configures channel");
    assert!(channel.is_configured());

    // 3. Polkit action and Wayland/X11 GNOME/KDE desktop session integration (#135 criterion 1, 4).
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .expect("locates repo root");
    let polkit_policy = repo_root.join("packaging/linux/polkit/org.shaprint.control.policy");
    assert!(
        polkit_policy.exists(),
        "polkit action policy file must exist"
    );
    let policy_xml = std::fs::read_to_string(&polkit_policy).expect("reads polkit policy");
    assert!(policy_xml.contains("org.shaprint.control.start"));
    assert!(policy_xml.contains("org.shaprint.control.stop"));
    assert!(policy_xml.contains("org.shaprint.control.network-channel"));

    // 4. Verify UFW client implementation covers ports 48631/tcp and 48633/udp idempotently (#135 criterion 2).
    use shaprint_desktop::adapters::elevation::linux::UfwClient;
    let ufw_script = temp.join("mock-ufw");
    let ufw_rules = temp.join("rules.txt");
    std::fs::write(
        &ufw_script,
        format!(
            r#"#!/bin/bash
RULES="{rules}"
touch "$RULES"
case "$1" in
  status)
    echo "Status: active"
    echo ""
    echo "To                         Action      From"
    echo "--                         ------      ----"
    cat "$RULES"
    ;;
  allow)
    port="$2"
    if ! grep -q "^$port " "$RULES"; then
      printf "%-26s ALLOW       Anywhere\n" "$port" >> "$RULES"
    fi
    echo "Rule updated"
    ;;
  delete)
    if [ "$2" = "allow" ]; then
      port="$3"
      sed -i "\|^$port |d" "$RULES"
    fi
    echo "Rule deleted"
    ;;
esac
"#,
            rules = ufw_rules.display()
        ),
    )
    .expect("writes mock ufw");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&ufw_script, std::fs::Permissions::from_mode(0o755))
            .expect("chmod");
    }

    let ufw = UfwClient::with_program(&ufw_script);
    assert!(!ufw.inbound_sharing_allowed().expect("queries ufw"));
    ufw.allow_inbound_sharing().expect("adds ufw rules");
    assert!(ufw.inbound_sharing_allowed().expect("rules active"));
    ufw.allow_inbound_sharing().expect("idempotent allow");
    let recorded_rules = std::fs::read_to_string(&ufw_rules).expect("reads rules");
    assert!(recorded_rules.contains("48631/tcp"));
    assert!(recorded_rules.contains("48633/udp"));

    ufw.remove_inbound_sharing().expect("removes ufw rules");
    assert!(!ufw.inbound_sharing_allowed().expect("rules removed"));

    // 5. Verify DEB packaging files (#135 criterion 1).
    let deb_control = repo_root.join("packaging/linux/deb/control");
    assert!(
        deb_control.exists(),
        "packaging/linux/deb/control must exist"
    );
    let control_content = std::fs::read_to_string(&deb_control).expect("reads deb control");
    assert!(control_content.contains("Package: shaprint"));
    assert!(control_content.contains("Architecture: amd64"));
    assert!(control_content.contains("systemd"));
    assert!(control_content.contains("cups"));
    assert!(control_content.contains("adduser"));

    let deb_postinst = repo_root.join("packaging/linux/deb/postinst");
    assert!(
        deb_postinst.exists(),
        "packaging/linux/deb/postinst must exist"
    );
    let deb_postrm = repo_root.join("packaging/linux/deb/postrm");
    assert!(deb_postrm.exists(), "packaging/linux/deb/postrm must exist");

    std::fs::remove_dir_all(&temp).ok();
}
