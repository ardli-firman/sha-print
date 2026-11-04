//! End-to-end lifecycle of the desktop shell's runtime, exercised through the public API with the
//! real client proxy and server sharing services.

mod support;

use std::{
    io,
    net::TcpListener,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

use shaprint_desktop::adapters::discovery::MdnsAdvertiser;
use shaprint_desktop::adapters::ipps::NetworkChannel;
use shaprint_desktop::adapters::port_binding::{
    PortBinder, PortOwner, PortOwnerInspector, PortTransport,
};
use shaprint_desktop::adapters::printers::dev_mode::{
    apply_settings_to_dev_mode, DMRES_DRAFT, DMRES_HIGH, DMRES_MEDIUM, DM_PRINTQUALITY,
};
use shaprint_desktop::adapters::printers::raster::decode_pwg;
use shaprint_desktop::adapters::{
    client_connections::ClientConnections, ClientProxyService, ServerSharingService,
};
use shaprint_desktop::application::{
    PrintFailures, PrintJob, PrintJobSubmitter, PrintQuality, RuntimeCoordinator,
    SharedPrinterSource, Sharing,
};
use shaprint_desktop::domain::{AppError, ErrorCode, PrinterName, ServiceId, ServiceState};
use shaprint_desktop::Shell;

#[cfg(unix)]
use shaprint_desktop::adapters::printers::cups::build_cups_job_options;

use support::{
    attribute_integers, attribute_resolutions, build_test_pwg_raster, get_printer_attributes,
    print_job_request, printer_names, sharing_runtime, sharing_runtime_with_submitter, status_code,
    temporary_directory, IppClient,
};

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
    let probe = match TcpListener::bind("127.0.0.1:48632") {
        Ok(listener) => listener,
        Err(_) => {
            eprintln!(
                "Skipping test: port 48632 is already bound by an external system daemon or concurrent process"
            );
            return;
        }
    };
    drop(probe);

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

    // Session 1: printer is selected and sharing is started on a port that conflicts, causing run to fail
    {
        // Bind a port first so IppsServer fails to listen
        let conflict_socket =
            std::net::TcpListener::bind("0.0.0.0:0").expect("binds conflict port");
        let port = conflict_socket.local_addr().expect("local addr").port();

        let (sharing, conflicting_endpoint) =
            support::sharing_runtime_persistent_on(&["HP LaserJet"], &dir, port);
        sharing
            .set_shared(printer_names(&["HP LaserJet"]))
            .await
            .expect("selects printer");

        let service = support::sharing_service(Arc::clone(&sharing), conflicting_endpoint);
        let runtime = RuntimeCoordinator::new(vec![Arc::new(service)]);

        let start_result = runtime.start(ServiceId::ServerSharing).await;
        assert!(
            start_result.is_err(),
            "start should fail when port is already bound"
        );
        assert_eq!(
            state(&runtime, ServiceId::ServerSharing),
            ServiceState::Failed,
            "service should enter Failed state"
        );

        // Explicit user stop from failed state
        runtime
            .stop(ServiceId::ServerSharing)
            .await
            .expect("stops sharing from failed state");
        assert_eq!(
            state(&runtime, ServiceId::ServerSharing),
            ServiceState::Stopped
        );

        drop(conflict_socket);
        runtime.shutdown().await.expect("clean shutdown");
    }

    // Session 2: restart must stay stopped even when the port is free now
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

struct HeldTcpOwner {
    owner: Mutex<Option<PortOwner>>,
    listener: Mutex<Option<TcpListener>>,
    terminations: AtomicUsize,
}

impl PortOwnerInspector for HeldTcpOwner {
    fn owners(&self, transport: PortTransport, _port: u16) -> io::Result<Vec<PortOwner>> {
        if transport != PortTransport::Tcp {
            return Ok(Vec::new());
        }
        Ok(self
            .owner
            .lock()
            .map_err(|_| io::Error::other("owner lock poisoned"))?
            .clone()
            .into_iter()
            .collect())
    }

    fn terminate(&self, _owner: &PortOwner) -> io::Result<()> {
        self.terminations.fetch_add(1, Ordering::SeqCst);
        self.owner
            .lock()
            .map_err(|_| io::Error::other("owner lock poisoned"))?
            .take();
        self.listener
            .lock()
            .map_err(|_| io::Error::other("listener lock poisoned"))?
            .take();
        Ok(())
    }
}

fn proxy_service(port: u16, binder: PortBinder) -> Arc<ClientProxyService> {
    let connections = Arc::new(
        ClientConnections::new(temporary_directory("runtime-port-owner-trust"))
            .expect("opens proxy trust store"),
    );
    Arc::new(ClientProxyService::with_port_binder(
        connections,
        Arc::new(NetworkChannel::in_memory()),
        Arc::new(PrintFailures::new()),
        Arc::new(shaprint_desktop::application::PrintJobTracker::new()),
        port,
        binder,
    ))
}

#[tokio::test]
async fn stale_shaprint_owner_is_reclaimed_and_proxy_reaches_running() {
    let held = TcpListener::bind("127.0.0.1:0").expect("holds a local port");
    let address = held.local_addr().expect("reads held port");
    let inspector = Arc::new(HeldTcpOwner {
        owner: Mutex::new(Some(PortOwner::new(4321, "shaprint-desktop"))),
        listener: Mutex::new(Some(held)),
        terminations: AtomicUsize::new(0),
    });
    let proxy = proxy_service(
        address.port(),
        PortBinder::with_inspector(inspector.clone()),
    );
    let runtime = RuntimeCoordinator::new(vec![proxy.clone()]);

    runtime
        .start(ServiceId::ClientProxy)
        .await
        .expect("reclaims stale ShaPrint and starts proxy");

    assert_eq!(
        state(&runtime, ServiceId::ClientProxy),
        ServiceState::Running
    );
    assert_eq!(inspector.terminations.load(Ordering::SeqCst), 1);
    let bound = proxy.bound_address().expect("proxy listener is recorded");
    assert_eq!(bound.port(), address.port());
    runtime
        .stop(ServiceId::ClientProxy)
        .await
        .expect("stops proxy");
    let rebound = TcpListener::bind(bound).expect("stop released the proxy port");
    drop(rebound);
    runtime.shutdown().await.expect("shutdown is idempotent");
}

#[tokio::test]
async fn non_shaprint_owner_is_reported_without_termination() {
    let held = TcpListener::bind("127.0.0.1:0").expect("holds a local port");
    let address = held.local_addr().expect("reads held port");
    let inspector = Arc::new(HeldTcpOwner {
        owner: Mutex::new(Some(PortOwner::new(9876, "unrelated-worker"))),
        listener: Mutex::new(Some(held)),
        terminations: AtomicUsize::new(0),
    });
    let proxy = proxy_service(
        address.port(),
        PortBinder::with_inspector(inspector.clone()),
    );
    let runtime = RuntimeCoordinator::new(vec![proxy]);

    let error = runtime
        .start(ServiceId::ClientProxy)
        .await
        .expect_err("must not take over an unrelated port");

    assert!(error.message().contains(&address.port().to_string()));
    assert!(error.message().contains("unrelated-worker"));
    assert!(error.message().contains("9876"));
    assert_eq!(inspector.terminations.load(Ordering::SeqCst), 0);
    assert_eq!(
        state(&runtime, ServiceId::ClientProxy),
        ServiceState::Failed
    );
    runtime
        .shutdown()
        .await
        .expect("shutdown after failed start");
}

#[tokio::test]
async fn stopping_server_sharing_releases_its_tcp_and_udp_listeners() {
    let (sharing, endpoint) = sharing_runtime(&["HP LaserJet"]);
    sharing
        .set_shared(printer_names(&["HP LaserJet"]))
        .await
        .expect("selects a printer");
    let channel = Arc::new(NetworkChannel::in_memory());
    channel
        .configure_sync("test-channel-secret")
        .expect("configures the test Network Channel");
    let mdns = Arc::new(MdnsAdvertiser::on(vec![0]));
    let advertiser: Arc<dyn shaprint_desktop::application::ServerAdvertiser> = mdns.clone();
    let service = Arc::new(ServerSharingService::new(
        sharing,
        Arc::clone(&endpoint),
        advertiser,
        support::allowed_inbound_setup(),
        channel as Arc<dyn shaprint_desktop::application::ChannelState>,
    ));
    let runtime = RuntimeCoordinator::new(vec![service]);

    runtime
        .start(ServiceId::ServerSharing)
        .await
        .expect("starts sharing");
    let tcp_address = endpoint.bound_address().expect("IPPS listener is recorded");
    let udp_port = mdns.bound_port().expect("discovery listener is recorded");

    runtime
        .stop(ServiceId::ServerSharing)
        .await
        .expect("stops sharing");

    assert!(endpoint.bound_address().is_none());
    let tcp_rebound = TcpListener::bind(tcp_address).expect("stop released the IPPS port");
    let udp_rebound =
        std::net::UdpSocket::bind(("0.0.0.0", udp_port)).expect("stop released the discovery port");
    drop((tcp_rebound, udp_rebound));
    runtime.shutdown().await.expect("shutdown is idempotent");
}

#[derive(Default)]
struct CapturingSubmitter {
    jobs: Mutex<Vec<(String, PrintJob)>>,
}

#[async_trait::async_trait]
impl PrintJobSubmitter for CapturingSubmitter {
    async fn submit(&self, printer: &PrinterName, job: PrintJob) -> Result<u32, AppError> {
        self.jobs
            .lock()
            .map_err(|_| AppError::internal("capturing submitter lock poisoned"))?
            .push((printer.as_str().to_owned(), job));
        Ok(101)
    }
}

struct LiveQualityServer {
    runtime: RuntimeCoordinator,
    client: IppClient,
    printer_uri: String,
    channel_secret: String,
    submitter: Arc<CapturingSubmitter>,
}

impl LiveQualityServer {
    async fn start(queue_name: &str) -> Self {
        let channel = Arc::new(NetworkChannel::in_memory());
        let channel_secret = "quality-test-channel-secret".to_owned();
        channel
            .configure(&channel_secret)
            .await
            .expect("configures test Network Channel");
        let submitter = Arc::new(CapturingSubmitter::default());
        let (sharing, endpoint) =
            sharing_runtime_with_submitter(&[queue_name], submitter.clone(), Arc::clone(&channel));
        sharing
            .set_shared(printer_names(&[queue_name]))
            .await
            .expect("selects queue to share");

        let service = Arc::new(support::sharing_service_with_channel(
            Arc::clone(&sharing),
            Arc::clone(&endpoint),
            channel,
        ));
        let runtime = RuntimeCoordinator::new(vec![service]);
        runtime
            .start(ServiceId::ServerSharing)
            .await
            .expect("starts server sharing service");

        let address = endpoint.bound_address().expect("endpoint is listening");
        let client = IppClient::new(address.port(), endpoint.fingerprint().to_string());
        let encoded_queue: String = queue_name
            .bytes()
            .map(|byte| {
                if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
                    char::from(byte).to_string()
                } else {
                    format!("%{byte:02X}")
                }
            })
            .collect();
        let printer_uri = format!(
            "ipps://127.0.0.1:{}/ipp/print/{encoded_queue}",
            address.port()
        );

        Self {
            runtime,
            client,
            printer_uri,
            channel_secret,
            submitter,
        }
    }
}

#[tokio::test]
async fn print_quality_lifecycle_end_to_end_across_ipps_submission_and_platform_mapping() {
    let server = LiveQualityServer::start("Office Printer").await;
    let document = build_test_pwg_raster(2480, 3508, 300, true);

    for (quality_value, expected_enum, expected_dm_quality, _expected_cups_val) in [
        (3, PrintQuality::Draft, DMRES_DRAFT, "3"),
        (4, PrintQuality::Normal, DMRES_MEDIUM, "4"),
        (5, PrintQuality::High, DMRES_HIGH, "5"),
    ] {
        let request = print_job_request(
            &server.printer_uri,
            Some(&server.channel_secret),
            Some(quality_value),
            &[],
            &document,
        );

        let (http_status, response) = server
            .client
            .post(&request)
            .await
            .expect("submits job over TLS IPPS");

        assert_eq!(http_status, 200);
        assert_eq!(status_code(&response), 0x0000);

        let jobs = server.submitter.jobs.lock().expect("locks captured jobs");
        let (printer, job) = jobs
            .last()
            .expect("captured job must be present on submitter");
        assert_eq!(printer, "Office Printer");
        assert_eq!(job.settings().quality, Some(expected_enum));

        // Verify Windows DEVMODE driver translation
        let mut devmode = vec![0u8; 220];
        devmode[68..70].copy_from_slice(&220u16.to_ne_bytes());
        apply_settings_to_dev_mode(&mut devmode, job.settings()).expect("applies to devmode");
        let fields = u32::from_ne_bytes(devmode[72..76].try_into().unwrap());
        assert_ne!(
            fields & DM_PRINTQUALITY,
            0,
            "DM_PRINTQUALITY must be flagged for quality {quality_value}"
        );
        let dm_quality = i16::from_ne_bytes(devmode[90..92].try_into().unwrap());
        assert_eq!(
            dm_quality, expected_dm_quality,
            "dmPrintQuality must match expected Win32 constant for quality {quality_value}"
        );

        // Verify Linux CUPS IPP option translation
        #[cfg(unix)]
        {
            let cups_options = build_cups_job_options(job.settings()).expect("builds cups options");
            assert!(
                cups_options.contains(&("print-quality".to_owned(), _expected_cups_val.to_owned())),
                "CUPS options must contain print-quality={_expected_cups_val}"
            );
        }
    }

    server.runtime.shutdown().await.expect("clean shutdown");
}

#[tokio::test]
async fn print_quality_wire_preservation_bounds_pwg_raster_at_300_dpi() {
    let server = LiveQualityServer::start("Office Printer").await;

    // 1. Verify capability advertisement on Get-Printer-Attributes
    let req_attrs = get_printer_attributes(1, &server.printer_uri);
    let (http_status, attrs_response) = server
        .client
        .post(&req_attrs)
        .await
        .expect("queries printer attributes over TLS IPPS");
    assert_eq!(http_status, 200);
    assert_eq!(status_code(&attrs_response), 0x0000);

    let supported_quality = attribute_integers(&attrs_response, "print-quality-supported");
    assert_eq!(
        supported_quality,
        vec![3, 4, 5],
        "print-quality-supported must advertise [3, 4, 5]"
    );

    let default_quality = attribute_integers(&attrs_response, "print-quality-default");
    assert_eq!(
        default_quality,
        vec![4],
        "print-quality-default must advertise 4 (Normal)"
    );

    let resolutions =
        attribute_resolutions(&attrs_response, "pwg-raster-document-resolution-supported");
    assert_eq!(
        resolutions,
        vec![(300, 300, 3)],
        "pwg-raster wire format must be advertised at 300 DPI"
    );

    // 2. Submit high quality print job and verify 300 DPI wire preservation
    let pwg_doc = build_test_pwg_raster(2480, 3508, 300, true);
    let request = print_job_request(
        &server.printer_uri,
        Some(&server.channel_secret),
        Some(5), // High quality
        &[],
        &pwg_doc,
    );

    let (http_status, response) = server
        .client
        .post(&request)
        .await
        .expect("submits high-quality job over TLS IPPS");
    assert_eq!(http_status, 200);
    assert_eq!(status_code(&response), 0x0000);

    {
        let jobs = server.submitter.jobs.lock().expect("locks captured jobs");
        let (_, job) = jobs
            .last()
            .expect("captured high-quality job must be present");
        assert_eq!(job.settings().quality, Some(PrintQuality::High));

        // Verify wire preservation: document is intact, decode succeeds, and resolution is strictly 300 DPI
        let pages = decode_pwg(job.document()).expect("decodes wire PWG Raster payload");
        assert_eq!(pages.len(), 1);
        assert_eq!(
            pages[0].dpi,
            [300, 300],
            "wire raster resolution must remain 300 DPI even when High quality is requested"
        );
        assert_eq!(pages[0].width, 2480);
        assert_eq!(pages[0].height, 3508);
        assert!(
            job.document().len() < 10 * 1024 * 1024,
            "payload size must remain compact and bounded"
        );
    }

    server.runtime.shutdown().await.expect("clean shutdown");
}

#[tokio::test]
async fn print_quality_cross_version_compatibility_preserves_defaults_for_unspecified_and_handles_unknown_attributes(
) {
    let server = LiveQualityServer::start("Office Printer").await;
    let document = build_test_pwg_raster(2480, 3508, 300, false);

    // 1. Stable Client scenario: quality is omitted from request
    let request_stable = print_job_request(
        &server.printer_uri,
        Some(&server.channel_secret),
        None, // omitted quality attribute
        &[],
        &document,
    );
    let (http_status, response_stable) = server
        .client
        .post(&request_stable)
        .await
        .expect("submits job without quality over TLS IPPS");
    assert_eq!(http_status, 200);
    assert_eq!(status_code(&response_stable), 0x0000);

    {
        let jobs = server.submitter.jobs.lock().expect("locks captured jobs");
        let (_, job) = jobs.last().expect("captured stable job");
        assert_eq!(
            job.settings().quality,
            None,
            "omitted quality must be represented as None"
        );

        // Windows DEVMODE: DM_PRINTQUALITY unflagged, offset 90..92 preserved
        let mut devmode = vec![0u8; 220];
        devmode[68..70].copy_from_slice(&220u16.to_ne_bytes());
        devmode[90..92].copy_from_slice(&999i16.to_ne_bytes()); // pre-set default preference
        apply_settings_to_dev_mode(&mut devmode, job.settings())
            .expect("applies settings with unspecified quality");
        let fields = u32::from_ne_bytes(devmode[72..76].try_into().unwrap());
        assert_eq!(
            fields & DM_PRINTQUALITY,
            0,
            "DM_PRINTQUALITY must remain unflagged when quality is None"
        );
        let dm_quality = i16::from_ne_bytes(devmode[90..92].try_into().unwrap());
        assert_eq!(
            dm_quality, 999,
            "offset 90..92 must remain untouched to preserve queue default preferences"
        );

        // Linux CUPS: no print-quality option injected
        #[cfg(unix)]
        {
            let cups_options = build_cups_job_options(job.settings()).expect("builds cups options");
            assert!(
                !cups_options.iter().any(|(k, _)| k == "print-quality"),
                "no print-quality option must be passed when quality is None"
            );
        }
    }

    // 2. Future / vendor unknown attributes are ignored per RFC 8011 §5.2.8
    let request_unknown = print_job_request(
        &server.printer_uri,
        Some(&server.channel_secret),
        Some(4),
        &[("vendor-unknown-feature", "custom-future-client-value")],
        &document,
    );
    let (http_status, response_unknown) = server
        .client
        .post(&request_unknown)
        .await
        .expect("submits job with unknown attribute");
    assert_eq!(http_status, 200);
    assert_eq!(status_code(&response_unknown), 0x0000);

    // 3. Out-of-range quality values (e.g. 2 and 6) are rejected with 0x040B
    let count_before = server
        .submitter
        .jobs
        .lock()
        .expect("locks captured jobs")
        .len();
    for invalid_val in [2, 6] {
        let request_invalid = print_job_request(
            &server.printer_uri,
            Some(&server.channel_secret),
            Some(invalid_val),
            &[],
            &document,
        );
        let (http_status, response_invalid) = server
            .client
            .post(&request_invalid)
            .await
            .expect("submits invalid quality job");
        assert_eq!(http_status, 200);
        assert_eq!(
            status_code(&response_invalid),
            0x040B, // client-error-attributes-or-values-not-supported
            "quality {invalid_val} must be rejected with 0x040B"
        );
    }
    let count_after = server
        .submitter
        .jobs
        .lock()
        .expect("locks captured jobs")
        .len();
    assert_eq!(
        count_before, count_after,
        "rejected jobs must not reach the physical printer submitter"
    );

    server.runtime.shutdown().await.expect("clean shutdown");
}
