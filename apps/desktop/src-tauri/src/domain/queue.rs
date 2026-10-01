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
}
