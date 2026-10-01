//! The HTTP/1.1 framing IPP is carried in (RFC 8010 §4): one POST with `Content-Type:
//! application/ipp` and one response, then the connection closes.

use std::io;

use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt,
};

/// Longest request head the endpoint reads.
pub const MAX_HEAD_BYTES: usize = 8 * 1024;

/// Longest request body the endpoint reads, including the IPP attributes and document data.
pub const MAX_BODY_BYTES: usize = 64 * 1024 * 1024;

/// Content type IPP requests and responses use.
pub const IPP_CONTENT_TYPE: &str = "application/ipp";

/// A request head the endpoint can act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Head {
    pub method: String,
    pub path: String,
    headers: Vec<(String, String)>,
}

impl Head {
    /// Case-insensitive header lookup, as HTTP requires.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// Whether the client waits for the endpoint's permission before sending the body.
    pub fn expects_continue(&self) -> bool {
        self.header("expect")
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("100-continue"))
    }
}

/// Why a request was refused before any IPP was read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpError {
    MethodNotAllowed,
    LengthRequired,
    TooLarge,
    UnsupportedTransferEncoding,
    Malformed,
}

impl HttpError {
    /// The status line the endpoint answers with.
    pub const fn status_line(self) -> &'static str {
        match self {
            HttpError::MethodNotAllowed => "405 Method Not Allowed",
            HttpError::LengthRequired => "411 Length Required",
            HttpError::TooLarge => "413 Payload Too Large",
            HttpError::UnsupportedTransferEncoding => "501 Not Implemented",
            HttpError::Malformed => "400 Bad Request",
        }
    }
}

/// Reads a request head from `reader`.
///
/// The head is read in bounded pieces and refused as soon as it exceeds [`MAX_HEAD_BYTES`], so a
/// client cannot make the endpoint allocate more than the limit just by withholding a newline.
pub async fn read_head<R: AsyncBufRead + Unpin>(reader: &mut R) -> Result<Head, HttpError> {
    let mut head: Vec<u8> = Vec::with_capacity(512);
    let mut line_start = 0usize;

    loop {
        let previous = head.len();
        {
            let available = reader.fill_buf().await.map_err(|_| HttpError::Malformed)?;
            if available.is_empty() {
                // The client closed the connection before finishing its head.
                return Err(HttpError::Malformed);
            }
            let budget = MAX_HEAD_BYTES.saturating_sub(head.len());
            if budget == 0 {
                return Err(HttpError::TooLarge);
            }
            head.extend_from_slice(&available[..available.len().min(budget)]);
        }

        // A head ends at the first empty line; anything after it is the body and stays unread.
        let mut scan = line_start;
        while let Some(offset) = head[scan..].iter().position(|byte| *byte == b'\n') {
            let line_end = scan + offset;
            if without_line_ending(&head[scan..line_end]).is_empty() {
                let head_bytes = line_end + 1;
                reader.consume(head_bytes - previous);
                return parse_head(&head[..head_bytes]);
            }
            scan = line_end + 1;
        }

        line_start = scan;
        reader.consume(head.len() - previous);
        if head.len() >= MAX_HEAD_BYTES {
            return Err(HttpError::TooLarge);
        }
    }
}

/// Removes the line ending from a line of the head.
fn without_line_ending(line: &[u8]) -> &[u8] {
    match line.split_last() {
        Some((b'\r', rest)) => rest,
        _ => line,
    }
}

/// Turns the raw head into a request line and its headers.
fn parse_head(head: &[u8]) -> Result<Head, HttpError> {
    let text = String::from_utf8_lossy(head);
    let lines: Vec<&str> = text
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect();

    let request_line = lines.first().ok_or(HttpError::Malformed)?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().ok_or(HttpError::Malformed)?.to_owned();
    let path = parts.next().ok_or(HttpError::Malformed)?.to_owned();
    let version = parts.next().ok_or(HttpError::Malformed)?;
    if !version.starts_with("HTTP/") {
        return Err(HttpError::Malformed);
    }

    let mut headers = Vec::with_capacity(lines.len() - 1);
    for line in &lines[1..] {
        if line.is_empty() {
            break;
        }
        let (name, value) = line.split_once(':').ok_or(HttpError::Malformed)?;
        headers.push((name.trim().to_owned(), value.trim().to_owned()));
    }

    Ok(Head {
        method,
        path,
        headers,
    })
}

/// Checks the head and reports how many body bytes to read.
pub fn body_length(head: &Head) -> Result<usize, HttpError> {
    if head.method != "POST" {
        return Err(HttpError::MethodNotAllowed);
    }
    if head.header("transfer-encoding").is_some() {
        // IPP clients send a length; chunked bodies would need a decoder this endpoint does not
        // have, and silently ignoring the framing would corrupt the request.
        return Err(HttpError::UnsupportedTransferEncoding);
    }
    let content_type = head.header("content-type").ok_or(HttpError::Malformed)?;
    if !content_type
        .trim()
        .to_ascii_lowercase()
        .starts_with(IPP_CONTENT_TYPE)
    {
        return Err(HttpError::Malformed);
    }

    let length = head
        .header("content-length")
        .ok_or(HttpError::LengthRequired)?;
    let length: usize = length.trim().parse().map_err(|_| HttpError::Malformed)?;
    if length > MAX_BODY_BYTES {
        return Err(HttpError::TooLarge);
    }
    Ok(length)
}

/// Tells a client to go ahead with the body it held back.
pub async fn send_continue<W: AsyncWrite + Unpin>(writer: &mut W) -> io::Result<()> {
    writer.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").await?;
    writer.flush().await
}

/// Writes one response and closes the connection.
pub async fn write_response<W: AsyncWrite + Unpin>(
    writer: &mut W,
    status_line: &str,
    content_type: &str,
    body: &[u8],
) -> io::Result<()> {
    let head = format!(
        "HTTP/1.1 {status_line}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    writer.write_all(head.as_bytes()).await?;
    writer.write_all(body).await?;
    writer.flush().await
}

/// Reads exactly `length` bytes of request body.
pub async fn read_body<R: AsyncRead + Unpin>(reader: &mut R, length: usize) -> io::Result<Vec<u8>> {
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).await?;
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::BufReader;

    async fn head(text: &str) -> Result<Head, HttpError> {
        let mut reader = BufReader::new(text.as_bytes());
        read_head(&mut reader).await
    }

    #[tokio::test]
    async fn a_post_head_is_parsed() {
        let parsed = head(
            "POST /ipp/print HTTP/1.1\r\nHost: server:8631\r\nContent-Type: application/ipp\r\nContent-Length: 42\r\n\r\n",
        )
        .await
        .expect("parses");

        assert_eq!(parsed.method, "POST");
        assert_eq!(parsed.path, "/ipp/print");
        assert_eq!(parsed.header("Host"), Some("server:8631"));
        assert_eq!(parsed.header("content-length"), Some("42"));
        assert!(!parsed.expects_continue());
    }

    #[tokio::test]
    async fn a_head_that_never_ends_is_rejected() {
        let long = format!("GET / HTTP/1.1\r\nHost: {}\r\n", "a".repeat(MAX_HEAD_BYTES));
        assert_eq!(
            head(&long).await.expect_err("too large"),
            HttpError::TooLarge
        );
        assert_eq!(head("").await.expect_err("closed"), HttpError::Malformed);
        assert_eq!(
            head("MALFORMED\r\n\r\n").await.expect_err("bad line"),
            HttpError::Malformed
        );
        assert_eq!(
            head("POST / HTTP/1.1\r\nbroken\r\n\r\n")
                .await
                .expect_err("no colon"),
            HttpError::Malformed
        );
    }

    #[tokio::test]
    async fn a_client_streaming_without_a_newline_is_refused_without_buffering_it() {
        // The client keeps sending and never ends its head; the endpoint must give up at its limit
        // instead of growing a buffer to whatever the client sends.
        let (mut client, server) = tokio::io::duplex(64);
        let reading = tokio::spawn(async move {
            let mut reader = BufReader::with_capacity(256, server);
            read_head(&mut reader).await
        });

        let chunk = vec![b'a'; 64];
        let mut sent = 0usize;
        while sent < MAX_HEAD_BYTES * 2 {
            // Once the endpoint refuses the head it closes its side, and writing fails.
            if client.write_all(&chunk).await.is_err() {
                break;
            }
            sent += chunk.len();
        }

        assert_eq!(
            reading.await.expect("task finishes").expect_err("refused"),
            HttpError::TooLarge
        );
    }

    #[tokio::test]
    async fn a_body_that_arrived_with_the_head_stays_readable() {
        // A client may send the head and the body in one packet; the head reader must stop exactly
        // at the blank line so the body is not swallowed.
        let mut reader = BufReader::new(
            &b"POST /ipp/print HTTP/1.1\r\nContent-Length: 4\r\nContent-Type: application/ipp\r\n\r\nBODY"[..],
        );

        let parsed = read_head(&mut reader).await.expect("parses");
        let length = body_length(&parsed).expect("accepted");
        let body = read_body(&mut reader, length)
            .await
            .expect("reads the body");

        assert_eq!(body, b"BODY");
    }

    #[tokio::test]
    async fn a_head_with_bare_line_feeds_is_parsed() {
        let parsed = head("POST /ipp/print HTTP/1.1\nHost: server\nContent-Length: 0\nContent-Type: application/ipp\n\n")
            .await
            .expect("parses");

        assert_eq!(parsed.header("host"), Some("server"));
    }

    #[tokio::test]
    async fn only_a_well_formed_ipp_post_is_accepted() {
        let accepted = head(
            "POST /ipp/print HTTP/1.1\r\nContent-Type: application/ipp\r\nContent-Length: 10\r\n\r\n",
        )
        .await
        .expect("parses");
        assert_eq!(body_length(&accepted).expect("accepted"), 10);

        let get = head("GET / HTTP/1.1\r\n\r\n").await.expect("parses");
        assert_eq!(
            body_length(&get).expect_err("refused"),
            HttpError::MethodNotAllowed
        );

        let chunked = head(
            "POST / HTTP/1.1\r\nContent-Type: application/ipp\r\nTransfer-Encoding: chunked\r\n\r\n",
        )
        .await
        .expect("parses");
        assert_eq!(
            body_length(&chunked).expect_err("refused"),
            HttpError::UnsupportedTransferEncoding
        );

        let no_length = head("POST / HTTP/1.1\r\nContent-Type: application/ipp\r\n\r\n")
            .await
            .expect("parses");
        assert_eq!(
            body_length(&no_length).expect_err("refused"),
            HttpError::LengthRequired
        );

        let wrong_type =
            head("POST / HTTP/1.1\r\nContent-Type: text/plain\r\nContent-Length: 1\r\n\r\n")
                .await
                .expect("parses");
        assert_eq!(
            body_length(&wrong_type).expect_err("refused"),
            HttpError::Malformed
        );

        let huge = head(
            "POST / HTTP/1.1\r\nContent-Type: application/ipp\r\nContent-Length: 99999999\r\n\r\n",
        )
        .await
        .expect("parses");
        assert_eq!(
            body_length(&huge).expect_err("refused"),
            HttpError::TooLarge
        );
    }

    #[tokio::test]
    async fn a_client_that_waits_for_permission_is_told_to_continue() {
        let waiting = head(
            "POST /ipp/print HTTP/1.1\r\nContent-Type: application/ipp\r\nContent-Length: 4\r\nExpect: 100-continue\r\n\r\n",
        )
        .await
        .expect("parses");

        assert!(waiting.expects_continue());
    }

    #[tokio::test]
    async fn a_response_carries_the_ipp_content_type_and_closes() {
        let mut written = Vec::new();
        write_response(&mut written, "200 OK", IPP_CONTENT_TYPE, &[1, 2, 3])
            .await
            .expect("writes");

        let text = String::from_utf8_lossy(&written);
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(text.contains("Content-Type: application/ipp\r\n"));
        assert!(text.contains("Content-Length: 3\r\n"));
        assert!(text.contains("Connection: close\r\n\r\n"));
        assert!(written.ends_with(&[1, 2, 3]));
    }
}
