//! The server sharing runtime.
//!
//! Sharing exposes the local Windows queues the user selected through IPPS, answers printer
//! queries, and submits authorized Print-Job requests only through the injected spooler adapter
//! (#31, #32; ADR 0003).
//!
//! Sharing never starts on its own: the user controls it through the shell's lifecycle commands,
//! and the endpoint only exists while the service runs.

use std::sync::Arc;

use async_trait::async_trait;

use crate::adapters::ipps::IppsServer;
use crate::application::{RuntimeService, ServiceContext, SharedPrinterSource, Sharing};
use crate::domain::{AppError, ServiceId};

/// Supervises IPPS sharing of the local printer queues.
pub struct ServerSharingService {
    sharing: Arc<Sharing>,
    endpoint: Arc<IppsServer>,
}

impl ServerSharingService {
    pub fn new(sharing: Arc<Sharing>, endpoint: Arc<IppsServer>) -> Self {
        Self { sharing, endpoint }
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
        let directory: Arc<dyn SharedPrinterSource> = self.sharing.clone();
        self.endpoint.serve(directory, context).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::identity::ServerIdentity;
    use crate::adapters::ipps::NetworkChannel;
    use crate::application::{LocalPrinterCatalog, PrintJob, PrintJobSubmitter};
    use crate::domain::PrinterName;

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
        (
            ServerSharingService::new(Arc::clone(&sharing), endpoint),
            sharing,
        )
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
