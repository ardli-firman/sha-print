//! The client half of discovery: ask the local network which servers share printers.
//!
//! The browser owns an ordinary socket and asks for the answer on it, instead of binding the
//! multicast port itself. That keeps discovery out of the way of the operating system's own
//! multicast DNS responder and means a client needs no inbound rule in the firewall: it only hears
//! answers to the questions it asked.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::net::UdpSocket;

use crate::adapters::discovery::{
    active_ipv4_interfaces, send_multicast_on_interfaces, send_to_all, system_ipv4_interfaces,
    system_multicast_interface_sender, wire, MulticastInterfaceSender, DISCOVERY_PORTS,
    MAX_DATAGRAM, MDNS_GROUP, MDNS_MULTICAST_TTL, NAME_PROPERTY, QUEUE_PROPERTY, READ_BACKOFF,
    VERSION_PROPERTY,
};
use crate::application::{AdvertisementSink, Browse, DiscoveryBrowser, Shutdown};
use crate::domain::{AppError, NearbyServer, PrinterName};

/// How often a browsing client asks the network what is there.
///
/// An advertisement is refreshed by asking, so this interval is also how quickly a server that
/// started after the client did becomes visible.
const QUERY_INTERVAL: Duration = Duration::from_secs(15);

/// Browses the local network for servers that share printers.
pub struct MdnsBrowser {
    targets: Vec<SocketAddr>,
    interval: Duration,
    interfaces: Option<Vec<Ipv4Addr>>,
    multicast_sender: Arc<dyn MulticastInterfaceSender>,
}

impl Default for MdnsBrowser {
    fn default() -> Self {
        Self::new()
    }
}

impl MdnsBrowser {
    /// Asks the multicast group on every port a ShaPrint responder may have taken.
    pub fn new() -> Self {
        Self {
            targets: DISCOVERY_PORTS
                .iter()
                .map(|port| SocketAddr::from((MDNS_GROUP, *port)))
                .collect(),
            interval: QUERY_INTERVAL,
            interfaces: None,
            multicast_sender: system_multicast_interface_sender(),
        }
    }

    /// Asks `targets` instead of the multicast group. Used by tests.
    pub fn targeting(targets: Vec<SocketAddr>) -> Self {
        Self {
            targets,
            ..Self::new()
        }
    }

    /// Overrides how often the network is asked. Used by tests.
    #[must_use]
    pub fn with_query_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    /// Overrides interface enumeration for a real multi-interface UDP test.
    #[must_use]
    pub fn with_interfaces(mut self, interfaces: Vec<Ipv4Addr>) -> Self {
        self.interfaces = Some(active_ipv4_interfaces(interfaces));
        self
    }

    /// Uses a multicast socket adapter for an observable UDP integration test.
    #[must_use]
    pub fn with_multicast_sender(mut self, sender: Arc<dyn MulticastInterfaceSender>) -> Self {
        self.multicast_sender = sender;
        self
    }
}

#[async_trait]
impl DiscoveryBrowser for MdnsBrowser {
    async fn open(&self) -> Result<Box<dyn Browse>, AppError> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))
            .await
            .map_err(|_| {
                AppError::internal(
                    "cannot open a socket to discover nearby servers on this network",
                )
            })?;
        if let Err(error) = self
            .multicast_sender
            .set_hop_limit(&socket, MDNS_MULTICAST_TTL)
        {
            log::debug!(
                "cannot set discovery multicast hop limit ttl={MDNS_MULTICAST_TTL} message={error}"
            );
        }
        if let Err(error) = socket.set_multicast_loop_v4(true) {
            log::debug!("cannot enable discovery multicast loopback message={error}");
        }
        Ok(Box::new(MdnsBrowse {
            socket,
            targets: self.targets.clone(),
            interfaces: self
                .interfaces
                .clone()
                .unwrap_or_else(system_ipv4_interfaces),
            multicast_sender: Arc::clone(&self.multicast_sender),
            multicast_lock: tokio::sync::Mutex::new(()),
            interval: self.interval,
        }))
    }
}

/// One open browse session.
struct MdnsBrowse {
    socket: UdpSocket,
    targets: Vec<SocketAddr>,
    interfaces: Vec<Ipv4Addr>,
    multicast_sender: Arc<dyn MulticastInterfaceSender>,
    multicast_lock: tokio::sync::Mutex<()>,
    interval: Duration,
}

#[async_trait]
impl Browse for MdnsBrowse {
    async fn run(
        &self,
        sink: Arc<dyn AdvertisementSink>,
        mut shutdown: Shutdown,
    ) -> Result<(), AppError> {
        let mut ticker = tokio::time::interval(self.interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut buffer = vec![0u8; MAX_DATAGRAM];

        loop {
            tokio::select! {
                () = shutdown.cancelled() => return Ok(()),
                _ = ticker.tick() => {
                    if let Err(error) = self.query().await {
                        log::debug!(
                            "cannot ask the network for nearby servers code={} message={}",
                            error.code_str(),
                            error
                        );
                    }
                }
                received = self.socket.recv_from(&mut buffer) => match received {
                    Ok((length, from)) => report(&buffer[..length], from, &sink),
                    Err(error) => {
                        log::debug!("cannot read a discovery answer message={error}");
                        tokio::time::sleep(READ_BACKOFF).await;
                    }
                },
            }
        }
    }
}

impl MdnsBrowse {
    /// Asks every target what it advertises.
    async fn query(&self) -> Result<(), AppError> {
        let packet = wire::encode_query(wire::SERVICE_TYPE)?;
        let (multicast_targets, direct_targets): (Vec<_>, Vec<_>) = self
            .targets
            .iter()
            .copied()
            .partition(|target| target.ip().is_multicast());
        let mut failures = 0;
        if !multicast_targets.is_empty()
            && send_multicast_on_interfaces(
                &self.socket,
                &*self.multicast_sender,
                &self.multicast_lock,
                &self.interfaces,
                &packet,
                &multicast_targets,
                "query",
            )
            .await
            .is_err()
        {
            failures += 1;
        }
        if !direct_targets.is_empty()
            && send_to_all(&self.socket, &packet, &direct_targets, "query")
                .await
                .is_err()
        {
            failures += 1;
        }
        if failures == 0 {
            Ok(())
        } else {
            Err(AppError::internal(format!(
                "the discovery query had {failures} independent send failure(s)"
            )))
        }
    }
}

/// Turns one received packet into what the cache should hold.
fn report(packet: &[u8], from: SocketAddr, sink: &Arc<dyn AdvertisementSink>) {
    let Ok(message) = wire::Message::parse(packet) else {
        // Anything else on the network is not ours to interpret; a malformed packet is ignored
        // rather than reported, because a client cannot act on it.
        return;
    };
    for service in wire::services(&message) {
        let Some(address) = endpoint_address(&service, from) else {
            continue;
        };
        // A zero lifetime is how a server says it stopped sharing (RFC 6762 §10.1).
        if service.ttl.is_zero() {
            sink.withdrawn(&address);
            continue;
        }
        if let Some(server) = nearby_server(&service, &address) {
            sink.advertised(server, service.ttl);
        }
    }
}

/// The address a user would review for this advertisement.
///
/// The answer's own source address is the one address this browser knows it can reach, so that is
/// what the user reviews; the advertised port is where the endpoint listens. Without a port there
/// is nothing to connect to, which is also what makes an unconfigured advertisement unusable.
fn endpoint_address(service: &wire::Service, from: SocketAddr) -> Option<String> {
    if service.port == 0 {
        return None;
    }
    Some(format!("{}:{}", from.ip(), service.port))
}

/// One advertised instance, as the client will offer it to a user.
fn nearby_server(service: &wire::Service, address: &str) -> Option<NearbyServer> {
    let mut label = None;
    let mut version = None;
    let mut printers: Vec<PrinterName> = Vec::new();
    for (key, value) in &service.properties {
        match key.as_str() {
            NAME_PROPERTY if label.is_none() => label = Some(value.clone()),
            VERSION_PROPERTY if version.is_none() => version = Some(value.clone()),
            QUEUE_PROPERTY => {
                if let Ok(name) = PrinterName::parse(value) {
                    if !printers.contains(&name) {
                        printers.push(name);
                    }
                }
            }
            _ => {}
        }
    }
    let label = label.unwrap_or_else(|| instance_label(&service.instance));
    NearbyServer::with_version(&label, address, printers, version).ok()
}

/// The first label of an instance name, used when an advertisement carries no server label.
fn instance_label(instance: &str) -> String {
    instance.split('.').next().unwrap_or_default().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service(properties: &[(&str, &str)]) -> wire::Service {
        wire::Service {
            instance: "DESKTOP-ABC._shaprint-ipps._tcp.local.".to_owned(),
            target: "DESKTOP-ABC.local.".to_owned(),
            port: 8631,
            properties: properties
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect(),
            ttl: Duration::from_secs(120),
        }
    }

    fn source(value: &str) -> SocketAddr {
        value.parse().expect("a socket address")
    }

    /// What the browser makes of one answer, the way `report` does.
    fn discovered(service: &wire::Service, from: &str) -> Option<NearbyServer> {
        let address = endpoint_address(service, source(from))?;
        nearby_server(service, &address)
    }

    /// Records what a browser reported, standing in for the cache.
    #[derive(Default)]
    struct Recording {
        advertised: Mutex<Vec<(String, Duration)>>,
        withdrawn: Mutex<Vec<String>>,
    }

    impl Recording {
        fn advertisements(&self) -> Vec<(String, Duration)> {
            self.advertised
                .lock()
                .map(|v| v.clone())
                .unwrap_or_default()
        }

        fn withdrawals(&self) -> Vec<String> {
            self.withdrawn.lock().map(|v| v.clone()).unwrap_or_default()
        }
    }

    impl AdvertisementSink for Recording {
        fn advertised(&self, server: NearbyServer, lifetime: Duration) {
            if let Ok(mut advertised) = self.advertised.lock() {
                advertised.push((server.address().to_owned(), lifetime));
            }
        }

        fn withdrawn(&self, address: &str) {
            if let Ok(mut withdrawn) = self.withdrawn.lock() {
                withdrawn.push(address.to_owned());
            }
        }
    }

    #[test]
    fn an_answer_becomes_a_nearby_server_at_the_address_it_came_from() {
        let server = discovered(
            &service(&[
                ("rp", "ipp/print"),
                ("name", "DESKTOP-ABC"),
                ("queue", "Zebra"),
                ("queue", "Canon"),
            ]),
            "192.0.2.10:5353",
        )
        .expect("a usable advertisement");

        assert_eq!(server.name(), "DESKTOP-ABC");
        // The address is the one the answer actually arrived from, and the port is the endpoint
        // the advertisement named: a responder can be heard on an interface the browser can reach.
        assert_eq!(server.address(), "192.0.2.10:8631");
        assert_eq!(
            server
                .printers()
                .iter()
                .map(PrinterName::as_str)
                .collect::<Vec<_>>(),
            vec!["Zebra", "Canon"]
        );
    }

    use std::sync::Mutex;

    #[test]
    fn an_answer_without_a_label_falls_back_to_the_instance_name() {
        let server = discovered(&service(&[("rp", "ipp/print")]), "192.0.2.10:5353")
            .expect("a usable advertisement");

        assert_eq!(server.name(), "DESKTOP-ABC");
    }

    #[test]
    fn a_queue_that_cannot_be_used_is_left_out_without_losing_the_server() {
        let server = nearby_server(
            &service(&[
                ("name", "DESKTOP-ABC"),
                ("queue", "Zebra"),
                ("queue", "Bad\u{7}Queue"),
            ]),
            "192.0.2.10:5353",
        )
        .expect("a usable advertisement");

        assert_eq!(
            server
                .printers()
                .iter()
                .map(PrinterName::as_str)
                .collect::<Vec<_>>(),
            vec!["Zebra"]
        );
    }

    #[test]
    fn a_repeated_queue_is_only_offered_once() {
        let server = nearby_server(
            &service(&[
                ("name", "DESKTOP-ABC"),
                ("queue", "Zebra"),
                ("queue", "Zebra"),
            ]),
            "192.0.2.10:5353",
        )
        .expect("a usable advertisement");

        assert_eq!(server.printers().len(), 1);
    }

    #[test]
    fn an_answer_without_a_usable_port_is_ignored() {
        let mut service = service(&[("name", "DESKTOP-ABC"), ("queue", "Zebra")]);
        service.port = 0;

        assert!(discovered(&service, "192.0.2.10:5353").is_none());
    }

    #[test]
    fn a_goodbye_withdraws_the_address_the_answer_came_from() {
        let properties = [("name", "DESKTOP-ABC"), ("queue", "Zebra")];
        let goodbye = wire::encode_announcement(
            "DESKTOP-ABC",
            "DESKTOP-ABC.local.",
            8631,
            &properties
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect::<Vec<_>>(),
            Duration::ZERO,
        )
        .expect("encodes a goodbye");
        let recording = Arc::new(Recording::default());
        let sink: Arc<dyn AdvertisementSink> = Arc::clone(&recording) as Arc<dyn AdvertisementSink>;

        report(&goodbye, source("192.0.2.10:5353"), &sink);

        assert_eq!(recording.withdrawals(), vec!["192.0.2.10:8631"]);
        assert!(recording.advertisements().is_empty());
    }

    #[test]
    fn a_live_answer_is_reported_for_the_address_it_came_from() {
        let properties = [("name", "DESKTOP-ABC"), ("queue", "Zebra")];
        let announcement = wire::encode_announcement(
            "DESKTOP-ABC",
            "DESKTOP-ABC.local.",
            8631,
            &properties
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect::<Vec<_>>(),
            Duration::from_secs(120),
        )
        .expect("encodes an announcement");
        let recording = Arc::new(Recording::default());
        let sink: Arc<dyn AdvertisementSink> = Arc::clone(&recording) as Arc<dyn AdvertisementSink>;

        report(&announcement, source("192.0.2.10:5353"), &sink);

        assert_eq!(
            recording.advertisements(),
            vec![("192.0.2.10:8631".to_owned(), Duration::from_secs(120))]
        );
        assert!(recording.withdrawals().is_empty());
    }

    #[test]
    fn an_answer_with_version_exposes_the_advertised_version() {
        let server = discovered(
            &service(&[
                ("rp", "ipp/print"),
                ("name", "DESKTOP-ABC"),
                ("v", "3.0.0"),
                ("queue", "Zebra"),
            ]),
            "192.0.2.10:5353",
        )
        .expect("a usable advertisement");

        assert_eq!(server.version(), Some("3.0.0"));
    }

    #[test]
    fn an_older_server_answer_without_version_succeeds_cleanly_with_none() {
        let server = discovered(
            &service(&[
                ("rp", "ipp/print"),
                ("name", "DESKTOP-LEGACY"),
                ("queue", "LaserJet"),
            ]),
            "192.0.2.10:5353",
        )
        .expect("a usable legacy advertisement");

        assert_eq!(server.version(), None);
    }
}
