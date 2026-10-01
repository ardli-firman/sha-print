//! Servers a client user can see on the local network before trusting any of them.
//!
//! A nearby server is a hint: it says where a server claims to be and what it claims to share. It
//! carries no authority at all — the address still has to pass the same certificate-fingerprint
//! approval as a manually entered one before a single printer query is sent (ADR 0004).

use crate::domain::{AppError, PrinterName};

/// Longest server label the UI shows; a computer name is far shorter than this.
pub const NAME_LIMIT: usize = 64;
/// Longest address the UI accepts from an advertisement; `host:port` is a fraction of this.
pub const ADDRESS_LIMIT: usize = 255;
/// Longest instance name the browser keeps as a cache key (the DNS limit for a name is 255 bytes).
const INSTANCE_LIMIT: usize = 255;

/// A server the client found on the local network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NearbyServer {
    instance: String,
    name: String,
    address: String,
    printers: Vec<PrinterName>,
}

impl NearbyServer {
    /// Builds a nearby server from what one advertisement reported.
    ///
    /// Text that arrives over the network is bounded here rather than at each call site, so an
    /// advertisement can never push unusable text into the UI. Whether the address is actually
    /// reachable is not decided here: reviewing it does that, exactly as for a manual address.
    pub fn new(
        instance: &str,
        name: &str,
        address: &str,
        printers: Vec<PrinterName>,
    ) -> Result<Self, AppError> {
        let instance = instance.trim();
        let name = name.trim();
        let address = address.trim();
        if instance.is_empty() || instance.len() > INSTANCE_LIMIT || has_control(instance) {
            return Err(AppError::invalid_input(
                "the advertised server name is not usable",
            ));
        }
        if name.is_empty() || name.chars().count() > NAME_LIMIT || has_control(name) {
            return Err(AppError::invalid_input(
                "the advertised server label is not usable",
            ));
        }
        if address.is_empty() || address.len() > ADDRESS_LIMIT || has_control(address) {
            return Err(AppError::invalid_input(
                "the advertised server address is not usable",
            ));
        }
        Ok(Self {
            instance: instance.to_owned(),
            name: name.to_owned(),
            address: address.to_owned(),
            printers,
        })
    }

    /// The advertisement this came from, which is how a browser tells two servers apart.
    pub fn instance(&self) -> &str {
        &self.instance
    }

    /// The label the UI shows for the server.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The address a user reviews and approves before printers are listed.
    pub fn address(&self) -> &str {
        &self.address
    }

    /// The queues the server says it shares.
    pub fn printers(&self) -> &[PrinterName] {
        &self.printers
    }
}

fn has_control(value: &str) -> bool {
    value.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ErrorCode, PrinterName};

    fn name(value: &str) -> PrinterName {
        PrinterName::parse(value).expect("valid printer name")
    }

    #[test]
    fn a_nearby_server_carries_what_the_client_reviews_and_connects_to() {
        let server = NearbyServer::new(
            "DESKTOP-ABC._shaprint-ipps._tcp.local.",
            "DESKTOP-ABC",
            "192.0.2.10:8631",
            vec![name("Zebra"), name("HP LaserJet")],
        )
        .expect("a valid advertisement");

        assert_eq!(server.instance(), "DESKTOP-ABC._shaprint-ipps._tcp.local.");
        assert_eq!(server.name(), "DESKTOP-ABC");
        assert_eq!(server.address(), "192.0.2.10:8631");
        assert_eq!(server.printers(), [name("Zebra"), name("HP LaserJet")]);
    }

    #[test]
    fn an_advertisement_that_cannot_be_shown_is_rejected() {
        let long_address = "a".repeat(ADDRESS_LIMIT + 1);
        let long_name = "n".repeat(NAME_LIMIT + 1);
        let cases: Vec<(&str, &str, &str)> = vec![
            ("", "DESKTOP-ABC", "192.0.2.10:8631"),
            ("   ", "DESKTOP-ABC", "192.0.2.10:8631"),
            (
                "DESKTOP-ABC._shaprint-ipps._tcp.local.",
                "",
                "192.0.2.10:8631",
            ),
            (
                "DESKTOP-ABC._shaprint-ipps._tcp.local.",
                "   ",
                "192.0.2.10:8631",
            ),
            (
                "DESKTOP-ABC._shaprint-ipps._tcp.local.",
                "DESKTOP\u{7}ABC",
                "192.0.2.10:8631",
            ),
            (
                "DESKTOP-ABC._shaprint-ipps._tcp.local.",
                &long_name,
                "192.0.2.10:8631",
            ),
            ("DESKTOP-ABC._shaprint-ipps._tcp.local.", "DESKTOP-ABC", ""),
            (
                "DESKTOP-ABC._shaprint-ipps._tcp.local.",
                "DESKTOP-ABC",
                "192.0.2.10:8631\u{7}",
            ),
            (
                "DESKTOP-ABC._shaprint-ipps._tcp.local.",
                "DESKTOP-ABC",
                &long_address,
            ),
        ];

        for (instance, label, address) in cases {
            let error = NearbyServer::new(instance, label, address, Vec::new())
                .expect_err("an advertisement the UI cannot render is rejected");
            assert_eq!(error.code(), ErrorCode::InvalidInput, "{address}");
        }
    }

    #[test]
    fn surrounding_whitespace_from_the_network_is_trimmed_rather_than_shown() {
        let server = NearbyServer::new(
            "  DESKTOP-ABC._shaprint-ipps._tcp.local.  ",
            "  DESKTOP-ABC  ",
            "  192.0.2.10:8631  ",
            Vec::new(),
        )
        .expect("a valid advertisement");

        assert_eq!(server.instance(), "DESKTOP-ABC._shaprint-ipps._tcp.local.");
        assert_eq!(server.name(), "DESKTOP-ABC");
        assert_eq!(server.address(), "192.0.2.10:8631");
    }

    #[test]
    fn a_server_that_shares_nothing_yet_is_still_a_server() {
        let server = NearbyServer::new(
            "DESKTOP-ABC._shaprint-ipps._tcp.local.",
            "DESKTOP-ABC",
            "192.0.2.10:8631",
            Vec::new(),
        )
        .expect("a valid advertisement");

        assert!(server.printers().is_empty());
    }
}
