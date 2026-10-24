//! Assembled guided-troubleshooting qualification for #179 under the #171 specification.
//!
//! Walks the complete user journey through the machine control boundary against a real
//! isolated `cupsd`, the daemon, the client proxy, and a TLS IPPS Server with a recording
//! fake physical-printer adapter: passive diagnosis, observed failures and recovery,
//! pending-job protection, native test print with physical confirmation and symptom,
//! explicit retry after fresh checks, observed authorization rejection, changed Server
//! identity with reapproval, closure with stale-callback suppression, and target
//! switching. Every public payload produced along the way is inspected for protected
//! data, and persisted state is compared before and after the journey.
//!
//! Windows-native and physical-paper criteria remain explicitly open; the verification
//! record is docs/test-print-troubleshooting-179-verification.md.
#![cfg(all(unix, feature = "desktop"))]

mod support;

use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use ring::rand::{SecureRandom, SystemRandom};
use shaprint_desktop::adapters::daemon::{
    DaemonClient, DaemonConfig, DaemonServer, PeerAccessPolicy,
};
use shaprint_desktop::adapters::elevated_ctl::ElevatedCtl;
use shaprint_desktop::adapters::ipps::NetworkChannel;
use shaprint_desktop::adapters::printers::raster::pwg_pages;
use shaprint_desktop::adapters::{IppsServer, ServerIdentity};
use shaprint_desktop::application::{
    DuplexMode, PrintFailures, PrintJob, PrintJobSubmitter, PrintJobTracker, PrintSettings,
    RuntimeCoordinator, Sharing,
};
use shaprint_desktop::domain::{AppError, ClientQueueRequest, PrinterName, ServiceId};
use shaprint_desktop::ipc::control::ControlPlane;
use shaprint_desktop::ipc::dto::{LocalTroubleshootingDto, RecognisedClientQueueDto};
use shaprint_desktop::RuntimePorts;
use support::IsolatedCupsd;

static CUPS_MUTEX: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Recording fake physical printer: shared-printer name, settings, and page count only.
#[derive(Default)]
struct ReceivedJobs(Mutex<Vec<(String, PrintSettings, usize)>>);

#[async_trait]
impl PrintJobSubmitter for ReceivedJobs {
    async fn submit(&self, printer: &PrinterName, job: PrintJob) -> Result<u32, AppError> {
        let pages = pwg_pages(job.document())?.collect::<Result<Vec<_>, _>>()?;
        self.0.lock().unwrap().push((
            printer.as_str().to_owned(),
            job.settings().clone(),
            pages.len(),
        ));
        Ok(1)
    }
}

fn check<'a>(snapshot: &'a LocalTroubleshootingDto, id: &str) -> &'a str {
    snapshot
        .checks
        .iter()
        .find(|row| row.check == id)
        .unwrap_or_else(|| panic!("check {id} exists"))
        .status
        .as_str()
}

fn failed_with(snapshot: &LocalTroubleshootingDto, id: &str, code: &str) -> bool {
    snapshot
        .checks
        .iter()
        .any(|row| row.check == id && row.status == "failed" && row.code.as_deref() == Some(code))
}

/// Rechecks through the same "Check again" path the UI uses, until the predicate holds.
async fn wait_until<P>(
    control: &ControlPlane,
    session: u64,
    description: &str,
    mut predicate: P,
) -> LocalTroubleshootingDto
where
    P: FnMut(&LocalTroubleshootingDto) -> bool,
{
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        let candidate = control
            .refresh_test_print_session(session)
            .await
            .expect("refreshes during polling");
        if predicate(&candidate) {
            return candidate;
        }
        if std::time::Instant::now() > deadline {
            panic!("timed out waiting for {description}: {candidate:?}");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn wait_for_job_count(received: &ReceivedJobs, count: usize) {
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    loop {
        if received.0.lock().unwrap().len() >= count {
            return;
        }
        if std::time::Instant::now() > deadline {
            panic!(
                "timed out waiting for {count} job(s) at the Server; got {}",
                received.0.lock().unwrap().len()
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Generates held native work at runtime; no document content is committed.
fn lp_submit_job(cups: &IsolatedCupsd, queue: &str, hold: bool) {
    let mut cmd = Command::new("/usr/bin/lp");
    cmd.arg("-h")
        .arg(cups.endpoint.lpadmin_host_arg())
        .arg("-d")
        .arg(queue);
    if hold {
        cmd.arg("-H").arg("hold");
    }
    let mut job = cmd
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawns lp");
    let payload = format!("{:?}", std::time::SystemTime::now());
    use std::io::Write;
    job.stdin
        .take()
        .unwrap()
        .write_all(payload.as_bytes())
        .unwrap();
    assert!(job.wait().unwrap().success(), "lp must accept the job");
}

/// Fixture-only cleanup of native work the qualification itself created.
fn clear_queue(cups: &IsolatedCupsd, queue: &str) {
    let host = cups.endpoint.lpadmin_host_arg();
    let _ = Command::new("/usr/bin/cancel")
        .arg("-h")
        .arg(&host)
        .arg("-a")
        .status();
    let _ = Command::new("/usr/sbin/cupsenable")
        .arg("-h")
        .arg(&host)
        .arg(queue)
        .status();
}

fn query_cups_options(cups: &IsolatedCupsd, queue: &str) -> Vec<(String, String)> {
    let out = Command::new("/usr/bin/lpoptions")
        .arg("-h")
        .arg(cups.endpoint.lpadmin_host_arg())
        .arg("-p")
        .arg(queue)
        .output()
        .expect("runs lpoptions");
    assert!(out.status.success(), "lpoptions must succeed");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut pairs = Vec::new();
    for part in stdout.split_whitespace() {
        if let Some((k, v)) = part.split_once('=') {
            if !k.starts_with("printer-state") {
                pairs.push((k.to_owned(), v.to_owned()));
            }
        }
    }
    pairs.sort();
    pairs
}

/// Recursive (path, bytes) snapshot of persisted state, proving the journey neither
/// mutates persisted settings nor adds troubleshooting history.
fn persisted_snapshot(root: &std::path::Path) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    fn walk(dir: &std::path::Path, out: &mut Vec<(std::path::PathBuf, Vec<u8>)>) {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if let Ok(bytes) = std::fs::read(&path) {
                out.push((path, bytes));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, &mut out);
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// The only persisted files the journey may intentionally change: the trust store is
/// rewritten by the user's reapproval, and the Network Channel verifier/credential by
/// the deliberate wrong-then-restored channel configuration. Nothing else may differ,
/// and no file may be added.
fn assert_persisted_unchanged(
    context: &str,
    before: &[(std::path::PathBuf, Vec<u8>)],
    after: &[(std::path::PathBuf, Vec<u8>)],
    allow_changes_to: &[&str],
) {
    let allowed = |path: &std::path::Path| {
        allow_changes_to
            .iter()
            .any(|name| path.file_name().and_then(|n| n.to_str()) == Some(name))
    };
    for (path, bytes) in before {
        match after.iter().find(|(p, _)| p == path) {
            Some((_, after_bytes)) => {
                assert!(
                    after_bytes == bytes || allowed(path),
                    "{context}: {} changed unexpectedly",
                    path.display()
                );
            }
            None => panic!("{context}: {} disappeared", path.display()),
        }
    }
    for (path, _) in after {
        assert!(
            before.iter().any(|(p, _)| p == path),
            "{context}: {} was added; troubleshooting must stay session-only",
            path.display()
        );
    }
}

/// In-process capture of daemon log records, standing in for the stderr logger that
/// `shaprintd` installs in production. The qualification asserts capture is live
/// (the daemon start event arrives) before trusting it as a protected-data boundary.
struct CaptureLogger(Arc<Mutex<Vec<String>>>);
impl log::Log for CaptureLogger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }
    fn log(&self, record: &log::Record) {
        self.0
            .lock()
            .unwrap()
            .push(format!("[{}] {}", record.target(), record.args()));
    }
    fn flush(&self) {}
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn linux_assembled_guided_troubleshooting_journey_qualifies_the_user_flow() {
    if !IsolatedCupsd::is_supported() {
        eprintln!("Skipping test: isolated cupsd or lpadmin not available");
        return;
    }

    let _cups_guard = CUPS_MUTEX.lock().await;
    let cupsd = IsolatedCupsd::start();
    let root = support::temporary_directory("guided-qualification");

    // Capture daemon log records before launch; a live capture is itself asserted later.
    let log_records = Arc::new(Mutex::new(Vec::<String>::new()));
    log::set_boxed_logger(Box::new(CaptureLogger(log_records.clone())))
        .expect("installs the qualification logger");
    log::set_max_level(log::LevelFilter::Info);

    // Server side: shared printer behind TLS IPPS with a recording fake physical printer.
    let server_printer = "Office Laser";
    let sharing = Arc::new(Sharing::new(support::FakeCatalog::new(&[server_printer])));
    sharing
        .set_shared(support::printer_names(&[server_printer]))
        .await
        .unwrap();
    let mut secret_bytes = [0u8; 32];
    SystemRandom::new().fill(&mut secret_bytes).unwrap();
    let channel_secret = hex::encode(secret_bytes);
    let server_channel = Arc::new(NetworkChannel::in_memory());
    server_channel.configure(&channel_secret).await.unwrap();
    let received = Arc::new(ReceivedJobs::default());
    let server_port = support::free_port();
    let endpoint = Arc::new(IppsServer::new(
        server_port,
        Arc::new(ServerIdentity::generate().unwrap()),
        server_channel.clone(),
        received.clone(),
        Arc::new(PrintFailures::new()),
        Arc::new(PrintJobTracker::new()),
    ));
    let server_address = format!("127.0.0.1:{server_port}");
    let server_coordinator = Arc::new(RuntimeCoordinator::new(vec![Arc::new(
        support::sharing_service_with_channel(
            sharing.clone(),
            endpoint.clone(),
            server_channel.clone(),
        ),
    )]));
    server_coordinator
        .start(ServiceId::ServerSharing)
        .await
        .unwrap();

    // Client side: real daemon + machine control boundary + isolated CUPS queues.
    let state_dir = root.join("var/lib/shaprint");
    let socket = root.join("run/shaprint/shaprintd.sock");
    let proxy_port = support::free_port();
    let daemon = DaemonServer::launch(
        DaemonConfig::new(&state_dir, &socket)
            .with_config_dir(root.join("etc/shaprint"))
            .with_access_policy(PeerAccessPolicy::RootOrDaemonOwner)
            .with_cups_endpoint(cupsd.endpoint.clone())
            .with_ports(RuntimePorts {
                proxy_port,
                server_port: 0,
                discovery_ports: vec![0],
            }),
    )
    .await
    .expect("launches daemon");

    let helper_dir = root.join("helper");
    std::fs::create_dir_all(&helper_dir).unwrap();
    let dummy_pkexec = helper_dir.join("pkexec");
    std::fs::write(&dummy_pkexec, "#!/bin/sh\nexit 0\n").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dummy_pkexec, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let shell = daemon.shell().expect("daemon launched with shell");
    shell
        .network_channel()
        .configure(&channel_secret)
        .await
        .unwrap();
    let review = shell
        .client_connections()
        .inspect(&server_address)
        .await
        .unwrap();
    shell
        .client_connections()
        .approve(&review.address, &review.current_fingerprint)
        .await
        .unwrap();
    // The daemon autostarts the client proxy (ADR 0001); no manual start is needed.

    let proxy_authority = format!("127.0.0.1:{proxy_port}");
    let office_req =
        ClientQueueRequest::new(&server_address, PrinterName::parse(server_printer).unwrap())
            .unwrap();
    let office_queue = office_req.cups_destination_name();
    let labels_req =
        ClientQueueRequest::new(&server_address, PrinterName::parse("Labels").unwrap()).unwrap();
    let labels_queue = labels_req.cups_destination_name();
    for (request, queue) in [
        (office_req.clone(), office_queue.clone()),
        (labels_req.clone(), labels_queue.clone()),
    ] {
        let printer = request.printer().as_str().to_owned();
        let uri = shaprint_desktop::adapters::client_queue_uri(
            &proxy_authority,
            &server_address,
            &printer,
        )
        .expect("builds client queue uri");
        cupsd.add_printer(
            &queue,
            &uri,
            request.queue_name().as_str(),
            &[("media", "iso_a4_210x297mm")],
        );
    }

    let control = ControlPlane::machine_with(
        DaemonClient::new(&socket),
        ElevatedCtl::new(&dummy_pkexec, &dummy_pkexec, &socket),
    );
    let office_target = RecognisedClientQueueDto {
        queue_name: office_queue.clone(),
        server_address: server_address.clone(),
        printer_name: server_printer.to_owned(),
    };
    let labels_target = RecognisedClientQueueDto {
        queue_name: labels_queue.clone(),
        server_address: server_address.clone(),
        printer_name: "Labels".to_owned(),
    };

    // Persisted state baseline over the stores the daemon actually uses: the Shell's
    // data dir (trust store, verifier, identity, sharing selections) and the separate
    // credential directory that holds the Network Channel client credential.
    let credential_dir = root.join("etc/shaprint-credentials");
    let state_baseline = || {
        let mut files = persisted_snapshot(&state_dir);
        files.extend(persisted_snapshot(&credential_dir));
        files
    };
    let persisted_before = state_baseline();
    let names: Vec<&str> = persisted_before
        .iter()
        .map(|(path, _)| path.file_name().and_then(|n| n.to_str()).unwrap_or(""))
        .collect();
    for expected in [
        "client-server-trust.json",
        "network-channel-verifier.json",
        "network-channel",
        "server-identity.cert.der",
    ] {
        assert!(
            names.contains(&expected),
            "baseline must contain the real persisted stores (missing {expected}): {names:?}"
        );
    }
    let lpoptions_before = query_cups_options(&cupsd, &office_queue);
    println!(
        "[qualification] version {} queues {:?} | {:?} route cups({}) -> ipp backend -> proxy(127.0.0.1:{proxy_port}) -> ipps tls -> {server_address}",
        env!("CARGO_PKG_VERSION"),
        office_target.queue_name,
        labels_target.queue_name,
        cupsd.endpoint.lpadmin_host_arg(),
    );

    // Public payloads observed across the whole journey, inspected for protected data.
    let mut public_payloads: Vec<String> = Vec::new();
    let mut record = |snapshot: &LocalTroubleshootingDto| {
        public_payloads.push(serde_json::to_string(snapshot).unwrap());
    };

    // 1. Passive diagnosis of a healthy queue through the machine boundary.
    let healthy = control
        .inspect_local_client_queue(office_target.clone())
        .await
        .expect("inspects healthy queue");
    record(&healthy);
    let session_id = healthy.session_id.expect("session opens");
    assert_eq!(check(&healthy, "spooler"), "passed");
    assert_eq!(check(&healthy, "client-queue"), "passed");
    assert_eq!(check(&healthy, "pending-jobs"), "passed");
    assert_eq!(check(&healthy, "client-proxy"), "passed");
    assert_eq!(check(&healthy, "server-reachability"), "passed");
    assert_eq!(check(&healthy, "server-identity"), "passed");
    assert_eq!(check(&healthy, "shared-printer"), "passed");
    // A locally configured but unobserved Network Channel is inconclusive, never "passed";
    // optional firewall inspection is not applicable without elevation, and the default
    // Requested media is a check of its own.
    assert_eq!(check(&healthy, "network-channel"), "inconclusive");
    assert_eq!(check(&healthy, "firewall"), "not-applicable");
    assert_eq!(check(&healthy, "requested-media"), "passed");
    assert_eq!(healthy.checks.len(), 10, "every check is classified");
    assert!(healthy.queue_empty && healthy.test_print_enabled);
    assert_eq!(healthy.requested_media.as_deref(), Some("iso_a4_210x297mm"));

    // 2. A stopped client proxy is observed passively; the real start path recovers it.
    // Service control reaches the daemon through the elevated helper in production; the
    // daemon-side runtime effect is exercised directly here, as in the native diagnostics test.
    shell.runtime().stop(ServiceId::ClientProxy).await.unwrap();
    let broken = wait_until(&control, session_id, "stopped proxy observed", |s| {
        s.checks
            .iter()
            .any(|row| row.check == "client-proxy" && row.status == "failed")
    })
    .await;
    record(&broken);
    assert!(
        !broken.test_print_enabled,
        "sending stays blocked while the proxy is down"
    );
    shell.runtime().start(ServiceId::ClientProxy).await.unwrap();
    let recovered = wait_until(&control, session_id, "proxy recovered", |s| {
        s.checks
            .iter()
            .any(|row| row.check == "client-proxy" && row.status == "passed")
            && s.test_print_enabled
    })
    .await;
    record(&recovered);

    // 3. Pending native work blocks testing and is never deleted by the flow.
    lp_submit_job(&cupsd, &office_queue, true);
    let pending = control
        .refresh_test_print_session(session_id)
        .await
        .unwrap();
    record(&pending);
    assert_eq!(check(&pending, "pending-jobs"), "failed");
    assert!(!pending.queue_empty && !pending.test_print_enabled && !pending.repair_enabled);
    assert!(
        control.send_test_print(session_id).await.is_err(),
        "a busy queue must refuse the explicit send"
    );
    clear_queue(&cupsd, &office_queue);
    let drained = wait_until(&control, session_id, "queue drained", |s| {
        s.queue_empty && s.test_print_enabled
    })
    .await;
    record(&drained);

    // 4. Native test print through the real CUPS queue, proxy, and TLS Server.
    let sent = control
        .send_test_print(session_id)
        .await
        .expect("sends test print");
    record(&sent);
    assert_eq!(sent.submission.as_deref(), Some("accepted"));
    wait_for_job_count(&received, 1);
    {
        let jobs = received.0.lock().unwrap();
        assert_eq!(jobs[0].0, server_printer);
        assert_eq!(jobs[0].1.media.as_deref(), Some("iso_a4_210x297mm"));
        assert_eq!(jobs[0].1.color, Some(false), "grayscale");
        assert_eq!(jobs[0].1.duplex, Some(DuplexMode::Simplex), "one-sided");
        assert_eq!(jobs[0].1.copies.unwrap_or(1), 1, "one copy");
        assert_eq!(jobs[0].2, 1, "exactly one page");
    }
    assert_eq!(
        query_cups_options(&cupsd, &office_queue),
        lpoptions_before,
        "saved queue defaults stay unchanged"
    );
    let incorrect = control
        .confirm_test_print(session_id, "incorrect", Some("clipping"))
        .await
        .expect("confirms incorrect output with symptom");
    record(&incorrect);
    assert_eq!(incorrect.physical_result.as_deref(), Some("incorrect"));
    assert_eq!(incorrect.user_symptom.as_deref(), Some("clipping"));

    // 5. Explicit retry only after fresh checks observe an empty queue again.
    let ready_again = wait_until(&control, session_id, "retry re-enabled", |s| {
        s.test_print_enabled
    })
    .await;
    assert_eq!(ready_again.session_id, Some(session_id));
    let retried = control
        .send_test_print(session_id)
        .await
        .expect("retries explicitly");
    record(&retried);
    assert_eq!(retried.submission.as_deref(), Some("accepted"));
    assert_eq!(
        retried.user_symptom, None,
        "a retry clears the earlier classification"
    );
    wait_for_job_count(&received, 2);
    let confirmed = control
        .confirm_test_print(session_id, "correct", None)
        .await
        .expect("confirms correct output");
    record(&confirmed);
    assert_eq!(
        confirmed
            .attempted_actions
            .iter()
            .map(|action| (action.action.as_str(), action.outcome.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("send-test-print", "accepted"),
            ("retry-test-print", "accepted")
        ]
    );
    // Frozen here, before the intentional channel and trust mutations below.
    let persisted_mid = state_baseline();

    // 6. An observed authorization rejection blocks sending until repaired and dismissed.
    let wrong_secret = {
        let mut bytes = [0u8; 32];
        SystemRandom::new().fill(&mut bytes).unwrap();
        hex::encode(bytes)
    };
    shell
        .network_channel()
        .configure(&wrong_secret)
        .await
        .unwrap();
    lp_submit_job(&cupsd, &office_queue, false);
    let rejected = wait_until(
        &control,
        session_id,
        "observed authorization rejection",
        |s| failed_with(s, "network-channel", "not-authorized"),
    )
    .await;
    record(&rejected);
    assert!(!rejected.test_print_enabled);
    assert!(control.send_test_print(session_id).await.is_err());
    // The user repairs the channel and dismisses the observed failure card.
    shell
        .network_channel()
        .configure(&channel_secret)
        .await
        .unwrap();
    shell.print_failures().clear();
    clear_queue(&cupsd, &office_queue);
    let authorized = wait_until(&control, session_id, "channel restored", |s| {
        s.test_print_enabled
    })
    .await;
    record(&authorized);

    // 7. A changed Server identity blocks sending until the user reapproves it.
    let new_identity = Arc::new(ServerIdentity::generate().unwrap());
    let new_fingerprint = new_identity.fingerprint().to_string();
    server_coordinator.shutdown().await.unwrap();
    let replacement = Arc::new(IppsServer::new(
        server_port,
        new_identity,
        server_channel.clone(),
        received.clone(),
        Arc::new(PrintFailures::new()),
        Arc::new(PrintJobTracker::new()),
    ));
    let replacement_coordinator = Arc::new(RuntimeCoordinator::new(vec![Arc::new(
        support::sharing_service_with_channel(
            sharing.clone(),
            replacement.clone(),
            server_channel.clone(),
        ),
    )]));
    replacement_coordinator
        .start(ServiceId::ServerSharing)
        .await
        .unwrap();
    let changed = wait_until(&control, session_id, "changed identity observed", |s| {
        failed_with(s, "server-identity", "server-identity-changed")
    })
    .await;
    record(&changed);
    assert!(!changed.test_print_enabled);
    assert!(control.send_test_print(session_id).await.is_err());
    shell
        .client_connections()
        .approve(&server_address, &new_fingerprint)
        .await
        .unwrap();
    let reapproved = wait_until(&control, session_id, "reapproved identity trusted", |s| {
        s.test_print_enabled
    })
    .await;
    record(&reapproved);

    // 8. Closure discards the session and refuses stale callbacks.
    control.close_test_print_session(session_id).await.unwrap();
    assert!(control
        .confirm_test_print(session_id, "correct", None)
        .await
        .is_err());
    assert!(control.send_test_print(session_id).await.is_err());
    assert_eq!(
        received.0.lock().unwrap().len(),
        2,
        "no page was sent by stale callbacks"
    );

    // 9. Switching targets supersedes the previous session with a fresh one.
    let labels_snapshot = control
        .inspect_local_client_queue(labels_target.clone())
        .await
        .expect("switches to the second queue");
    record(&labels_snapshot);
    let labels_session = labels_snapshot.session_id.expect("labels session opens");
    assert_ne!(labels_session, session_id);
    assert!(
        control
            .confirm_test_print(session_id, "correct", None)
            .await
            .is_err(),
        "the superseded session no longer accepts callbacks"
    );
    control
        .close_test_print_session(labels_session)
        .await
        .unwrap();

    // 10. Protected data never entered any public payload or daemon log of the journey.
    public_payloads.push(serde_json::to_string(&control.diagnostics().await.unwrap()).unwrap());
    public_payloads.push(serde_json::to_string(&control.status().await.unwrap()).unwrap());
    for payload in &public_payloads {
        assert!(
            !payload.contains(&channel_secret),
            "channel secret leaked: {payload}"
        );
        assert!(
            !payload.contains(&wrong_secret),
            "wrong channel secret leaked: {payload}"
        );
        assert!(!payload.contains("ipps://"), "IPP URL leaked: {payload}");
    }
    {
        let records = log_records.lock().unwrap();
        assert!(
            records
                .iter()
                .any(|line| line.contains("shaprintd started")),
            "the log capture must be live before it can be trusted as a boundary: {:?}",
            records
        );
        for line in records.iter() {
            assert!(
                !line.contains(&channel_secret),
                "secret reached log: {line}"
            );
            assert!(
                !line.contains(&wrong_secret),
                "wrong secret reached log: {line}"
            );
            assert!(!line.contains("ipps://"), "IPP URL reached log: {line}");
        }
    }

    // 11. The journey is session-only. Steps 1-5 (diagnosis, recovery, pending
    // protection, native send/confirm, explicit retry) must leave the persisted
    // stores byte-identical; steps 6-7 intentionally rewrote the trust store
    // (reapproval) and the channel verifier/credential (wrong-then-restored
    // configuration), so only those files may differ, and nothing may be added.
    assert_persisted_unchanged(
        "after diagnosis, recovery, test print, confirmation, and retry",
        &persisted_before,
        &persisted_mid,
        &[],
    );
    assert_persisted_unchanged(
        "after the full journey",
        &persisted_before,
        &state_baseline(),
        &[
            "client-server-trust.json",
            "network-channel-verifier.json",
            "network-channel",
        ],
    );

    daemon.shutdown().await.unwrap();
    replacement_coordinator.shutdown().await.unwrap();
    let _ = std::fs::remove_dir_all(&root);
}
