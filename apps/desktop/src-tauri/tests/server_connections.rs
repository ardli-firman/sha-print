//! Observable manual-IPPS trust seam: TLS certificate review, explicit pinning and Get-Printers.

use rustls::ServerConfig;
use shaprint_desktop::adapters::client_connections::ServerAddress;
use shaprint_desktop::{adapters::ServerIdentity, application::ServerConnections};
use std::{
    fs,
    net::SocketAddr,
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_rustls::TlsAcceptor;

struct TestServer {
    address: SocketAddr,
    queries: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl TestServer {
    async fn start() -> Self {
        Self::start_on(None).await
    }
    async fn start_on(bind_address: Option<SocketAddr>) -> Self {
        let identity =
            ServerIdentity::generate().unwrap_or_else(|error| panic!("identity: {error}"));
        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![identity.certificate().clone()], identity.key())
            .unwrap_or_else(|error| panic!("tls config: {error}"));
        let listener = match bind_address {
            Some(address) => TcpListener::bind(address).await,
            None => TcpListener::bind("127.0.0.1:0").await,
        }
        .unwrap_or_else(|error| panic!("bind: {error}"));
        let address = listener
            .local_addr()
            .unwrap_or_else(|error| panic!("address: {error}"));
        let queries = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&queries);
        let acceptor = TlsAcceptor::from(Arc::new(config));
        let task = tokio::spawn(async move {
            while let Ok((tcp, _)) = listener.accept().await {
                let acceptor = acceptor.clone();
                let observed = Arc::clone(&observed);
                tokio::spawn(async move {
                    let Ok(mut tls) = acceptor.accept(tcp).await else {
                        return;
                    };
                    let mut request = vec![0; 8192];
                    let Ok(count) = tls.read(&mut request).await else {
                        return;
                    };
                    if count == 0 {
                        return;
                    }
                    let header = request[..count]
                        .windows(4)
                        .position(|w| w == b"\r\n\r\n")
                        .map(|i| i + 4);
                    let Some(body_start) = header else {
                        return;
                    };
                    let mut body = request[body_start..count].to_vec();
                    let content_length = String::from_utf8_lossy(&request[..body_start])
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|v| v.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    while body.len() < content_length {
                        let mut chunk = vec![0; content_length - body.len()];
                        let Ok(n) = tls.read(&mut chunk).await else {
                            return;
                        };
                        if n == 0 {
                            return;
                        }
                        body.extend_from_slice(&chunk[..n]);
                    }
                    if body.get(2..4) != Some(&0x4002u16.to_be_bytes()[..]) {
                        return;
                    }
                    observed.fetch_add(1, Ordering::SeqCst);
                    let mut ipp = vec![2, 0, 0, 0, 0, 0, 0, 1, 0x04, 0x42, 0, 12];
                    ipp.extend_from_slice(b"printer-name");
                    ipp.extend_from_slice(&5u16.to_be_bytes());
                    ipp.extend_from_slice(b"Zebra");
                    ipp.push(0x03);
                    let http = format!("HTTP/1.1 200 OK\r\nContent-Type: application/ipp\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", ipp.len());
                    let _ = tls.write_all(http.as_bytes()).await;
                    let _ = tls.write_all(&ipp).await;
                    let _ = tls.shutdown().await;
                });
            }
        });
        Self {
            address,
            queries,
            task,
        }
    }
}
impl Drop for TestServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl TestServer {
    async fn stop(mut self) {
        self.task.abort();
        let _ = (&mut self.task).await;
    }
}

fn temp_dir() -> PathBuf {
    std::env::temp_dir().join(format!(
        "shaprint-client-trust-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    ))
}
fn endpoint(server: &TestServer) -> String {
    format!("127.0.0.1:{}", server.address.port())
}

#[tokio::test]
async fn an_untrusted_probe_sends_no_printer_query_and_approval_survives_reopen() {
    let path = temp_dir();
    let server = TestServer::start().await;
    let address = endpoint(&server);
    let client = ServerConnections::new(&path).unwrap_or_else(|error| panic!("store: {error}"));
    let review = client
        .inspect(&address)
        .await
        .unwrap_or_else(|error| panic!("inspect: {error}"));
    assert!(!review.trusted);
    assert!(review.previous_fingerprint.is_none());
    assert_eq!(server.queries.load(Ordering::SeqCst), 0);
    client
        .approve(&address, &review.current_fingerprint)
        .await
        .unwrap_or_else(|error| panic!("approve: {error}"));
    drop(client);
    let reopened = ServerConnections::new(&path).unwrap_or_else(|error| panic!("reopen: {error}"));
    let trusted = reopened
        .inspect(&address)
        .await
        .unwrap_or_else(|error| panic!("inspect approved: {error}"));
    assert!(trusted.trusted);
    let printers = reopened
        .printers(&address)
        .await
        .unwrap_or_else(|error| panic!("printers: {error}"));
    assert_eq!(printers.printers, vec!["Zebra"]);
    assert_eq!(server.queries.load(Ordering::SeqCst), 1);
    drop(reopened);
    let _ = fs::remove_dir_all(path);
}

#[tokio::test]
async fn changed_certificate_is_blocked_until_current_fingerprint_is_explicitly_approved() {
    let path = temp_dir();
    let first = TestServer::start().await;
    let socket = first.address;
    let address = endpoint(&first);
    let client = ServerConnections::new(&path).unwrap_or_else(|error| panic!("store: {error}"));
    let original = client
        .inspect(&address)
        .await
        .unwrap_or_else(|error| panic!("inspect: {error}"));
    client
        .approve(&address, &original.current_fingerprint)
        .await
        .unwrap_or_else(|error| panic!("approve: {error}"));
    first.stop().await;
    let replacement = TestServer::start_on(Some(socket)).await;
    let changed = client
        .inspect(&address)
        .await
        .unwrap_or_else(|error| panic!("inspect changed: {error}"));
    assert!(!changed.trusted);
    assert_eq!(
        changed.previous_fingerprint.as_deref(),
        Some(original.current_fingerprint.as_str())
    );
    assert!(client.printers(&address).await.is_err());
    assert_eq!(replacement.queries.load(Ordering::SeqCst), 0);
    client
        .approve(&address, &changed.current_fingerprint)
        .await
        .unwrap_or_else(|error| panic!("reapprove: {error}"));
    assert_eq!(
        client
            .printers(&address)
            .await
            .unwrap_or_else(|error| panic!("printers: {error}"))
            .printers,
        vec!["Zebra"]
    );
    assert_eq!(replacement.queries.load(Ordering::SeqCst), 1);
    drop(client);
    let _ = fs::remove_dir_all(path);
}
#[test]
fn addresses_normalize_case_and_apply_default_port() {
    assert_eq!(
        ServerAddress::parse("PrintServer.local")
            .ok()
            .map(|a| a.normalized),
        Some("printserver.local:8631".to_owned())
    );
    assert_eq!(
        ServerAddress::parse("192.0.2.4:9100")
            .ok()
            .map(|a| a.normalized),
        Some("192.0.2.4:9100".to_owned())
    );
    assert!(ServerAddress::parse("https://printer.local").is_err());
    assert!(ServerAddress::parse("printer.local:0").is_err());
}
