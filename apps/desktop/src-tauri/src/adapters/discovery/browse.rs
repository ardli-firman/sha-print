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
    wire, DISCOVERY_PORTS, MAX_DATAGRAM, MDNS_GROUP, NAME_PROPERTY, QUEUE_PROPERTY,
};
use crate::application::{AdvertisementSink, Browse, DiscoveryBrowser, Shutdown};
use crate::domain::{AppError, NearbyServer, PrinterName};

/// How often a browsing client asks the network what is there.
///
/// An advertisement is refreshed by asking, so this interval is also how quickly a server that
/// started after the client did becomes visible.
const QUERY_INTERVAL: Duration = Duration::from_secs(15);

/// How long a failed read waits before reading again, so a rejected packet cannot spin the loop.
const READ_BACKOFF: Duration = Duration::from_millis(50);

/// Browses the local network for servers that share printers.
pub struct MdnsBrowser {
    targets: Vec<SocketAddr>,
    interval: Duration,
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
        Ok(Box::new(MdnsBrowse {
            socket,
            targets: self.targets.clone(),
            interval: self.interval,
        }))
    }
}

/// One open browse session.
struct MdnsBrowse {
    socket: UdpSocket,
    targets: Vec<SocketAddr>,
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
        let mut failure = None;
        for target in &self.targets {
            if let Err(error) = self.socket.send_to(&packet, target).await {
                log::debug!("cannot send a discovery query target={target} message={error}");
                failure = Some(AppError::internal(
                    "the discovery query could not be sent on this network",
                ));
            }
        }
        match failure {
            Some(error) => Err(error),
            None => Ok(()),
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
        if service.ttl.is_zero() {
            sink.withdrawn(&service.instance);
            continue;
        }
        if let Some(server) = nearby_server(&service, from) {
            sink.advertised(server, service.ttl);
        }
    }
}

/// One advertised instance, as the client will offer it to a user.
fn nearby_server(service: &wire::Service, from: SocketAddr) -> Option<NearbyServer> {
    if service.port == 0 {
        return None;
    }
    let mut label = None;
    let mut queues: Vec<PrinterName> = Vec::new();
    for (key, value) in &service.properties {
        match key.as_str() {
            NAME_PROPERTY if label.is_none() => label = Some(value.clone()),
            QUEUE_PROPERTY => {
                if let Ok(name) = PrinterName::parse(value) {
                    if !queues.contains(&name) {
                        queues.push(name);
                    }
                }
            }
            _ => {}
        }
    }
    let label = label.unwrap_or_else(|| instance_label(&service.instance));
    // The answer's own source address is the one address this browser knows it can reach, so that
    // is what the user reviews; the advertised port is where the endpoint listens.
    let address = format!("{}:{}", from.ip(), service.port);
    NearbyServer::new(&service.instance, &label, &address, queues).ok()
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

    #[test]
    fn an_answer_becomes_a_nearby_server_at_the_address_it_came_from() {
        let server = nearby_server(
            &service(&[
                ("rp", "ipp/print"),
                ("name", "DESKTOP-ABC"),
                ("queue", "Zebra"),
                ("queue", "Canon"),
            ]),
            source("192.0.2.10:5353"),
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

    #[test]
    fn an_answer_without_a_label_falls_back_to_the_instance_name() {
        let server = nearby_server(&service(&[("rp", "ipp/print")]), source("192.0.2.10:5353"))
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
            source("192.0.2.10:5353"),
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
            source("192.0.2.10:5353"),
        )
        .expect("a usable advertisement");

        assert_eq!(server.printers().len(), 1);
    }

    #[test]
    fn an_answer_without_a_usable_port_is_ignored() {
        let mut service = service(&[("name", "DESKTOP-ABC"), ("queue", "Zebra")]);
        service.port = 0;

        assert!(nearby_server(&service, source("192.0.2.10:5353")).is_none());
    }
}
