//! Minimal IPP codec (RFC 8010/8011): enough to answer printer queries over IPPS.
//!
//! The sharing endpoint answers `Get-Printers` and `Get-Printer-Attributes` (#31). Print job
//! submission and the attributes it needs arrive with #32; the encoder here writes the attribute
//! groups a query response requires, and the decoder reads only what a query needs to be answered.

/// Default IPP version the endpoint uses when it cannot honour the request's version.
pub const IPP_VERSION_1_1: (u8, u8) = (1, 1);
/// Newest IPP version the endpoint understands.
pub const IPP_VERSION_2_0: (u8, u8) = (2, 0);

/// Versions an IPP client may use in a request.
const SUPPORTED_VERSIONS: [(u8, u8); 3] = [IPP_VERSION_2_0, IPP_VERSION_1_1, (1, 0)];

/// Operation ids the sharing endpoint answers.
pub const OPERATION_GET_PRINTER_ATTRIBUTES: u16 = 0x000b;
pub const OPERATION_GET_PRINTERS: u16 = 0x0402;

/// `printer-state` for a queue that is idle and able to accept a job.
const PRINTER_STATE_IDLE: i32 = 3;

/// Attribute group and value tags used by the queries the endpoint answers.
mod tag {
    pub(super) const OPERATION_ATTRIBUTES: u8 = 0x01;
    pub(super) const END_OF_ATTRIBUTES: u8 = 0x03;
    pub(super) const PRINTER_ATTRIBUTES: u8 = 0x04;

    pub(super) const INTEGER: u8 = 0x21;
    pub(super) const BOOLEAN: u8 = 0x22;
    pub(super) const ENUM: u8 = 0x23;
    pub(super) const NAME: u8 = 0x42;
    pub(super) const KEYWORD: u8 = 0x44;
    pub(super) const URI: u8 = 0x45;
    pub(super) const CHARSET: u8 = 0x47;
    pub(super) const NATURAL_LANGUAGE: u8 = 0x48;

    /// Delimiter tags that introduce an attribute group; they carry no name or value.
    pub(super) const fn is_delimiter(tag: u8) -> bool {
        matches!(tag, 0x01..=0x05)
    }

    /// Value tags that carry text (RFC 8010 §3.5).
    pub(super) const fn is_text(tag: u8) -> bool {
        matches!(tag, 0x41 | 0x42 | 0x44..=0x4A)
    }
}

/// Status of an IPP response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// `successful-ok`: the request was answered.
    Ok,
    /// `client-error-bad-request`: the request could not be decoded.
    BadRequest,
    /// `client-error-not-found`: no such printer is shared.
    NotFound,
    /// `server-error-version-not-supported`.
    VersionNotSupported,
    /// `server-error-operation-not-supported`: this endpoint does not implement the operation.
    UnsupportedOperation,
}

impl Status {
    pub const fn code(self) -> u16 {
        match self {
            Status::Ok => 0x0000,
            Status::BadRequest => 0x0400,
            Status::NotFound => 0x0406,
            Status::VersionNotSupported => 0x0503,
            Status::UnsupportedOperation => 0x0501,
        }
    }
}

/// A request that could not be decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    /// The request is shorter than its own header.
    Truncated,
    /// A name or value length runs past the end of the request.
    Overrun,
    /// A text value is not valid UTF-8.
    NotText,
}

/// One attribute of a request: its name, value tag, and one or more values.
#[derive(Debug, Clone)]
struct Attribute {
    name: String,
    tag: u8,
    values: Vec<Vec<u8>>,
}

impl Attribute {
    /// The first value as text, for the string tags the endpoint reads.
    fn text(&self) -> Option<&str> {
        if !tag::is_text(self.tag) {
            return None;
        }
        let value = self.values.first()?;
        let value = std::str::from_utf8(value).ok()?;
        Some(value.trim_end_matches('\0'))
    }
}

/// A decoded IPP request.
#[derive(Debug, Clone)]
pub struct Request {
    version: (u8, u8),
    operation: u16,
    request_id: u32,
    attributes: Vec<Attribute>,
}

impl Request {
    /// Decodes the header and the operation attributes of a request.
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        if bytes.len() < 8 {
            return Err(ParseError::Truncated);
        }
        let mut request = Self {
            version: (bytes[0], bytes[1]),
            operation: u16::from_be_bytes([bytes[2], bytes[3]]),
            request_id: u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            attributes: Vec::new(),
        };

        let mut position = 8;
        while position < bytes.len() {
            let value_tag = bytes[position];
            position += 1;
            if value_tag == tag::END_OF_ATTRIBUTES {
                break;
            }
            if tag::is_delimiter(value_tag) {
                continue;
            }

            let name = read_text(bytes, &mut position)?;
            let value = read_raw(bytes, &mut position)?;
            if name.is_empty() {
                // A further value of a 1setOf attribute repeats neither name nor tag.
                if let Some(previous) = request.attributes.last_mut() {
                    previous.values.push(value);
                    continue;
                }
                return Err(ParseError::NotText);
            }
            request.attributes.push(Attribute {
                name,
                tag: value_tag,
                values: vec![value],
            });
        }

        Ok(request)
    }

    pub fn version(&self) -> (u8, u8) {
        self.version
    }

    pub fn operation(&self) -> u16 {
        self.operation
    }

    pub fn request_id(&self) -> u32 {
        self.request_id
    }

    /// The first value of `name` as text, if the request carries it.
    pub fn value(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|attribute| attribute.name == name)
            .and_then(Attribute::text)
    }

    /// Whether the request uses a version the endpoint understands.
    pub fn version_is_supported(&self) -> bool {
        SUPPORTED_VERSIONS.contains(&self.version)
    }

    /// The version to answer with: the request's own when it is supported, else the oldest
    /// version every client understands.
    pub fn response_version(&self) -> (u8, u8) {
        if self.version_is_supported() {
            self.version
        } else {
            IPP_VERSION_1_1
        }
    }
}

fn read_text(bytes: &[u8], position: &mut usize) -> Result<String, ParseError> {
    let raw = read_raw(bytes, position)?;
    let text = std::str::from_utf8(&raw).map_err(|_| ParseError::NotText)?;
    Ok(text.trim_end_matches('\0').to_owned())
}

/// Reads one length-prefixed value.
///
/// Values stay raw: an integer or enum value is not text and may not be valid UTF-8.
fn read_raw(bytes: &[u8], position: &mut usize) -> Result<Vec<u8>, ParseError> {
    let length = read_u16(bytes, position)? as usize;
    let end = position.checked_add(length).ok_or(ParseError::Overrun)?;
    let slice = bytes.get(*position..end).ok_or(ParseError::Overrun)?;
    *position = end;
    Ok(slice.to_vec())
}

fn read_u16(bytes: &[u8], position: &mut usize) -> Result<u16, ParseError> {
    let end = position.checked_add(2).ok_or(ParseError::Overrun)?;
    let slice = bytes.get(*position..end).ok_or(ParseError::Overrun)?;
    *position = end;
    Ok(u16::from_be_bytes([slice[0], slice[1]]))
}

/// One printer the endpoint advertises in a response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrinterEntry {
    /// `printer-name`, the queue's name on the server.
    pub name: String,
    /// `printer-uri`, the address a client submits to.
    pub uri: String,
}

/// Builds the response to `Get-Printers` or `Get-Printer-Attributes`.
///
/// RFC 8010: the header echoes the request's version, operation id, and request id; the operation
/// attributes carry the charset, language, and status; every advertised printer becomes one
/// printer attributes group; a single end-of-attributes tag closes the section.
pub fn response(
    request_id: u32,
    version: (u8, u8),
    operation: u16,
    status: Status,
    printers: &[PrinterEntry],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(256 + printers.len() * 128);
    out.push(version.0);
    out.push(version.1);
    out.extend(operation.to_be_bytes());
    out.extend(request_id.to_be_bytes());

    out.push(tag::OPERATION_ATTRIBUTES);
    write_text(&mut out, tag::CHARSET, "attributes-charset", "utf-8");
    write_text(
        &mut out,
        tag::NATURAL_LANGUAGE,
        "attributes-natural-language",
        "en",
    );
    write_integers(
        &mut out,
        tag::INTEGER,
        "status-code",
        &[i32::from(status.code())],
    );

    for printer in printers {
        out.push(tag::PRINTER_ATTRIBUTES);
        write_text(&mut out, tag::URI, "printer-uri", &printer.uri);
        write_text(&mut out, tag::URI, "printer-uri-supported", &printer.uri);
        write_text(&mut out, tag::NAME, "printer-name", &printer.name);
        write_integers(&mut out, tag::ENUM, "printer-state", &[PRINTER_STATE_IDLE]);
        write_text(&mut out, tag::KEYWORD, "printer-state-reasons", "none");
        write_boolean(&mut out, "printer-is-accepting-jobs", true);
        write_text(&mut out, tag::CHARSET, "charset-configured", "utf-8");
        write_text(&mut out, tag::CHARSET, "charset-supported", "utf-8");
        write_text(
            &mut out,
            tag::NATURAL_LANGUAGE,
            "natural-language-configured",
            "en",
        );
        write_integers(
            &mut out,
            tag::ENUM,
            "operations-supported",
            &[
                i32::from(OPERATION_GET_PRINTER_ATTRIBUTES),
                i32::from(OPERATION_GET_PRINTERS),
            ],
        );
    }

    out.push(tag::END_OF_ATTRIBUTES);
    out
}

fn write_text(out: &mut Vec<u8>, value_tag: u8, name: &str, value: &str) {
    out.push(value_tag);
    write_name_and_value(out, name, value.as_bytes());
}

fn write_integers(out: &mut Vec<u8>, value_tag: u8, name: &str, values: &[i32]) {
    for (index, value) in values.iter().enumerate() {
        out.push(value_tag);
        // A 1setOf repeats the value tag and a zero-length name for every value after the first.
        let name = if index == 0 { name } else { "" };
        write_name_and_value(out, name, &value.to_be_bytes());
    }
}

fn write_boolean(out: &mut Vec<u8>, name: &str, value: bool) {
    out.push(tag::BOOLEAN);
    write_name_and_value(out, name, &[u8::from(value)]);
}

fn write_name_and_value(out: &mut Vec<u8>, name: &str, value: &[u8]) {
    out.extend((name.len() as u16).to_be_bytes());
    out.extend(name.as_bytes());
    out.extend((value.len() as u16).to_be_bytes());
    out.extend(value);
}

/// Percent-encodes the parts of a URI that a printer name may contain.
///
/// Unreserved characters (RFC 3986) stay as they are; everything else, including non-ASCII bytes,
/// becomes `%XX`.
pub fn percent_encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

/// Decodes the percent escapes a printer URI carries.
///
/// Unusable escapes are kept verbatim and the result is lossy, so a malformed URI can never make
/// the endpoint fail.
pub fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut position = 0;
    while position < bytes.len() {
        if bytes[position] == b'%' && position + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (
                hex_digit(bytes[position + 1]),
                hex_digit(bytes[position + 2]),
            ) {
                decoded.push(high * 16 + low);
                position += 3;
                continue;
            }
        }
        decoded.push(bytes[position]);
        position += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

const fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encodes a request the way an IPP client would, for the decoder tests.
    fn request(
        version: (u8, u8),
        operation: u16,
        request_id: u32,
        attributes: &[(&str, &str)],
    ) -> Vec<u8> {
        let mut out = vec![version.0, version.1];
        out.extend(operation.to_be_bytes());
        out.extend(request_id.to_be_bytes());
        out.push(tag::OPERATION_ATTRIBUTES);
        write_text(&mut out, tag::CHARSET, "attributes-charset", "utf-8");
        write_text(
            &mut out,
            tag::NATURAL_LANGUAGE,
            "attributes-natural-language",
            "en",
        );
        for (name, value) in attributes {
            write_text(&mut out, tag::URI, name, value);
        }
        out.push(tag::END_OF_ATTRIBUTES);
        out
    }

    /// One decoded attribute of a response: its group, its value tag, its name, and its values.
    struct Decoded {
        group: u8,
        value_tag: u8,
        name: String,
        values: Vec<Vec<u8>>,
    }

    /// Decodes the attribute groups of a response so the tests can check its structure instead of
    /// its raw bytes.
    fn decode(bytes: &[u8]) -> Vec<Decoded> {
        let mut attributes: Vec<Decoded> = Vec::new();
        let mut group = 0u8;
        let mut position = 8;
        while position < bytes.len() {
            let value_tag = bytes[position];
            position += 1;
            if value_tag == tag::END_OF_ATTRIBUTES {
                break;
            }
            if tag::is_delimiter(value_tag) {
                group = value_tag;
                continue;
            }
            let name = raw(bytes, &mut position);
            let value = raw(bytes, &mut position);
            if name.is_empty() {
                // A further value of a 1setOf attribute repeats neither name nor tag.
                if let Some(previous) = attributes.last_mut() {
                    previous.values.push(value);
                }
                continue;
            }
            attributes.push(Decoded {
                group,
                value_tag,
                name: String::from_utf8(name).expect("names are ASCII"),
                values: vec![value],
            });
        }
        attributes
    }

    fn raw(bytes: &[u8], position: &mut usize) -> Vec<u8> {
        let length = usize::from(read_u16(bytes, position).expect("length"));
        let end = *position + length;
        let value = bytes[*position..end].to_vec();
        *position = end;
        value
    }

    fn text(attributes: &[Decoded], name: &str) -> Option<String> {
        attributes
            .iter()
            .find(|attribute| attribute.name == name)
            .and_then(|attribute| attribute.values.first())
            .map(|value| String::from_utf8_lossy(value).into_owned())
    }

    #[test]
    fn a_request_header_and_its_attributes_are_decoded() {
        let bytes = request(
            IPP_VERSION_2_0,
            OPERATION_GET_PRINTER_ATTRIBUTES,
            42,
            &[("printer-uri", "ipps://server:8631/ipp/print/HP%20LaserJet")],
        );

        let request = Request::parse(&bytes).expect("decodes");

        assert_eq!(request.version(), IPP_VERSION_2_0);
        assert_eq!(request.operation(), OPERATION_GET_PRINTER_ATTRIBUTES);
        assert_eq!(request.request_id(), 42);
        assert_eq!(
            request.value("printer-uri"),
            Some("ipps://server:8631/ipp/print/HP%20LaserJet")
        );
        assert_eq!(request.value("attributes-charset"), Some("utf-8"));
        assert_eq!(request.value("printer-name"), None);
    }

    #[test]
    fn a_truncated_request_is_rejected() {
        assert_eq!(
            Request::parse(&[]).expect_err("empty"),
            ParseError::Truncated
        );
        assert_eq!(
            Request::parse(&[2, 0, 0x00, 0x0b, 0, 0, 0]).expect_err("short"),
            ParseError::Truncated
        );

        let mut bytes = request(IPP_VERSION_2_0, OPERATION_GET_PRINTERS, 1, &[]);
        bytes.truncate(bytes.len() - 3);
        // The end-of-attributes tag is gone, so the decoder runs out of bytes.
        assert!(Request::parse(&bytes).is_err());
    }

    #[test]
    fn a_value_length_past_the_end_is_rejected() {
        let mut bytes = vec![2, 0, 0x00, 0x0b, 0, 0, 0, 1, tag::OPERATION_ATTRIBUTES];
        bytes.push(tag::URI);
        bytes.extend(4u16.to_be_bytes());
        bytes.extend(b"name");
        bytes.extend(4000u16.to_be_bytes());

        assert_eq!(
            Request::parse(&bytes).expect_err("overrun"),
            ParseError::Overrun
        );
    }

    #[test]
    fn every_supported_version_is_accepted() {
        for version in SUPPORTED_VERSIONS {
            let bytes = request(version, OPERATION_GET_PRINTERS, 1, &[]);
            let request = Request::parse(&bytes).expect("decodes");
            assert!(request.version_is_supported());
            assert_eq!(request.response_version(), version);
        }
    }

    #[test]
    fn an_unsupported_version_is_answered_with_a_version_clients_understand() {
        let bytes = request((3, 0), OPERATION_GET_PRINTERS, 1, &[]);
        let request = Request::parse(&bytes).expect("decodes");

        assert!(!request.version_is_supported());
        assert_eq!(request.response_version(), IPP_VERSION_1_1);
    }

    #[test]
    fn a_get_printers_response_gives_every_printer_its_own_attribute_group() {
        let printers = vec![
            PrinterEntry {
                name: "HP LaserJet".to_owned(),
                uri: "ipps://server:8631/ipp/print/HP%20LaserJet".to_owned(),
            },
            PrinterEntry {
                name: "Zebra".to_owned(),
                uri: "ipps://server:8631/ipp/print/Zebra".to_owned(),
            },
        ];

        let bytes = response(
            7,
            IPP_VERSION_2_0,
            OPERATION_GET_PRINTERS,
            Status::Ok,
            &printers,
        );

        assert_eq!(&bytes[0..2], &[2, 0]);
        assert_eq!(&bytes[2..4], &OPERATION_GET_PRINTERS.to_be_bytes());
        assert_eq!(&bytes[4..8], &7u32.to_be_bytes());
        assert_eq!(bytes[bytes.len() - 1], tag::END_OF_ATTRIBUTES);

        let attributes = decode(&bytes);
        let groups = attributes
            .iter()
            .filter(|attribute| attribute.group == tag::PRINTER_ATTRIBUTES)
            .count();
        assert!(groups > 0);
        let names: Vec<String> = attributes
            .iter()
            .filter(|attribute| attribute.name == "printer-name")
            .flat_map(|attribute| attribute.values.clone())
            .map(|value| String::from_utf8_lossy(&value).into_owned())
            .collect();
        assert_eq!(names, vec!["HP LaserJet", "Zebra"]);
        assert_eq!(
            text(&attributes, "printer-uri"),
            Some("ipps://server:8631/ipp/print/HP%20LaserJet".to_owned())
        );
        // Every attribute of a response must sit in a named group.
        assert!(attributes.iter().all(|attribute| attribute.group != 0));
    }

    #[test]
    fn a_response_reports_the_status_code_and_the_endpoint_capabilities() {
        let bytes = response(
            7,
            IPP_VERSION_2_0,
            OPERATION_GET_PRINTERS,
            Status::Ok,
            &[PrinterEntry {
                name: "Zebra".to_owned(),
                uri: "ipps://server:8631/ipp/print/Zebra".to_owned(),
            }],
        );

        let attributes = decode(&bytes);
        let status = attributes
            .iter()
            .find(|attribute| attribute.name == "status-code")
            .expect("status-code is reported");
        assert_eq!(status.values, vec![vec![0, 0, 0, 0]]);
        assert_eq!(status.value_tag, tag::INTEGER);
        assert_eq!(status.group, tag::OPERATION_ATTRIBUTES);

        let operations: Vec<Vec<u8>> = attributes
            .iter()
            .filter(|attribute| attribute.name == "operations-supported")
            .flat_map(|attribute| attribute.values.clone())
            .collect();
        assert_eq!(
            operations,
            vec![
                i32::from(OPERATION_GET_PRINTER_ATTRIBUTES)
                    .to_be_bytes()
                    .to_vec(),
                i32::from(OPERATION_GET_PRINTERS).to_be_bytes().to_vec(),
            ]
        );
        assert_eq!(
            text(&attributes, "attributes-charset"),
            Some("utf-8".to_owned())
        );
    }

    #[test]
    fn a_response_without_printers_is_still_well_formed() {
        let bytes = response(
            9,
            IPP_VERSION_1_1,
            OPERATION_GET_PRINTERS,
            Status::NotFound,
            &[],
        );

        assert_eq!(&bytes[0..8], &[1, 1, 0x04, 0x02, 0, 0, 0, 9]);
        assert_eq!(bytes[bytes.len() - 1], tag::END_OF_ATTRIBUTES);

        let attributes = decode(&bytes);
        assert_eq!(
            text(&attributes, "attributes-charset"),
            Some("utf-8".to_owned())
        );
        assert_eq!(
            attributes
                .iter()
                .find(|attribute| attribute.name == "status-code")
                .map(|attribute| attribute.values.clone()),
            Some(vec![vec![0, 0, 4, 6]])
        );
        assert!(!attributes
            .iter()
            .any(|attribute| attribute.group == tag::PRINTER_ATTRIBUTES));
    }

    #[test]
    fn printer_names_are_percent_encoded_in_the_uri() {
        assert_eq!(percent_encode("HP LaserJet"), "HP%20LaserJet");
        assert_eq!(percent_encode("Queue#1/2"), "Queue%231%2F2");
        assert_eq!(percent_encode("Printer-1.final_2~3"), "Printer-1.final_2~3");
        assert_eq!(percent_encode("Håndskrift"), "H%C3%A5ndskrift");
    }

    #[test]
    fn percent_encoding_round_trips_and_tolerates_broken_escapes() {
        for name in ["HP LaserJet", "Queue#1/2", "Håndskrift", "Zebra"] {
            assert_eq!(percent_decode(&percent_encode(name)), name);
        }
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("bad%zz"), "bad%zz");
        assert_eq!(percent_decode("a%2"), "a%2");
    }
}
