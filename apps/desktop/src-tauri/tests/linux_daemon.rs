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
