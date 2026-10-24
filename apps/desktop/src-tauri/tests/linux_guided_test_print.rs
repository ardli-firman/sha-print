//! Linux native test print integration tests.
//!
//! Submits a guided diagnostic page through an isolated CUPS daemon to the local ClientProxy
//! and TLS IPPS Server, verifying that the diagnostic page reaches the server submitter
//! without mutating saved queue defaults or escalation.
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
use shaprint_desktop::domain::{
    AppError, ClientQueueName, ClientQueueRequest, PrinterName, RecognisedClientQueue, ServiceId,
};
use shaprint_desktop::ipc::control::ControlPlane;
use shaprint_desktop::{RuntimePorts, Shell};
use support::IsolatedCupsd;

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

static CUPS_MUTEX: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct LpoptionsCleanup {
    queue: String,
    host_arg: String,
}

impl Drop for LpoptionsCleanup {
    fn drop(&mut self) {
        let mut cmd = Command::new("lpoptions");
        cmd.arg("-h").arg(&self.host_arg);
        cmd.arg("-x").arg(&self.queue);
        let _ = cmd.status();
    }
}

fn query_cups_options(
    endpoint: &shaprint_desktop::adapters::printers::cups::CupsServerEndpoint,
    queue: &str,
) -> Vec<(String, String)> {
    let mut cmd = Command::new("lpoptions");
    cmd.arg("-h").arg(endpoint.lpadmin_host_arg());
    cmd.arg("-p").arg(queue);
    let out = cmd.output().expect("runs lpoptions");
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn linux_native_client_queue_prints_guided_diagnostic_page_without_mutating_defaults() {
    if !IsolatedCupsd::is_supported() {
        eprintln!("Skipping test: isolated cupsd or lpadmin not available");
        return;
    }

    let _cups_guard = CUPS_MUTEX.lock().await;
    let cupsd = IsolatedCupsd::start();
    let root = support::temporary_directory("linux-guided-test");

    // 1. Setup Server with FakeCatalog and ReceivedJobs submitter
    let server_printer_name = "Office Laser";
    let sharing = Arc::new(Sharing::new(support::FakeCatalog::new(&[
        server_printer_name,
    ])));
    sharing
        .set_shared(support::printer_names(&[server_printer_name]))
        .await
        .unwrap();

    let mut secret = [0u8; 32];
    SystemRandom::new().fill(&mut secret).unwrap();
    let channel_hex = hex::encode(secret);

    let server_channel = Arc::new(NetworkChannel::in_memory());
    server_channel.configure(&channel_hex).await.unwrap();

    let received = Arc::new(ReceivedJobs::default());
    let endpoint = Arc::new(IppsServer::new(
        0,
        Arc::new(ServerIdentity::generate().unwrap()),
        server_channel.clone(),
        received.clone(),
        Arc::new(PrintFailures::new()),
        Arc::new(PrintJobTracker::new()),
    ));
    let server_coordinator = Arc::new(RuntimeCoordinator::new(vec![Arc::new(
        support::sharing_service_with_channel(sharing, endpoint.clone(), server_channel),
    )]));
    server_coordinator
        .start(ServiceId::ServerSharing)
        .await
        .unwrap();

    let server_address = format!("127.0.0.1:{}", endpoint.bound_address().unwrap().port());

    // 2. Setup Client Shell connected to the isolated cupsd
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("binds ephemeral proxy port");
    let proxy_port = listener.local_addr().expect("reads addr").port();
    drop(listener);

    let shell = Arc::new(
        Shell::with_linux_options(
            &root,
            RuntimePorts {
                proxy_port,
                server_port: 0,
                discovery_ports: vec![0],
            },
            None,
            Some(cupsd.endpoint.clone()),
            None,
        )
        .unwrap(),
    );

    shell
        .network_channel()
        .configure(&channel_hex)
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

    shell.runtime().start(ServiceId::ClientProxy).await.unwrap();

    // 3. Create ShaPrint client queue in the isolated CUPS instance
    let queue_req = ClientQueueRequest::new(
        &server_address,
        PrinterName::parse(server_printer_name).unwrap(),
    )
    .unwrap();
    let cups_queue_name = queue_req.cups_destination_name();
    let proxy_authority = format!("127.0.0.1:{proxy_port}");
    let proxy_uri = shaprint_desktop::adapters::client_queue_uri(
        &proxy_authority,
        &server_address,
        server_printer_name,
    )
    .expect("builds client queue uri");

    cupsd.add_printer(
        &cups_queue_name,
        &proxy_uri,
        queue_req.queue_name().as_str(),
        &[("media", "iso_a4_210x297mm")],
    );

    let _cleanup = LpoptionsCleanup {
        queue: cups_queue_name.clone(),
        host_arg: cupsd.endpoint.lpadmin_host_arg(),
    };

    let options_before = query_cups_options(&cupsd.endpoint, &cups_queue_name);

    // 4. Open Guided Test Print session
    let target = RecognisedClientQueue::new(
        ClientQueueName::parse(&cups_queue_name).unwrap(),
        server_address.clone(),
        PrinterName::parse(server_printer_name).unwrap(),
    );

    let control = ControlPlane::local(shell.clone());
    let snapshot = control
        .inspect_local_client_queue(target.clone().into())
        .await
        .expect("inspects queue and opens session");

    assert!(snapshot.session_id.is_some());
    let session_id = snapshot.session_id.unwrap();
    assert_eq!(
        snapshot.requested_media.as_deref(),
        Some("iso_a4_210x297mm")
    );
    assert!(snapshot.queue_empty);
    assert!(
        snapshot.test_print_enabled,
        "test print must be enabled when queue is ready"
    );
    assert_eq!(snapshot.submission.as_deref(), Some("not-sent"));
    assert_eq!(snapshot.physical_result.as_deref(), Some("unconfirmed"));

    // 5. Send guided test print
    let after_send = control
        .send_test_print(session_id)
        .await
        .expect("sends test print");
    assert_eq!(after_send.submission.as_deref(), Some("accepted"));
    assert!(
        !after_send.test_print_enabled,
        "test print must be disabled while unconfirmed"
    );

    // 6. Verify received job at the Server
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        {
            let jobs = received.0.lock().unwrap();
            if !jobs.is_empty() {
                assert_eq!(jobs.len(), 1);
                assert_eq!(jobs[0].0, server_printer_name);
                assert_eq!(jobs[0].1.media.as_deref(), Some("iso_a4_210x297mm"));
                assert_eq!(jobs[0].1.color, Some(false));
                assert_eq!(jobs[0].1.copies.unwrap_or(1), 1);
                assert_eq!(jobs[0].1.duplex, Some(DuplexMode::Simplex));
                assert_eq!(jobs[0].2, 1, "exactly 1 page");
                break;
            }
        }
        if std::time::Instant::now() > deadline {
            panic!("timed out waiting for server to receive test print job");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // 7. Verify queue options did not mutate
    let options_after = query_cups_options(&cupsd.endpoint, &cups_queue_name);
    assert_eq!(
        options_before, options_after,
        "saved queue defaults must not change after test print"
    );

    // 8. Confirm test print outcome
    let after_confirm = control
        .confirm_test_print(session_id, "correct", None)
        .await
        .expect("confirms physical outcome");
    assert_eq!(after_confirm.physical_result.as_deref(), Some("correct"));
    assert_eq!(after_confirm.user_symptom, None);

    // 9. Close test print session
    control
        .close_test_print_session(session_id)
        .await
        .expect("closes session");

    // 10. Shutdown
    shell.runtime().shutdown().await.unwrap();
    server_coordinator.shutdown().await.unwrap();
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn linux_guided_test_print_operates_over_daemon_socket() {
    if !IsolatedCupsd::is_supported() {
        eprintln!("Skipping test: isolated cupsd or lpadmin not available");
        return;
    }

    let _cups_guard = CUPS_MUTEX.lock().await;
    let cupsd = IsolatedCupsd::start();
    let root = support::temporary_directory("linux-daemon-guided-test");

    // 1. Setup Server
    let server_printer_name = "Office Socket";
    let sharing = Arc::new(Sharing::new(support::FakeCatalog::new(&[
        server_printer_name,
    ])));
    sharing
        .set_shared(support::printer_names(&[server_printer_name]))
        .await
        .unwrap();

    let mut secret = [0u8; 32];
    SystemRandom::new().fill(&mut secret).unwrap();
    let channel_hex = hex::encode(secret);

    let server_channel = Arc::new(NetworkChannel::in_memory());
    server_channel.configure(&channel_hex).await.unwrap();

    let received = Arc::new(ReceivedJobs::default());
    let endpoint = Arc::new(IppsServer::new(
        0,
        Arc::new(ServerIdentity::generate().unwrap()),
        server_channel.clone(),
        received.clone(),
        Arc::new(PrintFailures::new()),
        Arc::new(PrintJobTracker::new()),
    ));
    let server_coordinator = Arc::new(RuntimeCoordinator::new(vec![Arc::new(
        support::sharing_service_with_channel(sharing, endpoint.clone(), server_channel),
    )]));
    server_coordinator
        .start(ServiceId::ServerSharing)
        .await
        .unwrap();

    let server_address = format!("127.0.0.1:{}", endpoint.bound_address().unwrap().port());

    // 2. Launch DaemonServer with cups endpoint
    let state_dir = root.join("var/lib/shaprint");
    let socket = root.join("run/shaprint/shaprintd.sock");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("binds ephemeral proxy port");
    let proxy_port = listener.local_addr().expect("reads addr").port();
    drop(listener);

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
    std::fs::create_dir_all(&helper_dir).expect("creates helper dir");
    let dummy_pkexec = helper_dir.join("pkexec");
    std::fs::write(&dummy_pkexec, "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dummy_pkexec, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let shell = daemon.shell().expect("daemon launched with shell");
    shell
        .network_channel()
        .configure(&channel_hex)
        .await
        .unwrap();
    let inspection = shell
        .client_connections()
        .inspect(&server_address)
        .await
        .unwrap();
    shell
        .client_connections()
        .approve(&server_address, &inspection.current_fingerprint)
        .await
        .unwrap();

    let control = ControlPlane::machine_with(
        DaemonClient::new(&socket),
        ElevatedCtl::new(&dummy_pkexec, &dummy_pkexec, &socket),
    );

    // 3. Add CUPS queue
    let queue_req = ClientQueueRequest::new(
        &server_address,
        PrinterName::parse(server_printer_name).unwrap(),
    )
    .unwrap();
    let cups_queue_name = queue_req.cups_destination_name();
    let proxy_authority = format!("127.0.0.1:{proxy_port}");
    let proxy_uri = shaprint_desktop::adapters::client_queue_uri(
        &proxy_authority,
        &server_address,
        server_printer_name,
    )
    .expect("builds client queue uri");

    cupsd.add_printer(
        &cups_queue_name,
        &proxy_uri,
        queue_req.queue_name().as_str(),
        &[("media", "iso_a4_210x297mm")],
    );

    let _cleanup = LpoptionsCleanup {
        queue: cups_queue_name.clone(),
        host_arg: cupsd.endpoint.lpadmin_host_arg(),
    };

    let target = RecognisedClientQueue::new(
        ClientQueueName::parse(&cups_queue_name).unwrap(),
        server_address.clone(),
        PrinterName::parse(server_printer_name).unwrap(),
    );

    // 4. Open session over socket
    let snapshot = control
        .inspect_local_client_queue(target.into())
        .await
        .expect("inspects over socket");
    assert!(snapshot.session_id.is_some());
    let session_id = snapshot.session_id.unwrap();
    assert_eq!(
        snapshot.requested_media.as_deref(),
        Some("iso_a4_210x297mm")
    );
    assert!(snapshot.test_print_enabled);

    // 5. Refresh session over socket
    let refreshed = control
        .refresh_test_print_session(session_id)
        .await
        .expect("refreshes over socket");
    assert_eq!(refreshed.session_id, Some(session_id));
    assert!(refreshed.test_print_enabled);

    // 6. Send test print over socket
    let sent = control
        .send_test_print(session_id)
        .await
        .expect("sends test print over socket");
    assert_eq!(sent.submission.as_deref(), Some("accepted"));

    // 7. Wait for print job at server
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        {
            let jobs = received.0.lock().unwrap();
            if !jobs.is_empty() {
                assert_eq!(jobs[0].0, server_printer_name);
                assert_eq!(jobs[0].1.media.as_deref(), Some("iso_a4_210x297mm"));
                assert_eq!(jobs[0].2, 1);
                break;
            }
        }
        if std::time::Instant::now() > deadline {
            panic!("timed out waiting for server to receive test print job over socket");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // 8. Confirm an incorrect result with a classified symptom over the socket
    let incorrect = control
        .confirm_test_print(session_id, "incorrect", Some("clipping"))
        .await
        .expect("confirms outcome with symptom over socket");
    assert_eq!(incorrect.physical_result.as_deref(), Some("incorrect"));
    assert_eq!(incorrect.user_symptom.as_deref(), Some("clipping"));

    // A symptom is only valid alongside incorrect output.
    control
        .confirm_test_print(session_id, "correct", Some("clipping"))
        .await
        .unwrap_err();

    // 9. Explicit retry: fresh checks must re-enable sending before another page.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let refreshed = loop {
        let refreshed = control
            .refresh_test_print_session(session_id)
            .await
            .expect("refreshes over socket");
        if refreshed.test_print_enabled || std::time::Instant::now() > deadline {
            break refreshed;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    assert!(
        refreshed.test_print_enabled,
        "fresh checks must re-enable the retry: {refreshed:?}"
    );
    let retried = control
        .send_test_print(session_id)
        .await
        .expect("sends the explicit retry over socket");
    assert_eq!(retried.submission.as_deref(), Some("accepted"));
    assert_eq!(retried.physical_result.as_deref(), Some("unconfirmed"));
    assert_eq!(retried.user_symptom, None);
    assert_eq!(
        retried
            .attempted_actions
            .iter()
            .map(|action| (action.action.as_str(), action.outcome.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("send-test-print", "accepted"),
            ("retry-test-print", "accepted"),
        ]
    );

    // 10. The retried diagnostic page also arrives at the Server.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        {
            let jobs = received.0.lock().unwrap();
            if jobs.len() >= 2 {
                assert_eq!(jobs.len(), 2, "one page per explicit send");
                assert_eq!(jobs[1].0, server_printer_name);
                assert_eq!(jobs[1].2, 1);
                break;
            }
        }
        if std::time::Instant::now() > deadline {
            panic!("timed out waiting for the retried test print job");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // 11. Confirm and close over socket
    let confirmed = control
        .confirm_test_print(session_id, "correct", None)
        .await
        .expect("confirms outcome over socket");
    assert_eq!(confirmed.physical_result.as_deref(), Some("correct"));
    assert_eq!(confirmed.user_symptom, None);

    control
        .close_test_print_session(session_id)
        .await
        .expect("closes session over socket");

    daemon.shutdown().await.unwrap();
    server_coordinator.shutdown().await.unwrap();
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn linux_guided_test_print_disabled_or_rejected_on_stopped_or_mismatched_queue() {
    if !IsolatedCupsd::is_supported() {
        eprintln!("Skipping test: isolated cupsd or lpadmin not available");
        return;
    }

    let _cups_guard = CUPS_MUTEX.lock().await;
    let cupsd = IsolatedCupsd::start();
    let root = support::temporary_directory("linux-negative-guided-test");

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("binds ephemeral proxy port");
    let proxy_port = listener.local_addr().expect("reads addr").port();
    drop(listener);

    let shell = Arc::new(
        Shell::with_linux_options(
            &root,
            RuntimePorts {
                proxy_port,
                server_port: 0,
                discovery_ports: vec![0],
            },
            None,
            Some(cupsd.endpoint.clone()),
            None,
        )
        .unwrap(),
    );

    let server_printer_name = "Stopped Printer";
    let server_address = "127.0.0.1:48631";
    let queue_req = ClientQueueRequest::new(
        server_address,
        PrinterName::parse(server_printer_name).unwrap(),
    )
    .unwrap();
    let cups_queue_name = queue_req.cups_destination_name();
    let proxy_authority = format!("127.0.0.1:{proxy_port}");
    let proxy_uri = shaprint_desktop::adapters::client_queue_uri(
        &proxy_authority,
        server_address,
        server_printer_name,
    )
    .expect("builds client queue uri");

    cupsd.add_printer(
        &cups_queue_name,
        &proxy_uri,
        queue_req.queue_name().as_str(),
        &[("media", "iso_a4_210x297mm")],
    );

    let _cleanup = LpoptionsCleanup {
        queue: cups_queue_name.clone(),
        host_arg: cupsd.endpoint.lpadmin_host_arg(),
    };

    // Stop/disable the printer in CUPS
    let _ = Command::new("/usr/sbin/cupsdisable")
        .args(["-h", &cupsd.endpoint.lpadmin_host_arg(), &cups_queue_name])
        .status();

    let target = RecognisedClientQueue::new(
        ClientQueueName::parse(&cups_queue_name).unwrap(),
        server_address.to_owned(),
        PrinterName::parse(server_printer_name).unwrap(),
    );

    let control = ControlPlane::local(shell.clone());
    let snapshot = control
        .inspect_local_client_queue(target.into())
        .await
        .expect("inspects queue");

    // Because queue is stopped/disabled in CUPS, test print must NOT be enabled
    assert!(
        !snapshot.test_print_enabled,
        "test print must be disabled on a stopped queue"
    );

    if let Some(session_id) = snapshot.session_id {
        // Attempting to send on a disabled session returns InvalidState
        let err = control.send_test_print(session_id).await.unwrap_err();
        assert_eq!(
            err.code(),
            shaprint_desktop::domain::ErrorCode::InvalidState
        );
        control.close_test_print_session(session_id).await.unwrap();
    }

    shell.runtime().shutdown().await.unwrap();
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn linux_guided_test_print_disabled_on_unsupported_media() {
    if !IsolatedCupsd::is_supported() {
        eprintln!("Skipping test: isolated cupsd or lpadmin not available");
        return;
    }

    let _cups_guard = CUPS_MUTEX.lock().await;
    let cupsd = IsolatedCupsd::start();
    let root = support::temporary_directory("linux-unsupported-media-test");

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("binds ephemeral proxy port");
    let proxy_port = listener.local_addr().expect("reads addr").port();
    drop(listener);

    let shell = Arc::new(
        Shell::with_linux_options(
            &root,
            RuntimePorts {
                proxy_port,
                server_port: 0,
                discovery_ports: vec![0],
            },
            None,
            Some(cupsd.endpoint.clone()),
            None,
        )
        .unwrap(),
    );

    let server_printer_name = "Banner Printer";
    let server_address = "127.0.0.1:48631";
    let queue_req = ClientQueueRequest::new(
        server_address,
        PrinterName::parse(server_printer_name).unwrap(),
    )
    .unwrap();
    let cups_queue_name = queue_req.cups_destination_name();
    let proxy_authority = format!("127.0.0.1:{proxy_port}");
    let proxy_uri = shaprint_desktop::adapters::client_queue_uri(
        &proxy_authority,
        server_address,
        server_printer_name,
    )
    .expect("builds client queue uri");

    cupsd.add_printer(
        &cups_queue_name,
        &proxy_uri,
        queue_req.queue_name().as_str(),
        &[("media", "unsupported_banner_custom_roll")],
    );

    let _cleanup = LpoptionsCleanup {
        queue: cups_queue_name.clone(),
        host_arg: cupsd.endpoint.lpadmin_host_arg(),
    };

    let target = RecognisedClientQueue::new(
        ClientQueueName::parse(&cups_queue_name).unwrap(),
        server_address.to_owned(),
        PrinterName::parse(server_printer_name).unwrap(),
    );

    let control = ControlPlane::local(shell.clone());
    let snapshot = control
        .inspect_local_client_queue(target.into())
        .await
        .expect("inspects queue");

    // Unsupported media means requested_media is None and test print is disabled
    assert_eq!(snapshot.requested_media, None);
    assert!(
        !snapshot.test_print_enabled,
        "test print must be disabled on unsupported media without silent replacement"
    );

    if let Some(session_id) = snapshot.session_id {
        let err = control.send_test_print(session_id).await.unwrap_err();
        assert_eq!(
            err.code(),
            shaprint_desktop::domain::ErrorCode::InvalidState
        );
        control.close_test_print_session(session_id).await.unwrap();
    }

    shell.runtime().shutdown().await.unwrap();
    let _ = std::fs::remove_dir_all(&root);
}
