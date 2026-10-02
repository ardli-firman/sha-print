//! The IPPS sharing endpoint: IPP over HTTP over TLS (ADR 0003).
//!
//! The endpoint exists only while sharing runs: starting it binds the port and publishes its
//! certificate fingerprint, and stopping it closes the listener. Queries use the current sharing
//! selection, and authorized Print-Job requests are submitted only through the injected printer
//! adapter.

mod channel;
mod endpoint;
mod http;
pub mod protocol;

pub use channel::NetworkChannel;

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinSet;
use tokio::time::timeout;
use tokio_rustls::{rustls::ServerConfig, TlsAcceptor};

use crate::adapters::identity::ServerIdentity;
use crate::application::{PrintFailures, PrintJobSubmitter, ServiceContext, SharedPrinterSource};
use crate::domain::{AppError, CertificateFingerprint};

/// Port the sharing endpoint listens on.
///
/// Deliberately not 631: that port belongs to the Windows IPP service when the Internet Printing
/// feature is installed, and the MVP endpoint must not compete for it (ADR 0003).
pub const DEFAULT_PORT: u16 = 8631;

/// How long a client has for the TLS handshake before the connection is dropped.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// How long a client has to send its request before the connection is dropped.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Serves public IPP queries and authorized Print-Job submissions for shared queues over TLS.
pub struct IppsServer {
    port: u16,
    identity: Arc<ServerIdentity>,
    channel: Arc<NetworkChannel>,
    submitter: Arc<dyn PrintJobSubmitter>,
    failures: Arc<PrintFailures>,
    bound: Mutex<Option<SocketAddr>>,
}

impl IppsServer {
    /// Builds an endpoint with its Network Channel verifier and printer-submission adapter.
    pub fn new(
        port: u16,
        identity: Arc<ServerIdentity>,
        channel: Arc<NetworkChannel>,
        submitter: Arc<dyn PrintJobSubmitter>,
        failures: Arc<PrintFailures>,
    ) -> Self {
        Self {
            port,
            identity,
            channel,
            submitter,
            failures,
            bound: Mutex::new(None),
        }
    }
    /// The port clients are told to use.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The address the endpoint is listening on, once it is serving.
    pub fn bound_address(&self) -> Option<SocketAddr> {
        self.bound.lock().ok().and_then(|bound| *bound)
    }

    /// The value a client user approves before trusting this server.
    pub fn fingerprint(&self) -> CertificateFingerprint {
        self.identity.fingerprint()
    }

    /// Binds the endpoint, reports readiness, and serves clients until cancelled.
    pub async fn serve(
        &self,
        directory: Arc<dyn SharedPrinterSource>,
        context: ServiceContext,
    ) -> Result<(), AppError> {
        let tls = Arc::new(self.tls_config()?);
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, self.port)))
            .await
            .map_err(|error| {
                AppError::internal(format!(
                    "cannot listen for clients on port {}: {error}",
                    self.port
                ))
            })?;
        let local = listener.local_addr().map_err(|error| {
            AppError::internal(format!("cannot read the sharing endpoint address: {error}"))
        })?;
        if let Ok(mut bound) = self.bound.lock() {
            *bound = Some(local);
        }

        context.reporter().ready()?;
        log::info!("sharing endpoint listening port={}", local.port());

        let mut connections: JoinSet<()> = JoinSet::new();
        let mut shutdown = context.shutdown();
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => break,
                Some(_) = connections.join_next(), if !connections.is_empty() => {}
                accepted = listener.accept() => match accepted {
                    Ok((stream, peer)) => {
                        connections.spawn(serve_client(
                            stream,
                            Arc::clone(&tls),
                            Arc::clone(&directory),
                            Arc::clone(&self.channel),
                            Arc::clone(&self.submitter),
                            Arc::clone(&self.failures),
                            peer,
                        ));
                    }
                    Err(error) => log::warn!("cannot accept a client connection message={error}"),
                },
            }
        }

        // Stop accepting immediately and drop the connections still in flight: a stopped server
        // answers no client.
        connections.shutdown().await;
        if let Ok(mut bound) = self.bound.lock() {
            *bound = None;
        }
        log::info!("sharing endpoint stopped port={}", local.port());
        Ok(())
    }

    fn tls_config(&self) -> Result<ServerConfig, AppError> {
        ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![self.identity.certificate().clone()],
                self.identity.key(),
            )
            .map_err(|error| {
                AppError::internal(format!(
                    "cannot prepare the sharing endpoint certificate: {error}"
                ))
            })
    }
}

/// Completes the TLS handshake and answers one request.
async fn serve_client(
    stream: TcpStream,
    tls: Arc<ServerConfig>,
    directory: Arc<dyn SharedPrinterSource>,
    channel: Arc<NetworkChannel>,
    submitter: Arc<dyn PrintJobSubmitter>,
    failures: Arc<PrintFailures>,
    peer: SocketAddr,
) {
    let acceptor = TlsAcceptor::from(tls);
    let handshake = timeout(HANDSHAKE_TIMEOUT, acceptor.accept(stream)).await;
    let tls_stream = match handshake {
        Ok(Ok(stream)) => stream,
        Ok(Err(error)) => {
            log::warn!("client TLS handshake failed peer={peer} message={error}");
            return;
        }
        Err(_) => {
            log::warn!("client TLS handshake timed out peer={peer}");
            return;
        }
    };

    if let Err(error) = answer_request_with_jobs(
        tls_stream,
        directory.as_ref(),
        channel.as_ref(),
        submitter.as_ref(),
        failures.as_ref(),
    )
    .await
    {
        log::warn!(
            "client request failed peer={peer} code={} message={error}",
            error.code_str()
        );
    }
}

/// Testable HTTP/IPPS seam with explicit channel and printer-submission ports.
pub async fn answer_request_with_jobs<S>(
    stream: S,
    directory: &dyn SharedPrinterSource,
    channel: &NetworkChannel,
    submitter: &dyn PrintJobSubmitter,
    failures: &PrintFailures,
) -> Result<(), AppError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (read, mut write) = tokio::io::split(stream);
    let mut reader = BufReader::new(read);

    let head = match timeout(REQUEST_TIMEOUT, http::read_head(&mut reader)).await {
        Ok(Ok(head)) => head,
        Ok(Err(error)) => {
            log::warn!("refusing a client request status={}", error.status_line());
            return respond(&mut write, error.status_line(), "text/plain", &[]).await;
        }
        Err(_) => return Err(AppError::timeout("a client did not send a request in time")),
    };

    let length = match http::body_length(&head) {
        Ok(length) => length,
        Err(error) => {
            log::warn!("refusing a client request status={}", error.status_line());
            return respond(&mut write, error.status_line(), "text/plain", &[]).await;
        }
    };

    if head.expects_continue() {
        http::send_continue(&mut write).await.map_err(write_error)?;
    }

    let body = timeout(REQUEST_TIMEOUT, http::read_body(&mut reader, length))
        .await
        .map_err(|_| AppError::timeout("a client did not send its request in time"))?
        .map_err(|error| AppError::invalid_input(format!("unreadable request body: {error}")))?;

    let authority = match head.header("host") {
        Some(host) if !host.trim().is_empty() => host.trim(),
        _ => {
            log::warn!("refusing a client request status=400 Bad Request missing Host");
            return respond(&mut write, "400 Bad Request", "text/plain", &[]).await;
        }
    };
    let answer =
        endpoint::answer_job(body, authority, directory, channel, submitter, failures).await;
    respond(&mut write, "200 OK", http::IPP_CONTENT_TYPE, &answer).await
}

/// Writes one response and closes the connection cleanly.
///
/// The clean TLS shutdown matters: a client that sees the socket drop instead cannot tell a
/// complete answer from a truncated one.
async fn respond<W: AsyncWrite + Unpin>(
    writer: &mut W,
    status_line: &str,
    content_type: &str,
    body: &[u8],
) -> Result<(), AppError> {
    http::write_response(writer, status_line, content_type, body)
        .await
        .map_err(write_error)?;
    writer.shutdown().await.map_err(write_error)
}

fn write_error(error: std::io::Error) -> AppError {
    AppError::internal(format!("cannot answer a client: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use crate::application::{PrintJob, PrintJobSubmitter, SharedPrinterSource};
    use crate::domain::PrinterName;

    /// Shares a fixed set of queues.
    struct FakeShared(Vec<PrinterName>);

    impl SharedPrinterSource for FakeShared {
        fn shared_printers(&self) -> Vec<PrinterName> {
            self.0.clone()
        }
    }

    struct UnavailableSubmitter;

    #[async_trait::async_trait]
    impl PrintJobSubmitter for UnavailableSubmitter {
        fn is_available(&self) -> bool {
            false
        }

        async fn submit(&self, _printer: &PrinterName, _job: PrintJob) -> Result<u32, AppError> {
            Err(AppError::unsupported(
                "printer submission is unavailable in this test",
            ))
        }
    }

    /// An IPP `Get-Printers` request as a client sends it.
    fn get_printers_request() -> Vec<u8> {
        let mut request = vec![2, 0];
        request.extend(protocol::OPERATION_GET_PRINTERS.to_be_bytes());
        request.extend(1u32.to_be_bytes());
        request.push(0x01);
        for (value_tag, name, value) in [
            (0x47, "attributes-charset", "utf-8"),
            (0x48, "attributes-natural-language", "en"),
        ] {
            request.push(value_tag);
            request.extend((name.len() as u16).to_be_bytes());
            request.extend(name.as_bytes());
            request.extend((value.len() as u16).to_be_bytes());
            request.extend(value.as_bytes());
        }
        request.push(0x03);
        request
    }

    /// Drives one exchange over an in-memory stream: the client writes `request`, the endpoint
    /// answers, and the raw answer comes back.
    async fn exchange(request: &[u8], directory: Arc<dyn SharedPrinterSource>) -> Vec<u8> {
        let (server, mut client) = tokio::io::duplex(16 * 1024);
        let channel = NetworkChannel::in_memory();
        let submitter = UnavailableSubmitter;
        let failures = PrintFailures::new();
        let answering =
            answer_request_with_jobs(server, directory.as_ref(), &channel, &submitter, &failures);
        let exchange = async move {
            client.write_all(request).await.expect("sends the request");
            let mut answer = Vec::new();
            client
                .read_to_end(&mut answer)
                .await
                .expect("reads the answer");
            answer
        };
        let (answer, result) = tokio::join!(exchange, answering);
        result.expect("answers");
        answer
    }

    fn post(body: &[u8]) -> Vec<u8> {
        let head = format!(
            "POST /ipp/print HTTP/1.1\r\nHost: server:8631\r\nContent-Type: application/ipp\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        let mut request = head.into_bytes();
        request.extend_from_slice(body);
        request
    }

    #[tokio::test]
    async fn an_ipp_post_is_answered_over_a_plain_stream() {
        let directory: Arc<dyn SharedPrinterSource> = Arc::new(FakeShared(vec![
            PrinterName::parse("Zebra").expect("valid name"),
        ]));

        let answer = exchange(&post(&get_printers_request()), directory).await;

        let text = String::from_utf8_lossy(&answer).into_owned();
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(text.contains("Content-Type: application/ipp\r\n"));
        assert!(text.contains("Zebra"));
        assert!(answer.ends_with(&[0x03]));
    }

    #[tokio::test]
    async fn a_non_post_is_refused_with_its_own_status() {
        let directory: Arc<dyn SharedPrinterSource> = Arc::new(FakeShared(Vec::new()));

        let answer = exchange(
            b"GET /ipp/print HTTP/1.1\r\nHost: server\r\n\r\n",
            directory,
        )
        .await;

        assert!(String::from_utf8_lossy(&answer).starts_with("HTTP/1.1 405 Method Not Allowed\r\n"));
    }

    #[tokio::test]
    async fn a_client_that_waits_for_permission_gets_a_continue_first() {
        let directory: Arc<dyn SharedPrinterSource> = Arc::new(FakeShared(vec![
            PrinterName::parse("Zebra").expect("valid name"),
        ]));
        let body = get_printers_request();
        let head = format!(
            "POST /ipp/print HTTP/1.1\r\nHost: server:8631\r\nContent-Type: application/ipp\r\nContent-Length: {}\r\nExpect: 100-continue\r\n\r\n",
            body.len()
        );
        let mut request = head.into_bytes();
        request.extend_from_slice(&body);

        let answer = exchange(&request, directory).await;

        let text = String::from_utf8_lossy(&answer).into_owned();
        assert!(text.starts_with("HTTP/1.1 100 Continue\r\n\r\n"));
        assert!(text.contains("HTTP/1.1 200 OK"));
    }

    #[tokio::test]
    async fn a_request_without_host_is_refused_with_400_bad_request() {
        let directory: Arc<dyn SharedPrinterSource> = Arc::new(FakeShared(Vec::new()));

        let answer = exchange(
            b"POST /ipp/print HTTP/1.1\r\nContent-Type: application/ipp\r\nContent-Length: 0\r\n\r\n",
            directory,
        )
        .await;

        let text = String::from_utf8_lossy(&answer).into_owned();
        assert!(text.starts_with("HTTP/1.1 400 Bad Request\r\n"));
    }
}
