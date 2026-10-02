//! The local IPP client proxy.
//!
//! Installed Windows queues send IPP to loopback. The proxy routes the queue URI to an explicitly
//! trusted server, adds the configured Network Channel for Print-Job, and forwards the request over
//! IPPS so the Windows inbox driver does not handle the authentication challenge (issue #34).

use std::{net::SocketAddr, sync::Arc, time::Duration};

use async_trait::async_trait;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::Mutex,
    task::JoinSet,
    time::timeout,
};

use crate::{
    adapters::{client_connections::ClientConnections, ipps::protocol, ipps::NetworkChannel},
    application::{PrintFailures, RuntimeService, ServiceContext},
    domain::{AppError, ErrorCode, PrintFailure, PrinterName, ServiceId},
};

/// Default loopback IPP port used by native client queues.
pub const CLIENT_PROXY_DEFAULT_PORT: u16 = 8632;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_HEAD_BYTES: usize = 8 * 1024;
const MAX_BODY_BYTES: usize = 64 * 1024 * 1024;
const ISSUE34_TRACE_ENV: &str = "SHAPRINT_ISSUE34_IPP_TRACE";

fn trace_issue34(message: &str) {
    if std::env::var_os(ISSUE34_TRACE_ENV).is_some() {
        eprintln!("[DEBUG-34IPP] {message}");
    }
}

/// Supervises the local client proxy.
pub struct ClientProxyService {
    connections: Arc<ClientConnections>,
    channel: Arc<NetworkChannel>,
    failures: Arc<PrintFailures>,
    port: u16,
    bound: Mutex<Option<SocketAddr>>,
}

impl std::fmt::Debug for ClientProxyService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ClientProxyService")
            .field("port", &self.port)
            .finish_non_exhaustive()
    }
}

impl ClientProxyService {
    pub fn new(
        connections: Arc<ClientConnections>,
        channel: Arc<NetworkChannel>,
        failures: Arc<PrintFailures>,
    ) -> Self {
        Self::with_port(connections, channel, failures, CLIENT_PROXY_DEFAULT_PORT)
    }

    /// Builds the loopback endpoint; port zero lets tests ask the OS for an unused port.
    pub fn with_port(
        connections: Arc<ClientConnections>,
        channel: Arc<NetworkChannel>,
        failures: Arc<PrintFailures>,
        port: u16,
    ) -> Self {
        Self {
            connections,
            channel,
            failures,
            port,
            bound: Mutex::new(None),
        }
    }

    /// The loopback address the native queues send requests to, once the service is running.
    pub fn bound_address(&self) -> Option<SocketAddr> {
        self.bound.try_lock().ok().and_then(|bound| *bound)
    }
}

/// Builds the IPP URI a manually configured native queue uses to route through this proxy. The
/// same format is consumed by the native queue installer planned in issue #35.
pub fn client_queue_uri(
    proxy_authority: &str,
    server_address: &str,
    printer_name: &str,
) -> Result<String, AppError> {
    let server_address = ClientConnections::normalize_address(server_address)?;
    let printer_name = PrinterName::parse(printer_name)?;
    Ok(format!(
        "ipp://{proxy_authority}/ipp/print/{}/{}",
        protocol::percent_encode(&server_address),
        protocol::percent_encode(printer_name.as_str())
    ))
}

#[async_trait]
impl RuntimeService for ClientProxyService {
    fn id(&self) -> ServiceId {
        ServiceId::ClientProxy
    }

    /// The proxy runs whenever the app runs: installed queues must always reach it (ADR 0001).
    fn autostart(&self) -> bool {
        true
    }

    async fn run(&self, context: ServiceContext) -> Result<(), AppError> {
        let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], self.port)))
            .await
            .map_err(|_| AppError::internal(format!(
                "Could not start the local print proxy on port {}. Close the program using that port, then restart ShaPrint.",
                self.port
            )))?;
        let local = listener
            .local_addr()
            .map_err(|_| AppError::internal("Could not read the local print proxy address."))?;
        *self.bound.lock().await = Some(local);
        context.reporter().ready()?;

        let mut shutdown = context.shutdown();
        let mut connections = JoinSet::new();
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => break,
                Some(_) = connections.join_next(), if !connections.is_empty() => {}
                accepted = listener.accept() => if let Ok((stream, peer)) = accepted {
                    trace_issue34(&format!("accepted-peer={peer}"));
                    let client_connections = Arc::clone(&self.connections);
                    let channel = Arc::clone(&self.channel);
                    let failures = Arc::clone(&self.failures);
                    let authority = local.to_string();
                    connections.spawn(async move {
                        if let Err(error) = serve_client(
                            stream,
                            &authority,
                            &client_connections,
                            &channel,
                            &failures,
                        )
                        .await
                        {
                            trace_issue34(&format!("request-error={}: {}", error.code_str(), error.message()));
                            log::warn!(
                                "local proxy request failed code={}: {}",
                                error.code_str(),
                                error.message()
                            );
                        }
                    });
                },
            }
        }
        connections.shutdown().await;
        *self.bound.lock().await = None;
        Ok(())
    }
}

async fn serve_client(
    stream: TcpStream,
    authority: &str,
    connections: &ClientConnections,
    channel: &NetworkChannel,
    failures: &PrintFailures,
) -> Result<(), AppError> {
    let (read, mut write) = tokio::io::split(stream);
    let mut reader = BufReader::new(read);
    let head = timeout(REQUEST_TIMEOUT, read_head(&mut reader))
        .await
        .map_err(|_| {
            AppError::timeout("The local printer did not finish its request in time.")
        })??;
    trace_issue34(&format!(
        "http-method={} target={}",
        head.method,
        if head.path.starts_with("/ipp/print") {
            "ipp-print"
        } else {
            "other"
        }
    ));
    if head.method != "POST"
        || !head.path.starts_with("/ipp/print")
        || !head
            .content_type
            .as_deref()
            .is_some_and(|value| value.eq_ignore_ascii_case("application/ipp"))
    {
        write_http(&mut write, "400 Bad Request", &[]).await?;
        return Ok(());
    }
    if head.expects_continue {
        trace_issue34("sending 100-continue");
        write
            .write_all(b"HTTP/1.1 100 Continue\r\n\r\n")
            .await
            .map_err(|_| {
                AppError::internal("Could not send HTTP continue to the local printer.")
            })?;
        write.flush().await.map_err(|_| {
            AppError::internal("Could not flush HTTP continue to the local printer.")
        })?;
    }
    let body = if head.is_chunked {
        trace_issue34("reading-chunked-body");
        timeout(REQUEST_TIMEOUT, read_chunked_body(&mut reader))
            .await
            .map_err(|_| {
                AppError::timeout("The local printer did not finish sending the document.")
            })??
    } else if let Some(length) = head.content_length {
        if length > MAX_BODY_BYTES {
            write_http(&mut write, "413 Payload Too Large", &[]).await?;
            return Ok(());
        }
        trace_issue34(&format!("reading-body length={length}"));
        let mut buf = vec![0; length];
        timeout(REQUEST_TIMEOUT, reader.read_exact(&mut buf))
            .await
            .map_err(|_| {
                AppError::timeout("The local printer did not finish sending the document.")
            })?
            .map_err(|_| {
                AppError::invalid_input("The local printer sent an incomplete IPP request.")
            })?;
        buf
    } else {
        write_http(&mut write, "411 Length Required", &[]).await?;
        return Ok(());
    };

    let request = match protocol::Request::parse(&body) {
        Ok(request) => request,
        Err(_) => {
            write_ipp_error(&mut write, protocol::Status::BadRequest).await?;
            return Ok(());
        }
    };
    let request_id = request.request_id();
    let operation = request.operation();
    let version = request.response_version();
    trace_issue34(&format!(
        "operation=0x{operation:04x} req_id={request_id} uri={:?} req_attrs={:?}",
        request.value("printer-uri"),
        request.text_values("requested-attributes")
    ));
    if !request.version_is_supported() {
        let response = protocol::response(
            request_id,
            version,
            protocol::Status::VersionNotSupported,
            &[],
        );
        write_http(&mut write, "200 OK", &response).await?;
        return Ok(());
    }
    if !matches!(
        operation,
        protocol::OPERATION_PRINT_JOB
            | protocol::OPERATION_VALIDATE_JOB
            | protocol::OPERATION_GET_PRINTER_ATTRIBUTES
            | protocol::OPERATION_GET_PRINTERS
    ) {
        let response = protocol::response(
            request_id,
            version,
            protocol::Status::UnsupportedOperation,
            &[],
        );
        write_http(&mut write, "200 OK", &response).await?;
        return Ok(());
    }
    let Some(local_uri) = request.value("printer-uri") else {
        trace_issue34("printer-uri=missing");
        write_ipp_error(&mut write, protocol::Status::BadRequest).await?;
        return Ok(());
    };
    let (server_address, printer_name) = match route_uri(local_uri, authority) {
        Ok(route) => route,
        Err(_) => {
            trace_issue34("route=rejected");
            write_ipp_error(&mut write, protocol::Status::NotFound).await?;
            return Ok(());
        }
    };
    trace_issue34("route=accepted");
    let remote_uri = format!(
        "ipps://{server_address}/ipp/print/{}",
        protocol::percent_encode(printer_name.as_str())
    );
    let credential = if operation == protocol::OPERATION_PRINT_JOB
        || operation == protocol::OPERATION_VALIDATE_JOB
    {
        match channel.client_credential() {
            Some(secret) => Some(secret),
            None => {
                // The installed queue did everything right; the client is simply not configured
                // for this ShaPrint network yet.
                failures.report(PrintFailure::client(
                    ErrorCode::NotAuthorized,
                    Some(&printer_name),
                ));
                write_ipp_error(&mut write, protocol::Status::NotAuthorized).await?;
                return Ok(());
            }
        }
    } else {
        None
    };
    let forwarded = connections
        .forward_ipp(&server_address, &remote_uri, &body, credential.as_deref())
        .await;
    match forwarded {
        Ok(response) => {
            let status = match response.get(2..4) {
                Some(bytes) => {
                    let status = u16::from_be_bytes([bytes[0], bytes[1]]);
                    trace_issue34(&format!("response-status=0x{status:04x}"));
                    Some(status)
                }
                None => {
                    trace_issue34("response=truncated");
                    None
                }
            };
            if operation == protocol::OPERATION_PRINT_JOB {
                // A query the driver makes while probing is not a print failure; a rejected job is.
                let code = match status {
                    Some(status) => forwarded_failure(status),
                    None => Some(ErrorCode::QueueUnavailable),
                };
                if let Some(code) = code {
                    failures.report(PrintFailure::client(code, Some(&printer_name)));
                }
            }
            let response = if operation == protocol::OPERATION_GET_PRINTER_ATTRIBUTES {
                match protocol::rewrite_printer_uri_supported(&response, &remote_uri, local_uri) {
                    Ok(response) => response,
                    Err(_) => protocol::response(
                        request_id,
                        version,
                        protocol::Status::InternalError,
                        &[],
                    ),
                }
            } else {
                response
            };
            write_http(&mut write, "200 OK", &response).await
        }
        Err(error) => {
            trace_issue34(&format!("forward-error={}", error.code_str()));
            if operation == protocol::OPERATION_PRINT_JOB {
                failures.report(PrintFailure::client(error.code(), Some(&printer_name)));
            }
            let response =
                protocol::response(request_id, version, ipp_status_for(error.code()), &[]);
            write_http(&mut write, "200 OK", &response).await
        }
    }
}

/// The failure a user should see for an IPP status the server returned for a print job, if any.
fn forwarded_failure(status: u16) -> Option<ErrorCode> {
    // Every `successful` status shares the 0x00xx range (RFC 8011 Appendix B.1), and each of them
    // means the job was accepted: 0x0001 and 0x0002 report substituted or conflicting attributes,
    // not a failed print.
    if (0x0000..=0x00ff).contains(&status) {
        return None;
    }
    if status == protocol::Status::NotAuthorized.code() {
        return Some(ErrorCode::NotAuthorized);
    }
    if status == protocol::Status::NotFound.code() {
        return Some(ErrorCode::PrinterNotShared);
    }
    // Every `server-error-*` status shares the 0x05xx range (RFC 8011 Appendix B.1); none of them
    // reached the printer queue.
    if (0x0500..=0x05ff).contains(&status) {
        return Some(ErrorCode::QueueUnavailable);
    }
    Some(ErrorCode::InvalidInput)
}

/// The IPP status a local driver sees when forwarding failed.
///
/// The driver cannot explain these codes, so the reason a user reads travels through the failure
/// record instead; this only picks the closest standard answer.
fn ipp_status_for(code: ErrorCode) -> protocol::Status {
    match code {
        ErrorCode::InvalidInput => protocol::Status::BadRequest,
        ErrorCode::NotAuthorized
        | ErrorCode::InvalidState
        | ErrorCode::ServerNotTrusted
        | ErrorCode::ServerIdentityChanged => protocol::Status::NotAuthorized,
        ErrorCode::PrinterNotShared => protocol::Status::NotFound,
        _ => protocol::Status::InternalError,
    }
}

struct RequestHead {
    method: String,
    path: String,
    content_type: Option<String>,
    content_length: Option<usize>,
    is_chunked: bool,
    expects_continue: bool,
}

async fn read_chunked_body<R: tokio::io::AsyncBufRead + Unpin>(
    reader: &mut R,
) -> Result<Vec<u8>, AppError> {
    let mut body = Vec::new();
    loop {
        let mut line = Vec::new();
        let count = reader
            .take(1024)
            .read_until(b'\n', &mut line)
            .await
            .map_err(|_| {
                AppError::invalid_input("The local printer sent an unreadable chunk size.")
            })?;
        if count == 0 {
            return Err(AppError::invalid_input(
                "Connection closed while reading chunk size.",
            ));
        }
        let size_str = std::str::from_utf8(&line)
            .map_err(|_| AppError::invalid_input("Chunk size header not valid UTF-8."))?
            .trim();
        let chunk_size_hex = size_str.split(';').next().unwrap_or("").trim();
        let chunk_size = usize::from_str_radix(chunk_size_hex, 16).map_err(|_| {
            AppError::invalid_input(format!("Invalid chunk size: {chunk_size_hex}"))
        })?;

        if chunk_size == 0 {
            // Read trailing headers until empty line
            loop {
                let mut trailer = Vec::new();
                let trailer_count = reader
                    .take(MAX_HEAD_BYTES as u64)
                    .read_until(b'\n', &mut trailer)
                    .await
                    .map_err(|_| {
                        AppError::invalid_input(
                            "The local printer sent an unreadable chunk trailer.",
                        )
                    })?;
                if trailer_count == 0 || trailer == b"\r\n" || trailer == b"\n" {
                    break;
                }
            }
            break;
        }

        if chunk_size > MAX_BODY_BYTES.saturating_sub(body.len()) {
            return Err(AppError::invalid_input(
                "Chunked body exceeds MAX_BODY_BYTES.",
            ));
        }

        let start = body.len();
        body.resize(start + chunk_size, 0);
        reader
            .read_exact(&mut body[start..])
            .await
            .map_err(|_| AppError::invalid_input("Incomplete chunk data from local printer."))?;

        // Consume the trailing CRLF after each chunk data
        let mut delim = Vec::new();
        reader
            .take(4)
            .read_until(b'\n', &mut delim)
            .await
            .map_err(|_| AppError::invalid_input("Missing delimiter after chunk data."))?;
        if delim != b"\r\n" && delim != b"\n" {
            return Err(AppError::invalid_input("Malformed chunk delimiter."));
        }
    }
    Ok(body)
}

async fn read_head<R: tokio::io::AsyncBufRead + Unpin>(
    reader: &mut R,
) -> Result<RequestHead, AppError> {
    let mut bytes = Vec::with_capacity(512);
    loop {
        let mut line = Vec::new();
        let count = reader
            .take((MAX_HEAD_BYTES - bytes.len() + 1) as u64)
            .read_until(b'\n', &mut line)
            .await
            .map_err(|_| {
                AppError::invalid_input("The local printer sent an unreadable HTTP request.")
            })?;
        if count == 0 || bytes.len() + count > MAX_HEAD_BYTES {
            return Err(AppError::invalid_input(
                "The local printer sent an invalid HTTP request.",
            ));
        }
        bytes.extend_from_slice(&line);
        if line == b"\r\n" || line == b"\n" {
            break;
        }
    }
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| AppError::invalid_input("The local printer sent an invalid HTTP request."))?;
    let lines: Vec<&str> = text
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect();
    let mut iter = lines.iter();
    let request_line = iter.next().copied().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let path = parts.next().unwrap_or_default().to_owned();
    let version = parts.next();
    if version.is_none() || parts.next().is_some() || method.is_empty() || path.is_empty() {
        return Err(AppError::invalid_input(
            "The local printer sent an invalid HTTP request.",
        ));
    }
    let mut content_type = None;
    let mut content_length = None;
    let mut is_chunked = false;
    let mut expects_continue = false;
    for line in iter {
        if line.is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-type") {
            content_type = Some(value.trim().to_owned());
        } else if name.eq_ignore_ascii_case("content-length") {
            content_length = value.trim().parse::<usize>().ok();
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            let encodings: Vec<&str> = value.split(',').map(|s| s.trim()).collect();
            if encodings.iter().any(|&e| e.eq_ignore_ascii_case("chunked")) {
                is_chunked = true;
            } else {
                return Err(AppError::invalid_input(
                    "The local printer used an unsupported HTTP transfer encoding",
                ));
            }
        } else if name.eq_ignore_ascii_case("expect") {
            expects_continue = value.trim().eq_ignore_ascii_case("100-continue");
        }
    }
    Ok(RequestHead {
        method,
        path,
        content_type,
        content_length,
        is_chunked,
        expects_continue,
    })
}

/// Splits a local queue URI into the server it targets and the validated queue name.
fn route_uri(uri: &str, proxy_authority: &str) -> Result<(String, PrinterName), AppError> {
    let prefix = format!("ipp://{proxy_authority}/ipp/print/");
    let route = uri.strip_prefix(&prefix).ok_or_else(|| {
        AppError::invalid_input("The printer queue does not point to this local proxy.")
    })?;
    let mut segments = route.split('/');
    let server = segments.next().unwrap_or_default();
    let printer = segments.next().unwrap_or_default();
    if server.is_empty() || printer.is_empty() || segments.next().is_some() {
        return Err(AppError::invalid_input(
            "The local printer queue has an invalid destination.",
        ));
    }
    let server = protocol::percent_decode(server);
    let address = ClientConnections::normalize_address(&server)?;
    let printer = protocol::percent_decode(printer);
    let name = PrinterName::parse(&printer)?;
    Ok((address, name))
}

async fn write_ipp_error<W: tokio::io::AsyncWrite + Unpin>(
    writer: &mut W,
    status: protocol::Status,
) -> Result<(), AppError> {
    let response = protocol::response(0, protocol::IPP_VERSION_1_1, status, &[]);
    write_http(writer, "200 OK", &response).await
}

async fn write_http<W: tokio::io::AsyncWrite + Unpin>(
    writer: &mut W,
    status: &str,
    body: &[u8],
) -> Result<(), AppError> {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/ipp\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    writer
        .write_all(response.as_bytes())
        .await
        .map_err(|_| AppError::internal("Could not answer the local printer."))?;
    writer
        .write_all(body)
        .await
        .map_err(|_| AppError::internal("Could not answer the local printer."))?;
    writer
        .flush()
        .await
        .map_err(|_| AppError::internal("Could not finish the local printer response."))?;
    writer
        .shutdown()
        .await
        .map_err(|_| AppError::internal("Could not finish the local printer response."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[tokio::test]
    async fn read_head_parses_crlf_and_bare_lf() {
        let crlf = b"POST /ipp/print HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/ipp\r\nContent-Length: 42\r\nExpect: 100-continue\r\n\r\n";
        let mut reader = Cursor::new(&crlf[..]);
        let head = read_head(&mut reader).await.expect("parses CRLF head");
        assert_eq!(head.method, "POST");
        assert_eq!(head.path, "/ipp/print");
        assert_eq!(head.content_type.as_deref(), Some("application/ipp"));
        assert_eq!(head.content_length, Some(42));
        assert!(head.expects_continue);

        let bare_lf = b"POST /ipp/print HTTP/1.1\nHost: 127.0.0.1\nContent-Type: application/ipp\nContent-Length: 10\n\n";
        let mut reader = Cursor::new(&bare_lf[..]);
        let head = read_head(&mut reader).await.expect("parses bare LF head");
        assert_eq!(head.method, "POST");
        assert_eq!(head.path, "/ipp/print");
        assert_eq!(head.content_type.as_deref(), Some("application/ipp"));
        assert_eq!(head.content_length, Some(10));
        assert!(!head.expects_continue);
        assert!(!head.is_chunked);

        let chunked = b"POST /ipp/print HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/ipp\r\nTransfer-Encoding: chunked\r\n\r\n";
        let mut reader = Cursor::new(&chunked[..]);
        let head = read_head(&mut reader).await.expect("parses chunked head");
        assert_eq!(head.method, "POST");
        assert_eq!(head.path, "/ipp/print");
        assert!(head.is_chunked);
        assert_eq!(head.content_length, None);
    }

    #[tokio::test]
    async fn read_chunked_body_reads_chunks_and_trailer() {
        let chunked_data = b"5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n";
        let mut reader = Cursor::new(&chunked_data[..]);
        let body = read_chunked_body(&mut reader)
            .await
            .expect("reads chunked body");
        assert_eq!(body, b"hello world");

        let chunked_with_trailer = b"4\r\ntest\r\n0\r\nX-Trailer: value\r\n\r\n";
        let mut reader = Cursor::new(&chunked_with_trailer[..]);
        let body = read_chunked_body(&mut reader)
            .await
            .expect("reads with trailer");
        assert_eq!(body, b"test");
    }

    #[test]
    fn a_job_printed_with_substituted_attributes_is_not_a_failure() {
        // RFC 8011 Appendix B.1: the whole 0x00xx range is the `successful` class, and a printer
        // that substituted or ignored attributes (0x0001, 0x0002) still printed the job. Reporting
        // those would put a red failure panel in front of a user whose print succeeded.
        assert_eq!(forwarded_failure(0x0001), None);
        assert_eq!(forwarded_failure(0x0002), None);
        assert_eq!(forwarded_failure(0x00ff), None);
    }

    #[test]
    fn a_rejected_job_maps_to_the_condition_the_user_can_act_on() {
        assert_eq!(forwarded_failure(protocol::Status::Ok.code()), None);
        assert_eq!(
            forwarded_failure(protocol::Status::NotAuthorized.code()),
            Some(ErrorCode::NotAuthorized)
        );
        assert_eq!(
            forwarded_failure(protocol::Status::NotFound.code()),
            Some(ErrorCode::PrinterNotShared)
        );
        assert_eq!(
            forwarded_failure(protocol::Status::NotAcceptingJobs.code()),
            Some(ErrorCode::QueueUnavailable)
        );
        assert_eq!(
            forwarded_failure(protocol::Status::InternalError.code()),
            Some(ErrorCode::QueueUnavailable)
        );
        assert_eq!(
            forwarded_failure(protocol::Status::DocumentFormatNotSupported.code()),
            Some(ErrorCode::InvalidInput)
        );
    }

    #[test]
    fn a_local_driver_gets_a_standard_status_when_forwarding_fails() {
        assert_eq!(
            ipp_status_for(ErrorCode::ServerUnavailable),
            protocol::Status::InternalError
        );
        assert_eq!(
            ipp_status_for(ErrorCode::ServerNotTrusted),
            protocol::Status::NotAuthorized
        );
        assert_eq!(
            ipp_status_for(ErrorCode::ServerIdentityChanged),
            protocol::Status::NotAuthorized
        );
        assert_eq!(
            ipp_status_for(ErrorCode::NotAuthorized),
            protocol::Status::NotAuthorized
        );
        assert_eq!(
            ipp_status_for(ErrorCode::PrinterNotShared),
            protocol::Status::NotFound
        );
        assert_eq!(
            ipp_status_for(ErrorCode::InvalidInput),
            protocol::Status::BadRequest
        );
        assert_eq!(
            ipp_status_for(ErrorCode::InvalidState),
            protocol::Status::NotAuthorized
        );
    }
}
