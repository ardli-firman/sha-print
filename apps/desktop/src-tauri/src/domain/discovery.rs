//! Servers a client user can see on the local network before trusting any of them.
//!
//! A nearby server is a hint: it says where a server claims to be and what it claims to share. It
//! carries no authority at all — the address still has to pass the same certificate-fingerprint
//! approval as a manually entered one before a single printer query is sent (ADR 0004).
//!
//! The address is what identifies one nearby server from another, which is also what the saved
//! approvals are keyed by. Two servers that advertise the same label are two entries, because they
//! are two addresses a user reviews separately.

use crate::domain::{AppError, PrinterName};

/// Longest server label the UI shows; a computer name is far shorter than this.
pub const NAME_LIMIT: usize = 64;
/// Longest address the UI accepts from an advertisement; `host:port` is a fraction of this.
pub const ADDRESS_LIMIT: usize = 255;
/// Longest version string an advertisement may carry.
pub const VERSION_LIMIT: usize = 32;

/// A server the client found on the local network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NearbyServer {
    name: String,
    address: String,
    printers: Vec<PrinterName>,
    version: Option<String>,
}

impl NearbyServer {
    /// Builds a nearby server from what one advertisement reported.
    ///
    /// Text that arrives over the network is bounded here rather than at each call site, so an
    /// advertisement can never push unusable text into the UI. Whether the address is actually
    /// reachable is not decided here: reviewing it does that, exactly as for a manual address.
    pub fn new(name: &str, address: &str, printers: Vec<PrinterName>) -> Result<Self, AppError> {
        Self::with_version(name, address, printers, None)
    }

    /// Builds a nearby server including an optional advertised version.
    pub fn with_version(
        name: &str,
        address: &str,
        printers: Vec<PrinterName>,
        version: Option<String>,
    ) -> Result<Self, AppError> {
        let name = name.trim();
        let address = address.trim();
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
        let clean_version = version.and_then(|v| {
            let trimmed = v.trim();
            if trimmed.is_empty() || trimmed.chars().count() > VERSION_LIMIT || has_control(trimmed)
            {
                None
            } else {
                Some(trimmed.to_owned())
            }
        });
        Ok(Self {
            name: name.to_owned(),
            address: address.to_owned(),
            printers,
            version: clean_version,
        })
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

    /// The semantic version the server advertised, if any.
    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
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
            "DESKTOP-ABC",
            "192.0.2.10:8631",
            vec![name("Zebra"), name("HP LaserJet")],
        )
        .expect("a valid advertisement");

        assert_eq!(server.name(), "DESKTOP-ABC");
        assert_eq!(server.address(), "192.0.2.10:8631");
        assert_eq!(server.printers(), [name("Zebra"), name("HP LaserJet")]);
    }

    #[test]
    fn an_advertisement_that_cannot_be_shown_is_rejected() {
        let long_address = "a".repeat(ADDRESS_LIMIT + 1);
        let long_name = "n".repeat(NAME_LIMIT + 1);
        let cases: Vec<(&str, &str)> = vec![
            ("", "192.0.2.10:8631"),
            ("   ", "192.0.2.10:8631"),
            ("DESKTOP\u{7}ABC", "192.0.2.10:8631"),
            (&long_name, "192.0.2.10:8631"),
            ("DESKTOP-ABC", ""),
            ("DESKTOP-ABC", "192.0.2.10:8631\u{7}"),
            ("DESKTOP-ABC", &long_address),
        ];

        for (label, address) in cases {
            let error = NearbyServer::new(label, address, Vec::new())
                .expect_err("an advertisement the UI cannot render is rejected");
            assert_eq!(error.code(), ErrorCode::InvalidInput, "{address}");
        }
    }

    #[test]
    fn surrounding_whitespace_from_the_network_is_trimmed_rather_than_shown() {
        let server = NearbyServer::new("  DESKTOP-ABC  ", "  192.0.2.10:8631  ", Vec::new())
            .expect("a valid advertisement");

        assert_eq!(server.name(), "DESKTOP-ABC");
        assert_eq!(server.address(), "192.0.2.10:8631");
    }

    #[test]
    fn a_server_that_shares_nothing_yet_is_still_a_server() {
        let server = NearbyServer::new("DESKTOP-ABC", "192.0.2.10:8631", Vec::new())
            .expect("a valid advertisement");

        assert!(server.printers().is_empty());
    }

    #[test]
    fn server_exposes_advertised_version_when_present() {
        let server = NearbyServer::with_version(
            "DESKTOP-ABC",
            "192.0.2.10:8631",
            vec![name("Zebra")],
            Some("3.0.0".to_owned()),
        )
        .expect("valid server");

        assert_eq!(server.version(), Some("3.0.0"));
    }

    #[test]
    fn server_omits_version_when_absent_or_invalid() {
        let older_server =
            NearbyServer::new("DESKTOP-OLD", "192.0.2.11:8631", Vec::new()).expect("valid server");
        assert_eq!(older_server.version(), None);

        let whitespace_version = NearbyServer::with_version(
            "DESKTOP-ABC",
            "192.0.2.10:8631",
            Vec::new(),
            Some("   ".to_owned()),
        )
        .expect("valid server");
        assert_eq!(whitespace_version.version(), None);

        let overlong = "v".repeat(VERSION_LIMIT + 1);
        let overlong_server = NearbyServer::with_version(
            "DESKTOP-ABC",
            "192.0.2.10:8631",
            Vec::new(),
            Some(overlong),
        )
        .expect("valid server");
        assert_eq!(overlong_server.version(), None);
    }
}
