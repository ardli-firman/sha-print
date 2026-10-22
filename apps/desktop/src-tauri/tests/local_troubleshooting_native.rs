//! Opt-in runtime exercise through the desktop control boundary, real daemon, and native CUPS.
#![cfg(all(unix, feature = "desktop"))]

use shaprint_desktop::{
    adapters::{
        daemon::{DaemonConfig, DaemonServer},
        printers::cups::CupsServerEndpoint,
    },
    domain::{ClientQueueRequest, PrinterName, ServiceId},
    ipc::{control::ControlPlane, dto::RecognisedClientQueueDto},
    RuntimePorts,
};
use std::{
    io::Write,
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::Duration,
};

struct CupsFixture {
    root: PathBuf,
    socket: String,
    child: Child,
}

impl CupsFixture {
    fn start() -> Self {
        let root = std::env::temp_dir().join(format!(
            "shaprint-native-diagnostics-{}",
            std::process::id()
        ));
        for dir in [
            "conf",
            "spool",
            "spool/tmp",
            "cache",
            "state",
            "state/certs",
            "logs",
            "serverbin/backend",
        ] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        // A private non-setuid backend permits an unprivileged isolated scheduler to list it.
        use std::os::unix::fs::PermissionsExt;
        let backend = root.join("serverbin/backend/ipp");
        std::fs::copy("/usr/lib/cups/backend/ipp", &backend).unwrap();
        std::fs::set_permissions(&backend, std::fs::Permissions::from_mode(0o755)).unwrap();
        let socket = root.join("cups.sock").to_str().unwrap().to_owned();
        std::fs::write(root.join("cupsd.conf"), format!(
            "Listen {socket}\nBrowsing Off\nDefaultAuthType None\nWebInterface No\n<Location />\nOrder allow,deny\nAllow all\n</Location>\n<Policy default>\n<Limit All>\nOrder allow,deny\nAllow all\n</Limit>\n</Policy>\n"
        )).unwrap();
        std::fs::write(root.join("cups-files.conf"), format!(
            "ServerRoot {d}/conf\nServerBin {d}/serverbin\nStateDir {d}/state\nRequestRoot {d}/spool\nTempDir {d}/spool/tmp\nCacheDir {d}/cache\nAccessLog {d}/logs/access\nErrorLog {d}/logs/error\nPageLog {d}/logs/page\n",
            d = root.display(),
        )).unwrap();
        let child = Command::new("/usr/sbin/cupsd")
            .args(["-f", "-c"])
            .arg(root.join("cupsd.conf"))
            .arg("-s")
            .arg(root.join("cups-files.conf"))
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let fixture = Self {
            root,
            socket,
            child,
        };
        for _ in 0..50 {
            if std::os::unix::net::UnixStream::connect(&fixture.socket).is_ok() {
                return fixture;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("isolated CUPS did not start");
    }

    fn admin(&self, args: &[&str]) {
        let output = Command::new("/usr/sbin/lpadmin")
            .args(["-h", &self.socket])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

impl Drop for CupsFixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires cupsd, lpadmin, and lp; uses an isolated spooler and no physical printer"]
async fn real_native_queue_checks_preserve_pending_work_and_classify_missing_or_mismatched_destinations(
) {
    let cups = CupsFixture::start();
    let request =
        ClientQueueRequest::new("192.0.2.10:48631", PrinterName::parse("Office").unwrap()).unwrap();
    let queue_name = request.cups_destination_name();
    let uri = "ipp://127.0.0.1:48632/ipp/print/192.0.2.10%3A48631/Office";
    cups.admin(&["-p", &queue_name, "-E", "-v", uri, "-m", "raw"]);
    let socket = cups.root.join("daemon.sock");
    let daemon = DaemonServer::launch(
        DaemonConfig::new(cups.root.join("machine"), &socket)
            .with_config_dir(cups.root.join("config"))
            .with_cups_endpoint(CupsServerEndpoint::parse(&cups.socket))
            .with_ports(RuntimePorts {
                proxy_port: 0,
                server_port: 0,
                discovery_ports: vec![0],
            }),
    )
    .await
    .unwrap();
    let control = ControlPlane::machine(&socket);
    let target = RecognisedClientQueueDto {
        queue_name: queue_name.clone(),
        server_address: "192.0.2.10:48631".to_owned(),
        printer_name: "Office".to_owned(),
    };
    let healthy = control
        .inspect_local_client_queue(target.clone())
        .await
        .unwrap();
    assert!(healthy.queue_empty, "{:?}", healthy.checks);
    assert!(healthy.checks.iter().all(|check| check.status == "passed"));
    assert!(!healthy.test_print_enabled && healthy.repair_enabled);

    // Generate held work at runtime; no document content or stored fixture is committed.
    let mut job = Command::new("/usr/bin/lp")
        .args(["-h", &cups.socket, "-d", &queue_name, "-H", "hold"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    writeln!(
        job.stdin.take().unwrap(),
        "{:?}",
        std::time::SystemTime::now()
    )
    .unwrap();
    assert!(job.wait().unwrap().success());
    for _ in 0..2 {
        let pending = control
            .inspect_local_client_queue(target.clone())
            .await
            .unwrap();
        assert!(!pending.queue_empty);
        assert_eq!(
            pending
                .checks
                .iter()
                .find(|check| check.check == "pending-jobs")
                .unwrap()
                .status,
            "failed"
        );
    }
    // Fixture lifecycle only: passive GUI reads must never launch a system elevation helper.
    daemon
        .shell()
        .unwrap()
        .runtime()
        .stop(ServiceId::ClientProxy)
        .await
        .unwrap();
    let stopped = control
        .inspect_local_client_queue(target.clone())
        .await
        .unwrap();
    assert_eq!(
        stopped
            .checks
            .iter()
            .find(|check| check.check == "client-proxy")
            .unwrap()
            .status,
        "failed"
    );

    cups.admin(&[
        "-p",
        &queue_name,
        "-v",
        "ipp://127.0.0.1:48632/ipp/print/192.0.2.11%3A48631/Office",
    ]);
    let mismatched = control
        .inspect_local_client_queue(target.clone())
        .await
        .unwrap();
    assert_eq!(mismatched.checks[1].status, "failed");
    assert!(mismatched.checks[1].detail.contains("does not match"));
    assert_eq!(mismatched.checks[2].status, "not-applicable");
    let missing = control
        .inspect_local_client_queue(RecognisedClientQueueDto {
            queue_name: "missing_queue".to_owned(),
            ..target
        })
        .await
        .unwrap();
    assert_eq!(missing.checks[1].status, "failed");
    assert!(missing.checks[1].detail.contains("no longer installed"));
    daemon.shutdown().await.unwrap();
}
