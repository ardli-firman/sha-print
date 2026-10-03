//! The multicast-DNS wire format: queries, announcements, goodbyes, and the record reader.
//!
//! ShaPrint speaks the subset of DNS-SD (RFC 6763) it needs over the message format of RFC 1035,
//! with the multicast rules of RFC 6762: one PTR question asks for a service type, and each
//! responder answers with a PTR to its instance plus the SRV and TXT records that describe it.
//! Withdrawing a service is the same answer with a zero lifetime, so a browser drops it immediately
//! instead of waiting for a cache entry to expire.
//!
//! The reader is defensive because every packet it sees comes from the network: truncated packets,
//! reserved label types, and compression loops are rejected as `invalid-input` instead of being
//! followed or panicking.

use std::time::Duration;

use crate::domain::AppError;

/// The DNS-SD service type ShaPrint servers advertise under.
///
/// Deliberately private to ShaPrint rather than the generic `_ipps._tcp`: a ShaPrint browser must
/// not offer every AirPrint printer on the network as a server to approve (ADR 0004).
pub const SERVICE_TYPE: &str = "_shaprint-ipps._tcp.local.";

pub const RECORD_TYPE_PTR: u16 = 12;
pub const RECORD_TYPE_TXT: u16 = 16;
pub const RECORD_TYPE_SRV: u16 = 33;

/// Message flags for a question: the response bit is clear and the opcode is a standard query.
const MESSAGE_FLAGS_QUERY: u16 = 0x0000;
/// Message flags for an authoritative response.
const MESSAGE_FLAGS_RESPONSE: u16 = 0x8400;

/// The internet class, the only class ShaPrint answers in.
pub const CLASS_IN: u16 = 1;

/// The top bit of a class field. In a question it asks the responder to answer this socket directly
/// instead of multicasting (RFC 6762 §5.4); in a record it means "this replaces what you cached"
/// (RFC 6762 §10.2).
pub const UNICAST_OR_CACHE_FLUSH: u16 = 0x8000;

/// How many compression pointers one name may follow before the packet is treated as malformed; a
/// pointer loop would otherwise make the reader spin forever.
const MAX_NAME_JUMPS: usize = 64;
/// Longest label a DNS name may carry (RFC 1035 §2.3.4).
const MAX_LABEL: usize = 63;
/// Longest string a TXT record may carry.
const MAX_TXT_STRING: usize = 255;
/// Priority, weight, and port: the bytes a service record spends before its target name.
const FIXED_SERVICE_LENGTH: usize = 6;

/// One question a peer asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    pub name: String,
    pub record_type: u16,
    /// Whether the asker wants the answer on its own socket rather than multicast.
    pub unicast_response: bool,
}

/// The data of one resource record, decoded as far as ShaPrint needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordData {
    /// A `PTR`: the instance a service type currently has.
    Pointer(String),
    /// An `SRV`: where the instance can be reached.
    Service {
        priority: u16,
        weight: u16,
        port: u16,
        target: String,
    },
    /// A `TXT`: the instance's key/value properties, in wire order.
    Text(Vec<(String, String)>),
    /// A record type ShaPrint does not interpret.
    Other(Vec<u8>),
}

/// One resource record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub name: String,
    pub record_type: u16,
    /// Whether the sender wants this record to replace a cached one.
    pub cache_flush: bool,
    /// How long the sender wants this record cached; zero withdraws it.
    pub ttl: Duration,
    pub data: RecordData,
}

/// A decoded packet: the questions it asks and the records it reports.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Message {
    pub queries: Vec<Query>,
    pub records: Vec<Record>,
}

impl Message {
    /// Reads one received packet.
    pub fn parse(packet: &[u8]) -> Result<Self, AppError> {
        let mut reader = Reader::new(packet);
        let _id = reader.u16()?;
        let _flags = reader.u16()?;
        let question_count = reader.u16()?;
        let answer_count = reader.u16()?;
        let authority_count = reader.u16()?;
        let additional_count = reader.u16()?;

        let mut queries = Vec::new();
        for _ in 0..question_count {
            let name = reader.name()?;
            let record_type = reader.u16()?;
            let class = reader.u16()?;
            queries.push(Query {
                name,
                record_type,
                unicast_response: class & UNICAST_OR_CACHE_FLUSH != 0,
            });
        }

        // Every record has to be read even when it is not interesting: the sections follow one
        // another in the packet, so stopping early would misplace the rest. The counts arrive from
        // the network, so they are summed as sizes: three maximal counts do not fit in a u16, and a
        // packet must never be able to overflow the reader's arithmetic.
        let total_records = usize::from(answer_count)
            + usize::from(authority_count)
            + usize::from(additional_count);
        let mut records = Vec::new();
        for _ in 0..total_records {
            let name = reader.name()?;
            let record_type = reader.u16()?;
            let class = reader.u16()?;
            let ttl = reader.u32()?;
            let length = reader.u16()? as usize;
            let start = reader.position;
            let bytes = reader.take(length)?;
            records.push(Record {
                name,
                record_type,
                cache_flush: class & UNICAST_OR_CACHE_FLUSH != 0,
                ttl: Duration::from_secs(u64::from(ttl)),
                data: decode_data(record_type, packet, start, bytes)?,
            });
        }

        Ok(Self { queries, records })
    }
}

/// One service instance an announcement describes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    /// The full instance name, which is how a browser tells one advertisement from another.
    pub instance: String,
    /// The host name the instance runs on.
    pub target: String,
    pub port: u16,
    /// The `TXT` properties, in wire order.
    pub properties: Vec<(String, String)>,
    /// How long the advertisement stays valid; zero means it has been withdrawn.
    pub ttl: Duration,
}

/// Reads the service instances a response advertises.
///
/// An instance is only reported when the answer carries both the `SRV` that says where it is and
/// the `TXT` that says what it shares: a browser cannot connect with half an advertisement.
pub fn services(message: &Message) -> Vec<Service> {
    let mut found = Vec::new();
    for pointer in &message.records {
        if pointer.record_type != RECORD_TYPE_PTR || !is_named(&pointer.name, SERVICE_TYPE) {
            continue;
        }
        let RecordData::Pointer(instance) = &pointer.data else {
            continue;
        };
        let transport = message.records.iter().find(|record| {
            is_named(&record.name, instance) && matches!(record.data, RecordData::Service { .. })
        });
        let text = message.records.iter().find(|record| {
            is_named(&record.name, instance) && matches!(record.data, RecordData::Text(_))
        });
        let (Some(transport), Some(text)) = (transport, text) else {
            continue;
        };
        let RecordData::Service { port, target, .. } = &transport.data else {
            continue;
        };
        let RecordData::Text(properties) = &text.data else {
            continue;
        };
        found.push(Service {
            instance: instance.clone(),
            target: target.clone(),
            port: *port,
            properties: properties.clone(),
            // The PTR answers "is this instance present?"; its lifetime is what a browser caches.
            ttl: pointer.ttl,
        });
    }
    found
}

/// Whether `query` asks for the instances of `service_type`.
pub fn queries_service(query: &Query, service_type: &str) -> bool {
    query.record_type == RECORD_TYPE_PTR && is_named(&query.name, service_type)
}

/// Whether two DNS names are the same name: DNS comparison ignores case and the trailing dot.
fn is_named(left: &str, right: &str) -> bool {
    left.trim_end_matches('.')
        .eq_ignore_ascii_case(right.trim_end_matches('.'))
}

/// Encodes a browse query for `service_type`, asking for the answer on this socket.
pub fn encode_query(service_type: &str) -> Result<Vec<u8>, AppError> {
    let mut packet = header(MESSAGE_FLAGS_QUERY, 1, 0);
    push_name(&mut packet, service_type)?;
    packet.extend_from_slice(&RECORD_TYPE_PTR.to_be_bytes());
    packet.extend_from_slice(&(CLASS_IN | UNICAST_OR_CACHE_FLUSH).to_be_bytes());
    Ok(packet)
}

/// Encodes the answer for one service instance.
///
/// The same bytes serve as the announcement a server multicasts when it starts sharing, the answer
/// it sends to a browse query, and — with a zero `ttl` — the goodbye that withdraws it.
pub fn encode_announcement(
    label: &str,
    target: &str,
    port: u16,
    properties: &[(String, String)],
    ttl: Duration,
) -> Result<Vec<u8>, AppError> {
    let instance = instance_name(label)?;
    let lifetime = record_lifetime(ttl)?;

    let mut packet = header(MESSAGE_FLAGS_RESPONSE, 0, 3);
    // PTR: the service type currently has this instance. A shared record, so no cache-flush bit.
    push_name(&mut packet, SERVICE_TYPE)?;
    packet.extend_from_slice(&RECORD_TYPE_PTR.to_be_bytes());
    packet.extend_from_slice(&CLASS_IN.to_be_bytes());
    packet.extend_from_slice(&lifetime.to_be_bytes());
    let mut pointer = Vec::new();
    push_name(&mut pointer, &instance)?;
    push_record_data(&mut packet, &pointer)?;

    // SRV: where the instance is, and which port its endpoint listens on.
    push_name(&mut packet, &instance)?;
    packet.extend_from_slice(&RECORD_TYPE_SRV.to_be_bytes());
    packet.extend_from_slice(&(CLASS_IN | UNICAST_OR_CACHE_FLUSH).to_be_bytes());
    packet.extend_from_slice(&lifetime.to_be_bytes());
    let mut service = Vec::new();
    service.extend_from_slice(&0u16.to_be_bytes()); // priority
    service.extend_from_slice(&0u16.to_be_bytes()); // weight
    service.extend_from_slice(&port.to_be_bytes());
    push_name(&mut service, target)?;
    push_record_data(&mut packet, &service)?;

    // TXT: what the instance shares, as `key=value` strings.
    push_name(&mut packet, &instance)?;
    packet.extend_from_slice(&RECORD_TYPE_TXT.to_be_bytes());
    packet.extend_from_slice(&(CLASS_IN | UNICAST_OR_CACHE_FLUSH).to_be_bytes());
    packet.extend_from_slice(&lifetime.to_be_bytes());
    let mut text = Vec::new();
    for (key, value) in properties {
        let entry = format!("{key}={value}");
        if entry.len() > MAX_TXT_STRING {
            return Err(AppError::invalid_input(format!(
                "the advertised property '{key}' is longer than {MAX_TXT_STRING} characters"
            )));
        }
        text.push(entry.len() as u8);
        text.extend_from_slice(entry.as_bytes());
    }
    push_record_data(&mut packet, &text)?;

    Ok(packet)
}

/// The lifetime to put in a record.
///
/// A record carries whole seconds and zero means "gone", so a lifetime shorter than a second is
/// rounded up rather than down: truncating it would withdraw a server that is still sharing.
fn record_lifetime(ttl: Duration) -> Result<u32, AppError> {
    let seconds = if ttl.is_zero() {
        0
    } else {
        ttl.as_secs() + u64::from(ttl.subsec_nanos() > 0)
    };
    u32::try_from(seconds)
        .map_err(|_| AppError::invalid_input("the advertised lifetime does not fit in a record"))
}

/// The full instance name for a server label, rejecting labels DNS cannot carry unambiguously.
pub fn instance_name(label: &str) -> Result<String, AppError> {
    let label = label.trim();
    let usable = !label.is_empty()
        && label.len() <= MAX_LABEL
        && !label.contains('.')
        && label.bytes().all(|byte| (0x20..0x7F).contains(&byte));
    if !usable {
        return Err(AppError::invalid_input(format!(
            "'{label}' cannot be used as a discovery instance name"
        )));
    }
    Ok(format!("{label}.{SERVICE_TYPE}"))
}

/// A message header carrying `questions` questions and `answers` records.
///
/// The order follows the wire layout (RFC 1035 §4.1.1): both counters are `u16`, so a swapped
/// argument would compile and only show up on the network.
fn header(flags: u16, questions: u16, answers: u16) -> Vec<u8> {
    let mut packet = Vec::new();
    packet.extend_from_slice(&0u16.to_be_bytes()); // id: multicast DNS carries no id
    packet.extend_from_slice(&flags.to_be_bytes());
    packet.extend_from_slice(&questions.to_be_bytes());
    packet.extend_from_slice(&answers.to_be_bytes());
    packet.extend_from_slice(&0u16.to_be_bytes());
    packet.extend_from_slice(&0u16.to_be_bytes());
    packet
}

/// Appends `name` in label form, ending at the root label.
fn push_name(out: &mut Vec<u8>, name: &str) -> Result<(), AppError> {
    let mut wrote_label = false;
    for label in name.split('.') {
        if label.is_empty() {
            continue;
        }
        if label.len() > MAX_LABEL {
            return Err(AppError::invalid_input(format!(
                "the DNS name '{name}' has a label longer than {MAX_LABEL} characters"
            )));
        }
        out.push(label.len() as u8);
        out.extend_from_slice(label.as_bytes());
        wrote_label = true;
    }
    if !wrote_label {
        return Err(AppError::invalid_input(format!(
            "the DNS name '{name}' is empty"
        )));
    }
    out.push(0);
    Ok(())
}

fn push_record_data(packet: &mut Vec<u8>, data: &[u8]) -> Result<(), AppError> {
    let length = u16::try_from(data.len())
        .map_err(|_| AppError::invalid_input("a DNS record is too long to encode"))?;
    packet.extend_from_slice(&length.to_be_bytes());
    packet.extend_from_slice(data);
    Ok(())
}

fn decode_data(
    record_type: u16,
    packet: &[u8],
    start: usize,
    bytes: &[u8],
) -> Result<RecordData, AppError> {
    match record_type {
        RECORD_TYPE_PTR => {
            let mut reader = Reader::new(packet);
            reader.position = start;
            let instance = reader.name()?;
            expect_consumed(&reader, start, bytes)?;
            Ok(RecordData::Pointer(instance))
        }
        RECORD_TYPE_SRV => {
            if bytes.len() < FIXED_SERVICE_LENGTH {
                return Err(malformed_with(
                    "a service record is shorter than its fixed fields",
                ));
            }
            let mut reader = Reader::new(packet);
            reader.position = start;
            let priority = reader.u16()?;
            let weight = reader.u16()?;
            let port = reader.u16()?;
            let target = reader.name()?;
            expect_consumed(&reader, start, bytes)?;
            Ok(RecordData::Service {
                priority,
                weight,
                port,
                target,
            })
        }
        RECORD_TYPE_TXT => {
            let mut properties = Vec::new();
            let mut position = 0;
            while position < bytes.len() {
                let length = bytes[position] as usize;
                position += 1;
                let end = position.checked_add(length).ok_or_else(malformed)?;
                let entry = bytes.get(position..end).ok_or_else(malformed)?;
                position = end;
                // A string without '=' is a valid TXT entry with no value (RFC 6763 §6.4).
                let text = String::from_utf8_lossy(entry).into_owned();
                let (key, value) = match text.split_once('=') {
                    Some((key, value)) => (key.to_owned(), value.to_owned()),
                    None => (text, String::new()),
                };
                properties.push((key, value));
            }
            Ok(RecordData::Text(properties))
        }
        // Everything else, including the address records a responder may publish, is carried but
        // not interpreted: the client reviews the address an answer came from, never one it claims.
        _ => Ok(RecordData::Other(bytes.to_vec())),
    }
}

/// Checks that reading a record's data consumed exactly the length the record declared.
///
/// A record's length is authoritative (RFC 1035 §4.1.3). A reader that ignores it walks into the
/// record that follows and reports a name that was never in this one.
fn expect_consumed(reader: &Reader<'_>, start: usize, data: &[u8]) -> Result<(), AppError> {
    if reader.position != start + data.len() {
        return Err(malformed_with("a record's data does not match its length"));
    }
    Ok(())
}

fn malformed() -> AppError {
    malformed_with("the packet ends before the record it describes")
}

fn malformed_with(reason: &str) -> AppError {
    AppError::invalid_input(format!("the multicast DNS packet is malformed: {reason}"))
}

/// A cursor over a packet that follows name compression.
struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn u16(&mut self) -> Result<u16, AppError> {
        let bytes = self.take(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    fn u32(&mut self) -> Result<u32, AppError> {
        let bytes = self.take(4)?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], AppError> {
        let end = self.position.checked_add(length).ok_or_else(malformed)?;
        let bytes = self.bytes.get(self.position..end).ok_or_else(malformed)?;
        self.position = end;
        Ok(bytes)
    }

    /// Reads one name, following compression pointers, and leaves the cursor after it.
    fn name(&mut self) -> Result<String, AppError> {
        let mut labels: Vec<&str> = Vec::new();
        let mut jumps = 0;
        let mut position = self.position;
        // Where to continue once this name ends; set by the first pointer encountered.
        let mut after_name = None;

        loop {
            let length = *self.bytes.get(position).ok_or_else(malformed)?;
            match length & 0xC0 {
                0x00 => {
                    position += 1;
                    if length == 0 {
                        break;
                    }
                    let end = position + length as usize;
                    let label = self.bytes.get(position..end).ok_or_else(malformed)?;
                    labels.push(
                        std::str::from_utf8(label)
                            .map_err(|_| malformed_with("a name label is not valid utf-8"))?,
                    );
                    position = end;
                }
                0xC0 => {
                    let second = *self.bytes.get(position + 1).ok_or_else(malformed)?;
                    let target = ((usize::from(length & 0x3F)) << 8) | usize::from(second);
                    after_name.get_or_insert(position + 2);
                    jumps += 1;
                    if jumps > MAX_NAME_JUMPS {
                        return Err(malformed_with("a name compression pointer loops"));
                    }
                    position = target;
                }
                // 0x40 and 0x80 are reserved label types (RFC 1035 §4.1.4).
                _ => return Err(malformed_with("a name uses a reserved label type")),
            }
        }

        self.position = after_name.unwrap_or(position);
        Ok(if labels.is_empty() {
            String::new()
        } else {
            format!("{}.", labels.join("."))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A browse query as RFC 6762 §5.4 describes one: a single PTR question whose class carries the
    /// unicast-response bit, so a responder answers this socket directly instead of multicasting.
    ///
    /// Hand-assembled from RFC 1035 §4.1.1, independently of the encoder under test.
    const QUERY: &[u8] = &[
        0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0E, 0x5F, 0x73,
        0x68, 0x61, 0x70, 0x72, 0x69, 0x6E, 0x74, 0x2D, 0x69, 0x70, 0x70, 0x73, 0x04, 0x5F, 0x74,
        0x63, 0x70, 0x05, 0x6C, 0x6F, 0x63, 0x61, 0x6C, 0x00, 0x00, 0x0C, 0x80, 0x01,
    ];

    /// A response that uses the name compression of RFC 1035 §4.1.4: the SRV and TXT records are
    /// named by a pointer to the instance label inside the PTR record, exactly as a responder that
    /// avoids repeating names would send it.
    ///
    /// Hand-assembled from RFC 1035 §4.1.1 and RFC 6763 §6, independently of the reader under test.
    const RESPONSE: &[u8] = &[
        0x00, 0x00, 0x84, 0x00, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x0E, 0x5F, 0x73,
        0x68, 0x61, 0x70, 0x72, 0x69, 0x6E, 0x74, 0x2D, 0x69, 0x70, 0x70, 0x73, 0x04, 0x5F, 0x74,
        0x63, 0x70, 0x05, 0x6C, 0x6F, 0x63, 0x61, 0x6C, 0x00, 0x00, 0x0C, 0x00, 0x01, 0x00, 0x00,
        0x00, 0x78, 0x00, 0x0E, 0x0B, 0x44, 0x45, 0x53, 0x4B, 0x54, 0x4F, 0x50, 0x2D, 0x41, 0x42,
        0x43, 0xC0, 0x0C, 0xC0, 0x31, 0x00, 0x21, 0x80, 0x01, 0x00, 0x00, 0x00, 0x78, 0x00, 0x19,
        0x00, 0x00, 0x00, 0x00, 0x21, 0xB7, 0x0B, 0x44, 0x45, 0x53, 0x4B, 0x54, 0x4F, 0x50, 0x2D,
        0x41, 0x42, 0x43, 0x05, 0x6C, 0x6F, 0x63, 0x61, 0x6C, 0x00, 0xC0, 0x31, 0x00, 0x10, 0x80,
        0x01, 0x00, 0x00, 0x00, 0x78, 0x00, 0x2A, 0x0C, 0x72, 0x70, 0x3D, 0x69, 0x70, 0x70, 0x2F,
        0x70, 0x72, 0x69, 0x6E, 0x74, 0x0B, 0x71, 0x75, 0x65, 0x75, 0x65, 0x3D, 0x5A, 0x65, 0x62,
        0x72, 0x61, 0x10, 0x6E, 0x61, 0x6D, 0x65, 0x3D, 0x44, 0x45, 0x53, 0x4B, 0x54, 0x4F, 0x50,
        0x2D, 0x41, 0x42, 0x43,
    ];

    /// The same response plus one address record in the additional section, as a responder that
    /// publishes its own address sends it.
    ///
    /// Hand-assembled like [`RESPONSE`], with the address record naming the service target through
    /// a compression pointer and carrying 192.0.2.10.
    const RESPONSE_WITH_ADDRESS: &[u8] = &[
        0x00, 0x00, 0x84, 0x00, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x01, 0x0E, 0x5F, 0x73,
        0x68, 0x61, 0x70, 0x72, 0x69, 0x6E, 0x74, 0x2D, 0x69, 0x70, 0x70, 0x73, 0x04, 0x5F, 0x74,
        0x63, 0x70, 0x05, 0x6C, 0x6F, 0x63, 0x61, 0x6C, 0x00, 0x00, 0x0C, 0x00, 0x01, 0x00, 0x00,
        0x00, 0x78, 0x00, 0x0E, 0x0B, 0x44, 0x45, 0x53, 0x4B, 0x54, 0x4F, 0x50, 0x2D, 0x41, 0x42,
        0x43, 0xC0, 0x0C, 0xC0, 0x31, 0x00, 0x21, 0x80, 0x01, 0x00, 0x00, 0x00, 0x78, 0x00, 0x19,
        0x00, 0x00, 0x00, 0x00, 0x21, 0xB7, 0x0B, 0x44, 0x45, 0x53, 0x4B, 0x54, 0x4F, 0x50, 0x2D,
        0x41, 0x42, 0x43, 0x05, 0x6C, 0x6F, 0x63, 0x61, 0x6C, 0x00, 0xC0, 0x31, 0x00, 0x10, 0x80,
        0x01, 0x00, 0x00, 0x00, 0x78, 0x00, 0x2A, 0x0C, 0x72, 0x70, 0x3D, 0x69, 0x70, 0x70, 0x2F,
        0x70, 0x72, 0x69, 0x6E, 0x74, 0x0B, 0x71, 0x75, 0x65, 0x75, 0x65, 0x3D, 0x5A, 0x65, 0x62,
        0x72, 0x61, 0x10, 0x6E, 0x61, 0x6D, 0x65, 0x3D, 0x44, 0x45, 0x53, 0x4B, 0x54, 0x4F, 0x50,
        0x2D, 0x41, 0x42, 0x43, 0xC0, 0x51, 0x00, 0x01, 0x80, 0x01, 0x00, 0x00, 0x00, 0x78, 0x00,
        0x04, 0xC0, 0x00, 0x02, 0x0A,
    ];

    /// A record whose name points at itself; a reader must reject it instead of looping.
    const POINTER_LOOP: &[u8] = &[
        0x00, 0x00, 0x84, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x0C, 0x00,
        0x0C, 0x00, 0x01, 0x00, 0x00, 0x00, 0x78, 0x00, 0x00,
    ];

    const INSTANCE: &str = "DESKTOP-ABC._shaprint-ipps._tcp.local.";
    const TARGET: &str = "DESKTOP-ABC.local.";

    fn properties() -> Vec<(String, String)> {
        vec![
            ("rp".to_owned(), "ipp/print".to_owned()),
            ("queue".to_owned(), "Zebra".to_owned()),
            ("name".to_owned(), "DESKTOP-ABC".to_owned()),
        ]
    }

    fn advertised() -> Service {
        Service {
            instance: INSTANCE.to_owned(),
            target: TARGET.to_owned(),
            port: 8631,
            properties: properties(),
            ttl: Duration::from_secs(120),
        }
    }

    #[test]
    fn a_browse_query_asks_for_the_service_type_and_a_direct_answer() {
        assert_eq!(encode_query(SERVICE_TYPE).expect("encodes a query"), QUERY);

        let message = Message::parse(QUERY).expect("parses a browse query");

        assert!(message.records.is_empty());
        assert_eq!(message.queries.len(), 1);
        assert_eq!(message.queries[0].name, SERVICE_TYPE);
        assert_eq!(message.queries[0].record_type, RECORD_TYPE_PTR);
        assert!(message.queries[0].unicast_response);
        assert!(queries_service(&message.queries[0], SERVICE_TYPE));
    }

    #[test]
    fn a_response_is_read_with_its_compressed_names() {
        let message = Message::parse(RESPONSE).expect("parses a response");

        assert_eq!(message.records.len(), 3);
        assert_eq!(message.records[0].name, SERVICE_TYPE);
        assert_eq!(message.records[0].record_type, RECORD_TYPE_PTR);
        assert_eq!(message.records[0].ttl, Duration::from_secs(120));
        assert_eq!(
            message.records[0].data,
            RecordData::Pointer(INSTANCE.to_owned())
        );
        // A PTR points at a set of instances, so it is a shared record and carries no cache-flush
        // bit; the SRV and TXT that describe one instance do (RFC 6762 §10.2).
        assert!(!message.records[0].cache_flush);

        assert_eq!(message.records[1].name, INSTANCE);
        assert_eq!(
            message.records[1].data,
            RecordData::Service {
                priority: 0,
                weight: 0,
                port: 8631,
                target: TARGET.to_owned(),
            }
        );
        assert!(message.records[1].cache_flush);

        assert_eq!(message.records[2].name, INSTANCE);
        assert_eq!(
            message.records[2].data,
            RecordData::Text(vec![
                ("rp".to_owned(), "ipp/print".to_owned()),
                ("queue".to_owned(), "Zebra".to_owned()),
                ("name".to_owned(), "DESKTOP-ABC".to_owned()),
            ])
        );

        assert_eq!(services(&message), vec![advertised()]);
    }

    #[test]
    fn an_announcement_reads_back_as_the_service_it_describes() {
        let packet = encode_announcement(
            "DESKTOP-ABC",
            TARGET,
            8631,
            &properties(),
            Duration::from_secs(120),
        )
        .expect("encodes an announcement");

        let message = Message::parse(&packet).expect("parses our own announcement");

        assert_eq!(services(&message), vec![advertised()]);
    }

    #[test]
    fn a_goodbye_is_an_announcement_that_expires_immediately() {
        let packet =
            encode_announcement("DESKTOP-ABC", TARGET, 8631, &properties(), Duration::ZERO)
                .expect("encodes a goodbye");

        let message = Message::parse(&packet).expect("parses a goodbye");

        assert_eq!(services(&message)[0].ttl, Duration::ZERO);
    }

    #[test]
    fn a_lifetime_shorter_than_a_second_is_not_a_withdrawal() {
        // A record carries whole seconds, and zero means "gone": rounding a short lifetime down
        // would withdraw a server that is still sharing.
        let packet = encode_announcement(
            "DESKTOP-ABC",
            TARGET,
            8631,
            &properties(),
            Duration::from_millis(400),
        )
        .expect("encodes an announcement");

        let message = Message::parse(&packet).expect("parses");
        assert_eq!(services(&message)[0].ttl, Duration::from_secs(1));
    }

    #[test]
    fn an_address_record_is_carried_but_not_interpreted() {
        let message = Message::parse(RESPONSE_WITH_ADDRESS).expect("parses");

        assert_eq!(message.records.len(), 4);
        assert_eq!(
            message.records[3].data,
            RecordData::Other(vec![192, 0, 2, 10]),
            "an address record is skipped rather than decoded"
        );
        // The client reviews the address an answer came from, so a record ShaPrint does not
        // interpret changes nothing about what it found.
        assert_eq!(services(&message), vec![advertised()]);
    }

    #[test]
    fn a_record_whose_data_disagrees_with_its_length_is_rejected() {
        // A record's own length is authoritative, so a name that does not fill it is a malformed
        // record rather than a longer name borrowed from the record that follows.
        let mut short_pointer = RESPONSE.to_vec();
        short_pointer[47] = 0x00;
        short_pointer[48] = 0x04; // the pointer data is 14 bytes, not 4
        assert_eq!(
            Message::parse(&short_pointer)
                .expect_err("a short pointer record is rejected")
                .code(),
            crate::domain::ErrorCode::InvalidInput
        );

        let mut short_service = RESPONSE.to_vec();
        short_service[73] = 0x00;
        short_service[74] = 0x04; // shorter than the fixed fields of a service record
        assert_eq!(
            Message::parse(&short_service)
                .expect_err("a short service record is rejected")
                .code(),
            crate::domain::ErrorCode::InvalidInput
        );

        let mut long_service = RESPONSE.to_vec();
        long_service[73] = 0x00;
        long_service[74] = 0x1E; // longer than the name it holds
        assert_eq!(
            Message::parse(&long_service)
                .expect_err("a long service record is rejected")
                .code(),
            crate::domain::ErrorCode::InvalidInput
        );
    }

    #[test]
    fn a_packet_that_claims_more_records_than_it_carries_is_rejected() {
        // The section counts arrive from the network. Three maximal counts do not fit in the u16
        // they were read as, and a reader that adds them as u16 panics before it reads one record.
        let claiming = &[
            0x00, 0x00, 0x84, 0x00, // id, flags: a response
            0x00, 0x00, // no questions
            0xFF, 0xFF, // 65535 answers
            0xFF, 0xFF, // 65535 authority records
            0x00, 0x01, // one additional record
        ];

        let error = Message::parse(claiming).expect_err("a packet with no records is rejected");

        assert_eq!(error.code(), crate::domain::ErrorCode::InvalidInput);
    }

    #[test]
    fn labels_dns_cannot_carry_are_rejected() {
        for label in ["", "   ", "DESKTOP.ABC", &"A".repeat(MAX_LABEL + 1)] {
            let error = instance_name(label).expect_err(label);
            assert_eq!(error.code(), crate::domain::ErrorCode::InvalidInput);
        }
        assert_eq!(instance_name("DESKTOP-ABC").expect("valid label"), INSTANCE);
        // A label that arrives from the platform is trimmed before it is used.
        assert_eq!(
            instance_name("  DESKTOP-ABC  ").expect("valid label"),
            INSTANCE
        );
    }

    #[test]
    fn malformed_packets_are_rejected_instead_of_panicking() {
        let cases: Vec<(&str, &[u8])> = vec![
            ("empty", &[]),
            ("header only", &QUERY[..12]),
            ("truncated question", &QUERY[..20]),
            ("truncated record", &RESPONSE[..40]),
            ("pointer loop", POINTER_LOOP),
            (
                "reserved label type",
                &[
                    0x00, 0x00, 0x84, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x40,
                    0x00, 0x0C, 0x00, 0x01, 0x00, 0x00, 0x00, 0x78, 0x00, 0x00,
                ],
            ),
        ];

        for (name, packet) in cases {
            let error = Message::parse(packet).expect_err(name);
            assert_eq!(
                error.code(),
                crate::domain::ErrorCode::InvalidInput,
                "{name}"
            );
        }
    }
}
