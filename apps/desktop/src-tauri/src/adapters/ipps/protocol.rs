//! Minimal IPP codec (RFC 8010/8011): query support and authorized Print-Job submissions over IPPS.
//!
//! The decoder keeps the document as opaque bytes and reads only common operation/job attributes;
//! document content is never decoded, logged, or included in a response.

/// Default IPP version the endpoint uses when it cannot honour the request's version.
pub const IPP_VERSION_1_1: (u8, u8) = (1, 1);
/// Newest IPP version the endpoint understands.
pub const IPP_VERSION_2_0: (u8, u8) = (2, 0);

/// Versions an IPP client may use in a request.
const SUPPORTED_VERSIONS: [(u8, u8); 3] = [IPP_VERSION_2_0, IPP_VERSION_1_1, (1, 0)];

/// Operation ids the sharing endpoint answers.
pub const OPERATION_PRINT_JOB: u16 = 0x0002;
pub const OPERATION_VALIDATE_JOB: u16 = 0x0004;
pub const OPERATION_GET_PRINTER_ATTRIBUTES: u16 = 0x000b;
pub const OPERATION_GET_PRINTERS: u16 = 0x4002;
/// `printer-state` for a queue that is idle and able to accept a job.
const PRINTER_STATE_IDLE: i32 = 3;

/// Attribute group and value tags used by the queries the endpoint answers.
mod tag {
    pub(super) const OPERATION_ATTRIBUTES: u8 = 0x01;
    pub(super) const END_OF_ATTRIBUTES: u8 = 0x03;
    pub(super) const PRINTER_ATTRIBUTES: u8 = 0x04;
    pub(super) const JOB_ATTRIBUTES: u8 = 0x02;

    pub(super) const BOOLEAN: u8 = 0x22;
    pub(super) const INTEGER: u8 = 0x21;
    pub(super) const ENUM: u8 = 0x23;
    pub(super) const NAME: u8 = 0x42;
    pub(super) const KEYWORD: u8 = 0x44;
    pub(super) const URI: u8 = 0x45;
    pub(super) const CHARSET: u8 = 0x47;
    pub(super) const NATURAL_LANGUAGE: u8 = 0x48;
    pub(super) const MIME_MEDIA_TYPE: u8 = 0x49;
    pub(super) const RESOLUTION: u8 = 0x32;
    pub(super) const RANGE_OF_INTEGER: u8 = 0x33;

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
    /// `client-error-not-authorized`.
    NotAuthorized,
    /// `client-error-attributes-or-values-not-supported`.
    AttributesOrValuesNotSupported,
    /// `client-error-bad-request`: the request could not be decoded.
    BadRequest,
    /// `client-error-document-format-not-supported`.
    DocumentFormatNotSupported,
    /// `client-error-not-found`: no such printer is shared.
    NotFound,
    /// `server-error-not-accepting-jobs`.
    NotAcceptingJobs,
    /// `server-error-internal-error`.
    InternalError,
    /// `server-error-version-not-supported`.
    VersionNotSupported,
    /// `server-error-operation-not-supported`.
    UnsupportedOperation,
}

impl Status {
    pub const fn code(self) -> u16 {
        match self {
            Status::Ok => 0x0000,
            Status::NotAuthorized => 0x0403,
            Status::AttributesOrValuesNotSupported => 0x040B,
            Status::BadRequest => 0x0400,
            Status::NotFound => 0x0406,
            Status::NotAcceptingJobs => 0x0508,
            Status::InternalError => 0x0500,
            Status::DocumentFormatNotSupported => 0x040A,
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

/// A decoded IPP request borrowing its opaque document from the HTTP body.
pub struct Request<'a> {
    version: (u8, u8),
    operation: u16,
    request_id: u32,
    attributes: Vec<Attribute>,
    document: &'a [u8],
    document_start: usize,
}

impl<'a> Request<'a> {
    /// Decodes the header and attributes, borrowing bytes after end-of-attributes as the document.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, ParseError> {
        if bytes.len() < 8 {
            return Err(ParseError::Truncated);
        }
        let mut request = Self {
            version: (bytes[0], bytes[1]),
            operation: u16::from_be_bytes([bytes[2], bytes[3]]),
            request_id: u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            attributes: Vec::new(),
            document: &[],
            document_start: bytes.len(),
        };

        let mut position = 8;
        while position < bytes.len() {
            let value_tag = bytes[position];
            position += 1;
            if value_tag == tag::END_OF_ATTRIBUTES {
                request.document = &bytes[position..];
                request.document_start = position;
                break;
            }
            if tag::is_delimiter(value_tag) {
                continue;
            }

            let name = read_text(bytes, &mut position)?;
            let value = read_raw(bytes, &mut position)?;
            if name.is_empty() {
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
    pub fn document(&self) -> &[u8] {
        self.document
    }

    /// The offset of the document within the original IPP request body.
    pub fn document_start(&self) -> usize {
        self.document_start
    }
    /// The first value of `name` as text, if the request carries it.
    pub fn value(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|attribute| attribute.name == name)
            .and_then(Attribute::text)
    }

    /// All text values for the named attribute in order.
    pub fn text_values(&self, name: &str) -> Vec<&str> {
        self.attributes
            .iter()
            .find(|attribute| attribute.name == name)
            .map(|attribute| {
                attribute
                    .values
                    .iter()
                    .filter_map(|val| {
                        std::str::from_utf8(val)
                            .ok()
                            .map(|s| s.trim_end_matches('\0'))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// An integer-valued attribute's first value.
    pub fn integer(&self, name: &str) -> Option<i32> {
        let attribute = self
            .attributes
            .iter()
            .find(|attribute| attribute.name == name)?;
        let value: [u8; 4] = attribute.values.first()?.as_slice().try_into().ok()?;
        Some(i32::from_be_bytes(value))
    }

    /// A boolean-valued attribute's first value.
    pub fn boolean(&self, name: &str) -> Option<bool> {
        let attribute = self
            .attributes
            .iter()
            .find(|attribute| attribute.name == name)?;
        match attribute.values.first()?.as_slice() {
            [0] => Some(false),
            [1] => Some(true),
            _ => None,
        }
    }

    pub fn version_is_supported(&self) -> bool {
        SUPPORTED_VERSIONS.contains(&self.version)
    }

    pub fn response_version(&self) -> (u8, u8) {
        if self.version_is_supported() {
            self.version
        } else {
            IPP_VERSION_1_1
        }
    }
}

/// Rewrites the local printer URI and injects the Network Channel without interpreting or copying
/// document bytes. Client supplied Network Channel values are discarded before the configured
/// credential is inserted.
pub fn prepare_proxy_request(
    bytes: &[u8],
    remote_printer_uri: &str,
    network_channel: Option<&str>,
) -> Result<Vec<u8>, ParseError> {
    Request::parse(bytes)?;
    let mut output = Vec::with_capacity(bytes.len() + 128);
    output.extend_from_slice(&bytes[..8]);
    let mut position = 8;
    let mut current_name = &[][..];
    let mut replaced_printer = false;
    let mut replaced_channel = false;
    while position < bytes.len() {
        let start = position;
        let value_tag = *bytes.get(position).ok_or(ParseError::Truncated)?;
        position += 1;
        if value_tag == 0x03 {
            if !replaced_printer {
                return Err(ParseError::NotText);
            }
            if let Some(channel) = network_channel.filter(|_| !replaced_channel) {
                write_text_attribute(&mut output, 0x41, "network-channel", channel);
            }
            output.push(value_tag);
            output.extend_from_slice(&bytes[position..]);
            return Ok(output);
        }
        if (0x01..=0x05).contains(&value_tag) {
            current_name = &[][..];
            output.push(value_tag);
            continue;
        }

        let name_length = read_u16(bytes, &mut position)? as usize;
        let name_end = position
            .checked_add(name_length)
            .ok_or(ParseError::Overrun)?;
        let name = bytes.get(position..name_end).ok_or(ParseError::Overrun)?;
        if !name.is_empty() {
            current_name = name;
        }
        position = name_end;
        let value_length = read_u16(bytes, &mut position)? as usize;
        let value_end = position
            .checked_add(value_length)
            .ok_or(ParseError::Overrun)?;
        bytes.get(position..value_end).ok_or(ParseError::Overrun)?;

        if current_name == b"printer-uri" {
            if name.is_empty() {
                position = value_end;
                continue;
            }
            if replaced_printer {
                return Err(ParseError::NotText);
            }
            replaced_printer = true;
            output.push(value_tag);
            output.extend_from_slice(&(name.len() as u16).to_be_bytes());
            output.extend_from_slice(name);
            output.extend_from_slice(&(remote_printer_uri.len() as u16).to_be_bytes());
            output.extend_from_slice(remote_printer_uri.as_bytes());
        } else if current_name == b"network-channel" {
            if name.is_empty() {
                position = value_end;
                continue;
            }
            if replaced_channel {
                return Err(ParseError::NotText);
            }
            replaced_channel = true;
            if let Some(channel) = network_channel {
                write_text_attribute(&mut output, 0x41, "network-channel", channel);
            }
        } else {
            output.extend_from_slice(&bytes[start..value_end]);
        }
        position = value_end;
    }
    Err(ParseError::Truncated)
}

/// Rewrites the advertised URI for a proxied printer so the local client sees the URI it used to
/// reach the proxy instead of the server's IPPS URI.
pub fn rewrite_printer_uri_supported(
    bytes: &[u8],
    remote_printer_uri: &str,
    local_printer_uri: &str,
) -> Result<Vec<u8>, ParseError> {
    Request::parse(bytes)?;

    let mut output = Vec::with_capacity(bytes.len() + local_printer_uri.len());
    output.extend_from_slice(&bytes[..8]);
    let mut position = 8;
    let mut current_name = &[][..];
    let mut rewrite_security_in_current_group = false;
    while position < bytes.len() {
        let start = position;
        let value_tag = *bytes.get(position).ok_or(ParseError::Truncated)?;
        position += 1;
        if value_tag == tag::END_OF_ATTRIBUTES {
            output.push(value_tag);
            output.extend_from_slice(&bytes[position..]);
            return Ok(output);
        }
        if tag::is_delimiter(value_tag) {
            if value_tag == tag::PRINTER_ATTRIBUTES {
                rewrite_security_in_current_group = false;
            }
            output.push(value_tag);
            continue;
        }

        let name_length = read_u16(bytes, &mut position)? as usize;
        let name_end = position
            .checked_add(name_length)
            .ok_or(ParseError::Overrun)?;
        let name = bytes.get(position..name_end).ok_or(ParseError::Overrun)?;
        if !name.is_empty() {
            current_name = name;
        }
        position = name_end;
        let value_length = read_u16(bytes, &mut position)? as usize;
        let value_end = position
            .checked_add(value_length)
            .ok_or(ParseError::Overrun)?;
        let value = bytes.get(position..value_end).ok_or(ParseError::Overrun)?;

        let matches_remote_printer = current_name == b"printer-uri-supported"
            && std::str::from_utf8(value)
                .is_ok_and(|value| same_printer_uri(value, remote_printer_uri));
        if matches_remote_printer {
            rewrite_security_in_current_group = true;
            output.push(value_tag);
            output.extend((name_length as u16).to_be_bytes());
            output.extend_from_slice(name);
            output.extend((local_printer_uri.len() as u16).to_be_bytes());
            output.extend_from_slice(local_printer_uri.as_bytes());
        } else if current_name == b"uri-security-supported"
            && rewrite_security_in_current_group
            && local_printer_uri.starts_with("ipp://")
        {
            let none_value = b"none";
            output.push(value_tag);
            output.extend((name_length as u16).to_be_bytes());
            output.extend_from_slice(name);
            output.extend((none_value.len() as u16).to_be_bytes());
            output.extend_from_slice(none_value);
        } else {
            output.extend_from_slice(&bytes[start..value_end]);
        }
        position = value_end;
    }
    Err(ParseError::Truncated)
}

fn same_printer_uri(left: &str, right: &str) -> bool {
    fn parts(uri: &str) -> Option<(&str, &str, &str)> {
        let (scheme, remainder) = uri.split_once("://")?;
        let (authority, path) = remainder.split_once('/')?;
        let printer = path.strip_prefix("ipp/print/")?;
        (!authority.is_empty()).then_some((scheme, authority, printer))
    }

    let (
        Some((left_scheme, left_authority, left_printer)),
        Some((right_scheme, right_authority, right_printer)),
    ) = (parts(left), parts(right))
    else {
        return false;
    };
    left_scheme.eq_ignore_ascii_case(right_scheme)
        && left_authority.eq_ignore_ascii_case(right_authority)
        && percent_decode(left_printer).eq_ignore_ascii_case(&percent_decode(right_printer))
}

fn write_text_attribute(output: &mut Vec<u8>, value_tag: u8, name: &str, value: &str) {
    output.push(value_tag);
    output.extend((name.len() as u16).to_be_bytes());
    output.extend(name.as_bytes());
    output.extend((value.len() as u16).to_be_bytes());
    output.extend(value.as_bytes());
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
    /// Whether the server can accept an authorized job right now.
    pub accepting_jobs: bool,
}

/// Deterministic RFC 4122 UUID for a shared printer queue.
pub fn printer_uuid(name: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(b"shaprint-printer-uuid:");
    hasher.update(name.as_bytes());
    let hash = hasher.finalize();
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&hash[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50; // UUID version 5
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // RFC 4122 variant
    format!(
        "urn:uuid:{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5],
        bytes[6], bytes[7],
        bytes[8], bytes[9],
        bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    )
}

fn response_start(request_id: u32, version: (u8, u8), status: Status, capacity: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(capacity);
    out.push(version.0);
    out.push(version.1);
    out.extend(status.code().to_be_bytes());
    out.extend(request_id.to_be_bytes());
    out.push(tag::OPERATION_ATTRIBUTES);
    write_text(&mut out, tag::CHARSET, "attributes-charset", "utf-8");
    write_text(
        &mut out,
        tag::NATURAL_LANGUAGE,
        "attributes-natural-language",
        "en",
    );
    out
}
/// Builds the response to `Get-Printers` or `Get-Printer-Attributes`.
///
/// RFC 8010: a response header carries the version, the status code (in place of the request's
/// operation id, §3.4.3), and the request id; the operation attributes carry the charset and
/// language; every advertised printer becomes one printer attributes group; a single
/// end-of-attributes tag closes the section.
pub fn response(
    request_id: u32,
    version: (u8, u8),
    status: Status,
    printers: &[PrinterEntry],
) -> Vec<u8> {
    let mut out = response_start(request_id, version, status, 256 + printers.len() * 1024);
    for printer in printers {
        out.push(tag::PRINTER_ATTRIBUTES);
        // `printer-uri` is an operation attribute in a request; a printer attributes group is
        // identified by `printer-uri-supported` (RFC 8011 §5.4.1).
        write_text(&mut out, tag::URI, "printer-uri-supported", &printer.uri);
        write_text(
            &mut out,
            tag::URI,
            "printer-uuid",
            &printer_uuid(&printer.name),
        );
        write_text(&mut out, tag::NAME, "printer-name", &printer.name);
        write_text(&mut out, 0x41, "printer-make-and-model", &printer.name);
        write_text(&mut out, 0x41, "printer-info", &printer.name);
        write_text(
            &mut out,
            tag::MIME_MEDIA_TYPE,
            "document-format-default",
            "image/pwg-raster",
        );
        write_texts(
            &mut out,
            tag::MIME_MEDIA_TYPE,
            "document-format-supported",
            &[
                "image/pwg-raster",
                "application/oxps",
                "application/pdf",
                "application/PCLm",
                "application/octet-stream",
            ],
        );
        write_text(&mut out, tag::KEYWORD, "media-default", "iso_a4_210x297mm");
        write_texts(
            &mut out,
            tag::KEYWORD,
            "media-supported",
            &[
                "iso_a4_210x297mm",
                "na_letter_8.5x11in",
                "na_legal_8.5x14in",
                "iso_a3_297x420mm",
                "iso_a5_148x210mm",
            ],
        );
        write_texts(
            &mut out,
            tag::KEYWORD,
            "media-ready",
            &[
                "iso_a4_210x297mm",
                "na_letter_8.5x11in",
                "na_legal_8.5x14in",
                "iso_a3_297x420mm",
                "iso_a5_148x210mm",
            ],
        );
        write_text(&mut out, tag::KEYWORD, "sides-default", "one-sided");
        write_texts(&mut out, tag::KEYWORD, "sides-supported", &["one-sided"]);
        write_text(&mut out, tag::KEYWORD, "print-color-mode-default", "color");
        write_texts(
            &mut out,
            tag::KEYWORD,
            "print-color-mode-supported",
            &["color", "monochrome"],
        );
        write_boolean(&mut out, "color-supported", true);
        write_integers(&mut out, tag::INTEGER, "copies-default", &[1]);
        write_range_of_integers(&mut out, "copies-supported", 1, 9999);
        write_integers(&mut out, tag::ENUM, "orientation-requested-default", &[3]);
        write_integers(
            &mut out,
            tag::ENUM,
            "orientation-requested-supported",
            &[3, 4],
        );
        write_resolution(&mut out, "printer-resolution-default", 300, 300, 3);
        write_resolution(&mut out, "printer-resolution-supported", 300, 300, 3);
        write_resolution(
            &mut out,
            "pwg-raster-document-resolution-supported",
            300,
            300,
            3,
        );
        write_texts(
            &mut out,
            tag::KEYWORD,
            "pwg-raster-document-type-supported",
            &["sgray_8", "srgb_8"],
        );
        write_text(&mut out, tag::KEYWORD, "output-bin-default", "face-down");
        write_texts(
            &mut out,
            tag::KEYWORD,
            "output-bin-supported",
            &["face-down"],
        );
        write_text(
            &mut out,
            tag::KEYWORD,
            "pdl-override-supported",
            "not-attempted",
        );
        write_text(&mut out, tag::KEYWORD, "uri-security-supported", "tls");
        write_text(
            &mut out,
            tag::KEYWORD,
            "uri-authentication-supported",
            "none",
        );

        write_texts(
            &mut out,
            tag::KEYWORD,
            "ipp-versions-supported",
            &["2.0", "1.1"],
        );
        write_integers(&mut out, tag::ENUM, "printer-state", &[PRINTER_STATE_IDLE]);
        write_text(&mut out, tag::KEYWORD, "printer-state-reasons", "none");
        // Print-Job is implemented; authorization is checked when each job arrives.
        write_boolean(
            &mut out,
            "printer-is-accepting-jobs",
            printer.accepting_jobs,
        );
        write_text(&mut out, tag::CHARSET, "charset-configured", "utf-8");
        write_text(&mut out, tag::CHARSET, "charset-supported", "utf-8");
        write_text(
            &mut out,
            tag::NATURAL_LANGUAGE,
            "natural-language-configured",
            "en",
        );
        write_text(
            &mut out,
            tag::NATURAL_LANGUAGE,
            "generated-natural-language-supported",
            "en",
        );
        let operations = if printer.accepting_jobs {
            &[
                i32::from(OPERATION_PRINT_JOB),
                i32::from(OPERATION_VALIDATE_JOB),
                i32::from(OPERATION_GET_PRINTER_ATTRIBUTES),
                i32::from(OPERATION_GET_PRINTERS),
            ][..]
        } else {
            &[
                i32::from(OPERATION_GET_PRINTER_ATTRIBUTES),
                i32::from(OPERATION_GET_PRINTERS),
            ][..]
        };
        write_integers(&mut out, tag::ENUM, "operations-supported", operations);
    }

    out.push(tag::END_OF_ATTRIBUTES);
    out
}

/// Builds the RFC 8011 success response to a submitted Print-Job.
pub fn job_response(request_id: u32, version: (u8, u8), job_id: u32, job_uri: &str) -> Vec<u8> {
    let mut out = response_start(request_id, version, Status::Ok, 160 + job_uri.len());
    out.push(tag::JOB_ATTRIBUTES);
    write_text(&mut out, tag::URI, "job-uri", job_uri);
    out.push(tag::INTEGER);
    write_name_and_value(&mut out, "job-id", &job_id.to_be_bytes());
    write_integers(&mut out, tag::ENUM, "job-state", &[3]);
    write_text(&mut out, tag::KEYWORD, "job-state-reasons", "none");
    out.push(tag::END_OF_ATTRIBUTES);
    out
}

fn write_resolution(out: &mut Vec<u8>, name: &str, xres: i32, yres: i32, units: u8) {
    out.push(tag::RESOLUTION);
    let mut val = [0u8; 9];
    val[0..4].copy_from_slice(&xres.to_be_bytes());
    val[4..8].copy_from_slice(&yres.to_be_bytes());
    val[8] = units;
    write_name_and_value(out, name, &val);
}

fn write_range_of_integers(out: &mut Vec<u8>, name: &str, lower: i32, upper: i32) {
    out.push(tag::RANGE_OF_INTEGER);
    let mut val = [0u8; 8];
    val[0..4].copy_from_slice(&lower.to_be_bytes());
    val[4..8].copy_from_slice(&upper.to_be_bytes());
    write_name_and_value(out, name, &val);
}

fn write_text(out: &mut Vec<u8>, value_tag: u8, name: &str, value: &str) {
    out.push(value_tag);
    write_name_and_value(out, name, value.as_bytes());
}

fn write_texts(out: &mut Vec<u8>, value_tag: u8, name: &str, values: &[&str]) {
    for (index, value) in values.iter().enumerate() {
        out.push(value_tag);
        // A 1setOf repeats the value tag and a zero-length name after the first value.
        write_name_and_value(out, if index == 0 { name } else { "" }, value.as_bytes());
    }
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

    fn texts(attributes: &[Decoded], name: &str) -> Vec<String> {
        attributes
            .iter()
            .find(|attribute| attribute.name == name)
            .map(|attribute| {
                attribute
                    .values
                    .iter()
                    .map(|value| String::from_utf8_lossy(value).into_owned())
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn printer_attributes_advertise_all_supported_media_sizes_and_ready_media() {
        let bytes = response(
            11,
            IPP_VERSION_2_0,
            Status::Ok,
            &[PrinterEntry {
                name: "LaserJet".to_owned(),
                uri: "ipps://server:8631/ipp/print/LaserJet".to_owned(),
                accepting_jobs: true,
            }],
        );
        let attributes = decode(&bytes);
        assert_eq!(
            text(&attributes, "media-default"),
            Some("iso_a4_210x297mm".to_owned())
        );
        let supported = texts(&attributes, "media-supported");
        assert_eq!(
            supported,
            vec![
                "iso_a4_210x297mm",
                "na_letter_8.5x11in",
                "na_legal_8.5x14in",
                "iso_a3_297x420mm",
                "iso_a5_148x210mm",
            ]
        );
        let ready = texts(&attributes, "media-ready");
        assert!(!ready.is_empty());
        assert!(ready.contains(&"iso_a4_210x297mm".to_string()));
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
        assert_eq!(Request::parse(&[]).err(), Some(ParseError::Truncated));
        assert_eq!(
            Request::parse(&[2, 0, 0x00, 0x0b, 0, 0, 0]).err(),
            Some(ParseError::Truncated)
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

        assert_eq!(Request::parse(&bytes).err(), Some(ParseError::Overrun));
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
                accepting_jobs: true,
            },
            PrinterEntry {
                name: "Zebra".to_owned(),
                uri: "ipps://server:8631/ipp/print/Zebra".to_owned(),
                accepting_jobs: true,
            },
        ];

        let bytes = response(7, IPP_VERSION_2_0, Status::Ok, &printers);

        assert_eq!(&bytes[0..2], &[2, 0]);
        assert_eq!(&bytes[2..4], &[0, 0]);
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
            text(&attributes, "printer-uri-supported"),
            Some("ipps://server:8631/ipp/print/HP%20LaserJet".to_owned())
        );
        // A printer attributes group is identified by `printer-uri-supported`; `printer-uri`
        // belongs to requests.
        assert!(!attributes
            .iter()
            .any(|attribute| attribute.name == "printer-uri"));
        assert_eq!(
            text(&attributes, "uri-security-supported"),
            Some("tls".to_owned())
        );
        assert_eq!(
            attributes
                .iter()
                .find(|attribute| attribute.name == "printer-name")
                .map(|attribute| attribute.value_tag),
            Some(tag::NAME)
        );
        assert_eq!(
            attributes
                .iter()
                .find(|attribute| attribute.name == "printer-is-accepting-jobs")
                .map(|attribute| attribute.values.clone()),
            Some(vec![vec![1]])
        );
        assert_eq!(
            text(&attributes, "ipp-versions-supported"),
            Some("2.0".to_owned())
        );
        // Every attribute of a response must sit in a named group.
        assert!(attributes.iter().all(|attribute| attribute.group != 0));
    }

    #[test]
    fn a_response_reports_its_status_in_the_header_and_its_capabilities() {
        let bytes = response(
            7,
            IPP_VERSION_2_0,
            Status::Ok,
            &[PrinterEntry {
                name: "Zebra".to_owned(),
                uri: "ipps://server:8631/ipp/print/Zebra".to_owned(),
                accepting_jobs: true,
            }],
        );

        // RFC 8010 §3.4.3: the status code sits in the third and fourth bytes, where a request
        // carries its operation id.
        assert_eq!(&bytes[2..4], &Status::Ok.code().to_be_bytes());

        let attributes = decode(&bytes);
        let operations: Vec<Vec<u8>> = attributes
            .iter()
            .filter(|attribute| attribute.name == "operations-supported")
            .flat_map(|attribute| attribute.values.clone())
            .collect();
        assert_eq!(
            operations,
            vec![
                i32::from(OPERATION_PRINT_JOB).to_be_bytes().to_vec(),
                i32::from(OPERATION_VALIDATE_JOB).to_be_bytes().to_vec(),
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
        let bytes = response(9, IPP_VERSION_1_1, Status::NotFound, &[]);

        // 0x0406 client-error-not-found in the header's status field.
        assert_eq!(&bytes[0..8], &[1, 1, 0x04, 0x06, 0, 0, 0, 9]);
        assert_eq!(bytes[bytes.len() - 1], tag::END_OF_ATTRIBUTES);

        let attributes = decode(&bytes);
        assert_eq!(
            text(&attributes, "attributes-charset"),
            Some("utf-8".to_owned())
        );
        assert!(!attributes
            .iter()
            .any(|attribute| attribute.name == "status-code"));
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

    #[test]
    fn printer_uuid_is_deterministic_and_urn_uuid_formatted() {
        let uuid1 = printer_uuid("Office Printer");
        let uuid2 = printer_uuid("Office Printer");
        let uuid_other = printer_uuid("Other Printer");

        assert_eq!(uuid1, uuid2);
        assert_ne!(uuid1, uuid_other);
        assert!(uuid1.starts_with("urn:uuid:"));
        let hex_part = uuid1.strip_prefix("urn:uuid:").unwrap();
        assert_eq!(hex_part.len(), 36);
        let segments: Vec<&str> = hex_part.split('-').collect();
        assert_eq!(segments.len(), 5);
        assert_eq!(segments[0].len(), 8);
        assert_eq!(segments[1].len(), 4);
        assert_eq!(segments[2].len(), 4);
        assert_eq!(segments[3].len(), 4);
        assert_eq!(segments[4].len(), 12);
        // Version 5
        assert!(segments[2].starts_with('5'));
    }

    #[test]
    fn rewrite_printer_uri_supported_changes_security_to_none_for_ipp_scheme() {
        let remote_uri = "ipps://server:8631/ipp/print/Office%20Printer";
        let local_uri = "ipp://127.0.0.1:8632/ipp/print/server%3A8631/Office%20Printer";
        let bytes = response(
            1,
            IPP_VERSION_2_0,
            Status::Ok,
            &[PrinterEntry {
                name: "Office Printer".to_owned(),
                uri: remote_uri.to_owned(),
                accepting_jobs: true,
            }],
        );

        let rewritten =
            rewrite_printer_uri_supported(&bytes, remote_uri, local_uri).expect("rewrites");
        let attributes = decode(&rewritten);
        assert_eq!(
            text(&attributes, "printer-uri-supported"),
            Some(local_uri.to_owned())
        );
        assert_eq!(
            text(&attributes, "uri-security-supported"),
            Some("none".to_owned())
        );
        assert_eq!(
            text(&attributes, "printer-uuid"),
            Some(printer_uuid("Office Printer"))
        );
    }

    #[test]
    fn prepare_proxy_request_discards_client_network_channel_and_secondary_values() {
        let mut request = Vec::new();
        request.extend_from_slice(&[2, 0, 0, 2, 0, 0, 0, 1]); // Print-Job request id 1
        request.push(tag::OPERATION_ATTRIBUTES);
        write_text_attribute(
            &mut request,
            tag::URI,
            "printer-uri",
            "ipp://127.0.0.1:8632/local",
        );
        // Extra 1setOf value for printer-uri (empty name)
        request.push(tag::URI);
        request.extend_from_slice(&0u16.to_be_bytes());
        request.extend_from_slice(&(b"ipp://127.0.0.1:8632/extra".len() as u16).to_be_bytes());
        request.extend_from_slice(b"ipp://127.0.0.1:8632/extra");

        // Client-supplied network-channel with 1setOf extra value
        write_text_attribute(&mut request, 0x41, "network-channel", "rogue-secret");
        request.push(0x41);
        request.extend_from_slice(&0u16.to_be_bytes());
        request.extend_from_slice(&(b"extra-rogue".len() as u16).to_be_bytes());
        request.extend_from_slice(b"extra-rogue");

        request.push(tag::END_OF_ATTRIBUTES);
        request.extend_from_slice(b"fake document payload");

        let prepared = prepare_proxy_request(
            &request,
            "ipps://server:8631/remote",
            Some("configured-secret"),
        )
        .expect("prepares proxy request");

        let parsed = Request::parse(&prepared).expect("valid IPP request");
        assert_eq!(
            parsed.value("printer-uri"),
            Some("ipps://server:8631/remote")
        );
        assert_eq!(
            parsed.text_values("printer-uri"),
            vec!["ipps://server:8631/remote"]
        );
        assert_eq!(parsed.value("network-channel"), Some("configured-secret"));
        assert_eq!(
            parsed.text_values("network-channel"),
            vec!["configured-secret"]
        );
        assert_eq!(parsed.document(), b"fake document payload");
    }
}
