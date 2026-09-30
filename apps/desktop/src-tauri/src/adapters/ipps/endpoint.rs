//! The IPP operations the sharing endpoint answers, independent of how the request arrived.
//!
//! Only queries are implemented (#31). Print jobs are #32, so this endpoint rejects every other
//! operation instead of pretending to accept it.

use crate::application::SharedPrinterSource;
use crate::domain::PrinterName;

use super::protocol::{
    percent_decode, percent_encode, response, PrinterEntry, Request, Status, IPP_VERSION_1_1,
    OPERATION_GET_PRINTERS, OPERATION_GET_PRINTER_ATTRIBUTES,
};

/// Answers one IPP request from a client.
///
/// `host` is the authority the client used (`server:8631`), which becomes the advertised
/// `printer-uri` authority. Every answer reflects the queues shared at this moment, so changing
/// the selection takes effect without restarting sharing.
pub fn answer(request: &[u8], host: &str, shared: &dyn SharedPrinterSource) -> Vec<u8> {
    let request = match Request::parse(request) {
        Ok(request) => request,
        // The header was unreadable, so there is no request id to echo.
        Err(_) => return response(0, IPP_VERSION_1_1, 0, Status::BadRequest, &[]),
    };

    let operation = request.operation();
    let request_id = request.request_id();
    let version = request.response_version();
    if !request.version_is_supported() {
        return response(
            request_id,
            version,
            operation,
            Status::VersionNotSupported,
            &[],
        );
    }

    match operation {
        OPERATION_GET_PRINTERS => {
            let printers: Vec<PrinterEntry> = shared
                .shared_printers()
                .iter()
                .map(|name| entry(host, name))
                .collect();
            response(request_id, version, operation, Status::Ok, &printers)
        }
        OPERATION_GET_PRINTER_ATTRIBUTES => {
            let wanted = request
                .value("printer-uri")
                .or_else(|| request.value("printer-name"));
            let shared = shared.shared_printers();
            match wanted.and_then(|wanted| find(&shared, wanted)) {
                Some(name) => response(
                    request_id,
                    version,
                    operation,
                    Status::Ok,
                    &[entry(host, name)],
                ),
                None => response(request_id, version, operation, Status::NotFound, &[]),
            }
        }
        _ => response(
            request_id,
            version,
            operation,
            Status::UnsupportedOperation,
            &[],
        ),
    }
}

/// The advertisement for one shared queue.
fn entry(host: &str, name: &PrinterName) -> PrinterEntry {
    PrinterEntry {
        name: name.as_str().to_owned(),
        uri: format!("ipps://{host}/ipp/print/{}", percent_encode(name.as_str())),
    }
}

/// Finds the shared queue a client asked about, by its URI or its bare name.
fn find<'a>(shared: &'a [PrinterName], requested: &str) -> Option<&'a PrinterName> {
    let requested = requested.trim_end_matches('/');
    let name = requested.rsplit('/').next().unwrap_or(requested);
    if name.is_empty() {
        return None;
    }
    let name = percent_decode(name);
    shared
        .iter()
        .find(|printer| printer.as_str().eq_ignore_ascii_case(&name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::SharedPrinterSource;
    use crate::domain::PrinterName;

    /// Shares a fixed set of queues.
    struct FakeShared {
        names: Vec<PrinterName>,
    }

    impl FakeShared {
        fn new(names: &[&str]) -> Self {
            Self {
                names: names
                    .iter()
                    .map(|name| PrinterName::parse(name).expect("valid name"))
                    .collect(),
            }
        }
    }

    impl SharedPrinterSource for FakeShared {
        fn shared_printers(&self) -> Vec<PrinterName> {
            self.names.clone()
        }
    }

    /// Builds a client request.
    fn request(operation: u16, attributes: &[(&str, &str)]) -> Vec<u8> {
        let mut out = vec![2, 0];
        out.extend(operation.to_be_bytes());
        out.extend(5u32.to_be_bytes());
        out.push(0x01);
        push(&mut out, 0x47, "attributes-charset", "utf-8");
        push(&mut out, 0x48, "attributes-natural-language", "en");
        for (name, value) in attributes {
            push(&mut out, 0x45, name, value);
        }
        out.push(0x03);
        out
    }

    /// Appends one name/value attribute.
    fn push(out: &mut Vec<u8>, value_tag: u8, name: &str, value: &str) {
        out.push(value_tag);
        out.extend((name.len() as u16).to_be_bytes());
        out.extend(name.as_bytes());
        out.extend((value.len() as u16).to_be_bytes());
        out.extend(value.as_bytes());
    }

    /// The `printer-name` values an answer advertises.
    fn advertised(answer: &[u8]) -> Vec<String> {
        values_of(answer, b"printer-name")
    }

    /// The values an answer carries for one attribute.
    fn values_of(answer: &[u8], wanted: &[u8]) -> Vec<String> {
        let mut values = Vec::new();
        let mut position = 8;
        while position + 1 < answer.len() {
            let value_tag = answer[position];
            position += 1;
            if value_tag == 0x03 {
                break;
            }
            if (0x01..=0x05).contains(&value_tag) {
                continue;
            }
            let name = read(answer, &mut position);
            let value = read(answer, &mut position);
            if name == wanted {
                values.push(String::from_utf8_lossy(&value).into_owned());
            }
        }
        values
    }

    fn read(bytes: &[u8], position: &mut usize) -> Vec<u8> {
        let length = usize::from(u16::from_be_bytes([bytes[*position], bytes[*position + 1]]));
        *position += 2;
        let value = bytes[*position..*position + length].to_vec();
        *position += length;
        value
    }

    fn status(answer: &[u8]) -> u16 {
        let mut position = 8;
        while position + 1 < answer.len() {
            let value_tag = answer[position];
            position += 1;
            if value_tag == 0x03 {
                break;
            }
            if (0x01..=0x05).contains(&value_tag) {
                continue;
            }
            let name = read(answer, &mut position);
            let value = read(answer, &mut position);
            if name == b"status-code" {
                return u16::from_be_bytes([value[2], value[3]]);
            }
        }
        panic!("no status-code in the answer");
    }

    #[test]
    fn get_printers_lists_exactly_the_shared_queues() {
        let shared = FakeShared::new(&["HP LaserJet", "Zebra"]);

        let answer = answer(
            &request(OPERATION_GET_PRINTERS, &[]),
            "server:8631",
            &shared,
        );

        assert_eq!(status(&answer), 0x0000);
        assert_eq!(advertised(&answer), vec!["HP LaserJet", "Zebra"]);
        assert!(
            String::from_utf8_lossy(&answer).contains("ipps://server:8631/ipp/print/HP%20LaserJet")
        );
    }

    #[test]
    fn get_printers_with_nothing_shared_is_a_successful_empty_answer() {
        let shared = FakeShared::new(&[]);

        let answer = answer(&request(OPERATION_GET_PRINTERS, &[]), "server", &shared);

        assert_eq!(status(&answer), 0x0000);
        assert!(advertised(&answer).is_empty());
    }

    #[test]
    fn get_printer_attributes_finds_a_shared_queue_by_its_advertised_uri() {
        let shared = FakeShared::new(&["HP LaserJet", "Zebra"]);

        let answer = answer(
            &request(
                OPERATION_GET_PRINTER_ATTRIBUTES,
                &[("printer-uri", "ipps://server:8631/ipp/print/Zebra")],
            ),
            "server:8631",
            &shared,
        );

        assert_eq!(status(&answer), 0x0000);
        assert_eq!(advertised(&answer), vec!["Zebra"]);
    }

    #[test]
    fn get_printer_attributes_also_accepts_a_bare_printer_name() {
        let shared = FakeShared::new(&["HP LaserJet"]);

        let answer = answer(
            &request(
                OPERATION_GET_PRINTER_ATTRIBUTES,
                &[("printer-name", "hp laserjet")],
            ),
            "server",
            &shared,
        );

        assert_eq!(status(&answer), 0x0000);
        assert_eq!(advertised(&answer), vec!["HP LaserJet"]);
    }

    #[test]
    fn an_unshared_queue_is_not_found() {
        let shared = FakeShared::new(&["HP LaserJet"]);

        let answer = answer(
            &request(
                OPERATION_GET_PRINTER_ATTRIBUTES,
                &[("printer-uri", "ipps://server:8631/ipp/print/Canon")],
            ),
            "server:8631",
            &shared,
        );

        assert_eq!(status(&answer), 0x0406);
        assert!(advertised(&answer).is_empty());
    }

    #[test]
    fn a_request_without_a_printer_is_not_found() {
        let shared = FakeShared::new(&["HP LaserJet"]);

        let answer = answer(
            &request(OPERATION_GET_PRINTER_ATTRIBUTES, &[]),
            "server",
            &shared,
        );

        assert_eq!(status(&answer), 0x0406);
    }

    #[test]
    fn operations_the_mvp_does_not_implement_are_rejected() {
        let shared = FakeShared::new(&["HP LaserJet"]);

        // Print-Job (0x0002) arrives with #32.
        let answer = answer(&request(0x0002, &[]), "server", &shared);

        assert_eq!(status(&answer), 0x0501);
        assert_eq!(&answer[2..4], &0x0002u16.to_be_bytes());
    }

    #[test]
    fn an_unreadable_request_is_a_client_error() {
        let shared = FakeShared::new(&["HP LaserJet"]);

        let answer = answer(&[2, 0, 0x00], "server", &shared);

        assert_eq!(status(&answer), 0x0400);
        assert_eq!(&answer[4..8], &0u32.to_be_bytes());
    }

    #[test]
    fn a_request_in_an_unknown_version_is_rejected_with_a_known_version() {
        let shared = FakeShared::new(&["HP LaserJet"]);
        let mut bytes = request(OPERATION_GET_PRINTERS, &[]);
        bytes[0] = 3;

        let answer = answer(&bytes, "server", &shared);

        assert_eq!(status(&answer), 0x0503);
        assert_eq!(&answer[0..2], &[1, 1]);
    }

    #[test]
    fn the_advertised_uri_round_trips_through_the_lookup() {
        let shared = FakeShared::new(&["Queue#1/2"]);

        let listing = answer(&request(OPERATION_GET_PRINTERS, &[]), "server", &shared);
        let uri = values_of(&listing, b"printer-uri")
            .into_iter()
            .next()
            .expect("a printer uri");

        let answer = answer(
            &request(OPERATION_GET_PRINTER_ATTRIBUTES, &[("printer-uri", &uri)]),
            "server",
            &shared,
        );

        assert_eq!(status(&answer), 0x0000);
    }
}
