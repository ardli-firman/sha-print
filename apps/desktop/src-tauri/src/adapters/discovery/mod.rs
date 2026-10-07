//! Local-network discovery of ShaPrint servers (ADR 0004).
//!
//! The client browses for nearby servers and the server advertises the queues it shares, using
//! multicast DNS messages. Discovery is a hint, never a trust decision: a discovered address still
//! has to pass the certificate-fingerprint approval before any printer query.

mod advertise;
mod browse;
mod wire;

pub use advertise::{MdnsAdvertiser, ADVERTISED_TTL};
pub use browse::MdnsBrowser;
// The codec is only reachable through what a caller outside this module needs: a test that speaks
// the wire, and nothing else.
pub use wire::{encode_query, SERVICE_TYPE};

use std::collections::BTreeSet;
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use async_trait::async_trait;
use tokio::net::UdpSocket;

use crate::domain::AppError;

/// The multicast group every multicast DNS responder listens on (RFC 6762 §3).
pub const MDNS_GROUP: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);

/// IPv4 hop limit for multicast discovery across routed VLANs (#84).
pub const MDNS_MULTICAST_TTL: u32 = 32;

/// Dedicated multicast DNS discovery port (ADR 0014).
pub const DISCOVERY_PORT: u16 = 48633;

/// Ports a ShaPrint responder listens on, in the order it tries them (ADR 0014).
pub const DISCOVERY_PORTS: [u16; 1] = [DISCOVERY_PORT];

/// Largest datagram discovery reads or writes.
pub const MAX_DATAGRAM: usize = 1500;

/// How long a failed socket read waits before reading again, so a peer that cannot be reached
/// cannot spin a loop.
const READ_BACKOFF: Duration = Duration::from_millis(50);

/// `TXT` keys ShaPrint puts in an advertisement, and reads back out of one.
pub const PATH_PROPERTY: &str = "rp";
pub const NAME_PROPERTY: &str = "name";
pub const QUEUE_PROPERTY: &str = "queue";
/// Application semver attribute (ADR 0012 §5).
pub const VERSION_PROPERTY: &str = "v";
/// The resource path a browser connects to, shared with the IPPS endpoint.
pub const RESOURCE_PATH: &str = "ipp/print";

/// Per-interface IPv4 multicast socket operations used by the advertiser and browser.
///
/// The production adapter joins groups and changes the multicast interface on the shared socket.
/// Tests can observe each attempt without relying on host multicast loopback support.
#[async_trait]
pub trait MulticastInterfaceSender: Send + Sync {
    fn set_hop_limit(&self, socket: &UdpSocket, ttl: u32) -> io::Result<()>;

    fn join_group(
        &self,
        socket: &UdpSocket,
        group: Ipv4Addr,
        interface: Ipv4Addr,
    ) -> io::Result<()>;

    async fn send_on(
        &self,
        socket: &UdpSocket,
        interface: Ipv4Addr,
        packet: &[u8],
        destination: SocketAddr,
    ) -> io::Result<()>;
}

struct SystemMulticastInterfaceSender;

#[async_trait]
impl MulticastInterfaceSender for SystemMulticastInterfaceSender {
    fn set_hop_limit(&self, socket: &UdpSocket, ttl: u32) -> io::Result<()> {
        socket.set_multicast_ttl_v4(ttl)
    }

    fn join_group(
        &self,
        socket: &UdpSocket,
        group: Ipv4Addr,
        interface: Ipv4Addr,
    ) -> io::Result<()> {
        socket.join_multicast_v4(group, interface)
    }

    async fn send_on(
        &self,
        socket: &UdpSocket,
        interface: Ipv4Addr,
        packet: &[u8],
        destination: SocketAddr,
    ) -> io::Result<()> {
        socket2::SockRef::from(socket).set_multicast_if_v4(&interface)?;
        socket.send_to(packet, destination).await.map(|_| ())
    }
}

pub(super) fn system_multicast_interface_sender() -> std::sync::Arc<dyn MulticastInterfaceSender> {
    std::sync::Arc::new(SystemMulticastInterfaceSender)
}

pub(super) fn active_ipv4_interfaces(
    addresses: impl IntoIterator<Item = Ipv4Addr>,
) -> Vec<Ipv4Addr> {
    addresses
        .into_iter()
        .filter(|address| !address.is_loopback() && !address.is_unspecified())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(super) fn system_ipv4_interfaces() -> Vec<Ipv4Addr> {
    match if_addrs::get_if_addrs() {
        Ok(interfaces) => active_ipv4_interfaces(interfaces.into_iter().filter_map(|interface| {
            match interface.addr {
                if_addrs::IfAddr::V4(address) => Some(address.ip),
                if_addrs::IfAddr::V6(_) => None,
            }
        })),
        Err(error) => {
            log::debug!("cannot enumerate IPv4 discovery interfaces message={error}");
            Vec::new()
        }
    }
}

/// Sends one multicast packet on every eligible interface, isolating per-interface failures.
pub(super) async fn send_multicast_on_interfaces(
    socket: &UdpSocket,
    sender: &dyn MulticastInterfaceSender,
    multicast_lock: &tokio::sync::Mutex<()>,
    interfaces: &[Ipv4Addr],
    packet: &[u8],
    destinations: &[SocketAddr],
    what: &str,
) -> Result<(), AppError> {
    let mut failures = 0;
    for interface in active_ipv4_interfaces(interfaces.iter().copied()) {
        let _multicast_guard = multicast_lock.lock().await;
        for destination in destinations {
            if let Err(error) = sender
                .send_on(socket, interface, packet, *destination)
                .await
            {
                log::debug!("cannot send a discovery {what} interface={interface} target={destination} message={error}");
                failures += 1;
            }
        }
    }
    if failures == 0 {
        Ok(())
    } else {
        Err(AppError::internal(format!(
            "the discovery {what} failed on {failures} interface send(s)"
        )))
    }
}

/// Sends one discovery packet to every destination.
///
/// A failed send is reported but never abandons the rest: the destinations are independent, and one
/// unreachable peer must not stop a server from announcing itself to the others.
async fn send_to_all(
    socket: &UdpSocket,
    packet: &[u8],
    destinations: &[SocketAddr],
    what: &str,
) -> Result<(), AppError> {
    let mut failure = None;
    for destination in destinations {
        if let Err(error) = socket.send_to(packet, *destination).await {
            log::debug!("cannot send a discovery {what} target={destination} message={error}");
            failure = Some(AppError::internal(format!(
                "the discovery {what} could not be sent on this network"
            )));
        }
    }
    match failure {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_port_is_dedicated_48633() {
        assert_eq!(DISCOVERY_PORTS, [48633]);
    }
}
