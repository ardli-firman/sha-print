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
