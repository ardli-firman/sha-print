//! Helpers the integration tests share: a printer catalog the test controls, the real sharing
//! runtime over it, and an IPPS client that talks to the endpoint the way a client would.

// Each integration test is its own crate and uses a different part of these helpers.
#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::ring;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, SignatureScheme};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

use shaprint_desktop::adapters::{IppsServer, ServerIdentity};
use shaprint_desktop::application::{LocalPrinterCatalog, Sharing};
use shaprint_desktop::domain::{AppError, PrinterName};

/// A catalog over a fixed set of queues, standing in for the Windows spooler.
pub struct FakeCatalog {
    queues: Vec<PrinterName>,
}

impl FakeCatalog {
    pub fn new(names: &[&str]) -> Arc<Self> {
        Arc::new(Self {
            queues: names
                .iter()
                .map(|name| PrinterName::parse(name).expect("valid printer name"))
                .collect(),
        })
    }
}

#[async_trait]
impl LocalPrinterCatalog for FakeCatalog {
    async fn local_printers(&self) -> Result<Vec<PrinterName>, AppError> {
        Ok(self.queues.clone())
    }
}

/// A directory unique to one test.
pub fn temporary_directory(name: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let unique = NEXT.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("shaprint-{name}-{}-{unique}", std::process::id()))
}

/// The sharing configuration over `queues` and the endpoint that serves it.
///
/// The endpoint binds port 0, so tests can run in parallel and find the port they were given.
pub fn sharing_runtime(queues: &[&str]) -> (Arc<Sharing>, Arc<IppsServer>) {
    let sharing = Arc::new(Sharing::new(FakeCatalog::new(queues)));
    let identity = ServerIdentity::generate().expect("generates a server identity");
    let endpoint = Arc::new(IppsServer::new(0, Arc::new(identity)));
    (sharing, endpoint)
}

/// Names of the queues the tests share through the fake catalog.
pub fn printer_names(values: &[&str]) -> Vec<PrinterName> {
    values
        .iter()
        .map(|value| PrinterName::parse(value).expect("valid printer name"))
        .collect()
}

/// An IPPS client: it approves the server by certificate fingerprint, exactly as the product's
/// trust model does, and speaks IPP over HTTP over TLS.
pub struct IppClient {
    authority: String,
    address: String,
    expected: [u8; 32],
}

impl IppClient {
    pub fn new(port: u16, fingerprint: String) -> Self {
        let fingerprint = fingerprint.replace(':', "");
        let expected = <[u8; 32]>::try_from(hex::decode(fingerprint).expect("hex fingerprint"))
            .expect("32 bytes");
        Self {
            authority: format!("127.0.0.1:{port}"),
            address: "127.0.0.1".to_owned(),
            expected,
        }
    }

    /// Sends one IPP request and returns the HTTP status with the response body.
    pub async fn post(&self, body: &[u8]) -> std::io::Result<(u16, Vec<u8>)> {
        let stream = TcpStream::connect(&self.authority).await?;
        let connector = TlsConnector::from(Arc::new(self.config()));
        let name = ServerName::try_from(self.address.clone())
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        let mut stream = connector.connect(name, stream).await?;

        let head = format!(
            "POST /ipp/print HTTP/1.1\r\nHost: {}\r\nContent-Type: application/ipp\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            self.authority,
            body.len()
        );
        stream.write_all(head.as_bytes()).await?;
        stream.write_all(body).await?;
        stream.flush().await?;

        let mut response = Vec::new();
        stream.read_to_end(&mut response).await?;
        Ok(split_response(&response))
    }

    fn config(&self) -> ClientConfig {
        ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(FingerprintVerifier {
                expected: self.expected,
            }))
            .with_no_client_auth()
    }
}

/// Splits an HTTP response into its status code and body.
fn split_response(response: &[u8]) -> (u16, Vec<u8>) {
    let separator = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("a response head");
    let head = String::from_utf8_lossy(&response[..separator]).into_owned();
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .expect("a status code");
    (status, response[separator + 4..].to_vec())
}

/// Accepts the server when the presented certificate hashes to the approved fingerprint.
#[derive(Debug)]
struct FingerprintVerifier {
    expected: [u8; 32],
}

impl ServerCertVerifier for FingerprintVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let presented: [u8; 32] = Sha256::digest(end_entity.as_ref()).into();
        if presented == self.expected {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General(
                "the server certificate fingerprint does not match the approved one".to_owned(),
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            certificate,
            signature,
            &ring::default_provider().signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            certificate,
            signature,
            &ring::default_provider().signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// Operation ids the client uses.
const GET_PRINTER_ATTRIBUTES: u16 = 0x000b;
const GET_PRINTERS: u16 = 0x0402;

/// An IPP `Get-Printers` request.
pub fn get_printers(request_id: u32) -> Vec<u8> {
    request(GET_PRINTERS, request_id, &[])
}

/// An IPP `Get-Printer-Attributes` request for one printer URI.
pub fn get_printer_attributes(request_id: u32, printer_uri: &str) -> Vec<u8> {
    request(
        GET_PRINTER_ATTRIBUTES,
        request_id,
        &[("printer-uri", printer_uri)],
    )
}

fn request(operation: u16, request_id: u32, attributes: &[(&str, &str)]) -> Vec<u8> {
    let mut body = vec![2, 0];
    body.extend(operation.to_be_bytes());
    body.extend(request_id.to_be_bytes());
    body.push(0x01);
    attribute(&mut body, 0x47, "attributes-charset", "utf-8");
    attribute(&mut body, 0x48, "attributes-natural-language", "en");
    for (name, value) in attributes {
        attribute(&mut body, 0x45, name, value);
    }
    body.push(0x03);
    body
}

fn attribute(body: &mut Vec<u8>, value_tag: u8, name: &str, value: &str) {
    body.push(value_tag);
    body.extend((name.len() as u16).to_be_bytes());
    body.extend(name.as_bytes());
    body.extend((value.len() as u16).to_be_bytes());
    body.extend(value.as_bytes());
}

/// The status code of an IPP response.
pub fn status_code(response: &[u8]) -> u16 {
    attribute_values(response, "status-code")
        .first()
        .map(|value| u16::from_be_bytes([value[2], value[3]]))
        .expect("a status code")
}

/// The `printer-name` values an IPP response advertises.
pub fn advertised_printers(response: &[u8]) -> Vec<String> {
    attribute_values(response, "printer-name")
        .into_iter()
        .map(|value| String::from_utf8_lossy(&value).into_owned())
        .collect()
}

/// Reads one attribute's values out of an IPP response, the way a client's parser would.
fn attribute_values(response: &[u8], wanted: &str) -> Vec<Vec<u8>> {
    let mut values = Vec::new();
    let mut position = 8;
    while position + 1 < response.len() {
        let value_tag = response[position];
        position += 1;
        if value_tag == 0x03 {
            break;
        }
        if (0x01..=0x05).contains(&value_tag) {
            continue;
        }
        let name = read_value(response, &mut position);
        let value = read_value(response, &mut position);
        if name == wanted.as_bytes() {
            values.push(value);
        }
    }
    values
}

fn read_value(response: &[u8], position: &mut usize) -> Vec<u8> {
    let length = usize::from(u16::from_be_bytes([
        response[*position],
        response[*position + 1],
    ]));
    *position += 2;
    let value = response[*position..*position + length].to_vec();
    *position += length;
    value
}
