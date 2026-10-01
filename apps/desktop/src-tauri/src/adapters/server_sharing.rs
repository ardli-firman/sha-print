//! The server sharing runtime.
//!
//! Sharing exposes the local Windows queues the user selected through IPPS, answers printer
//! queries, and submits authorized Print-Job requests only through the injected spooler adapter
//! (#31, #32; ADR 0003). While it runs it also advertises the same queues on the local network, so
//! a client can find the server without being told its address (#36; ADR 0004).
//!
//! Sharing never starts on its own: the user controls it through the shell's lifecycle commands,
//! and the endpoint only exists while the service runs.

use std::sync::Arc;

use async_trait::async_trait;

use crate::adapters::ipps::IppsServer;
use crate::application::{
    RuntimeService, ServerAdvertiser, ServiceContext, SharedPrinterSource, Sharing,
};
use crate::domain::{AppError, ServiceId};

/// Supervises IPPS sharing of the local printer queues.
pub struct ServerSharingService {
    sharing: Arc<Sharing>,
    endpoint: Arc<IppsServer>,
    advertiser: Arc<dyn ServerAdvertiser>,
}

impl ServerSharingService {
    pub fn new(
        sharing: Arc<Sharing>,
        endpoint: Arc<IppsServer>,
        advertiser: Arc<dyn ServerAdvertiser>,
    ) -> Self {
        Self {
            sharing,
            endpoint,
            advertiser,
        }
    }
}

#[async_trait]
impl RuntimeService for ServerSharingService {
    fn id(&self) -> ServiceId {
        ServiceId::ServerSharing
    }

    /// Sharing is opt-in: the user starts and stops it explicitly (ADR 0001).
    fn autostart(&self) -> bool {
        false
    }

    /// Sharing needs something to share: a server with no selected queue would open a port and
    /// answer every client with an empty printer list.
    fn preflight(&self) -> Result<(), AppError> {
        if self.sharing.shared_printers().is_empty() {
            return Err(AppError::invalid_state(
                "select at least one printer to share before starting",
            ));
        }
        Ok(())
    }

    async fn run(&self, context: ServiceContext) -> Result<(), AppError> {
        let queues = self.sharing.selected()?;
        let advertisement = match self
            .advertiser
            .advertise(self.endpoint.port(), queues.as_slice())
            .await
        {
            Ok(advertisement) => Some(advertisement),
            Err(error) => {
                // Printer sharing is the product's job; being found on the network is how a client
                // gets to it. A server that cannot advertise still serves every client that
                // reaches it by address, so this is reported and not fatal (ADR 0004).
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
        let mut selection = self.sharing.subscribe();

        let outcome = match &advertisement {
            Some(advertisement) => loop {
                tokio::select! {
                    result = &mut serving => break result,
                    // A change to the shared queues takes effect while sharing runs, exactly as it
                    // does for the endpoint itself (ADR 0003).
                    changed = selection.changed() => match changed {
                        Ok(()) => {
                            let queues = selection.borrow_and_update().clone();
                            if let Err(error) = advertisement.replace(queues.as_slice()).await {
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
        Advertisement, LocalPrinterCatalog, PrintJob, PrintJobSubmitter, RuntimeCoordinator,
    };
    use crate::domain::{PrinterName, ServiceState};
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
        async fn replace(&self, queues: &[PrinterName]) -> Result<(), AppError> {
            self.recorded
                .replaced
                .lock()
                .map_err(|_| AppError::internal("the recording lock is poisoned"))?
                .push(queues.to_vec());
            Ok(())
        }

        async fn withdraw(&self) -> Result<(), AppError> {
            self.recorded.withdrawn.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    /// An advertiser that records what it was asked to do, and can refuse to advertise at all.
    struct FakeAdvertiser {
        recorded: Arc<Recorded>,
        unavailable: bool,
    }

    impl FakeAdvertiser {
        fn available() -> (Arc<Self>, Arc<Recorded>) {
            let recorded = Arc::new(Recorded::default());
            (
                Arc::new(Self {
                    recorded: Arc::clone(&recorded),
                    unavailable: false,
                }),
                recorded,
            )
        }

        fn unavailable() -> Arc<Self> {
            Arc::new(Self {
                recorded: Arc::new(Recorded::default()),
                unavailable: true,
            })
        }
    }

    #[async_trait]
    impl ServerAdvertiser for FakeAdvertiser {
        async fn advertise(
            &self,
            port: u16,
            queues: &[PrinterName],
        ) -> Result<Arc<dyn Advertisement>, AppError> {
            if self.unavailable {
                return Err(AppError::internal("no discovery socket could be opened"));
            }
            self.recorded
                .opened
                .lock()
                .map_err(|_| AppError::internal("the recording lock is poisoned"))?
                .push((port, queues.to_vec()));
            Ok(Arc::new(FakeAdvertisement {
                recorded: Arc::clone(&self.recorded),
            }))
        }
    }

    /// The coordinator, the sharing configuration, and what the network was told.
    fn coordinator(
        queues: &[&str],
        advertiser: Arc<dyn ServerAdvertiser>,
    ) -> (RuntimeCoordinator, Arc<Sharing>, Arc<IppsServer>) {
        let catalog = Arc::new(FakeCatalog(
            queues.iter().map(|name| name_of(name)).collect(),
        ));
        let sharing = Arc::new(Sharing::new(catalog));
        let endpoint = Arc::new(IppsServer::new(
            0,
            Arc::new(ServerIdentity::generate().expect("generates")),
            Arc::new(NetworkChannel::in_memory()),
            Arc::new(UnavailableSubmitter),
        ));
        let runtime = RuntimeCoordinator::new(vec![Arc::new(ServerSharingService::new(
            Arc::clone(&sharing),
            Arc::clone(&endpoint),
            advertiser,
        ))]);
        (runtime, sharing, endpoint)
    }

    fn name_of(value: &str) -> PrinterName {
        PrinterName::parse(value).expect("a valid printer name")
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

    fn service(queues: &[&str]) -> (ServerSharingService, Arc<Sharing>) {
        let catalog = Arc::new(FakeCatalog(
            queues
                .iter()
                .map(|name| PrinterName::parse(name).expect("valid name"))
                .collect(),
        ));
        let sharing = Arc::new(Sharing::new(catalog));
        let identity = Arc::new(ServerIdentity::generate().expect("generates"));
        let endpoint = Arc::new(IppsServer::new(
            0,
            identity,
            Arc::new(NetworkChannel::in_memory()),
            Arc::new(UnavailableSubmitter),
        ));
        let (advertiser, _recorded) = FakeAdvertiser::available();
        (
            ServerSharingService::new(Arc::clone(&sharing), endpoint, advertiser),
            sharing,
        )
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
    async fn sharing_cannot_start_before_the_user_selects_a_queue() {
        let (service, sharing) = service(&["HP LaserJet"]);

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
