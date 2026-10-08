//! The end-to-end sharing seam: a server user selects local queues, starts sharing, and an IPPS
//! client queries the shared printers over TLS — the flow issue #31 delivers.
//!
//! The printer adapter is the fake catalog, so these tests cover the protocol, the sharing rules,
//! and the lifecycle without a Windows spooler.

mod support;

use std::sync::Arc;

use shaprint_desktop::adapters::IppsServer;
use shaprint_desktop::application::{RuntimeCoordinator, Sharing};
use shaprint_desktop::domain::{ErrorCode, ServiceId, ServiceState};
use tokio::net::TcpStream;

use support::{
    advertised_printers, get_printer_attributes, get_printers, printer_names, status_code,
    IppClient,
};

/// A running server plus a client that approved its fingerprint.
struct Shared {
    runtime: RuntimeCoordinator,
    sharing: Arc<Sharing>,
    endpoint: Arc<IppsServer>,
    client: IppClient,
}

impl Shared {
    /// The queues the fake spooler has installed.
    const LOCAL_QUEUES: [&'static str; 3] = ["HP LaserJet", "Zebra", "Canon"];

    /// Selects `selected`, starts sharing, and connects a client that approved the fingerprint.
    async fn start(selected: &[&str]) -> Self {
        let (sharing, endpoint) = support::sharing_runtime(&Self::LOCAL_QUEUES);
        sharing
            .set_shared(printer_names(selected))
            .await
            .expect("selects the shared queues");

        let runtime = RuntimeCoordinator::new(vec![Arc::new(support::sharing_service(
            Arc::clone(&sharing),
            Arc::clone(&endpoint),
        ))]);
        runtime
            .start(ServiceId::ServerSharing)
            .await
            .expect("sharing starts");

        let address = endpoint.bound_address().expect("the endpoint is listening");
        let client = IppClient::new(address.port(), endpoint.fingerprint().to_string());
        Self {
            runtime,
            sharing,
            endpoint,
            client,
        }
    }

    /// The URI the endpoint advertises for `name`.
    fn printer_uri(&self, name: &str) -> String {
        let address = self.endpoint.bound_address().expect("listening");
        let encoded: String = name
            .bytes()
            .map(|byte| {
                if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
                    char::from(byte).to_string()
                } else {
                    format!("%{byte:02X}")
                }
            })
            .collect();
        format!("ipps://127.0.0.1:{}/ipp/print/{encoded}", address.port())
    }

    async fn printers(&self) -> Vec<String> {
        let (http_status, body) = self
            .client
            .post(&get_printers(1))
            .await
            .expect("the client reaches the server");
        assert_eq!(http_status, 200);
        assert_eq!(status_code(&body), 0x0000);
        advertised_printers(&body)
    }
}

#[tokio::test]
async fn a_client_sees_exactly_the_shared_printers() {
    let shared = Shared::start(&["Zebra", "Canon"]).await;

    assert_eq!(shared.printers().await, vec!["Zebra", "Canon"]);

    shared.runtime.shutdown().await.expect("stops cleanly");
}

#[tokio::test]
async fn a_client_can_ask_about_one_printer_and_is_told_which_are_not_shared() {
    let shared = Shared::start(&["Zebra"]).await;

    let (_, body) = shared
        .client
        .post(&get_printer_attributes(2, &shared.printer_uri("Zebra")))
        .await
        .expect("the client reaches the server");
    assert_eq!(status_code(&body), 0x0000);
    assert_eq!(advertised_printers(&body), vec!["Zebra"]);

    let (_, body) = shared
        .client
        .post(&get_printer_attributes(3, &shared.printer_uri("Canon")))
        .await
        .expect("the client reaches the server");
    assert_eq!(status_code(&body), 0x0406);
    assert!(advertised_printers(&body).is_empty());

    shared.runtime.shutdown().await.expect("stops cleanly");
}

#[tokio::test]
async fn changing_the_selection_takes_effect_while_sharing_runs() {
    let shared = Shared::start(&["Zebra"]).await;
    assert_eq!(shared.printers().await, vec!["Zebra"]);

    shared
        .sharing
        .set_shared(printer_names(&["HP LaserJet", "Canon"]))
        .await
        .expect("changes the selection");

    assert_eq!(shared.printers().await, vec!["HP LaserJet", "Canon"]);

    shared.runtime.shutdown().await.expect("stops cleanly");
}

#[tokio::test]
async fn stopping_sharing_refuses_new_clients() {
    let shared = Shared::start(&["Zebra"]).await;
    let address = shared.endpoint.bound_address().expect("listening");
    assert_eq!(shared.printers().await, vec!["Zebra"]);

    shared
        .runtime
        .stop(ServiceId::ServerSharing)
        .await
        .expect("sharing stops");
    assert_eq!(
        shared
            .runtime
            .status()
            .service(ServiceId::ServerSharing)
            .map(|status| status.state()),
        Some(ServiceState::Stopped)
    );

    let refused = TcpStream::connect(address).await;
    assert!(refused.is_err(), "a stopped server still accepted a client");

    shared.runtime.shutdown().await.expect("stops cleanly");
}

#[tokio::test]
async fn a_client_that_has_not_approved_the_fingerprint_is_refused() {
    let shared = Shared::start(&["Zebra"]).await;
    let address = shared.endpoint.bound_address().expect("listening");
    let stranger = IppClient::new(address.port(), "00".repeat(32));

    let attempt = stranger.post(&get_printers(4)).await;

    assert!(attempt.is_err(), "a client trusted the wrong fingerprint");

    shared.runtime.shutdown().await.expect("stops cleanly");
}

#[tokio::test]
async fn operations_the_mvp_does_not_implement_are_rejected() {
    let shared = Shared::start(&["Zebra"]).await;

    // Print-URI (0x0003) remains outside this MVP.
    let mut request = vec![2, 0];
    request.extend(0x0003u16.to_be_bytes());
    request.extend(9u32.to_be_bytes());
    request.push(0x01);
    request.push(0x03);

    let (_, body) = shared
        .client
        .post(&request)
        .await
        .expect("the client reaches the server");
    assert_eq!(status_code(&body), 0x0501);

    shared.runtime.shutdown().await.expect("stops cleanly");
}

#[tokio::test]
async fn sharing_without_a_selection_never_opens_the_endpoint() {
    let (sharing, endpoint) = support::sharing_runtime(&Shared::LOCAL_QUEUES);
    let runtime =
        RuntimeCoordinator::new(vec![Arc::new(support::sharing_service(sharing, endpoint))]);

    let error = runtime
        .start(ServiceId::ServerSharing)
        .await
        .expect_err("rejected");

    assert_eq!(error.code(), ErrorCode::InvalidState);
    assert_eq!(
        runtime
            .status()
            .service(ServiceId::ServerSharing)
            .map(|status| status.state()),
        Some(ServiceState::Stopped)
    );
}

#[tokio::test]
async fn sharing_boundary_rejects_client_queues_and_restores_safely_with_interchangeable_spooler() {
    use async_trait::async_trait;
    use shaprint_desktop::application::{DestinationAwarePrinterCatalog, SpoolerReader};
    use shaprint_desktop::domain::{AppError, SpoolerRecord};

    struct TestSpooler(Vec<SpoolerRecord>);
    #[async_trait]
    impl SpoolerReader for TestSpooler {
        async fn read_spooler_records(&self) -> Result<Vec<SpoolerRecord>, AppError> {
            Ok(self.0.clone())
        }
    }

    let spooler = Arc::new(TestSpooler(vec![
        SpoolerRecord::new("HP LaserJet", "USB001"),
        // Lookalike on a real port remains selectable
        SpoolerRecord::new("Office Printer (ShaPrint 10.0.0.5-8631)", "USB002"),
        // Native client queue on loopback IPP
        SpoolerRecord::new(
            "Client Queue (ShaPrint 10.0.0.5-8631)",
            "ipp://127.0.0.1:8632/ipp/print/10.0.0.5%3A8631/Client%20Queue",
        ),
        // Windows-normalized port
        SpoolerRecord::new(
            "Windows Norm (ShaPrint 10.0.0.5-8631)",
            "http://127.0.0.1:8632/ipp/print/10.0.0.5%3A8631/Windows%20Norm",
        ),
        // Malformed destination
        SpoolerRecord::new(
            "Malformed (ShaPrint 10.0.0.5-8631)",
            "ipp://127.0.0.1:8632/ipp/print/bad",
        ),
    ]));

    let catalog = Arc::new(DestinationAwarePrinterCatalog::new(spooler));
    let sharing = Arc::new(Sharing::new(catalog));

    let local = sharing.local_printers().await.expect("reads local");
    let names: Vec<&str> = local.iter().map(|p| p.name().as_str()).collect();
    assert_eq!(
        names,
        vec!["HP LaserJet", "Office Printer (ShaPrint 10.0.0.5-8631)"]
    );

    // Direct selection of recognised client queue is rejected
    let err = sharing
        .set_shared(printer_names(&["Client Queue (ShaPrint 10.0.0.5-8631)"]))
        .await
        .expect_err("rejected");
    assert_eq!(err.code(), ErrorCode::InvalidInput);

    // Direct selection of malformed destination is rejected
    let err = sharing
        .set_shared(printer_names(&["Malformed (ShaPrint 10.0.0.5-8631)"]))
        .await
        .expect_err("rejected");
    assert_eq!(err.code(), ErrorCode::InvalidInput);
}
