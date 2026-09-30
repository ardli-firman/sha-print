//! The end-to-end sharing seam: a server user selects local queues, starts sharing, and an IPPS
//! client queries the shared printers over TLS — the flow issue #31 delivers.
//!
//! The printer adapter is the fake catalog, so these tests cover the protocol, the sharing rules,
//! and the lifecycle without a Windows spooler.

mod support;

use std::sync::Arc;

use shaprint_desktop::adapters::{IppsServer, ServerSharingService};
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

        let runtime = RuntimeCoordinator::new(vec![Arc::new(ServerSharingService::new(
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
        format!("ipps://{address}/ipp/print/{encoded}")
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
        RuntimeCoordinator::new(vec![Arc::new(ServerSharingService::new(sharing, endpoint))]);

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
