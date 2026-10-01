//! Manual IPPS connection and durable certificate trust for client users.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use rustls::{
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    pki_types::{CertificateDer, ServerName, UnixTime},
    DigitallySignedStruct, Error as TlsError, SignatureScheme,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex as AsyncMutex;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};
use tokio_rustls::{
    rustls::{self, ClientConfig},
    TlsConnector,
};

use crate::{
    adapters::ipps::protocol::OPERATION_GET_PRINTERS,
    domain::{AppError, ErrorCode},
};

const DEFAULT_PORT: u16 = 8631;
const STORE_FILE: &str = "client-server-trust.json";
const REQUEST_ID: u32 = 1;
const MAX_RESPONSE: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerAddress {
    pub host: String,
    pub port: u16,
    pub normalized: String,
}

impl ServerAddress {
    pub fn parse(input: &str) -> Result<Self, AppError> {
        let value = input.trim();
        if value.is_empty()
            || value.contains("@")
            || value.contains('/')
            || value.contains('?')
            || value.contains('#')
            || value.contains(char::is_whitespace)
        {
            return Err(AppError::invalid_input("Enter a server host or host:port (for example printer.local or printer.local:8631); do not include a URL or credentials."));
        }
        let (host, port) = if value.starts_with('[') {
            let end = value.find(']').ok_or_else(invalid_address)?;
            let host = &value[1..end];
            let ip = host
                .parse::<std::net::Ipv6Addr>()
                .map_err(|_| invalid_address())?;
            let rest = &value[end + 1..];
            let port = if rest.is_empty() {
                DEFAULT_PORT
            } else {
                rest.strip_prefix(':')
                    .ok_or_else(invalid_address)?
                    .parse::<u16>()
                    .map_err(|_| invalid_address())?
            };
            (ip.to_string(), port)
        } else if let Ok(ip) = value.parse::<std::net::IpAddr>() {
            (ip.to_string(), DEFAULT_PORT)
        } else {
            let mut parts = value.split(':');
            let host = parts.next().unwrap_or_default();
            let maybe_port = parts.next();
            if parts.next().is_some() {
                return Err(AppError::invalid_input(
                    "For IPv6 addresses, use brackets, for example [2001:db8::1]:8631.",
                ));
            }
            if host.is_empty()
                || host.len() > 253
                || host.split('.').any(|label| {
                    label.is_empty()
                        || label.len() > 63
                        || label.starts_with('-')
                        || label.ends_with('-')
                })
                || !host
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'))
            {
                return Err(invalid_address());
            }
            let port = match maybe_port {
                Some(port) => port.parse::<u16>().map_err(|_| invalid_address())?,
                None => DEFAULT_PORT,
            };
            (host.to_ascii_lowercase(), port)
        };
        if port == 0 {
            return Err(invalid_address());
        }
        let host = host.to_ascii_lowercase();
        let normalized = if host.contains(':') {
            format!("[{host}]:{port}")
        } else {
            format!("{host}:{port}")
        };
        Ok(Self {
            host,
            port,
            normalized,
        })
    }
    fn socket(&self) -> String {
        if self.host.contains(':') {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}
fn invalid_address() -> AppError {
    AppError::invalid_input("Enter a valid server host or host:port with a port from 1 to 65535; IPv6 addresses must be bracketed.")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct TrustRecord {
    fingerprint: String,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct TrustData {
    #[serde(default)]
    servers: BTreeMap<String, TrustRecord>,
}

pub struct ClientConnections {
    store_path: PathBuf,
    store: Mutex<TrustData>,
    approval_lock: AsyncMutex<()>,
}
impl ClientConnections {
    pub fn new(data_dir: impl AsRef<Path>) -> Result<Self, AppError> {
        fs::create_dir_all(data_dir.as_ref()).map_err(|_| {
            AppError::internal("Cannot create the app data directory for saved server approvals.")
        })?;
        let path = data_dir.as_ref().join(STORE_FILE);
        let store = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| AppError::internal("Saved server approvals could not be read. Restore or remove client-server-trust.json, then review each server again."))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => TrustData::default(),
            Err(_) => return Err(AppError::internal("Saved server approvals could not be opened. Check app data directory permissions.")),
        };
        Ok(Self {
            store_path: path,
            store: Mutex::new(store),
            approval_lock: AsyncMutex::new(()),
        })
    }

    /// Contacts TLS only. No IPP request is sent by this operation.
    pub async fn inspect(&self, input: &str) -> Result<ConnectionReview, AppError> {
        let address = ServerAddress::parse(input)?;
        let fingerprint = observe_fingerprint(&address).await?;
        let store = self.store.lock().map_err(|_| {
            AppError::internal(
                "Saved server approvals are unavailable; restart the app and try again.",
            )
        })?;
        let approved = store
            .servers
            .get(&address.normalized)
            .map(|r| r.fingerprint.clone());
        let trusted = approved.as_deref() == Some(fingerprint.as_str());
        Ok(ConnectionReview {
            address: address.normalized,
            current_fingerprint: fingerprint,
            previous_fingerprint: approved,
            trusted,
        })
    }

    pub async fn approve(&self, input: &str, observed: &str) -> Result<ConnectionReview, AppError> {
        let address = ServerAddress::parse(input)?;
        validate_fingerprint(observed)?;
        let live = observe_fingerprint(&address).await?;
        if live != observed.to_ascii_uppercase() {
            return Err(AppError::invalid_state("The server certificate changed since review. Inspect it again and approve only the fingerprint currently shown."));
        }
        let _approval = self.approval_lock.lock().await;
        let updated = {
            let store = self.store.lock().map_err(|_| {
                AppError::internal(
                    "Saved server approvals are unavailable; restart the app and try again.",
                )
            })?;
            let mut updated = store.clone();
            updated.servers.insert(
                address.normalized.clone(),
                TrustRecord {
                    fingerprint: live.clone(),
                },
            );
            updated
        };
        let persist_path = self.store_path.clone();
        let persist_store = updated.clone();
        tokio::task::spawn_blocking(move || persist(&persist_path, &persist_store))
            .await
            .map_err(|_| AppError::internal("The server approval storage worker stopped."))??;
        let mut store = self.store.lock().map_err(|_| {
            AppError::internal(
                "Saved server approvals are unavailable; restart the app and try again.",
            )
        })?;
        *store = updated;
        Ok(ConnectionReview {
            address: address.normalized,
            current_fingerprint: live,
            previous_fingerprint: None,
            trusted: true,
        })
    }

    pub async fn printers(&self, input: &str) -> Result<ConnectionPrinters, AppError> {
        let address = ServerAddress::parse(input)?;
        let pinned = self.store.lock().map_err(|_| AppError::internal("Saved server approvals are unavailable; restart the app and try again."))?.servers.get(&address.normalized).map(|r| r.fingerprint.clone()).ok_or_else(|| AppError::invalid_state("This server is not approved. Inspect its certificate fingerprint and explicitly approve it before listing shared printers."))?;
        let (stream, live) = connect(&address).await?;
        if live != pinned {
            return Err(AppError::invalid_state(format!("The server certificate changed. Previously approved: {pinned}. Currently presented: {live}. Shared printers are blocked; inspect the current fingerprint and explicitly reapprove it only if you trust the change.")));
        }
        let names = query_printers(stream, &address).await?;
        Ok(ConnectionPrinters {
            address: address.normalized,
            printers: names,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionReview {
    pub address: String,
    pub current_fingerprint: String,
    pub previous_fingerprint: Option<String>,
    pub trusted: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionPrinters {
    pub address: String,
    pub printers: Vec<String>,
}

fn persist(path: &Path, store: &TrustData) -> Result<(), AppError> {
    let bytes = serde_json::to_vec_pretty(store)
        .map_err(|_| AppError::internal("Could not save the server approval."))?;
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, bytes).map_err(|_| {
        AppError::internal(
            "Could not save the server approval. Check app data directory permissions.",
        )
    })?;
    fs::rename(&temp, path).map_err(|_| AppError::internal("Could not finish saving the server approval. Check app data directory permissions and try again."))
}
fn validate_fingerprint(value: &str) -> Result<(), AppError> {
    if value.len() != 95
        || !value.bytes().enumerate().all(|(i, b)| {
            if i % 3 == 2 {
                b == b':'
            } else {
                b.is_ascii_hexdigit()
            }
        })
    {
        return Err(AppError::invalid_input("The reviewed certificate fingerprint is invalid. Inspect the server again before approving."));
    }
    Ok(())
}

async fn observe_fingerprint(address: &ServerAddress) -> Result<String, AppError> {
    connect(address).await.map(|(_, fp)| fp)
}
async fn connect(
    address: &ServerAddress,
) -> Result<(tokio_rustls::client::TlsStream<TcpStream>, String), AppError> {
    let tcp = timeout(Duration::from_secs(8), TcpStream::connect(address.socket())).await.map_err(|_| AppError::timeout("The server did not respond. Check its address, network route, firewall, and that IPPS sharing is running."))?.map_err(|_| AppError::new(ErrorCode::Internal, "Could not reach the server. Check its address, network route, firewall, and that IPPS sharing is running."))?;
    let provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
    let config = ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_safe_default_protocol_versions()
        .map_err(|_| AppError::internal("This device could not configure secure IPPS."))?
        .dangerous()
        .with_custom_certificate_verifier(ArcVerifier::new(provider))
        .with_no_client_auth();
    let name = ServerName::try_from(address.host.clone()).map_err(|_| invalid_address())?;
    let tls = timeout(Duration::from_secs(8), TlsConnector::from(std::sync::Arc::new(config)).connect(name, tcp)).await.map_err(|_| AppError::timeout("The secure connection timed out. Check the server address and that IPPS sharing is running."))?.map_err(|_| AppError::new(ErrorCode::Internal, "The server could not complete a secure IPPS connection. Check that it is an IPPS server and retry."))?;
    let cert = tls.get_ref().1.peer_certificates().and_then(|c| c.first()).ok_or_else(|| AppError::internal("The server did not present a certificate. Confirm the address points to an IPPS server."))?;
    let fingerprint = fingerprint(cert);
    Ok((tls, fingerprint))
}
fn fingerprint(cert: &CertificateDer<'_>) -> String {
    let digest = Sha256::digest(cert.as_ref());
    digest
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

#[derive(Debug)]
struct ArcVerifier {
    provider: std::sync::Arc<rustls::crypto::CryptoProvider>,
}
impl ArcVerifier {
    fn new(provider: std::sync::Arc<rustls::crypto::CryptoProvider>) -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self { provider })
    }
}
impl ServerCertVerifier for ArcVerifier {
    fn verify_server_cert(
        &self,
        _: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, TlsError> {
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

async fn query_printers(
    mut tls: tokio_rustls::client::TlsStream<TcpStream>,
    address: &ServerAddress,
) -> Result<Vec<String>, AppError> {
    let mut ipp = vec![2, 0];
    ipp.extend(OPERATION_GET_PRINTERS.to_be_bytes());
    ipp.extend(REQUEST_ID.to_be_bytes());
    ipp.push(0x01);
    attr(&mut ipp, 0x47, "attributes-charset", "utf-8");
    attr(&mut ipp, 0x48, "attributes-natural-language", "en");
    ipp.push(0x03);
    let request = format!("POST /ipp/print HTTP/1.1\r\nHost: {}\r\nContent-Type: application/ipp\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", address.socket(), ipp.len());
    let response = timeout(Duration::from_secs(8), async {
        tls.write_all(request.as_bytes()).await?;
        tls.write_all(&ipp).await?;
        let mut response = Vec::new();
        tls.take((MAX_RESPONSE + 8192) as u64)
            .read_to_end(&mut response)
            .await?;
        Ok::<_, std::io::Error>(response)
    })
    .await
    .map_err(|_| {
        AppError::timeout(
            "The printer query timed out. Check that the server is online and sharing printers.",
        )
    })?
    .map_err(|_| {
        AppError::internal(
            "Could not complete the printer query. Check the server IPPS endpoint and retry.",
        )
    })?;
    let boundary = response.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4).ok_or_else(|| AppError::invalid_state("The server returned an invalid HTTP response. Confirm it is the ShaPrint IPPS endpoint."))?;
    if !response
        .get(..boundary)
        .is_some_and(|h| h.starts_with(b"HTTP/1.1 200"))
    {
        return Err(AppError::invalid_state(
            "The IPPS server rejected the printer query. Confirm sharing is enabled and retry.",
        ));
    }
    parse_printers(response.get(boundary..).unwrap_or_default())
}
fn attr(out: &mut Vec<u8>, tag: u8, name: &str, value: &str) {
    out.push(tag);
    out.extend((name.len() as u16).to_be_bytes());
    out.extend(name.as_bytes());
    out.extend((value.len() as u16).to_be_bytes());
    out.extend(value.as_bytes());
}
fn parse_printers(bytes: &[u8]) -> Result<Vec<String>, AppError> {
    if bytes.len() < 9 {
        return Err(AppError::invalid_state("The IPPS server returned an incomplete IPP response. Confirm it supports Get-Printers and retry."));
    }
    let status = u16::from_be_bytes([bytes[2], bytes[3]]);
    if status != 0 {
        return Err(AppError::invalid_state(
            "The IPPS server rejected Get-Printers. Confirm sharing is enabled and retry.",
        ));
    }
    let mut pos = 8;
    let mut names = Vec::new();
    let mut in_printer = false;
    let mut ended = false;
    while pos < bytes.len() {
        let tag = bytes[pos];
        pos += 1;
        if tag == 0x03 {
            ended = true;
            break;
        }
        if (0x01..=0x05).contains(&tag) {
            in_printer = tag == 0x04;
            continue;
        }
        if pos + 2 > bytes.len() {
            return Err(protocol_error());
        }
        let n = u16::from_be_bytes([bytes[pos], bytes[pos + 1]]) as usize;
        pos += 2;
        let ne = pos.checked_add(n).ok_or_else(protocol_error)?;
        let name = std::str::from_utf8(bytes.get(pos..ne).ok_or_else(protocol_error)?)
            .map_err(|_| protocol_error())?;
        pos = ne;
        if pos + 2 > bytes.len() {
            return Err(protocol_error());
        }
        let len = u16::from_be_bytes([bytes[pos], bytes[pos + 1]]) as usize;
        pos += 2;
        let end = pos.checked_add(len).ok_or_else(protocol_error)?;
        let value = bytes.get(pos..end).ok_or_else(protocol_error)?;
        pos = end;
        if in_printer && name == "printer-name" {
            let text = std::str::from_utf8(value)
                .map_err(|_| protocol_error())?
                .to_owned();
            if !text.is_empty() && !names.contains(&text) {
                names.push(text);
            }
        }
    }
    if !ended {
        return Err(protocol_error());
    }
    Ok(names)
}
fn protocol_error() -> AppError {
    AppError::invalid_state("The IPPS server returned malformed printer data. Confirm it is a ShaPrint IPPS endpoint and retry.")
}
