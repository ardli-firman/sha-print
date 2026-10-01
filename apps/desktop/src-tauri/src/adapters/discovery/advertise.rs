//! The server half of discovery: tell the local network which queues this server shares.
//!
//! The advertisement is a DNS-SD service instance carrying one `queue` property per shared queue,
//! so a browsing client can show what a server shares before it trusts it. Answering is deliberate:
//! a browser asks, this responder answers, and a stopped server simply stops answering — which is
//! what makes "stopping sharing also stops discovery" true even when a goodbye is lost.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use async_trait::async_trait;
use tokio::net::UdpSocket;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::adapters::discovery::{
    wire, DISCOVERY_PORTS, MAX_DATAGRAM, MDNS_GROUP, NAME_PROPERTY, PATH_PROPERTY, QUEUE_PROPERTY,
    RESOURCE_PATH,
};
use crate::application::{Advertisement, ServerAdvertiser};
use crate::domain::{AppError, PrinterName};

/// How long a browser may keep an advertisement it has already resolved.
pub const ADVERTISED_TTL: Duration = Duration::from_secs(120);

/// How many queues one advertisement may carry before it is trimmed to fit the network.
pub const MAX_ADVERTISED_QUEUES: usize = 48;

/// Largest advertisement this advertiser puts on the network.
///
/// Multicast DNS allows larger datagrams, but staying inside a typical 1500-byte path maximum
/// transmission unit avoids IP fragmentation, which loses the whole advertisement when a single
/// fragment is dropped.
pub const MAX_PACKET: usize = 1400;

/// How long a failed read waits before reading again, so an unreachable peer cannot spin the loop.
const READ_BACKOFF: Duration = Duration::from_millis(50);

/// What one server currently advertises.
#[derive(Debug, Clone)]
struct Advertised {
    label: String,
    target: String,
    port: u16,
    queues: Vec<PrinterName>,
}

impl Advertised {
    /// The advertisement to send, trimmed to the queues that fit in one datagram.
    ///
    /// A browser that cannot see the last few queues still lists the server, and the endpoint
    /// returns the full, authoritative list once the user approves it.
    fn encode(&self, ttl: Duration) -> Result<Vec<u8>, AppError> {
        let mut queues = self.queues.clone();
        queues.truncate(MAX_ADVERTISED_QUEUES);
        loop {
            let packet = wire::encode_announcement(
                &self.label,
                &self.target,
                self.port,
                &properties(&self.label, &queues),
                ttl,
            )?;
            if packet.len() <= MAX_PACKET || queues.is_empty() {
                return Ok(packet);
            }
            queues.pop();
        }
    }
}

/// The `TXT` properties a browser reads out of an advertisement.
fn properties(label: &str, queues: &[PrinterName]) -> Vec<(String, String)> {
    let mut properties = vec![
        (PATH_PROPERTY.to_owned(), RESOURCE_PATH.to_owned()),
        (NAME_PROPERTY.to_owned(), label.to_owned()),
    ];
    properties.extend(
        queues
            .iter()
            .map(|queue| (QUEUE_PROPERTY.to_owned(), queue.as_str().to_owned())),
    );
    properties
}

/// Advertises this server's shared queues over multicast DNS.
///
/// Multicast DNS messages go to the group every responder listens on (RFC 6762 §3).
pub struct MdnsAdvertiser {
    ports: Vec<u16>,
    ttl: Duration,
    /// Where announcements and goodbyes are sent; `None` means the multicast group.
    announce_to: Option<Vec<SocketAddr>>,
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
        Self {
            ports: DISCOVERY_PORTS.to_vec(),
            ttl: ADVERTISED_TTL,
            announce_to: None,
            bound_port: Mutex::new(None),
        }
    }

    /// Advertises on one specific port.
    ///
    /// Used by tests, which cannot take the multicast port from a machine's own resolver, and by
    /// any later phase that pins discovery to one interface.
    pub fn on(ports: Vec<u16>) -> Self {
        Self {
            ports,
            ..Self::new()
        }
    }

    /// Overrides how long a browser may keep an advertisement. Used by tests.
    #[must_use]
    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }

    /// Sends announcements and goodbyes to `targets` instead of the multicast group.
    ///
    /// Used by tests to observe the wire without depending on multicast support.
    #[must_use]
    pub fn announcing_to(mut self, targets: Vec<SocketAddr>) -> Self {
        self.announce_to = Some(targets);
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
    async fn bind(&self) -> Result<Arc<UdpSocket>, AppError> {
        for port in &self.ports {
            let socket = match UdpSocket::bind((Ipv4Addr::UNSPECIFIED, *port)).await {
                Ok(socket) => socket,
                Err(error) => {
                    log::debug!("cannot listen for discovery queries port={port} message={error}");
                    continue;
                }
            };
            // Without group membership a browser on another computer cannot reach this responder
            // at all, because it sends its query to the group. This is worth a warning, while the
            // advertisement still answers a query sent straight to its port.
            if let Err(error) = socket.join_multicast_v4(MDNS_GROUP, Ipv4Addr::UNSPECIFIED) {
                log::warn!("cannot join the discovery group, so other computers cannot find this server message={error}");
            }
            if let Err(error) = socket.set_multicast_loop_v4(true) {
                log::debug!("cannot enable discovery loopback message={error}");
            }
            if let Err(error) = socket.set_multicast_ttl_v4(1) {
                log::debug!("cannot set the discovery hop limit message={error}");
            }
            let bound = socket
                .local_addr()
                .map_err(|_| AppError::internal("cannot read the discovery socket address"))?;
            if let Ok(mut current) = self.bound_port.lock() {
                *current = Some(bound.port());
            }
            log::info!("discovery advertisement listening port={}", bound.port());
            return Ok(Arc::new(socket));
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
        queues: &[PrinterName],
    ) -> Result<Arc<dyn Advertisement>, AppError> {
        let socket = self.bind().await?;
        let label = server_label();
        let advertised = Arc::new(RwLock::new(Advertised {
            target: format!("{label}.local."),
            label,
            port,
            queues: queues.to_vec(),
        }));

        let (stop, stopped) = watch::channel(false);
        let responder = tokio::spawn(
            Responder {
                socket: Arc::clone(&socket),
                advertised: Arc::clone(&advertised),
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
            socket,
            announce_to: self.announce_to.clone(),
            ttl: self.ttl,
            stop,
            responder: Mutex::new(Some(responder)),
        };
        // Announcing is best effort: an advertisement that cannot be multicast still answers
        // whoever asks, so a network without multicast support does not lose discovery entirely.
        if let Err(error) = advertisement.announce(self.ttl).await {
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
    socket: Arc<UdpSocket>,
    announce_to: Option<Vec<SocketAddr>>,
    ttl: Duration,
    stop: watch::Sender<bool>,
    responder: Mutex<Option<JoinHandle<()>>>,
}

impl MdnsAdvertisement {
    /// Sends the current advertisement to every destination it has.
    async fn announce(&self, ttl: Duration) -> Result<(), AppError> {
        let packet = {
            let advertised = self
                .advertised
                .read()
                .map_err(|_| AppError::internal("the advertisement lock is poisoned"))?;
            advertised.encode(ttl)?
        };
        let destinations = match &self.announce_to {
            Some(targets) => targets.clone(),
            None => vec![SocketAddr::from((MDNS_GROUP, self.bound_port()))],
        };
        let mut failure = None;
        for destination in destinations {
            if let Err(error) = self.socket.send_to(&packet, destination).await {
                log::debug!("cannot send a discovery advertisement message={error}");
                failure = Some(AppError::internal(
                    "the discovery advertisement could not be sent on this network",
                ));
            }
        }
        match failure {
            Some(error) => Err(error),
            None => Ok(()),
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
    async fn replace(&self, queues: &[PrinterName]) -> Result<(), AppError> {
        {
            let mut advertised = self
                .advertised
                .write()
                .map_err(|_| AppError::internal("the advertisement lock is poisoned"))?;
            advertised.queues = queues.to_vec();
        }
        if queues.is_empty() {
            // Nothing is shared any more, so the network is told to forget the advertisement
            // instead of letting it age out.
            return self.announce(Duration::ZERO).await;
        }
        self.announce(self.ttl).await
    }

    async fn withdraw(&self) -> Result<(), AppError> {
        {
            let mut advertised = self
                .advertised
                .write()
                .map_err(|_| AppError::internal("the advertisement lock is poisoned"))?;
            advertised.queues.clear();
        }
        let announcement = self.announce(Duration::ZERO).await;
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
    advertised: Arc<RwLock<Advertised>>,
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
        let encoded = {
            let Ok(advertised) = self.advertised.read() else {
                return;
            };
            if advertised.queues.is_empty() {
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
        let destination = if asked.iter().any(|query| query.unicast_response) {
            from
        } else {
            SocketAddr::from((self.group, self.bound))
        };
        if let Err(error) = self.socket.send_to(&reply, destination).await {
            log::debug!("cannot answer a discovery query message={error}");
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

    fn advertised(queues: Vec<PrinterName>) -> Advertised {
        Advertised {
            label: "DESKTOP-ABC".to_owned(),
            target: "DESKTOP-ABC.local.".to_owned(),
            port: 8631,
            queues,
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
                ("queue".to_owned(), "Zebra".to_owned()),
                ("queue".to_owned(), "HP LaserJet".to_owned()),
            ]
        );
    }

    #[test]
    fn a_withdrawn_advertisement_says_it_is_gone_and_carries_no_queues() {
        let mut advertised = advertised(names(&["Zebra"]));
        advertised.queues.clear();

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
        let queues: Vec<PrinterName> = (0..MAX_ADVERTISED_QUEUES)
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
            carried.len() < MAX_ADVERTISED_QUEUES,
            "the advertisement was not trimmed"
        );
        // The queues that fit are the first ones, so the advertisement matches the selection order.
        assert_eq!(carried[0], "Very long printer queue name number 00");
        assert_eq!(carried.len(), carried.len());
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
