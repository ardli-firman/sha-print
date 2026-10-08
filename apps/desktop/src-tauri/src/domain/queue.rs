//! Native Windows client queues that route a shared printer through the local client proxy.
//!
//! A client queue is an ordinary Windows spooler queue whose port is an IPP URI aimed at
//! `127.0.0.1`, so the printer appears in the standard Windows print dialog and Windows itself
//! renders the document (issue #35). ShaPrint only creates and names the queue; the local proxy
//! (`adapters::client_proxy`) supplies the Network Channel and forwards the job over IPPS.
//!
//! Nothing here touches the spooler: creating the queue is an adapter concern
//! (`adapters::queue_installation`). These types are what the shell validates, hands to the elevated
//! helper, and shows the user.

use std::fmt;

use crate::domain::{AppError, PrinterName};

/// Longest queue name the shell accepts; the Windows spooler limits printer names to 220
/// characters.
const NAME_LIMIT: usize = 220;

/// Longest server address the shell accepts in a queue request.
const ADDRESS_LIMIT: usize = 300;

/// Name of a native Windows queue ShaPrint installs for a shared printer.
///
/// The name is derived from the printer and the server address rather than chosen by the caller, so
/// installing the same shared printer twice targets the same queue: re-running the install repairs
/// that queue instead of piling up duplicates, and the queue survives a restart of the app.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ClientQueueName(String);

impl ClientQueueName {
    /// Validates a queue name the shell builds or reads back.
    pub fn parse(value: &str) -> Result<Self, AppError> {
        let name = value.trim();
        if name.is_empty() {
            return Err(AppError::invalid_input("the Windows queue name is empty"));
        }
        if name.chars().count() > NAME_LIMIT {
            return Err(AppError::invalid_input(format!(
                "the Windows queue name is longer than {NAME_LIMIT} characters"
            )));
        }
        if name.chars().any(char::is_control) {
            return Err(AppError::invalid_input(
                "the Windows queue name contains control characters",
            ));
        }
        // The spooler's RPC name syntax uses these, so a queue that carries one cannot be created
        // or opened reliably.
        if let Some(rejected) = name
            .chars()
            .find(|character| matches!(character, '\\' | ','))
        {
            return Err(AppError::invalid_input(format!(
                "the Windows queue name \"{name}\" cannot contain '{rejected}'; rename the printer on the server and share it again"
            )));
        }
        Ok(Self(name.to_owned()))
    }

    /// The queue name for `printer` shared by the server at `server_address`.
    ///
    /// The address is folded into the name so that two servers sharing a queue with the same name
    /// get distinct local queues. Punctuation a printer name cannot carry is replaced, which keeps
    /// the derivation total for every address the trust store accepts.
    pub fn for_shared_printer(
        printer: &PrinterName,
        server_address: &str,
    ) -> Result<Self, AppError> {
        let label = address_label(server_address);
        Self::parse(&format!("{printer} (ShaPrint {label})"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ClientQueueName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Folds a server address into the label part of a queue name.
///
/// The canonical address is `host:port`, and IPv6 hosts add brackets; a Windows printer name keeps
/// alphanumerics, `.`, `-`, and spaces, so everything else becomes `-`.
fn address_label(server_address: &str) -> String {
    let label: String = server_address
        .chars()
        .map(|character| match character {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '-' => character,
            _ => '-',
        })
        .collect();
    let label = label.trim_matches('-');
    if label.is_empty() {
        "server".to_owned()
    } else {
        label.to_owned()
    }
}

/// Everything the elevated helper needs to install one native client queue.
///
/// The queue name is derived here, not supplied, so a caller cannot ask the helper for a queue with
/// an arbitrary name. The address is carried in the canonical form the client trust store approved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientQueueRequest {
    queue_name: ClientQueueName,
    server_address: String,
    printer: PrinterName,
}

impl ClientQueueRequest {
    /// Builds the request that installs `printer`, shared by the server at `server_address`.
    ///
    /// `server_address` must already be the canonical `host:port` form the trust store keeps;
    /// rejecting anything else here stops a malformed address reaching the elevated helper.
    pub fn new(server_address: &str, printer: PrinterName) -> Result<Self, AppError> {
        let address = server_address.trim();
        validate_address(address)?;
        let queue_name = ClientQueueName::for_shared_printer(&printer, address)?;
        Ok(Self {
            queue_name,
            server_address: address.to_owned(),
            printer,
        })
    }

    pub fn queue_name(&self) -> &ClientQueueName {
        &self.queue_name
    }

    pub fn server_address(&self) -> &str {
        &self.server_address
    }

    pub fn printer(&self) -> &PrinterName {
        &self.printer
    }
}

/// One printer queue reported by the platform spooler, with its queue name and port/destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpoolerRecord {
    pub name: String,
    pub port: String,
}

/// A ShaPrint client queue recognised in the Windows spooler.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RecognisedClientQueue {
    queue_name: ClientQueueName,
    server_address: String,
    printer_name: PrinterName,
}

impl RecognisedClientQueue {
    pub fn new(
        queue_name: ClientQueueName,
        server_address: String,
        printer_name: PrinterName,
    ) -> Self {
        Self {
            queue_name,
            server_address,
            printer_name,
        }
    }

    pub fn queue_name(&self) -> &ClientQueueName {
        &self.queue_name
    }

    pub fn server_address(&self) -> &str {
        &self.server_address
    }

    pub fn printer_name(&self) -> &PrinterName {
        &self.printer_name
    }
}

/// The conservative classification of one spooler record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpoolerRecordClassification {
    /// A genuine ShaPrint client queue, verified by endpoint, port form, destination, and identity.
    RecognisedClientQueue(RecognisedClientQueue),
    /// An ambiguous or malformed record (e.g. points to loopback/proxy endpoint but destination or identity doesn't match).
    AmbiguousClientQueue,
    /// An eligible local printer that is safe to share.
    EligibleLocal(PrinterName),
    /// Other non-shareable / ignored records (e.g. invalid printer name).
    Ignored,
}

impl SpoolerRecord {
    pub fn new(name: impl Into<String>, port: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            port: port.into(),
        }
    }

    /// Evaluates this spooler entry against the expected local proxy authority.
    pub fn classify(&self, proxy_authority: &str) -> SpoolerRecordClassification {
        let name_trimmed = self.name.trim();
        let port_trimmed = self.port.trim();
        let port_lower = port_trimmed.to_ascii_lowercase();

        let scheme_rest = if port_lower.starts_with("ipp://") {
            Some(&port_trimmed[6..])
        } else if port_lower.starts_with("http://") {
            Some(&port_trimmed[7..])
        } else {
            None
        };

        if let Some(rest) = scheme_rest {
            if let Some((authority, path)) = rest.split_once('/') {
                let path_with_slash = format!("/{path}");
                let path_lower = path_with_slash.to_ascii_lowercase();
                if is_loopback_authority(authority, proxy_authority)
                    && path_lower.starts_with("/ipp/print/")
                {
                    let remainder = &path_with_slash["/ipp/print/".len()..];
                    let segments: Vec<&str> = remainder.split('/').collect();
                    if segments.len() != 2 || segments[0].is_empty() || segments[1].is_empty() {
                        return SpoolerRecordClassification::AmbiguousClientQueue;
                    }

                    let server_decoded = percent_decode(segments[0]);
                    if validate_address(&server_decoded).is_err() {
                        return SpoolerRecordClassification::AmbiguousClientQueue;
                    }

                    let printer_decoded = percent_decode(segments[1]);
                    let Ok(printer_name) = PrinterName::parse(&printer_decoded) else {
                        return SpoolerRecordClassification::AmbiguousClientQueue;
                    };

                    let Ok(expected_queue_name) =
                        ClientQueueName::for_shared_printer(&printer_name, &server_decoded)
                    else {
                        return SpoolerRecordClassification::AmbiguousClientQueue;
                    };

                    if expected_queue_name.as_str() == name_trimmed {
                        return SpoolerRecordClassification::RecognisedClientQueue(
                            RecognisedClientQueue {
                                queue_name: expected_queue_name,
                                server_address: server_decoded,
                                printer_name,
                            },
                        );
                    } else {
                        return SpoolerRecordClassification::AmbiguousClientQueue;
                    }
                }
            }
        }

        // On Windows 10/11, queues created via Add-Printer -IppURL are managed by the WSD port monitor
        // with ports named "WSD-<GUID>". Recognise genuine ShaPrint client queues on WSD ports by
        // their derived naming identity.
        if port_lower.starts_with("wsd-") {
            if let Some((printer_name, server_address, queue_name)) =
                parse_client_queue_name(name_trimmed)
            {
                return SpoolerRecordClassification::RecognisedClientQueue(RecognisedClientQueue {
                    queue_name,
                    server_address,
                    printer_name,
                });
            } else if name_trimmed.contains(" (ShaPrint ") {
                return SpoolerRecordClassification::AmbiguousClientQueue;
            }
        }

        let is_loopback = scheme_rest.is_some_and(|rest| {
            let authority = rest.split_once('/').map(|(a, _)| a).unwrap_or(rest);
            is_loopback_authority(authority, proxy_authority)
        });

        if port_lower.contains(&proxy_authority.to_ascii_lowercase())
            || port_lower.contains("/ipp/print/")
            || is_loopback
        {
            // Mentions loopback proxy or IPP print endpoint, but failed valid client queue criteria
            SpoolerRecordClassification::AmbiguousClientQueue
        } else {
            // Real local queue (or lookalike name with USB/network port)
            match PrinterName::parse(name_trimmed) {
                Ok(printer_name) => SpoolerRecordClassification::EligibleLocal(printer_name),
                Err(_) => SpoolerRecordClassification::Ignored,
            }
        }
    }
}

fn is_loopback_authority(authority: &str, proxy_authority: &str) -> bool {
    if authority.eq_ignore_ascii_case(proxy_authority) {
        return true;
    }
    let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
        if let Some((ipv6, after)) = rest.split_once(']') {
            let port = if after.is_empty() {
                ""
            } else if let Some(p) = after.strip_prefix(':') {
                p
            } else {
                return false;
            };
            (ipv6, port)
        } else {
            return false;
        }
    } else if let Some((h, p)) = authority.split_once(':') {
        (h, p)
    } else {
        (authority, "")
    };

    let is_host_loopback = host.eq_ignore_ascii_case("127.0.0.1")
        || host.eq_ignore_ascii_case("localhost")
        || host == "::1";
    if !is_host_loopback {
        return false;
    }
    if port.is_empty() {
        return true;
    }
    port.parse::<u16>().is_ok()
}

fn percent_decode(value: &str) -> String {
    let mut bytes = Vec::with_capacity(value.len());
    let mut chars = value.bytes();
    while let Some(b) = chars.next() {
        if b == b'%' {
            let high = chars.next().and_then(hex_val);
            let low = chars.next().and_then(hex_val);
            if let (Some(h), Some(l)) = (high, low) {
                bytes.push((h << 4) | l);
                continue;
            }
        }
        bytes.push(b);
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn parse_client_queue_name(name: &str) -> Option<(PrinterName, String, ClientQueueName)> {
    let name_trimmed = name.trim();
    let without_suffix = name_trimmed.strip_suffix(')')?;
    let (printer_raw, label) = without_suffix.rsplit_once(" (ShaPrint ")?;
    let printer_name = PrinterName::parse(printer_raw).ok()?;

    let candidate_address = if let Some((host_part, port_part)) = label.rsplit_once('-') {
        if !port_part.is_empty() && port_part.chars().all(|c| c.is_ascii_digit()) {
            format!("{host_part}:{port_part}")
        } else {
            label.to_owned()
        }
    } else {
        label.to_owned()
    };

    if validate_address(&candidate_address).is_err() {
        return None;
    }

    let expected_queue_name =
        ClientQueueName::for_shared_printer(&printer_name, &candidate_address).ok()?;
    if expected_queue_name.as_str() != name_trimmed {
        return None;
    }

    Some((printer_name, candidate_address, expected_queue_name))
}

/// Accepts only the canonical `host:port` shapes the client trust store produces.
///
/// The authoritative address parser is `adapters::client_connections`, and the URI builder parses
/// again before anything is installed; this check keeps a malformed address from reaching the
/// elevated helper at all, and keeps the derived queue name honest.
fn validate_address(address: &str) -> Result<(), AppError> {
    if address.is_empty() {
        return Err(AppError::invalid_input(
            "enter the address of the server that shares the printer",
        ));
    }
    if address.chars().count() > ADDRESS_LIMIT {
        return Err(AppError::invalid_input("the server address is too long"));
    }

    let invalid = || {
        AppError::invalid_input(
            "the server address is not a host or host:port; review the server connection again",
        )
    };

    // A bracketed host is the canonical IPv6 form; everything else is host, or host:port.
    let port = if let Some(rest) = address.strip_prefix('[') {
        let (host, after) = rest.split_once(']').ok_or_else(invalid)?;
        if host.is_empty()
            || !host
                .chars()
                .all(|character| character.is_ascii_hexdigit() || matches!(character, ':' | '.'))
        {
            return Err(invalid());
        }
        match after.strip_prefix(':') {
            Some(port) if !port.is_empty() => Some(port),
            Some(_) => return Err(invalid()),
            None if after.is_empty() => None,
            None => return Err(invalid()),
        }
    } else {
        let (host, port) = match address.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (address, None),
        };
        if host.is_empty()
            || !host.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '.' | '-')
            })
        {
            return Err(invalid());
        }
        match port {
            Some(port) if !port.is_empty() => Some(port),
            Some(_) => return Err(invalid()),
            None => None,
        }
    };

    if let Some(port) = port {
        let valid = port.len() <= 5
            && port.bytes().all(|byte| byte.is_ascii_digit())
            && port.parse::<u16>().is_ok_and(|port| port > 0);
        if !valid {
            return Err(invalid());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ErrorCode;

    fn printer(value: &str) -> PrinterName {
        PrinterName::parse(value).expect("valid printer name")
    }

    fn queue(value: &str, address: &str) -> ClientQueueName {
        ClientQueueName::for_shared_printer(&printer(value), address).expect("derives a queue name")
    }

    #[test]
    fn a_queue_name_names_the_printer_and_the_server() {
        assert_eq!(
            queue("HP LaserJet", "192.168.1.20:8631").as_str(),
            "HP LaserJet (ShaPrint 192.168.1.20-8631)"
        );
        // The port separator cannot appear in a printer name, so the label replaces it.
        assert_eq!(
            queue("Office Printer", "printer.local").as_str(),
            "Office Printer (ShaPrint printer.local)"
        );
        assert_eq!(
            queue("Zebra", "[2001:db8::1]:8631").as_str(),
            "Zebra (ShaPrint 2001-db8--1--8631)"
        );
    }

    #[test]
    fn the_same_printer_and_server_always_produce_the_same_queue_name() {
        let first = queue("Office Printer", "10.0.0.5:8631");
        let second = queue("Office Printer", "10.0.0.5:8631");
        assert_eq!(first, second);

        // A different server sharing a queue with the same name stays a separate local queue.
        assert_ne!(first, queue("Office Printer", "10.0.0.6:8631"));
    }

    #[test]
    fn queue_names_that_windows_cannot_use_are_rejected() {
        for invalid in ["", "   ", "ShaPrint\nQueue"] {
            let error = ClientQueueName::parse(invalid).expect_err("rejected");
            assert_eq!(error.code(), ErrorCode::InvalidInput);
        }
        for rejected in ["ShaPrint, Queue", "ShaPrint\\Queue"] {
            let error = ClientQueueName::parse(rejected).expect_err("rejected");
            assert_eq!(error.code(), ErrorCode::InvalidInput);
            assert!(
                error.message().contains("rename the printer on the server"),
                "unhelpful message: {}",
                error.message()
            );
        }
    }

    #[test]
    fn overlong_queue_names_are_rejected() {
        let long_printer = printer(&"p".repeat(NAME_LIMIT - 5));
        let error = ClientQueueName::for_shared_printer(&long_printer, "10.0.0.5:8631")
            .expect_err("the derived name exceeds the limit");
        assert_eq!(error.code(), ErrorCode::InvalidInput);

        assert!(ClientQueueName::parse(&"p".repeat(NAME_LIMIT)).is_ok());
    }

    #[test]
    fn a_printer_name_with_a_comma_cannot_become_a_queue_name() {
        let error = ClientQueueName::for_shared_printer(&printer("Printer, Inc"), "10.0.0.5:8631")
            .expect_err("rejected");
        assert_eq!(error.code(), ErrorCode::InvalidInput);
    }

    #[test]
    fn a_request_derives_its_queue_name_and_keeps_the_address() {
        let request =
            ClientQueueRequest::new(" 10.0.0.5:8631 ", printer("Office Printer")).expect("valid");
        assert_eq!(request.server_address(), "10.0.0.5:8631");
        assert_eq!(request.printer().as_str(), "Office Printer");
        assert_eq!(
            request.queue_name().as_str(),
            "Office Printer (ShaPrint 10.0.0.5-8631)"
        );
        assert_eq!(
            request.queue_name().to_string(),
            "Office Printer (ShaPrint 10.0.0.5-8631)"
        );
    }

    #[test]
    fn an_address_that_is_not_a_host_or_host_port_is_rejected() {
        for invalid in [
            "",
            "   ",
            "http://10.0.0.5",
            "10.0.0.5; whoami",
            "10.0.0.5 8631",
            "printer'name",
            // The canonical form carries at most one port, and it is a real port.
            "10.0.0.5:99999",
            "10.0.0.5:0",
            "10.0.0.5:",
            "a:b:c",
            "[[x]]",
            "[2001:db8::1",
            "[2001:db8::1]:",
        ] {
            let error =
                ClientQueueRequest::new(invalid, printer("Office Printer")).expect_err("rejected");
            assert_eq!(
                error.code(),
                ErrorCode::InvalidInput,
                "accepted {invalid:?}"
            );
        }
        assert!(
            ClientQueueRequest::new(&"1".repeat(ADDRESS_LIMIT + 1), printer("Office Printer"))
                .is_err()
        );
    }

    #[test]
    fn the_canonical_address_forms_are_accepted() {
        for valid in [
            "printer.local",
            "printer.local:8631",
            "192.168.1.20:8631",
            "[2001:db8::1]:8631",
            "[2001:db8::1]",
        ] {
            assert!(
                ClientQueueRequest::new(valid, printer("Office Printer")).is_ok(),
                "rejected {valid:?}"
            );
        }
    }

    #[test]
    fn spooler_record_classifies_genuine_client_queue_with_ipp_and_http_schemes() {
        let authority = "127.0.0.1:8632";
        let valid_ipp = SpoolerRecord::new(
            "Office Printer (ShaPrint 10.0.0.5-8631)",
            "ipp://127.0.0.1:8632/ipp/print/10.0.0.5%3A8631/Office%20Printer",
        );
        let valid_http = SpoolerRecord::new(
            "Office Printer (ShaPrint 10.0.0.5-8631)",
            "http://127.0.0.1:8632/ipp/print/10.0.0.5%3A8631/Office%20Printer",
        );

        match valid_ipp.classify(authority) {
            SpoolerRecordClassification::RecognisedClientQueue(q) => {
                assert_eq!(
                    q.queue_name().as_str(),
                    "Office Printer (ShaPrint 10.0.0.5-8631)"
                );
                assert_eq!(q.server_address(), "10.0.0.5:8631");
                assert_eq!(q.printer_name().as_str(), "Office Printer");
            }
            other => panic!("expected RecognisedClientQueue, got {other:?}"),
        }

        match valid_http.classify(authority) {
            SpoolerRecordClassification::RecognisedClientQueue(q) => {
                assert_eq!(
                    q.queue_name().as_str(),
                    "Office Printer (ShaPrint 10.0.0.5-8631)"
                );
                assert_eq!(q.server_address(), "10.0.0.5:8631");
                assert_eq!(q.printer_name().as_str(), "Office Printer");
            }
            other => panic!("expected RecognisedClientQueue, got {other:?}"),
        }
    }

    #[test]
    fn spooler_record_classifies_existing_queue_on_older_or_different_loopback_proxy_port_as_recognised(
    ) {
        let current_authority = "127.0.0.1:48632";
        // An existing queue installed earlier when proxy listened on legacy 8632
        let legacy_queue = SpoolerRecord::new(
            "Office Printer (ShaPrint 10.0.0.5-8631)",
            "ipp://127.0.0.1:8632/ipp/print/10.0.0.5%3A8631/Office%20Printer",
        );
        match legacy_queue.classify(current_authority) {
            SpoolerRecordClassification::RecognisedClientQueue(q) => {
                assert_eq!(
                    q.queue_name().as_str(),
                    "Office Printer (ShaPrint 10.0.0.5-8631)"
                );
                assert_eq!(q.server_address(), "10.0.0.5:8631");
                assert_eq!(q.printer_name().as_str(), "Office Printer");
            }
            other => panic!("expected RecognisedClientQueue, got {other:?}"),
        }

        // An existing queue with localhost authority
        let localhost_queue = SpoolerRecord::new(
            "Office Printer (ShaPrint 10.0.0.5-8631)",
            "http://localhost:8632/ipp/print/10.0.0.5%3A8631/Office%20Printer",
        );
        match localhost_queue.classify(current_authority) {
            SpoolerRecordClassification::RecognisedClientQueue(q) => {
                assert_eq!(
                    q.queue_name().as_str(),
                    "Office Printer (ShaPrint 10.0.0.5-8631)"
                );
                assert_eq!(q.server_address(), "10.0.0.5:8631");
                assert_eq!(q.printer_name().as_str(), "Office Printer");
            }
            other => panic!("expected RecognisedClientQueue, got {other:?}"),
        }

        // An existing queue with IPv6 loopback authority [::1]:48632
        let ipv6_queue = SpoolerRecord::new(
            "Office Printer (ShaPrint 10.0.0.5-8631)",
            "ipp://[::1]:48632/ipp/print/10.0.0.5%3A8631/Office%20Printer",
        );
        match ipv6_queue.classify(current_authority) {
            SpoolerRecordClassification::RecognisedClientQueue(q) => {
                assert_eq!(
                    q.queue_name().as_str(),
                    "Office Printer (ShaPrint 10.0.0.5-8631)"
                );
                assert_eq!(q.server_address(), "10.0.0.5:8631");
                assert_eq!(q.printer_name().as_str(), "Office Printer");
            }
            other => panic!("expected RecognisedClientQueue, got {other:?}"),
        }

        // A malformed IPv6 authority without colon separator is not treated as loopback
        let malformed_ipv6 = SpoolerRecord::new(
            "Office Printer (ShaPrint 10.0.0.5-8631)",
            "ipp://[::1]48632/ipp/print/10.0.0.5%3A8631/Office%20Printer",
        );
        assert_eq!(
            malformed_ipv6.classify(current_authority),
            SpoolerRecordClassification::AmbiguousClientQueue
        );
    }

    #[test]
    fn spooler_record_treats_lookalikes_with_real_ports_as_eligible_local_printers() {
        let authority = "127.0.0.1:8632";
        // A queue with ShaPrint in the name but on a USB port is a real local queue (name prefix alone does not qualify)
        let lookalike = SpoolerRecord::new("Office Printer (ShaPrint 10.0.0.5-8631)", "USB001");
        assert_eq!(
            lookalike.classify(authority),
            SpoolerRecordClassification::EligibleLocal(printer(
                "Office Printer (ShaPrint 10.0.0.5-8631)"
            ))
        );

        let real_local = SpoolerRecord::new("HP LaserJet Pro", "WSD-1234");
        assert_eq!(
            real_local.classify(authority),
            SpoolerRecordClassification::EligibleLocal(printer("HP LaserJet Pro"))
        );
    }

    #[test]
    fn spooler_record_treats_loopback_destinations_with_bad_shapes_or_names_as_ambiguous() {
        let authority = "127.0.0.1:8632";
        // Port points to proxy, but name does not match derived identity
        let name_mismatch = SpoolerRecord::new(
            "Some Other Name",
            "ipp://127.0.0.1:8632/ipp/print/10.0.0.5%3A8631/Office%20Printer",
        );
        assert_eq!(
            name_mismatch.classify(authority),
            SpoolerRecordClassification::AmbiguousClientQueue
        );

        // Malformed path
        let malformed_path = SpoolerRecord::new(
            "Office Printer (ShaPrint 10.0.0.5-8631)",
            "ipp://127.0.0.1:8632/ipp/print/only-one-segment",
        );
        assert_eq!(
            malformed_path.classify(authority),
            SpoolerRecordClassification::AmbiguousClientQueue
        );

        // Extra path segments
        let extra_segments = SpoolerRecord::new(
            "Office Printer (ShaPrint 10.0.0.5-8631)",
            "ipp://127.0.0.1:8632/ipp/print/10.0.0.5%3A8631/Office%20Printer/extra",
        );
        assert_eq!(
            extra_segments.classify(authority),
            SpoolerRecordClassification::AmbiguousClientQueue
        );

        // Ambiguous loopback port mention
        let loopback_misc =
            SpoolerRecord::new("Unknown Spooler Entry", "ipp://127.0.0.1:8632/corrupt");
        assert_eq!(
            loopback_misc.classify(authority),
            SpoolerRecordClassification::AmbiguousClientQueue
        );
    }

    #[test]
    fn spooler_record_classifies_genuine_client_queue_on_windows_wsd_port() {
        let authority = "127.0.0.1:48632";
        let record = SpoolerRecord::new(
            "EPSON L3210 Series (Copy 1) (ShaPrint 10.102.10.21-48631)",
            "WSD-3b5a3ba4-1211-4937-8b8b-ad7bf63e5a22",
        );

        match record.classify(authority) {
            SpoolerRecordClassification::RecognisedClientQueue(q) => {
                assert_eq!(
                    q.queue_name().as_str(),
                    "EPSON L3210 Series (Copy 1) (ShaPrint 10.102.10.21-48631)"
                );
                assert_eq!(q.server_address(), "10.102.10.21:48631");
                assert_eq!(q.printer_name().as_str(), "EPSON L3210 Series (Copy 1)");
            }
            other => panic!("expected RecognisedClientQueue, got {other:?}"),
        }
    }

    #[test]
    fn spooler_record_classifies_malformed_wsd_shaprint_queue_as_ambiguous() {
        let authority = "127.0.0.1:48632";
        let record = SpoolerRecord::new(
            "Office Printer (ShaPrint bad--address--)",
            "WSD-3b5a3ba4-1211-4937-8b8b-ad7bf63e5a22",
        );
        assert_eq!(
            record.classify(authority),
            SpoolerRecordClassification::AmbiguousClientQueue
        );
    }
}
