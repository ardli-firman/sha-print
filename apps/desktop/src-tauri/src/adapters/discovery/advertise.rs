//! The server half of discovery: tell the local network which printers this server shares.
//!
//! The advertisement is a DNS-SD service instance carrying one `queue` property per shared printer,
//! so a browsing client can show what a server shares before it trusts it. Answering is deliberate:
//! a browser asks, this responder answers, and a stopped server stops answering — and tells the
//! browsers that asked recently, because a browsing client does not join the multicast group and
//! would otherwise keep a stopped server in its list until the advertisement aged out.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use async_trait::async_trait;
use tokio::net::UdpSocket;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::adapters::discovery::{
    active_ipv4_interfaces, send_multicast_on_interfaces, send_to_all, system_ipv4_interfaces,
    system_multicast_interface_sender, wire, MulticastInterfaceSender, DISCOVERY_PORTS,
    MAX_DATAGRAM, MDNS_GROUP, MDNS_MULTICAST_TTL, NAME_PROPERTY, PATH_PROPERTY, QUEUE_PROPERTY,
    READ_BACKOFF, RESOURCE_PATH, VERSION_PROPERTY,
};
use crate::adapters::port_binding::PortBinder;
use crate::application::{Advertisement, ServerAdvertiser};
use crate::domain::{AppError, PrinterName};

/// How long a browser may keep an advertisement it has already resolved.
pub const ADVERTISED_TTL: Duration = Duration::from_secs(120);

/// How many printers one advertisement may carry before it is trimmed to fit the network.
pub const MAX_ADVERTISED_PRINTERS: usize = 48;

/// Largest advertisement this advertiser puts on the network.
///
/// Multicast DNS allows larger datagrams, but staying inside a typical 1500-byte path maximum
/// transmission unit avoids IP fragmentation, which loses the whole advertisement when a single
/// fragment is dropped.
pub const MAX_PACKET: usize = 1400;

/// How many asking browsers a server remembers, so it can withdraw its advertisement directly.
const MAX_REMEMBERED_QUERIERS: usize = 32;

/// What one server currently advertises.
#[derive(Debug, Clone)]
struct Advertised {
    label: String,
    target: String,
    port: u16,
    printers: Vec<PrinterName>,
}

impl Advertised {
    /// The advertisement to send, trimmed to the printers that fit in one datagram.
    ///
    /// A browser that cannot see the last few printers still lists the server, and the endpoint
    /// returns the full, authoritative list once the user approves it.
    fn encode(&self, ttl: Duration) -> Result<Vec<u8>, AppError> {
        let mut printers = self.printers.clone();
        printers.truncate(MAX_ADVERTISED_PRINTERS);
        loop {
            let packet = wire::encode_announcement(
                &self.label,
                &self.target,
                self.port,
                &properties(&self.label, &printers),
                ttl,
            )?;
            if packet.len() <= MAX_PACKET || printers.is_empty() {
                return Ok(packet);
            }
            printers.pop();
        }
    }
}

/// The `TXT` properties a browser reads out of an advertisement.
fn properties(label: &str, printers: &[PrinterName]) -> Vec<(String, String)> {
    let mut properties = vec![
        (PATH_PROPERTY.to_owned(), RESOURCE_PATH.to_owned()),
        (NAME_PROPERTY.to_owned(), label.to_owned()),
        (
            VERSION_PROPERTY.to_owned(),
            env!("CARGO_PKG_VERSION").to_owned(),
        ),
    ];
    properties.extend(
        printers
            .iter()
            .map(|printer| (QUEUE_PROPERTY.to_owned(), printer.as_str().to_owned())),
    );
    properties
}

/// Advertises this server's shared printers over multicast DNS.
pub struct MdnsAdvertiser {
    ports: Vec<u16>,
    ttl: Duration,
    binder: PortBinder,
    interfaces: Option<Vec<Ipv4Addr>>,
    multicast_sender: Arc<dyn MulticastInterfaceSender>,
    bound_port: Mutex<Option<u16>>,
}

impl Default for MdnsAdvertiser {
    fn default() -> Self {
        Self::new()
    }
}

impl MdnsAdvertiser {
    /// Advertises on the local network, on the first discovery port this machine can take.
    pub fn new() -> Self {
        Self::on_with_binder(DISCOVERY_PORTS.to_vec(), PortBinder::system())
    }

    /// Advertises on one specific port.
    ///
    /// Used by tests, which cannot take the multicast port from a machine's own resolver, and by
    /// any later phase that pins discovery to one interface.
    pub fn on(ports: Vec<u16>) -> Self {
        Self::on_with_binder(ports, PortBinder::system())
    }

    /// Uses selected ports and an injected binder for lifecycle tests.
    pub fn on_with_binder(ports: Vec<u16>, binder: PortBinder) -> Self {
        Self {
            ports,
            ttl: ADVERTISED_TTL,
            binder,
            interfaces: None,
            multicast_sender: system_multicast_interface_sender(),
            bound_port: Mutex::new(None),
        }
    }

    /// Overrides how long a browser may keep an advertisement. Used by tests.
    #[must_use]
    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
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

    /// The port the current advertisement answers on, once there is one.
    pub fn bound_port(&self) -> Option<u16> {
        self.bound_port.lock().ok().and_then(|port| *port)
    }

    /// Takes the first discovery port that can be opened.
    ///
    /// Membership of the multicast group is best effort: a machine without multicast can still
    /// answer a query sent straight to its port, and printer sharing must not depend on discovery.
    async fn bind(&self) -> Result<(Arc<UdpSocket>, Arc<Vec<Ipv4Addr>>), AppError> {
        let interfaces = self
            .interfaces
            .clone()
            .unwrap_or_else(system_ipv4_interfaces);
        for port in &self.ports {
            let socket = match self
                .binder
                .bind_udp(SocketAddr::from((Ipv4Addr::UNSPECIFIED, *port)))
                .await
            {
                Ok(socket) => socket,
                Err(error) => {
                    log::debug!("cannot listen for discovery queries port={port} message={error}");
                    if error.message().contains(" in use")
                        || error
                            .message()
                            .contains("cannot inspect the process using UDP port")
                        || error.message().contains("holding UDP port")
                        || error.message().contains("did not release UDP port")
                    {
                        return Err(error);
                    }
                    continue;
                }
            };
            let mut membership_failures = 0;
            for interface in &interfaces {
                if let Err(error) = self
                    .multicast_sender
                    .join_group(&socket, MDNS_GROUP, *interface)
                {
                    membership_failures += 1;
                    log::debug!(
                        "cannot join the discovery group interface={interface} message={error}"
                    );
                }
            }
            if membership_failures > 0 {
                let joined_interfaces = interfaces.len().saturating_sub(membership_failures);
                log::warn!(
                    "discovery multicast group joined on {joined_interfaces}/{} interfaces",
                    interfaces.len()
                );
            }
            if let Err(error) = socket.set_multicast_loop_v4(true) {
                log::debug!("cannot enable discovery loopback message={error}");
            }
            if let Err(error) = self
                .multicast_sender
                .set_hop_limit(&socket, MDNS_MULTICAST_TTL)
            {
                log::debug!("cannot set discovery multicast hop limit ttl={MDNS_MULTICAST_TTL} message={error}");
            }
            let bound = socket
                .local_addr()
                .map_err(|_| AppError::internal("cannot read the discovery socket address"))?;
            if let Ok(mut current) = self.bound_port.lock() {
                *current = Some(bound.port());
            }
            log::info!("discovery advertisement listening port={}", bound.port());
            return Ok((Arc::new(socket), Arc::new(interfaces)));
        }
        Err(AppError::internal(
            "no discovery port could be opened; another multicast DNS responder may be holding them",
        ))
    }
}

#[async_trait]
impl ServerAdvertiser for MdnsAdvertiser {
    async fn advertise(
        &self,
        port: u16,
        printers: &[PrinterName],
    ) -> Result<Arc<dyn Advertisement>, AppError> {
        let (socket, interfaces) = self.bind().await?;
        let multicast_lock = Arc::new(tokio::sync::Mutex::new(()));
        let label = server_label();
        let advertised = Arc::new(RwLock::new(Advertised {
            target: format!("{label}.local."),
            label,
            port,
            printers: printers.to_vec(),
        }));
        let queriers = Arc::new(Mutex::new(Vec::new()));

        let (stop, stopped) = watch::channel(false);
        let responder = tokio::spawn(
            Responder {
                socket: Arc::clone(&socket),
                interfaces: Arc::clone(&interfaces),
                multicast_sender: Arc::clone(&self.multicast_sender),
                multicast_lock: Arc::clone(&multicast_lock),
                advertised: Arc::clone(&advertised),
                queriers: Arc::clone(&queriers),
                bound: socket
                    .local_addr()
                    .map_err(|_| AppError::internal("cannot read the discovery socket address"))?
                    .port(),
                ttl: self.ttl,
                group: MDNS_GROUP,
            }
            .run(stopped),
        );

        let advertisement = MdnsAdvertisement {
            advertised,
            queriers,
            socket,
            interfaces,
            multicast_sender: Arc::clone(&self.multicast_sender),
            multicast_lock,
            ttl: self.ttl,
            stop,
            responder: Mutex::new(Some(responder)),
        };
        // Announcing is best effort: an advertisement that cannot be multicast still answers
        // whoever asks, so a network without multicast support does not lose discovery entirely.
        if let Err(error) = advertisement.announce(self.ttl, "announcement").await {
            log::warn!(
                "cannot announce shared printers code={} message={}",
                error.code_str(),
                error
            );
        }
        Ok(Arc::new(advertisement))
    }
}

/// One live advertisement and the task answering queries for it.
struct MdnsAdvertisement {
    advertised: Arc<RwLock<Advertised>>,
    /// The browsers that asked recently, so a withdrawal reaches them directly.
    queriers: Arc<Mutex<Vec<SocketAddr>>>,
    socket: Arc<UdpSocket>,
    interfaces: Arc<Vec<Ipv4Addr>>,
    multicast_sender: Arc<dyn MulticastInterfaceSender>,
    multicast_lock: Arc<tokio::sync::Mutex<()>>,
    ttl: Duration,
    stop: watch::Sender<bool>,
    responder: Mutex<Option<JoinHandle<()>>>,
}

impl MdnsAdvertisement {
    /// Sends the current advertisement to the group and to the browsers that asked recently.
    async fn announce(&self, ttl: Duration, what: &str) -> Result<(), AppError> {
        let packet = {
            let advertised = self
                .advertised
                .read()
                .map_err(|_| AppError::internal("the advertisement lock is poisoned"))?;
            advertised.encode(ttl)?
        };
        let multicast_destination = SocketAddr::from((MDNS_GROUP, self.bound_port()));
        let multicast_result = send_multicast_on_interfaces(
            &self.socket,
            &*self.multicast_sender,
            &self.multicast_lock,
            &self.interfaces,
            &packet,
            &[multicast_destination],
            what,
        )
        .await;
        let queriers = self
            .queriers
            .lock()
            .map(|queriers| queriers.clone())
            .unwrap_or_default();
        let querier_result = send_to_all(&self.socket, &packet, &queriers, what).await;
        let failures =
            usize::from(multicast_result.is_err()) + usize::from(querier_result.is_err());
        if failures == 0 {
            Ok(())
        } else {
            Err(AppError::internal(format!(
                "the discovery {what} had {failures} independent send failure(s)"
            )))
        }
    }

    fn bound_port(&self) -> u16 {
        self.socket
            .local_addr()
            .map(|address| address.port())
            .unwrap_or(DISCOVERY_PORTS[0])
    }
}

#[async_trait]
impl Advertisement for MdnsAdvertisement {
    async fn replace(&self, printers: &[PrinterName]) -> Result<(), AppError> {
        {
            let mut advertised = self
                .advertised
                .write()
                .map_err(|_| AppError::internal("the advertisement lock is poisoned"))?;
            advertised.printers = printers.to_vec();
        }
        if printers.is_empty() {
            // Nothing is shared any more, so the network is told to forget the advertisement
            // instead of letting it age out.
            return self.announce(Duration::ZERO, "withdrawal").await;
        }
        self.announce(self.ttl, "announcement").await
    }

    async fn withdraw(&self) -> Result<(), AppError> {
        {
            let mut advertised = self
                .advertised
                .write()
                .map_err(|_| AppError::internal("the advertisement lock is poisoned"))?;
            advertised.printers.clear();
        }
        let announcement = self.announce(Duration::ZERO, "withdrawal").await;
        let _ = self.stop.send_replace(true);
        let responder = self.responder.lock().ok().and_then(|mut task| task.take());
        if let Some(responder) = responder {
            let _ = responder.await;
        }
        announcement
    }
}

/// Answers browse queries while one advertisement is live.
struct Responder {
    socket: Arc<UdpSocket>,
    interfaces: Arc<Vec<Ipv4Addr>>,
    multicast_sender: Arc<dyn MulticastInterfaceSender>,
    multicast_lock: Arc<tokio::sync::Mutex<()>>,
    advertised: Arc<RwLock<Advertised>>,
    queriers: Arc<Mutex<Vec<SocketAddr>>>,
    group: Ipv4Addr,
    bound: u16,
    /// The lifetime every answer carries, so a browser caches it for the same time the server means.
    ttl: Duration,
}

impl Responder {
    async fn run(self, stop: watch::Receiver<bool>) {
        let mut buffer = vec![0u8; MAX_DATAGRAM];
        loop {
            tokio::select! {
                () = cancelled(stop.clone()) => return,
                received = self.socket.recv_from(&mut buffer) => match received {
                    Ok((length, from)) => self.answer(&buffer[..length], from).await,
                    Err(error) => {
                        log::debug!("cannot read a discovery query message={error}");
                        tokio::time::sleep(READ_BACKOFF).await;
                    }
                },
            }
        }
    }

    async fn answer(&self, packet: &[u8], from: SocketAddr) {
        let Ok(message) = wire::Message::parse(packet) else {
            return;
        };
        let asked = message
            .queries
            .iter()
            .filter(|query| wire::queries_service(query, wire::SERVICE_TYPE))
            .collect::<Vec<_>>();
        if asked.is_empty() {
            return;
        }
        self.remember(from);
        let encoded = {
            let Ok(advertised) = self.advertised.read() else {
                return;
            };
            if advertised.printers.is_empty() {
                // Withdrawn: a stopped server refuses to be discovered again.
                return;
            }
            advertised.encode(self.ttl)
        };
        let Ok(reply) = encoded else {
            return;
        };
        // A browser asks for the answer on its own socket, because it is not listening on the
        // multicast port at all (RFC 6762 §5.4).
        if asked.iter().any(|query| query.unicast_response) {
            if let Err(error) = self.socket.send_to(&reply, from).await {
                log::debug!("cannot answer a discovery query from={from} message={error}");
            }
        } else {
            let destination = SocketAddr::from((self.group, self.bound));
            if let Err(error) = send_multicast_on_interfaces(
                &self.socket,
                &*self.multicast_sender,
                &self.multicast_lock,
                &self.interfaces,
                &reply,
                &[destination],
                "answer",
            )
            .await
            {
                log::debug!("cannot multicast a discovery answer message={error}");
            }
        }
    }

    /// Remembers a browser that asked, newest first and bounded.
    fn remember(&self, from: SocketAddr) {
        if let Ok(mut queriers) = self.queriers.lock() {
            queriers.retain(|querier| *querier != from);
            queriers.insert(0, from);
            queriers.truncate(MAX_REMEMBERED_QUERIERS);
        }
    }
}

/// Resolves once the advertisement has been withdrawn, or the sender is gone.
async fn cancelled(mut stop: watch::Receiver<bool>) {
    loop {
        if *stop.borrow_and_update() {
            return;
        }
        if stop.changed().await.is_err() {
            return;
        }
    }
}

/// The name this computer is known by on the network.
///
/// Windows sets `COMPUTERNAME` for every process; the fallback keeps discovery working where it is
/// not set, and a name that is not a usable DNS label falls back to a fixed one.
fn server_label() -> String {
    let name = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_default();
    usable_label(&name)
}

/// Trims a platform name down to what a DNS label can carry, so an advertised name is always one a
/// browser can show and match.
fn usable_label(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | ' ') {
                character
            } else {
                '-'
            }
        })
        .collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        return "ShaPrint server".to_owned();
    }
    cleaned.chars().take(63).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(values: &[&str]) -> Vec<PrinterName> {
        values
            .iter()
            .map(|value| PrinterName::parse(value).expect("valid printer name"))
            .collect()
    }

    fn advertised(printers: Vec<PrinterName>) -> Advertised {
        Advertised {
            label: "DESKTOP-ABC".to_owned(),
            target: "DESKTOP-ABC.local.".to_owned(),
            port: 8631,
            printers,
        }
    }

    fn carried_queues(packet: &[u8]) -> Vec<String> {
        let message = wire::Message::parse(packet).expect("parses the advertisement");
        wire::services(&message)
            .into_iter()
            .flat_map(|service| service.properties)
            .filter(|(key, _)| key == QUEUE_PROPERTY)
            .map(|(_, value)| value)
            .collect()
    }

    #[test]
    fn an_advertisement_names_the_server_and_every_shared_queue() {
        let packet = advertised(names(&["Zebra", "HP LaserJet"]))
            .encode(ADVERTISED_TTL)
            .expect("encodes an advertisement");

        let message = wire::Message::parse(&packet).expect("parses");
        let services = wire::services(&message);
        assert_eq!(services.len(), 1);
        assert_eq!(
            services[0].instance,
            "DESKTOP-ABC._shaprint-ipps._tcp.local."
        );
        assert_eq!(services[0].target, "DESKTOP-ABC.local.");
        assert_eq!(services[0].port, 8631);
        assert_eq!(services[0].ttl, ADVERTISED_TTL);
        assert_eq!(
            services[0].properties,
            vec![
                ("rp".to_owned(), "ipp/print".to_owned()),
                ("name".to_owned(), "DESKTOP-ABC".to_owned()),
                ("v".to_owned(), env!("CARGO_PKG_VERSION").to_owned()),
                ("queue".to_owned(), "Zebra".to_owned()),
                ("queue".to_owned(), "HP LaserJet".to_owned()),
            ]
        );
    }

    #[test]
    fn a_withdrawn_advertisement_says_it_is_gone_and_carries_no_queues() {
        let mut advertised = advertised(names(&["Zebra"]));
        advertised.printers.clear();

        let packet = advertised
            .encode(Duration::ZERO)
            .expect("encodes a goodbye");

        let message = wire::Message::parse(&packet).expect("parses");
        let services = wire::services(&message);
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].ttl, Duration::ZERO);
        assert!(carried_queues(&packet).is_empty());
    }

    #[test]
    fn an_advertisement_too_large_for_the_network_carries_the_queues_that_fit() {
        let queues: Vec<PrinterName> = (0..MAX_ADVERTISED_PRINTERS)
            .map(|index| {
                PrinterName::parse(&format!("Very long printer queue name number {index:02}"))
                    .expect("valid printer name")
            })
            .collect();

        let packet = advertised(queues)
            .encode(ADVERTISED_TTL)
            .expect("encodes an advertisement");

        assert!(packet.len() <= MAX_PACKET, "{} bytes", packet.len());
        let carried = carried_queues(&packet);
        assert!(!carried.is_empty(), "the advertisement carries nothing");
        assert!(
            carried.len() < MAX_ADVERTISED_PRINTERS,
            "the advertisement was not trimmed"
        );
        // The queues that fit are the first ones, so the advertisement matches the selection order.
        assert_eq!(carried[0], "Very long printer queue name number 00");
        assert!(carried.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn a_platform_name_is_trimmed_to_something_a_browser_can_show() {
        assert_eq!(usable_label("DESKTOP-ABC"), "DESKTOP-ABC");
        assert_eq!(usable_label("  DESKTOP-ABC  "), "DESKTOP-ABC");
        // A name the platform allows but a DNS label does not is not advertised verbatim.
        assert_eq!(usable_label("desk.top"), "desk-top");
        assert_eq!(usable_label(""), "ShaPrint server");
        assert_eq!(usable_label(&"A".repeat(100)).len(), 63);
    }
}
