//! The server sharing runtime.
//!
//! Sharing exposes the local Windows queues the user selected through IPPS, answers printer
//! queries, and submits authorized Print-Job requests only through the injected spooler adapter
//! (#31, #32; ADR 0003). While it runs it also advertises the same queues on the local network, so
//! a client can find the server without being told its address (#36; ADR 0004).
//!
//! The user starts sharing explicitly; a previously enabled service is restored at login.
//! The endpoint only exists while the service runs.

use std::sync::Arc;

use async_trait::async_trait;

use crate::adapters::ipps::IppsServer;
use crate::application::{
    ChannelState, RuntimeService, ServerAdvertiser, ServiceContext, Setup, SharedPrinterSource,
    Sharing,
};
use crate::domain::{AppError, ServiceId};

/// Supervises IPPS sharing of the local printer queues.
pub struct ServerSharingService {
    sharing: Arc<Sharing>,
    endpoint: Arc<IppsServer>,
    advertiser: Arc<dyn ServerAdvertiser>,
    setup: Arc<Setup>,
    channel: Arc<dyn ChannelState>,
}

impl ServerSharingService {
    pub fn new(
        sharing: Arc<Sharing>,
        endpoint: Arc<IppsServer>,
        advertiser: Arc<dyn ServerAdvertiser>,
        setup: Arc<Setup>,
        channel: Arc<dyn ChannelState>,
    ) -> Self {
        Self {
            sharing,
            endpoint,
            advertiser,
            setup,
            channel,
        }
    }
}

#[async_trait]
impl RuntimeService for ServerSharingService {
    fn id(&self) -> ServiceId {
        ServiceId::ServerSharing
    }

    /// Sharing autostarts if the user previously had sharing enabled and has valid printers.
    fn autostart(&self) -> bool {
        self.sharing.is_autostart_enabled()
    }

    fn started(&self) {
        if let Err(error) = self.sharing.set_sharing_enabled(true) {
            log::warn!("cannot persist sharing enabled state: {error}");
        }
    }

    fn stopped(&self) {
        if let Err(error) = self.sharing.set_sharing_enabled(false) {
            log::warn!("cannot persist sharing stopped state: {error}");
        }
    }

    /// Sharing needs something to share: a server with no selected queue would open a port and
    /// answer every client with an empty printer list.
    /// Sharing also requires a configured Network Channel to authorize print requests.
    fn preflight(&self) -> Result<(), AppError> {
        if self.sharing.shared_printers().is_empty() {
            return Err(AppError::invalid_state(
                "select at least one printer to share before starting",
            ));
        }
        if !self.channel.is_configured() {
            return Err(AppError::invalid_state(
                "a Network Channel is not configured. Set a Network Channel before starting sharing.",
            ));
        }
        Ok(())
    }

    async fn prepare(&self) -> Result<(), AppError> {
        let setup = Arc::clone(&self.setup);
        tokio::task::spawn_blocking(move || setup.ensure_inbound_sharing())
            .await
            .map_err(|_| AppError::internal("Clients cannot connect: the inbound access check did not finish. Try starting Server Sharing again."))??;
        Ok(())
    }

    async fn run(&self, context: ServiceContext) -> Result<(), AppError> {
        // Follow the selection before it is read and advertised: a change made while the
        // advertisement is being opened is then still waiting for the loop below, instead of
        // leaving the network with a list the server no longer shares.
        let mut selection = self.sharing.subscribe();
        let printers = self.sharing.selected()?;
        let advertisement = match self
            .advertiser
            .advertise(self.endpoint.port(), printers.as_slice())
            .await
        {
            Ok(advertisement) => Some(advertisement),
            Err(error) => {
                // A conflicting process on the dedicated discovery port is actionable and must
                // remain visible as a failed start; do not disguise it as a best-effort discovery
                // outage. Other discovery failures remain non-fatal to direct IPPS printing.
                if error.message().contains(" is in use")
                    || error.message().contains(" remains in use")
                    || error.message().contains("did not release UDP port")
                    || error.message().contains("holding UDP port")
                    || error
                        .message()
                        .contains("cannot inspect the process using UDP port")
                {
                    return Err(error);
                }
                log::warn!(
                    "sharing cannot advertise itself code={} message={}",
                    error.code_str(),
                    error
                );
                None
            }
        };

        let directory: Arc<dyn SharedPrinterSource> = self.sharing.clone();
        let mut serving = Box::pin(self.endpoint.serve(directory, context.clone()));

        let outcome = match &advertisement {
            Some(advertisement) => loop {
                tokio::select! {
                    result = &mut serving => break result,
                    // A change to the shared printers takes effect while sharing runs, exactly as
                    // it does for the endpoint itself (ADR 0003).
                    changed = selection.changed() => match changed {
                        Ok(()) => {
                            let printers = selection.borrow_and_update().clone();
                            if let Err(error) = advertisement.replace(printers.as_slice()).await {
                                log::warn!(
                                    "cannot update the discovery advertisement code={} message={}",
                                    error.code_str(),
                                    error
                                );
                            }
                        }
                        Err(_) => break serving.await,
                    },
                }
            },
            None => serving.await,
        };

        // Withdraw before reporting the outcome, so a stopped server stops being discoverable even
        // when it stopped because of a failure.
        if let Some(advertisement) = advertisement {
            if let Err(error) = advertisement.withdraw().await {
                log::warn!(
                    "cannot withdraw the discovery advertisement code={} message={}",
                    error.code_str(),
                    error
                );
            }
        }
        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::identity::ServerIdentity;
    use crate::adapters::ipps::NetworkChannel;
    use crate::application::{
        Advertisement, ElevationBroker, LocalPrinterCatalog, PrintFailures, PrintJob,
        PrintJobSubmitter, RuntimeCoordinator,
    };
    use crate::domain::{ClientQueueRequest, PrinterName, ServiceState, SetupAction, SetupFailure};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use std::time::Duration;

    fn name(value: &str) -> PrinterName {
        PrinterName::parse(value).expect("a valid printer name")
    }

    /// What the sharing runtime asked the network to advertise.
    #[derive(Default)]
    struct Recorded {
        opened: Mutex<Vec<(u16, Vec<PrinterName>)>>,
        replaced: Mutex<Vec<Vec<PrinterName>>>,
        withdrawn: AtomicUsize,
    }

    impl Recorded {
        fn opened(&self) -> Vec<(u16, Vec<PrinterName>)> {
            self.opened.lock().map(|o| o.clone()).unwrap_or_default()
        }

        fn replaced(&self) -> Vec<Vec<PrinterName>> {
            self.replaced.lock().map(|r| r.clone()).unwrap_or_default()
        }

        fn withdrawn(&self) -> usize {
            self.withdrawn.load(Ordering::SeqCst)
        }
    }

    struct FakeAdvertisement {
        recorded: Arc<Recorded>,
    }

    #[async_trait]
    impl Advertisement for FakeAdvertisement {
        async fn replace(&self, printers: &[PrinterName]) -> Result<(), AppError> {
            self.recorded
                .replaced
                .lock()
                .map_err(|_| AppError::internal("the recording lock is poisoned"))?
                .push(printers.to_vec());
            Ok(())
        }

        async fn withdraw(&self) -> Result<(), AppError> {
            self.recorded.withdrawn.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    /// Holds an advertisement open, so a test can act while sharing is still starting.
    #[derive(Default)]
    struct AdvertiseGate {
        entered: tokio::sync::Notify,
        released: tokio::sync::Notify,
    }

    /// An advertiser that records what it was asked to do, can refuse to advertise at all, and can
    /// be held open by a test.
    struct FakeAdvertiser {
        recorded: Arc<Recorded>,
        unavailable: bool,
        gate: Option<Arc<AdvertiseGate>>,
    }

    impl FakeAdvertiser {
        fn available() -> (Arc<Self>, Arc<Recorded>) {
            Self::build(false, None)
        }

        fn unavailable() -> Arc<Self> {
            Self::build(true, None).0
        }

        fn gated(gate: Arc<AdvertiseGate>) -> (Arc<Self>, Arc<Recorded>) {
            Self::build(false, Some(gate))
        }

        fn build(
            unavailable: bool,
            gate: Option<Arc<AdvertiseGate>>,
        ) -> (Arc<Self>, Arc<Recorded>) {
            let recorded = Arc::new(Recorded::default());
            (
                Arc::new(Self {
                    recorded: Arc::clone(&recorded),
                    unavailable,
                    gate,
                }),
                recorded,
            )
        }
    }

    #[async_trait]
    impl ServerAdvertiser for FakeAdvertiser {
        async fn advertise(
            &self,
            port: u16,
            printers: &[PrinterName],
        ) -> Result<Arc<dyn Advertisement>, AppError> {
            if self.unavailable {
                return Err(AppError::internal("no discovery socket could be opened"));
            }
            self.recorded
                .opened
                .lock()
                .map_err(|_| AppError::internal("the recording lock is poisoned"))?
                .push((port, printers.to_vec()));
            if let Some(gate) = &self.gate {
                gate.entered.notify_one();
                gate.released.notified().await;
            }
            Ok(Arc::new(FakeAdvertisement {
                recorded: Arc::clone(&self.recorded),
            }))
        }
    }

    struct AllowedAccess;
    impl ElevationBroker for AllowedAccess {
        fn inbound_sharing_allowed(&self) -> Result<bool, SetupFailure> {
            Ok(true)
        }
        fn elevate(&self, _action: SetupAction) -> Result<(), SetupFailure> {
            panic!("already allowed")
        }
        fn install_queue(&self, _request: &ClientQueueRequest) -> Result<(), SetupFailure> {
            panic!("not installing")
        }
    }

    fn allowed_setup() -> Arc<Setup> {
        Arc::new(Setup::new(Arc::new(AllowedAccess)))
    }

    /// The coordinator, the sharing configuration, and what the network was told.
    fn coordinator(
        queues: &[&str],
        advertiser: Arc<dyn ServerAdvertiser>,
    ) -> (RuntimeCoordinator, Arc<Sharing>, Arc<IppsServer>) {
        let catalog = Arc::new(FakeCatalog(
            queues.iter().map(|queue| name(queue)).collect(),
        ));
        let sharing = Arc::new(Sharing::new(catalog));
        let channel = Arc::new(NetworkChannel::in_memory());
        let _ = channel.configure_sync("test-channel-secret");
        let endpoint = Arc::new(IppsServer::new(
            0,
            Arc::new(ServerIdentity::generate().expect("generates")),
            Arc::clone(&channel),
            Arc::new(UnavailableSubmitter),
            Arc::new(PrintFailures::new()),
            Arc::new(crate::application::PrintJobTracker::new()),
        ));
        let runtime = RuntimeCoordinator::new(vec![Arc::new(ServerSharingService::new(
            Arc::clone(&sharing),
            Arc::clone(&endpoint),
            advertiser,
            allowed_setup(),
            channel,
        ))]);
        (runtime, sharing, endpoint)
    }

    /// Waits until `condition` holds, or fails the test.
    async fn until(condition: impl Fn() -> bool) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while tokio::time::Instant::now() < deadline {
            if condition() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the condition never held");
    }

    struct FakeCatalog(Vec<PrinterName>);

    #[async_trait]
    impl LocalPrinterCatalog for FakeCatalog {
        async fn local_printers(&self) -> Result<Vec<PrinterName>, AppError> {
            Ok(self.0.clone())
        }
    }

    struct UnavailableSubmitter;

    #[async_trait]
    impl PrintJobSubmitter for UnavailableSubmitter {
        fn is_available(&self) -> bool {
            false
        }

        async fn submit(&self, _printer: &PrinterName, _job: PrintJob) -> Result<u32, AppError> {
            Err(AppError::unsupported(
                "printer submission is unavailable in this test",
            ))
        }
    }

    fn service(queues: &[&str], setup: Arc<Setup>) -> (ServerSharingService, Arc<Sharing>) {
        service_with_channel(queues, setup, true)
    }

    fn service_with_channel(
        queues: &[&str],
        setup: Arc<Setup>,
        channel_configured: bool,
    ) -> (ServerSharingService, Arc<Sharing>) {
        let catalog = Arc::new(FakeCatalog(
            queues
                .iter()
                .map(|name| PrinterName::parse(name).expect("valid name"))
                .collect(),
        ));
        let sharing = Arc::new(Sharing::new(catalog));
        let identity = Arc::new(ServerIdentity::generate().expect("generates"));
        let channel = Arc::new(NetworkChannel::in_memory());
        if channel_configured {
            let _ = channel.configure_sync("test-channel-secret");
        }
        let endpoint = Arc::new(IppsServer::new(
            0,
            identity,
            Arc::clone(&channel),
            Arc::new(UnavailableSubmitter),
            Arc::new(PrintFailures::new()),
            Arc::new(crate::application::PrintJobTracker::new()),
        ));
        let (advertiser, _recorded) = FakeAdvertiser::available();
        (
            ServerSharingService::new(Arc::clone(&sharing), endpoint, advertiser, setup, channel),
            sharing,
        )
    }

    #[tokio::test]
    async fn starting_sharing_with_existing_client_access_does_not_request_administrator_permission(
    ) {
        use crate::application::{ElevationBroker, Setup};
        use crate::domain::{ClientQueueRequest, SetupAction, SetupFailure};

        struct AllowedBroker(AtomicUsize);
        impl ElevationBroker for AllowedBroker {
            fn inbound_sharing_allowed(&self) -> Result<bool, SetupFailure> {
                Ok(true)
            }
            fn elevate(&self, _action: SetupAction) -> Result<(), SetupFailure> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
            fn install_queue(&self, _request: &ClientQueueRequest) -> Result<(), SetupFailure> {
                Ok(())
            }
        }

        let broker = Arc::new(AllowedBroker(AtomicUsize::new(0)));
        let (service, sharing) = service(
            &["HP LaserJet"],
            Arc::new(Setup::new(Arc::clone(&broker) as Arc<dyn ElevationBroker>)),
        );
        sharing
            .set_shared(vec![name("HP LaserJet")])
            .await
            .expect("selects queue");
        let runtime = RuntimeCoordinator::new(vec![Arc::new(service)]);

        runtime
            .start(ServiceId::ServerSharing)
            .await
            .expect("sharing starts");
        assert_eq!(broker.0.load(Ordering::SeqCst), 0);
        runtime.stop(ServiceId::ServerSharing).await.expect("stops");
    }

    #[tokio::test]
    async fn starting_sharing_requests_missing_access_then_allows_clients_to_connect() {
        use crate::application::{ElevationBroker, Setup};
        use crate::domain::{ClientQueueRequest, SetupAction, SetupFailure};

        struct Broker {
            allowed: std::sync::atomic::AtomicBool,
            prompts: AtomicUsize,
        }
        impl ElevationBroker for Broker {
            fn inbound_sharing_allowed(&self) -> Result<bool, SetupFailure> {
                Ok(self.allowed.load(Ordering::SeqCst))
            }
            fn elevate(&self, action: SetupAction) -> Result<(), SetupFailure> {
                assert_eq!(action, SetupAction::AllowInboundSharing);
                self.prompts.fetch_add(1, Ordering::SeqCst);
                self.allowed.store(true, Ordering::SeqCst);
                Ok(())
            }
            fn install_queue(&self, _request: &ClientQueueRequest) -> Result<(), SetupFailure> {
                Ok(())
            }
        }
        let broker = Arc::new(Broker {
            allowed: std::sync::atomic::AtomicBool::new(false),
            prompts: AtomicUsize::new(0),
        });
        let (service, sharing) = service(
            &["HP LaserJet"],
            Arc::new(Setup::new(Arc::clone(&broker) as Arc<dyn ElevationBroker>)),
        );
        sharing
            .set_shared(vec![name("HP LaserJet")])
            .await
            .expect("selects queue");
        let runtime = RuntimeCoordinator::new(vec![Arc::new(service)]);

        runtime
            .start(ServiceId::ServerSharing)
            .await
            .expect("sharing starts after approval");
        assert_eq!(broker.prompts.load(Ordering::SeqCst), 1);
        runtime.stop(ServiceId::ServerSharing).await.expect("stops");
        runtime
            .start(ServiceId::ServerSharing)
            .await
            .expect("sharing restarts without prompt");
        assert_eq!(broker.prompts.load(Ordering::SeqCst), 1);
        runtime.stop(ServiceId::ServerSharing).await.expect("stops");
    }

    #[tokio::test]
    async fn denied_inbound_access_keeps_sharing_stopped_and_can_be_retried() {
        use crate::application::{ElevationBroker, Setup};
        use crate::domain::{
            ClientQueueRequest, ErrorCode, SetupAction, SetupFailure, SetupFailureKind,
        };

        struct Broker {
            allowed: std::sync::atomic::AtomicBool,
            prompts: AtomicUsize,
        }
        impl ElevationBroker for Broker {
            fn inbound_sharing_allowed(&self) -> Result<bool, SetupFailure> {
                Ok(self.allowed.load(Ordering::SeqCst))
            }
            fn elevate(&self, _action: SetupAction) -> Result<(), SetupFailure> {
                if self.prompts.fetch_add(1, Ordering::SeqCst) == 0 {
                    return Err(SetupFailure::new(
                        SetupFailureKind::PermissionDenied,
                        "declined",
                    ));
                }
                self.allowed.store(true, Ordering::SeqCst);
                Ok(())
            }
            fn install_queue(&self, _request: &ClientQueueRequest) -> Result<(), SetupFailure> {
                Ok(())
            }
        }
        let broker = Arc::new(Broker {
            allowed: std::sync::atomic::AtomicBool::new(false),
            prompts: AtomicUsize::new(0),
        });
        let (service, sharing) = service(
            &["HP LaserJet"],
            Arc::new(Setup::new(Arc::clone(&broker) as Arc<dyn ElevationBroker>)),
        );
        sharing
            .set_shared(vec![name("HP LaserJet")])
            .await
            .expect("selects queue");
        let runtime = RuntimeCoordinator::new(vec![Arc::new(service)]);

        let error = runtime
            .start(ServiceId::ServerSharing)
            .await
            .expect_err("denied");
        assert_eq!(error.code(), ErrorCode::Unsupported);
        assert!(error.message().contains("Clients cannot connect"));
        assert_eq!(
            runtime
                .status()
                .service(ServiceId::ServerSharing)
                .unwrap()
                .state(),
            ServiceState::Stopped
        );
        runtime
            .start(ServiceId::ServerSharing)
            .await
            .expect("retry starts after approval");
        assert_eq!(broker.prompts.load(Ordering::SeqCst), 2);
        runtime.stop(ServiceId::ServerSharing).await.expect("stops");
    }

    #[tokio::test]
    async fn concurrent_starts_do_not_open_two_administrator_prompts() {
        use crate::domain::{ErrorCode, SetupFailureKind};
        use std::sync::mpsc;

        struct WaitingBroker {
            attempts: AtomicUsize,
            release: Mutex<mpsc::Receiver<()>>,
        }
        impl ElevationBroker for WaitingBroker {
            fn inbound_sharing_allowed(&self) -> Result<bool, SetupFailure> {
                Ok(false)
            }
            fn elevate(&self, _action: SetupAction) -> Result<(), SetupFailure> {
                self.attempts.fetch_add(1, Ordering::SeqCst);
                self.release
                    .lock()
                    .expect("release lock")
                    .recv()
                    .expect("released");
                Err(SetupFailure::new(
                    SetupFailureKind::PermissionDenied,
                    "declined",
                ))
            }
            fn install_queue(&self, _request: &ClientQueueRequest) -> Result<(), SetupFailure> {
                panic!("not installing a printer queue")
            }
        }
        let (release, receiver) = mpsc::channel();
        let broker = Arc::new(WaitingBroker {
            attempts: AtomicUsize::new(0),
            release: Mutex::new(receiver),
        });
        let (service, sharing) = service(
            &["HP LaserJet"],
            Arc::new(Setup::new(Arc::clone(&broker) as Arc<dyn ElevationBroker>)),
        );
        sharing
            .set_shared(vec![name("HP LaserJet")])
            .await
            .expect("selects queue");
        let runtime = Arc::new(RuntimeCoordinator::new(vec![Arc::new(service)]));
        let first = tokio::spawn({
            let runtime = Arc::clone(&runtime);
            async move { runtime.start(ServiceId::ServerSharing).await }
        });
        until(|| broker.attempts.load(Ordering::SeqCst) == 1).await;

        let duplicate = runtime
            .start(ServiceId::ServerSharing)
            .await
            .expect_err("already starting");
        assert_eq!(duplicate.code(), ErrorCode::InvalidState);
        assert_eq!(broker.attempts.load(Ordering::SeqCst), 1);
        release.send(()).expect("releases first prompt");
        first
            .await
            .expect("first task joins")
            .expect_err("first prompt denied");
        assert_eq!(broker.attempts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn failed_firewall_setup_does_not_start_sharing_and_allows_retry() {
        use crate::application::{ElevationBroker, Setup};
        use crate::domain::{ClientQueueRequest, ErrorCode, SetupAction, SetupFailure};

        struct Broker {
            allowed: std::sync::atomic::AtomicBool,
            attempts: AtomicUsize,
        }
        impl ElevationBroker for Broker {
            fn inbound_sharing_allowed(&self) -> Result<bool, SetupFailure> {
                Ok(self.allowed.load(Ordering::SeqCst))
            }
            fn elevate(&self, _action: SetupAction) -> Result<(), SetupFailure> {
                if self.attempts.fetch_add(1, Ordering::SeqCst) > 0 {
                    self.allowed.store(true, Ordering::SeqCst);
                }
                Ok(())
            }
            fn install_queue(&self, _request: &ClientQueueRequest) -> Result<(), SetupFailure> {
                Ok(())
            }
        }
        let broker = Arc::new(Broker {
            allowed: std::sync::atomic::AtomicBool::new(false),
            attempts: AtomicUsize::new(0),
        });
        let (service, sharing) = service(
            &["HP LaserJet"],
            Arc::new(Setup::new(Arc::clone(&broker) as Arc<dyn ElevationBroker>)),
        );
        sharing
            .set_shared(vec![name("HP LaserJet")])
            .await
            .expect("selects queue");
        let runtime = RuntimeCoordinator::new(vec![Arc::new(service)]);

        let error = runtime
            .start(ServiceId::ServerSharing)
            .await
            .expect_err("rules still missing");
        assert_eq!(error.code(), ErrorCode::InvalidState);
        assert!(error.message().contains("Clients cannot connect"));
        assert_eq!(
            runtime
                .status()
                .service(ServiceId::ServerSharing)
                .unwrap()
                .state(),
            ServiceState::Stopped
        );
        runtime
            .start(ServiceId::ServerSharing)
            .await
            .expect("retry repairs firewall");
        assert_eq!(broker.attempts.load(Ordering::SeqCst), 2);
        runtime.stop(ServiceId::ServerSharing).await.expect("stops");
    }

    #[tokio::test]
    async fn starting_sharing_advertises_the_selected_queues_on_the_endpoint_port() {
        let (advertiser, recorded) = FakeAdvertiser::available();
        let (runtime, sharing, endpoint) = coordinator(&["HP LaserJet", "Zebra"], advertiser);
        sharing
            .set_shared(vec![name("Zebra")])
            .await
            .expect("selects a queue");

        runtime
            .start(ServiceId::ServerSharing)
            .await
            .expect("sharing starts");

        assert_eq!(
            recorded.opened(),
            vec![(endpoint.port(), vec![name("Zebra")])]
        );
        runtime.stop(ServiceId::ServerSharing).await.expect("stops");
    }

    #[tokio::test]
    async fn stopping_sharing_withdraws_the_advertisement() {
        let (advertiser, recorded) = FakeAdvertiser::available();
        let (runtime, sharing, _endpoint) = coordinator(&["HP LaserJet"], advertiser);
        sharing
            .set_shared(vec![name("HP LaserJet")])
            .await
            .expect("selects a queue");
        runtime
            .start(ServiceId::ServerSharing)
            .await
            .expect("sharing starts");
        assert_eq!(recorded.withdrawn(), 0);

        runtime
            .stop(ServiceId::ServerSharing)
            .await
            .expect("sharing stops");

        assert_eq!(recorded.withdrawn(), 1);
        runtime.shutdown().await.expect("shutdown succeeds");
    }

    #[tokio::test]
    async fn a_selection_change_made_while_sharing_starts_is_not_lost() {
        let gate = Arc::new(AdvertiseGate::default());
        let (advertiser, recorded) = FakeAdvertiser::gated(Arc::clone(&gate));
        let (runtime, sharing, _endpoint) = coordinator(&["HP LaserJet", "Zebra"], advertiser);
        sharing
            .set_shared(vec![name("Zebra")])
            .await
            .expect("selects a queue");
        let runtime = Arc::new(runtime);

        let starting = tokio::spawn({
            let runtime = Arc::clone(&runtime);
            async move { runtime.start(ServiceId::ServerSharing).await }
        });
        gate.entered.notified().await;

        // The user changes the selection while the advertisement is still being opened.
        sharing
            .set_shared(vec![name("HP LaserJet")])
            .await
            .expect("changes the selection");
        gate.released.notify_one();

        starting
            .await
            .expect("the start task joins")
            .expect("sharing starts");
        until(|| recorded.replaced().contains(&vec![name("HP LaserJet")])).await;

        runtime.stop(ServiceId::ServerSharing).await.expect("stops");
    }

    #[tokio::test]
    async fn changing_the_selection_updates_the_advertisement_without_restarting_sharing() {
        let (advertiser, recorded) = FakeAdvertiser::available();
        let (runtime, sharing, _endpoint) = coordinator(&["HP LaserJet", "Zebra"], advertiser);
        sharing
            .set_shared(vec![name("Zebra")])
            .await
            .expect("selects a queue");
        runtime
            .start(ServiceId::ServerSharing)
            .await
            .expect("sharing starts");

        sharing
            .set_shared(vec![name("HP LaserJet")])
            .await
            .expect("changes the selection");
        until(|| recorded.replaced().contains(&vec![name("HP LaserJet")])).await;

        // Sharing never restarted: the same advertisement followed the selection.
        assert_eq!(recorded.opened().len(), 1);
        runtime
            .stop(ServiceId::ServerSharing)
            .await
            .expect("sharing stops");
    }

    #[tokio::test]
    async fn a_server_that_cannot_advertise_still_serves_clients() {
        let (runtime, sharing, _endpoint) =
            coordinator(&["HP LaserJet"], FakeAdvertiser::unavailable());
        sharing
            .set_shared(vec![name("HP LaserJet")])
            .await
            .expect("selects a queue");

        runtime
            .start(ServiceId::ServerSharing)
            .await
            .expect("sharing starts without discovery");

        assert_eq!(
            runtime
                .status()
                .service(ServiceId::ServerSharing)
                .map(|status| status.state()),
            Some(ServiceState::Running)
        );
        runtime
            .stop(ServiceId::ServerSharing)
            .await
            .expect("sharing stops");
    }

    #[tokio::test]
    async fn sharing_cannot_start_before_network_channel_is_configured() {
        use crate::domain::ErrorCode;

        let (service, sharing) = service_with_channel(&["HP LaserJet"], allowed_setup(), false);
        sharing
            .set_shared(vec![name("HP LaserJet")])
            .await
            .expect("selects printer");

        let runtime = RuntimeCoordinator::new(vec![Arc::new(service)]);
        let error = runtime
            .start(ServiceId::ServerSharing)
            .await
            .expect_err("refuses start without channel");

        assert_eq!(error.code(), ErrorCode::InvalidState);
        assert!(error
            .message()
            .contains("Network Channel is not configured"));
    }

    #[tokio::test]
    async fn sharing_cannot_start_before_the_user_selects_a_queue() {
        let (service, sharing) = service(&["HP LaserJet"], allowed_setup());

        let error = service.preflight().expect_err("rejected");

        assert_eq!(error.code(), crate::domain::ErrorCode::InvalidState);
        assert_eq!(
            error.message(),
            "select at least one printer to share before starting"
        );

        sharing
            .set_shared(vec![PrinterName::parse("HP LaserJet").expect("valid name")])
            .await
            .expect("selects a queue");
        service.preflight().expect("sharing has something to share");
    }
}
