//! Observable discovery seam: a client finds a server that starts sharing, loses it when sharing
//! stops, and still approves its certificate before seeing a single printer (#36).
//!
//! Every test here runs the real advertiser and the real browser over real sockets. They ask
//! loopback instead of the multicast group, because a test machine cannot take the multicast port
//! from whatever resolver it already runs; the wire format, the query/answer exchange, the goodbye,
//! and the cache are the production ones.

mod support;

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use shaprint_desktop::adapters::client_connections::ClientConnections;
use shaprint_desktop::adapters::discovery::{
    encode_query, MdnsAdvertiser, MdnsBrowser, SERVICE_TYPE,
};
use shaprint_desktop::adapters::ServerSharingService;
use shaprint_desktop::application::{
    Discovery, DiscoveryBrowser, DiscoveryService, RuntimeCoordinator, ServerAdvertiser,
};
use shaprint_desktop::domain::{NearbyServer, PrinterName, ServiceId};
use support::{
    allowed_inbound_setup, free_port, free_udp_port, printer_names, sharing_runtime_on,
    temporary_directory,
};

/// How long the test advertisement stays valid.
///
/// Deliberately far longer than any test waits: a server that stops sharing has to *tell* the
/// browsers that asked, because waiting for this to run out is not a test's lifetime, or a user's.
const ADVERTISED: Duration = Duration::from_secs(120);

/// Waits until the client lists a server, and returns it.
async fn discovered(discovery: &Discovery) -> NearbyServer {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(server) = discovery.nearby_servers().into_iter().next() {
            return server;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the client never found the server"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Waits until the client lists nothing.
async fn undiscovered(discovery: &Discovery) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !discovery.nearby_servers().is_empty() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the client still lists a server that stopped sharing"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// The client side: a real browser feeding the cache the UI reads.
async fn start_client(asked: u16) -> (Arc<RuntimeCoordinator>, Arc<Discovery>) {
    start_client_with(Arc::new(
        MdnsBrowser::targeting(vec![SocketAddr::from((Ipv4Addr::LOCALHOST, asked))])
            .with_query_interval(Duration::from_millis(100)),
    ))
    .await
}

/// The client side over any browse source.
async fn start_client_with(
    browser: Arc<dyn DiscoveryBrowser>,
) -> (Arc<RuntimeCoordinator>, Arc<Discovery>) {
    let discovery = Arc::new(Discovery::new().with_sweep_interval(Duration::from_millis(25)));
    let runtime = Arc::new(RuntimeCoordinator::new(vec![Arc::new(
        DiscoveryService::new(Arc::clone(&discovery), browser),
    )]));
    runtime.start_autostart().await.expect("the client browses");
    (runtime, discovery)
}

/// Starts sharing `queues` through the real sharing runtime and its advertisement.
async fn start_server(
    queues: &[&str],
    port: u16,
    advertiser: Arc<MdnsAdvertiser>,
) -> (
    Arc<RuntimeCoordinator>,
    Arc<shaprint_desktop::adapters::IppsServer>,
) {
    let (sharing, endpoint) = sharing_runtime_on(queues, port);
    sharing
        .set_shared(printer_names(queues))
        .await
        .expect("selects the shared queues");
    let runtime = Arc::new(RuntimeCoordinator::new(vec![Arc::new(
        ServerSharingService::new(
            sharing,
            Arc::clone(&endpoint),
            Arc::clone(&advertiser) as Arc<dyn ServerAdvertiser>,
            allowed_inbound_setup(),
        ),
    )]));
    runtime
        .start(ServiceId::ServerSharing)
        .await
        .expect("sharing starts");
    (runtime, endpoint)
}

#[tokio::test]
async fn a_client_finds_a_server_that_starts_sharing_without_being_told_its_address() {
    let advertiser = Arc::new(MdnsAdvertiser::on(vec![0]).with_ttl(ADVERTISED));
    let (server_runtime, endpoint) = start_server(
        &["HP LaserJet", "Zebra"],
        free_port(),
        Arc::clone(&advertiser),
    )
    .await;
    let advertised_on = advertiser.bound_port().expect("the advertisement is open");
    let (client_runtime, discovery) = start_client(advertised_on).await;

    let server = discovered(&discovery).await;

    assert!(
        !server.name().is_empty(),
        "the advertisement names the server"
    );
    // The client connects with the address the answer came from and the endpoint's own port.
    assert_eq!(server.address(), format!("127.0.0.1:{}", endpoint.port()));
    assert_eq!(
        server
            .printers()
            .iter()
            .map(PrinterName::as_str)
            .collect::<Vec<_>>(),
        vec!["HP LaserJet", "Zebra"],
        "the advertisement names the queues the server shares"
    );

    client_runtime.shutdown().await.expect("the client stops");
    server_runtime
        .stop(ServiceId::ServerSharing)
        .await
        .expect("sharing stops");
    server_runtime.shutdown().await.expect("the server stops");
}

#[tokio::test]
async fn stopping_sharing_tells_the_clients_that_asked_so_they_forget_it_at_once() {
    let advertiser = Arc::new(MdnsAdvertiser::on(vec![0]).with_ttl(ADVERTISED));
    let (server_runtime, _endpoint) =
        start_server(&["Zebra"], free_port(), Arc::clone(&advertiser)).await;
    let advertised_on = advertiser.bound_port().expect("the advertisement is open");
    let (client_runtime, discovery) = start_client(advertised_on).await;
    discovered(&discovery).await;

    server_runtime
        .stop(ServiceId::ServerSharing)
        .await
        .expect("sharing stops");

    undiscovered(&discovery).await;
    client_runtime.shutdown().await.expect("the client stops");
    server_runtime.shutdown().await.expect("the server stops");
}

#[tokio::test]
async fn restarting_sharing_restores_discovery() {
    // A fixed discovery port, so restarting sharing re-opens the advertisement where the client is
    // already asking.
    let advertiser = Arc::new(MdnsAdvertiser::on(vec![free_udp_port()]).with_ttl(ADVERTISED));
    let (server_runtime, _endpoint) =
        start_server(&["Zebra"], free_port(), Arc::clone(&advertiser)).await;
    let advertised_on = advertiser.bound_port().expect("the advertisement is open");
    let (client_runtime, discovery) = start_client(advertised_on).await;
    discovered(&discovery).await;

    server_runtime
        .stop(ServiceId::ServerSharing)
        .await
        .expect("sharing stops");
    undiscovered(&discovery).await;

    server_runtime
        .start(ServiceId::ServerSharing)
        .await
        .expect("sharing starts again");

    discovered(&discovery).await;
    client_runtime.shutdown().await.expect("the client stops");
    server_runtime
        .stop(ServiceId::ServerSharing)
        .await
        .expect("sharing stops");
    server_runtime.shutdown().await.expect("the server stops");
}

#[tokio::test]
async fn a_discovered_server_is_approved_the_same_way_as_a_manual_address() {
    let advertiser = Arc::new(MdnsAdvertiser::on(vec![0]).with_ttl(ADVERTISED));
    let (server_runtime, endpoint) =
        start_server(&["Zebra"], free_port(), Arc::clone(&advertiser)).await;
    let advertised_on = advertiser.bound_port().expect("the advertisement is open");
    let (client_runtime, discovery) = start_client(advertised_on).await;

    let server = discovered(&discovery).await;
    let directory = temporary_directory("discovery-trust");
    let connections = ClientConnections::new(&directory).expect("opens the saved server approvals");

    // Discovery found the server, and the user has approved nothing yet: no printer query is sent.
    let review = connections
        .inspect(server.address())
        .await
        .expect("inspects the discovered address");
    assert!(!review.trusted);
    assert!(review.previous_fingerprint.is_none());
    assert!(
        connections.printers(server.address()).await.is_err(),
        "a discovered server must not answer printer queries before approval"
    );

    connections
        .approve(server.address(), &review.current_fingerprint)
        .await
        .expect("approves the reviewed fingerprint");
    assert_eq!(
        connections
            .printers(server.address())
            .await
            .expect("lists the shared printers")
            .printers,
        vec!["Zebra"]
    );
    // The endpoint's own fingerprint is what the client pinned.
    assert_eq!(
        review.current_fingerprint,
        endpoint.fingerprint().to_string()
    );

    client_runtime.shutdown().await.expect("the client stops");
    server_runtime
        .stop(ServiceId::ServerSharing)
        .await
        .expect("sharing stops");
    server_runtime.shutdown().await.expect("the server stops");
    std::fs::remove_dir_all(&directory).ok();
}

#[tokio::test]
async fn a_manual_address_still_works_when_discovery_cannot_reach_the_server() {
    let advertiser = Arc::new(MdnsAdvertiser::on(vec![0]).with_ttl(ADVERTISED));
    let (server_runtime, endpoint) =
        start_server(&["Zebra"], free_port(), Arc::clone(&advertiser)).await;
    // The client asks a port on which no server answers, as it would on another subnet.
    let (client_runtime, discovery) = start_client(free_port()).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        discovery.nearby_servers().is_empty(),
        "discovery reached a server it should not have"
    );

    let directory = temporary_directory("discovery-manual");
    let connections = ClientConnections::new(&directory).expect("opens the saved server approvals");
    let address = format!("127.0.0.1:{}", endpoint.port());
    let review = connections
        .inspect(&address)
        .await
        .expect("reviews a manually entered address");
    connections
        .approve(&address, &review.current_fingerprint)
        .await
        .expect("approves it");
    assert_eq!(
        connections
            .printers(&address)
            .await
            .expect("lists the shared printers")
            .printers,
        vec!["Zebra"]
    );

    client_runtime.shutdown().await.expect("the client stops");
    server_runtime
        .stop(ServiceId::ServerSharing)
        .await
        .expect("sharing stops");
    server_runtime.shutdown().await.expect("the server stops");
    std::fs::remove_dir_all(&directory).ok();
}

#[tokio::test]
async fn a_stopped_server_tells_whoever_asked_and_answers_nobody_afterwards() {
    let advertiser = MdnsAdvertiser::on(vec![0]).with_ttl(Duration::from_secs(60));
    let advertisement = advertiser
        .advertise(8631, &printer_names(&["Zebra"]))
        .await
        .expect("advertises");
    let port = advertiser.bound_port().expect("the advertisement is open");
    let probe = tokio::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("binds a probe socket");

    assert!(
        ask(&probe, port).await.is_some(),
        "a live advertisement answers a browse query"
    );

    advertisement.withdraw().await.expect("withdraws");

    // A browsing client does not join the multicast group, so the withdrawal is sent back to the
    // sockets that asked. Its contents (a zero lifetime) are covered where the wire is.
    assert!(
        ask(&probe, port).await.is_some(),
        "the withdrawal is delivered to the browser that asked"
    );
    assert!(
        ask(&probe, port).await.is_none(),
        "a stopped server refuses to be discovered again"
    );
}

/// Sends one browse query and returns the answer, if any arrives.
async fn ask(socket: &tokio::net::UdpSocket, port: u16) -> Option<Vec<u8>> {
    let query = encode_query(SERVICE_TYPE).expect("encodes a browse query");
    socket
        .send_to(&query, SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
        .await
        .expect("sends the query");
    let mut buffer = vec![0u8; 2048];
    match tokio::time::timeout(Duration::from_millis(500), socket.recv_from(&mut buffer)).await {
        Ok(Ok((length, _))) => Some(buffer[..length].to_vec()),
        _ => None,
    }
}

#[tokio::test]
async fn server_advertises_its_version_and_client_discovers_it() {
    let port = free_port();
    let advertiser = Arc::new(MdnsAdvertiser::on(vec![0]).with_ttl(ADVERTISED));
    let (server_runtime, _endpoint) = start_server(&["Zebra"], port, Arc::clone(&advertiser)).await;
    let bound = advertiser.bound_port().expect("advertiser binds");

    let (client_runtime, discovery) = start_client(bound).await;
    let server = discovered(&discovery).await;

    assert_eq!(server.version(), Some(env!("CARGO_PKG_VERSION")));

    client_runtime.shutdown().await.expect("the client stops");
    server_runtime.shutdown().await.expect("the server stops");
}
