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

use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use tokio::net::UdpSocket;

use crate::domain::AppError;

/// The multicast group every multicast DNS responder listens on (RFC 6762 §3).
pub const MDNS_GROUP: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);

/// Ports a ShaPrint responder listens on, in the order it tries them.
///
/// 5353 is the registered multicast DNS port; 5354 is the fallback for the common case where the
/// operating system already runs a multicast DNS responder of its own and holds 5353. A browser
/// asks on every port in this list, because it cannot know which one a server managed to take.
pub const DISCOVERY_PORTS: [u16; 2] = [5353, 5354];

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
