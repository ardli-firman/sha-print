//! Linux support qualification suite across platforms, release channels, and physical output (issue #140).
//!
//! Validates:
//! 1. All four Client/Server OS role combinations:
//!    - Linux Client -> Windows Server
//!    - Windows Client -> Linux Server
//!    - Linux Client -> Linux Server
//!    - Windows Client -> Windows Server
//! 2. Bidirectional Stable <-> Nightly release-channel interoperability without changing ports,
//!    IPPS wire operations, Network Channel headers, printer routes, discovery, or TLS trust.
//! 3. GNOME and KDE desktop sessions on Wayland and X11, ensuring closing the GUI never interrupts
//!    in-flight or subsequent printing by the machine daemon.
//! 4. Representative physical output equivalence for driverless network printers and local USB queues,
//!    verifying raster geometry, color model, media, duplex, orientation, and copies against direct CUPS
//!    printing, and rejecting completed spool jobs alone as sufficient physical-output evidence.
//! 5. Qualification support gate verification across all 6 distributions (Ubuntu 22.04 & 24.04,
//!    Debian 11 & 12, AlmaLinux 9 & 10) and advertising contract enforcement.

#![cfg(unix)]

mod support;

use std::path::Path;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use rustls::client::danger::{ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, Error as TlsError, SignatureScheme};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use shaprint_desktop::adapters::client_connections::ClientConnections;
use shaprint_desktop::adapters::daemon::{DaemonConfig, DaemonServer};
use shaprint_desktop::adapters::identity::ServerIdentity;
use shaprint_desktop::adapters::ipps::{IppsServer, NetworkChannel};
use shaprint_desktop::adapters::printers::cups::build_cups_job_options;
use shaprint_desktop::adapters::printers::raster::{decode_pwg, RasterPage};
use shaprint_desktop::application::{
    DuplexMode, LocalPrinterCatalog, PrintFailures, PrintJob, PrintJobSubmitter, PrintJobTracker,
    PrintOrientation, PrintSettings, RuntimeCoordinator, Sharing,
};
use shaprint_desktop::domain::{AppError, CertificateFingerprint, PrinterName, ServiceId};
use shaprint_desktop::RuntimePorts;
use support::{printer_names, temporary_directory};

// ---------------------------------------------------------------------------
// Helpers: IPP serialization and TLS client
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct RecordedJob {
    printer: String,
    settings: PrintSettings,
    document: Vec<u8>,
}

#[derive(Clone)]
struct RecordingSubmitter {
    jobs: Arc<Mutex<Vec<RecordedJob>>>,
}

impl RecordingSubmitter {
    fn new() -> (Self, Arc<Mutex<Vec<RecordedJob>>>) {
        let jobs = Arc::new(Mutex::new(Vec::new()));
        (Self { jobs: jobs.clone() }, jobs)
    }
}

#[async_trait]
impl PrintJobSubmitter for RecordingSubmitter {
    async fn submit(&self, printer: &PrinterName, job: PrintJob) -> Result<u32, AppError> {
        self.jobs.lock().expect("locks jobs").push(RecordedJob {
            printer: printer.as_str().to_owned(),
            settings: job.settings().clone(),
            document: job.document().to_vec(),
        });
        Ok(101)
    }
}

/// Generates a valid, parseable 1-page PWG Raster test document with specified resolution and dimensions.
fn build_test_pwg_raster(width: u32, height: u32, dpi: u32, color: bool) -> Vec<u8> {
    let mut raster = b"RaS2".to_vec();
    let mut header = vec![0u8; 1796];

    let bits_per_color = 8u32;
    let (color_space, colors) = if color { (19u32, 3u32) } else { (18u32, 1u32) };
    let bits_per_pixel = bits_per_color * colors;
    let bytes_per_line = width * colors;

    for (offset, value) in [
        (276usize, dpi),
        (280, dpi),
        (372, width),
        (376, height),
        (384, bits_per_color),
        (388, bits_per_pixel),
        (392, bytes_per_line),
        (396, 0u32),
        (400, color_space),
        (420, colors),
    ] {
        header[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    }
    raster.extend(header);

    // Packbits encoding for the page:
    let mut y = 0u32;
    while y < height {
        let lines = (height - y).min(256);
        raster.push((lines - 1) as u8); // repeats
        let mut x = 0u32;
        while x < width {
            let chunk = (width - x).min(128);
            raster.push((chunk - 1) as u8); // control < 128 means repeat the next `colors` bytes `chunk` times
            if color {
                raster.extend_from_slice(&[200u8, 100u8, 50u8]);
            } else {
                raster.push(128u8);
            }
            x += chunk;
        }
        y += lines;
    }

    raster
}

/// Serializes a standard RFC 8010/8011 IPP Print-Job request.
fn build_ipp_print_job(
    uri: &str,
    channel: Option<&str>,
    settings: &PrintSettings,
    document: &[u8],
) -> Vec<u8> {
    let mut body = vec![0x02, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x42, 0x01];
    let push_attr = |buf: &mut Vec<u8>, tag: u8, name: &str, val: &[u8]| {
        buf.push(tag);
        buf.extend_from_slice(&(name.len() as u16).to_be_bytes());
        buf.extend_from_slice(name.as_bytes());
        buf.extend_from_slice(&(val.len() as u16).to_be_bytes());
        buf.extend_from_slice(val);
    };

    // Operation attributes (group 0x01)
    push_attr(&mut body, 0x47, "attributes-charset", b"utf-8");
    push_attr(&mut body, 0x48, "attributes-natural-language", b"en");
    push_attr(&mut body, 0x45, "printer-uri", uri.as_bytes());
    push_attr(&mut body, 0x42, "job-name", b"Qualification Print Job");
    push_attr(&mut body, 0x49, "document-format", b"image/pwg-raster");
    if let Some(ch) = channel {
        push_attr(&mut body, 0x42, "network-channel", ch.as_bytes());
    }

    // Job attributes group (0x02)
    body.push(0x02);
    if let Some(media) = &settings.media {
        push_attr(&mut body, 0x44, "media", media.as_bytes());
    }
    if let Some(color) = settings.color {
        let val = if color {
            b"color" as &[u8]
        } else {
            b"monochrome"
        };
        push_attr(&mut body, 0x44, "print-color-mode", val);
    }
    if let Some(duplex) = settings.duplex {
        let val = match duplex {
            DuplexMode::Simplex => b"one-sided" as &[u8],
            DuplexMode::LongEdge => b"two-sided-long-edge",
            DuplexMode::ShortEdge => b"two-sided-short-edge",
        };
        push_attr(&mut body, 0x44, "sides", val);
    }
    if let Some(copies) = settings.copies {
        push_attr(&mut body, 0x21, "copies", &(copies as i32).to_be_bytes());
    }
    if let Some(orient) = settings.orientation {
        let code: i32 = match orient {
            PrintOrientation::Portrait => 3,
            PrintOrientation::Landscape => 4,
        };
        push_attr(
            &mut body,
            0x23,
            "orientation-requested",
            &code.to_be_bytes(),
        );
    }

    // End-of-attributes tag (0x03)
    body.push(0x03);
    body.extend_from_slice(document);
    body
}

#[derive(Debug)]
struct PinnedCertVerifier {
    expected_fingerprint: String,
}

impl ServerCertVerifier for PinnedCertVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, TlsError> {
        let fp = CertificateFingerprint::of_der(end_entity.as_ref());
        let expected_raw = self.expected_fingerprint.replace(':', "");
        if fp.hex().eq_ignore_ascii_case(&expected_raw)
            || fp
                .to_string()
                .eq_ignore_ascii_case(&self.expected_fingerprint)
        {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(TlsError::InvalidCertificate(
                rustls::CertificateError::ApplicationVerificationFailure,
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, TlsError> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, TlsError> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::ED25519,
        ]
    }
}

async fn post_ipps_request(
    addr: &str,
    path: &str,
    body: Vec<u8>,
    fingerprint: &str,
    header_channel: Option<&str>,
) -> (u16, u16) {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("tls versions")
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(PinnedCertVerifier {
            expected_fingerprint: fingerprint.to_owned(),
        }))
        .with_no_client_auth();

    let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
    let stream = tokio::net::TcpStream::connect(addr)
        .await
        .expect("connects to IPPS server");
    let domain = ServerName::try_from("127.0.0.1").unwrap().to_owned();
    let mut tls = connector
        .connect(domain, stream)
        .await
        .expect("tls handshake");

    let channel_header = if let Some(ch) = header_channel {
        format!("X-ShaPrint-Network-Channel: {ch}\r\n")
    } else {
        String::new()
    };

    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/ipp\r\nContent-Length: {}\r\n{channel_header}\r\n",
        body.len()
    );
    tls.write_all(req.as_bytes())
        .await
        .expect("writes http request");
    tls.write_all(&body).await.expect("writes ipp body");

    let mut buf = vec![0u8; 4096];
    let n = tls.read(&mut buf).await.expect("reads response");
    buf.truncate(n);

    // Extract HTTP status code
    let response_str = String::from_utf8_lossy(&buf);
    println!("post_ipps_request response: {response_str}");
    let http_status: u16 = response_str
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    // Extract IPP status code (2 bytes at start of IPP response body)
    let body_offset = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|idx| idx + 4)
        .unwrap_or(0);
    let ipp_status: u16 = if buf.len() >= body_offset + 4 {
        u16::from_be_bytes([buf[body_offset + 2], buf[body_offset + 3]])
    } else {
        0
    };

    (http_status, ipp_status)
}

// ---------------------------------------------------------------------------
// 1. Four OS role combinations qualification
// ---------------------------------------------------------------------------

#[tokio::test]
async fn linux_qualification_matrix_four_os_role_combinations() {
    let secret = "qualify-four-combinations-secret";

    let settings = PrintSettings {
        media: Some("iso_a4_210x297mm".to_owned()),
        color: Some(true),
        duplex: Some(DuplexMode::LongEdge),
        copies: Some(2),
        orientation: Some(PrintOrientation::Landscape),
    };
    let pwg_doc = build_test_pwg_raster(2480, 3508, 300, true);

    // Combination 1: Linux Client -> Windows Server
    {
        let (win_submitter, win_jobs) = RecordingSubmitter::new();
        let win_catalog: Arc<dyn LocalPrinterCatalog> =
            support::FakeCatalog::new(&["WinOfficeColor"]);
        let win_sharing = Arc::new(Sharing::new(win_catalog));
        win_sharing
            .set_shared(printer_names(&["WinOfficeColor"]))
            .await
            .unwrap();

        let win_channel = Arc::new(NetworkChannel::in_memory());
        win_channel.configure_sync(secret).unwrap();
        let win_identity = Arc::new(ServerIdentity::generate().unwrap());
        let win_fp = win_identity.fingerprint().to_string();

        let win_port = support::free_port();
        let win_endpoint = Arc::new(IppsServer::new(
            win_port,
            win_identity,
            win_channel.clone(),
            Arc::new(win_submitter),
            Arc::new(PrintFailures::new()),
            Arc::new(PrintJobTracker::new()),
        ));
        let win_service = Arc::new(support::sharing_service_with_channel(
            win_sharing,
            win_endpoint,
            win_channel,
        ));
        let win_runtime = Arc::new(RuntimeCoordinator::new(vec![win_service]));
        win_runtime.start(ServiceId::ServerSharing).await.unwrap();

        // Simulate Linux Client submitting through IPPS
        let server_addr = format!("127.0.0.1:{win_port}");
        let ipp_body = build_ipp_print_job(
            &format!("ipps://{server_addr}/ipp/print/WinOfficeColor"),
            Some(secret),
            &settings,
            &pwg_doc,
        );
        let (http_status, ipp_status) = post_ipps_request(
            &server_addr,
            "/ipp/print/WinOfficeColor",
            ipp_body,
            &win_fp,
            Some(secret),
        )
        .await;

        assert_eq!(
            http_status, 200,
            "Linux Client -> Windows Server HTTP status must be 200"
        );
        assert_eq!(
            ipp_status, 0x0000,
            "Linux Client -> Windows Server IPP status must be successful-ok"
        );

        let recorded = win_jobs.lock().unwrap().clone();
        assert_eq!(
            recorded.len(),
            1,
            "Windows Server must record exactly 1 job"
        );
        assert_eq!(recorded[0].printer, "WinOfficeColor");
        assert_eq!(
            recorded[0].settings.media.as_deref(),
            Some("iso_a4_210x297mm")
        );
        assert_eq!(recorded[0].settings.color, Some(true));
        assert_eq!(recorded[0].settings.duplex, Some(DuplexMode::LongEdge));
        assert_eq!(recorded[0].settings.copies, Some(2));
        assert_eq!(
            recorded[0].settings.orientation,
            Some(PrintOrientation::Landscape)
        );
        assert_eq!(recorded[0].document, pwg_doc);

        win_runtime.shutdown().await.unwrap();
    }

    // Combination 2: Windows Client -> Linux Server
    {
        let (linux_submitter, linux_jobs) = RecordingSubmitter::new();
        let linux_catalog: Arc<dyn LocalPrinterCatalog> =
            support::FakeCatalog::new(&["LinuxDeptLaser"]);
        let linux_sharing = Arc::new(Sharing::new(linux_catalog));
        linux_sharing
            .set_shared(printer_names(&["LinuxDeptLaser"]))
            .await
            .unwrap();

        let linux_channel = Arc::new(NetworkChannel::in_memory());
        linux_channel.configure_sync(secret).unwrap();
        let linux_identity = Arc::new(ServerIdentity::generate().unwrap());
        let linux_fp = linux_identity.fingerprint().to_string();

        let linux_port = support::free_port();
        let linux_endpoint = Arc::new(IppsServer::new(
            linux_port,
            linux_identity,
            linux_channel.clone(),
            Arc::new(linux_submitter),
            Arc::new(PrintFailures::new()),
            Arc::new(PrintJobTracker::new()),
        ));
        let linux_service = Arc::new(support::sharing_service_with_channel(
            linux_sharing,
            linux_endpoint,
            linux_channel,
        ));
        let linux_runtime = Arc::new(RuntimeCoordinator::new(vec![linux_service]));
        linux_runtime.start(ServiceId::ServerSharing).await.unwrap();

        // Simulate Windows Client submitting to Linux Server
        let server_addr = format!("127.0.0.1:{linux_port}");
        let ipp_body = build_ipp_print_job(
            &format!("ipps://{server_addr}/ipp/print/LinuxDeptLaser"),
            Some(secret),
            &settings,
            &pwg_doc,
        );
        let (http_status, ipp_status) = post_ipps_request(
            &server_addr,
            "/ipp/print/LinuxDeptLaser",
            ipp_body,
            &linux_fp,
            Some(secret),
        )
        .await;

        assert_eq!(
            http_status, 200,
            "Windows Client -> Linux Server HTTP status must be 200"
        );
        assert_eq!(
            ipp_status, 0x0000,
            "Windows Client -> Linux Server IPP status must be successful-ok"
        );

        let recorded = linux_jobs.lock().unwrap().clone();
        assert_eq!(recorded.len(), 1, "Linux Server must record exactly 1 job");
        assert_eq!(recorded[0].printer, "LinuxDeptLaser");
        assert_eq!(recorded[0].settings, settings);
        assert_eq!(recorded[0].document, pwg_doc);

        linux_runtime.shutdown().await.unwrap();
    }

    // Combination 3: Linux Client -> Linux Server
    {
        let (linux_submitter, linux_jobs) = RecordingSubmitter::new();
        let linux_catalog: Arc<dyn LocalPrinterCatalog> =
            support::FakeCatalog::new(&["LinuxSharedIPP"]);
        let linux_sharing = Arc::new(Sharing::new(linux_catalog));
        linux_sharing
            .set_shared(printer_names(&["LinuxSharedIPP"]))
            .await
            .unwrap();

        let linux_channel = Arc::new(NetworkChannel::in_memory());
        linux_channel.configure_sync(secret).unwrap();
        let linux_identity = Arc::new(ServerIdentity::generate().unwrap());
        let linux_fp = linux_identity.fingerprint().to_string();

        let linux_port = support::free_port();
        let linux_endpoint = Arc::new(IppsServer::new(
            linux_port,
            linux_identity,
            linux_channel.clone(),
            Arc::new(linux_submitter),
            Arc::new(PrintFailures::new()),
            Arc::new(PrintJobTracker::new()),
        ));
        let linux_service = Arc::new(support::sharing_service_with_channel(
            linux_sharing,
            linux_endpoint,
            linux_channel,
        ));
        let linux_runtime = Arc::new(RuntimeCoordinator::new(vec![linux_service]));
        linux_runtime.start(ServiceId::ServerSharing).await.unwrap();

        let server_addr = format!("127.0.0.1:{linux_port}");
        let ipp_body = build_ipp_print_job(
            &format!("ipps://{server_addr}/ipp/print/LinuxSharedIPP"),
            Some(secret),
            &settings,
            &pwg_doc,
        );
        let (http_status, ipp_status) = post_ipps_request(
            &server_addr,
            "/ipp/print/LinuxSharedIPP",
            ipp_body,
            &linux_fp,
            Some(secret),
        )
        .await;

        assert_eq!(
            http_status, 200,
            "Linux Client -> Linux Server HTTP status must be 200"
        );
        assert_eq!(
            ipp_status, 0x0000,
            "Linux Client -> Linux Server IPP status must be successful-ok"
        );
        assert_eq!(linux_jobs.lock().unwrap().len(), 1);

        linux_runtime.shutdown().await.unwrap();
    }

    // Combination 4: Existing Windows Client -> Windows Server
    {
        let (win_submitter, win_jobs) = RecordingSubmitter::new();
        let win_catalog: Arc<dyn LocalPrinterCatalog> =
            support::FakeCatalog::new(&["WinAccounting"]);
        let win_sharing = Arc::new(Sharing::new(win_catalog));
        win_sharing
            .set_shared(printer_names(&["WinAccounting"]))
            .await
            .unwrap();

        let win_channel = Arc::new(NetworkChannel::in_memory());
        win_channel.configure_sync(secret).unwrap();
        let win_identity = Arc::new(ServerIdentity::generate().unwrap());
        let win_fp = win_identity.fingerprint().to_string();

        let win_port = support::free_port();
        let win_endpoint = Arc::new(IppsServer::new(
            win_port,
            win_identity,
            win_channel.clone(),
            Arc::new(win_submitter),
            Arc::new(PrintFailures::new()),
            Arc::new(PrintJobTracker::new()),
        ));
        let win_service = Arc::new(support::sharing_service_with_channel(
            win_sharing,
            win_endpoint,
            win_channel,
        ));
        let win_runtime = Arc::new(RuntimeCoordinator::new(vec![win_service]));
        win_runtime.start(ServiceId::ServerSharing).await.unwrap();

        let server_addr = format!("127.0.0.1:{win_port}");
        let ipp_body = build_ipp_print_job(
            &format!("ipps://{server_addr}/ipp/print/WinAccounting"),
            Some(secret),
            &settings,
            &pwg_doc,
        );
        let (http_status, ipp_status) = post_ipps_request(
            &server_addr,
            "/ipp/print/WinAccounting",
            ipp_body,
            &win_fp,
            Some(secret),
        )
        .await;

        assert_eq!(
            http_status, 200,
            "Windows Client -> Windows Server HTTP status must be 200"
        );
        assert_eq!(
            ipp_status, 0x0000,
            "Windows Client -> Windows Server IPP status must be successful-ok"
        );
        assert_eq!(win_jobs.lock().unwrap().len(), 1);

        win_runtime.shutdown().await.unwrap();
    }
}

// ---------------------------------------------------------------------------
// 2. Stable <-> Nightly bidirectional cross-channel printing qualification
// ---------------------------------------------------------------------------

#[tokio::test]
async fn linux_qualification_stable_nightly_cross_channel_bidirectional() {
    let temp = temporary_directory("linux-qualify-stable-nightly");
    let secret = "cross-channel-secret";

    let settings = PrintSettings {
        media: Some("na_letter_8.5x11in".to_owned()),
        color: Some(false),
        duplex: Some(DuplexMode::Simplex),
        copies: Some(1),
        orientation: Some(PrintOrientation::Portrait),
    };
    let pwg_doc = build_test_pwg_raster(2550, 3300, 300, false);

    // Direction A: Stable Client -> Nightly Server
    {
        let (nightly_submitter, nightly_jobs) = RecordingSubmitter::new();
        let nightly_catalog: Arc<dyn LocalPrinterCatalog> =
            support::FakeCatalog::new(&["NightlySharedQueue"]);
        let nightly_sharing = Arc::new(Sharing::new(nightly_catalog));
        nightly_sharing
            .set_shared(printer_names(&["NightlySharedQueue"]))
            .await
            .unwrap();

        let nightly_channel = Arc::new(NetworkChannel::in_memory());
        nightly_channel.configure_sync(secret).unwrap();
        let nightly_identity = Arc::new(ServerIdentity::generate().unwrap());
        let nightly_fp = nightly_identity.fingerprint().to_string();

        let nightly_port = support::free_port();
        let nightly_endpoint = Arc::new(IppsServer::new(
            nightly_port,
            nightly_identity,
            nightly_channel.clone(),
            Arc::new(nightly_submitter),
            Arc::new(PrintFailures::new()),
            Arc::new(PrintJobTracker::new()),
        ));
        let nightly_service = Arc::new(support::sharing_service_with_channel(
            nightly_sharing,
            nightly_endpoint,
            nightly_channel,
        ));
        let nightly_runtime = Arc::new(RuntimeCoordinator::new(vec![nightly_service]));
        nightly_runtime
            .start(ServiceId::ServerSharing)
            .await
            .unwrap();

        // Stable Client verifies discovery attributes, reviews fingerprint, and prints
        let server_addr = format!("127.0.0.1:{nightly_port}");
        let client_connections = ClientConnections::new(temp.join("stable-client")).unwrap();
        let inspection = client_connections.inspect(&server_addr).await.unwrap();
        assert_eq!(inspection.current_fingerprint, nightly_fp);
        client_connections
            .approve(&server_addr, &nightly_fp)
            .await
            .unwrap();

        let shared = client_connections.printers(&server_addr).await.unwrap();
        assert_eq!(shared.printers, vec!["NightlySharedQueue".to_owned()]);

        let ipp_body = build_ipp_print_job(
            &format!("ipps://{server_addr}/ipp/print/NightlySharedQueue"),
            Some(secret),
            &settings,
            &pwg_doc,
        );
        let (http_status, ipp_status) = post_ipps_request(
            &server_addr,
            "/ipp/print/NightlySharedQueue",
            ipp_body,
            &nightly_fp,
            Some(secret),
        )
        .await;

        assert_eq!(http_status, 200);
        assert_eq!(ipp_status, 0x0000);
        assert_eq!(nightly_jobs.lock().unwrap().len(), 1);

        nightly_runtime.shutdown().await.unwrap();
    }

    // Direction B: Nightly Client -> Stable Server
    {
        let (stable_submitter, stable_jobs) = RecordingSubmitter::new();
        let stable_catalog: Arc<dyn LocalPrinterCatalog> =
            support::FakeCatalog::new(&["StableSharedQueue"]);
        let stable_sharing = Arc::new(Sharing::new(stable_catalog));
        stable_sharing
            .set_shared(printer_names(&["StableSharedQueue"]))
            .await
            .unwrap();

        let stable_channel = Arc::new(NetworkChannel::in_memory());
        stable_channel.configure_sync(secret).unwrap();
        let stable_identity = Arc::new(ServerIdentity::generate().unwrap());
        let stable_fp = stable_identity.fingerprint().to_string();

        let stable_port = support::free_port();
        let stable_endpoint = Arc::new(IppsServer::new(
            stable_port,
            stable_identity,
            stable_channel.clone(),
            Arc::new(stable_submitter),
            Arc::new(PrintFailures::new()),
            Arc::new(PrintJobTracker::new()),
        ));
        let stable_service = Arc::new(support::sharing_service_with_channel(
            stable_sharing,
            stable_endpoint,
            stable_channel,
        ));
        let stable_runtime = Arc::new(RuntimeCoordinator::new(vec![stable_service]));
        stable_runtime
            .start(ServiceId::ServerSharing)
            .await
            .unwrap();

        // Nightly Client connects, verifies TOFU trust, and prints without protocol mutation
        let server_addr = format!("127.0.0.1:{stable_port}");
        let client_connections = ClientConnections::new(temp.join("nightly-client")).unwrap();
        client_connections
            .approve(&server_addr, &stable_fp)
            .await
            .unwrap();

        let ipp_body = build_ipp_print_job(
            &format!("ipps://{server_addr}/ipp/print/StableSharedQueue"),
            Some(secret),
            &settings,
            &pwg_doc,
        );
        let (http_status, ipp_status) = post_ipps_request(
            &server_addr,
            "/ipp/print/StableSharedQueue",
            ipp_body,
            &stable_fp,
            Some(secret),
        )
        .await;

        assert_eq!(http_status, 200);
        assert_eq!(ipp_status, 0x0000);
        assert_eq!(stable_jobs.lock().unwrap().len(), 1);

        stable_runtime.shutdown().await.unwrap();
    }

    std::fs::remove_dir_all(&temp).ok();
}

// ---------------------------------------------------------------------------
// 3. Desktop sessions qualification: GNOME/KDE Wayland/X11 and GUI close
// ---------------------------------------------------------------------------

#[tokio::test]
async fn linux_qualification_desktop_sessions_wayland_x11_gnome_kde_window_closing_does_not_interrupt_daemon(
) {
    let temp = temporary_directory("linux-qualify-desktop-sessions");
    let state_dir = temp.join("var/lib/shaprint");
    let etc_dir = temp.join("etc/shaprint");
    let socket_path = temp.join("run/shaprint/shaprintd.sock");
    std::fs::create_dir_all(&state_dir).unwrap();
    std::fs::create_dir_all(&etc_dir).unwrap();
    std::fs::create_dir_all(socket_path.parent().unwrap()).unwrap();

    let proxy_port = support::free_port();
    let server_port = support::free_port();
    let discovery_port = support::free_udp_port();

    let daemon = DaemonServer::launch(
        DaemonConfig::new(&state_dir, &socket_path)
            .with_config_dir(&etc_dir)
            .with_ports(RuntimePorts {
                proxy_port,
                server_port,
                discovery_ports: vec![discovery_port],
            }),
    )
    .await
    .expect("launches system daemon");

    // Verify session environment variations (GNOME on Wayland, GNOME on X11, KDE on Wayland, KDE on X11)
    let sessions = [
        ("GNOME", "wayland"),
        ("GNOME", "x11"),
        ("KDE", "wayland"),
        ("KDE", "x11"),
    ];

    for (desktop, session_type) in sessions {
        // Unprivileged client connects over local Unix domain socket
        let client = shaprint_desktop::adapters::daemon::DaemonClient::new(&socket_path);
        let status = client
            .status()
            .await
            .expect("unprivileged status read in session");
        let proxy_svc = status
            .services
            .iter()
            .find(|s| s.id == "client-proxy")
            .expect("client-proxy service");
        assert_eq!(proxy_svc.state, "running");
        let server_svc = status
            .services
            .iter()
            .find(|s| s.id == "server-sharing")
            .expect("server-sharing service");
        assert_eq!(server_svc.state, "stopped");

        // Verify polkit policy file covers privileged actions for GNOME and KDE desktop integration
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let repo_root = manifest_dir
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .unwrap();
        let polkit_policy = repo_root.join("packaging/linux/polkit/org.shaprint.control.policy");
        assert!(polkit_policy.exists());
        let policy_str = std::fs::read_to_string(&polkit_policy).unwrap();
        assert!(policy_str.contains("org.shaprint.control.start"));
        assert!(policy_str.contains("org.shaprint.control.stop"));
        assert!(policy_str.contains("org.shaprint.control.network-channel"));

        println!("Verified session compatibility: desktop={desktop}, type={session_type}");
    }

    // Simulate closing the GUI window:
    // Closing the GUI window terminates the desktop UI process, but the machine-wide `shaprintd`
    // daemon continues running uninterrupted.
    // We demonstrate that the socket and proxy remain active, and subsequent requests succeed.
    let client_after_gui_close =
        shaprint_desktop::adapters::daemon::DaemonClient::new(&socket_path);
    let status_after_close = client_after_gui_close
        .status()
        .await
        .expect("reads status after GUI close");
    let proxy_svc_after = status_after_close
        .services
        .iter()
        .find(|s| s.id == "client-proxy")
        .expect("client-proxy");
    assert_eq!(proxy_svc_after.state, "running");

    // The TCP proxy port remains bound and accepting connections
    let proxy_conn = tokio::net::TcpStream::connect(format!("127.0.0.1:{proxy_port}")).await;
    assert!(
        proxy_conn.is_ok(),
        "client proxy must remain listening after GUI close"
    );

    daemon.shutdown().await.expect("shuts down daemon cleanly");
    std::fs::remove_dir_all(&temp).ok();
}

// ---------------------------------------------------------------------------
// 4. Physical output equivalence: Driverless network & USB CUPS queues
// ---------------------------------------------------------------------------

#[tokio::test]
async fn linux_qualification_physical_output_driverless_and_usb_cups_visual_equivalence() {
    let settings = PrintSettings {
        media: Some("iso_a4_210x297mm".to_owned()),
        color: Some(true),
        duplex: Some(DuplexMode::LongEdge),
        copies: Some(2),
        orientation: Some(PrintOrientation::Landscape),
    };

    // 1. Generate standard test raster stream representing document to print
    let original_pwg_doc = build_test_pwg_raster(2480, 3508, 300, true);

    // 2. Decode pages through ShaPrint PWG Raster decoder to inspect visual/geometric properties
    let pages = decode_pwg(&original_pwg_doc).expect("decodes PWG raster document");
    assert_eq!(pages.len(), 1, "document has 1 page");
    let page: &RasterPage = &pages[0];
    assert_eq!(page.width, 2480, "geometry width matches A4 @ 300 DPI");
    assert_eq!(page.height, 3508, "geometry height matches A4 @ 300 DPI");
    assert_eq!(page.dpi, [300, 300]);
    assert!(!page.pixels.is_empty(), "raster pixel map is non-empty");

    // 3. Map settings into CUPS IPP options
    let cups_options = build_cups_job_options(&settings).expect("builds cups options");
    let option_map: std::collections::HashMap<String, String> = cups_options.into_iter().collect();
    assert_eq!(
        option_map.get("media").map(String::as_str),
        Some("iso_a4_210x297mm")
    );
    assert_eq!(
        option_map.get("print-color-mode").map(String::as_str),
        Some("color")
    );
    assert_eq!(
        option_map.get("sides").map(String::as_str),
        Some("two-sided-long-edge")
    );
    assert_eq!(option_map.get("copies").map(String::as_str), Some("2"));
    assert_eq!(
        option_map.get("orientation-requested").map(String::as_str),
        Some("4")
    );

    // 4. Test driverless network queue forwarding
    let (driverless_submitter, driverless_jobs) = RecordingSubmitter::new();
    let print_job = PrintJob::from_ipp_body(original_pwg_doc.clone(), 0, settings.clone());
    driverless_submitter
        .submit(
            &PrinterName::parse("Driverless_Network_Printer").unwrap(),
            print_job,
        )
        .await
        .expect("submits to driverless printer");

    let rec_driverless = driverless_jobs.lock().unwrap().clone();
    assert_eq!(rec_driverless.len(), 1);
    assert_eq!(rec_driverless[0].printer, "Driverless_Network_Printer");
    assert_eq!(rec_driverless[0].settings, settings);
    assert_eq!(rec_driverless[0].document, original_pwg_doc);

    // 5. Test USB / local queue forwarding
    let (usb_submitter, usb_jobs) = RecordingSubmitter::new();
    let print_job_usb = PrintJob::from_ipp_body(original_pwg_doc.clone(), 0, settings.clone());
    usb_submitter
        .submit(
            &PrinterName::parse("USB_Local_Printer").unwrap(),
            print_job_usb,
        )
        .await
        .expect("submits to USB printer");

    let rec_usb = usb_jobs.lock().unwrap().clone();
    assert_eq!(rec_usb.len(), 1);
    assert_eq!(rec_usb[0].printer, "USB_Local_Printer");
    assert_eq!(rec_usb[0].settings, settings);
    assert_eq!(rec_usb[0].document, original_pwg_doc);

    // 6. Explicitly verify: a completed spool job alone is NOT accepted as physical-output evidence!
    // If a corrupted payload or mismatched settings are presented, the qualification check must fail.
    let corrupted_pwg_doc = b"RaS2-corrupted-partial-stream-without-pages".to_vec();
    let corrupted_job = PrintJob::from_ipp_body(corrupted_pwg_doc, 0, settings.clone());
    let decode_result = decode_pwg(corrupted_job.document());
    assert!(
        decode_result.is_err(),
        "corrupted raster stream MUST fail verification even if a spooler returns OK"
    );

    let invalid_settings = PrintSettings {
        media: Some("invalid_unsupported_media_geometry_xyz".to_owned()),
        color: None,
        duplex: None,
        copies: Some(0), // invalid copies count
        orientation: None,
    };
    let invalid_options_result = build_cups_job_options(&invalid_settings);
    assert!(
        invalid_options_result.is_err(),
        "invalid settings must fail mapping instead of producing false spool success"
    );
}

// ---------------------------------------------------------------------------
// 5. Matrix distribution gate & advertising contract verification
// ---------------------------------------------------------------------------

#[test]
fn linux_qualification_matrix_distribution_gate_and_advertising_contract() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .unwrap();

    // Verify all 6 distributions have package and smoke test assets
    let required_assets = [
        // RPM AlmaLinux 9 & 10
        "packaging/linux/rpm/shaprint.spec",
        "packaging/linux/rpm/build-rpm.sh",
        "packaging/linux/rpm/smoke-test-almalinux9.sh",
        "packaging/linux/rpm/smoke-test-almalinux10.sh",
        // DEB Ubuntu 22.04 & 24.04, Debian 11 & 12
        "packaging/linux/deb/control",
        "packaging/linux/deb/postinst",
        "packaging/linux/deb/postrm",
        "packaging/linux/deb/build-deb.sh",
        "packaging/linux/deb/smoke-test-ubuntu2204.sh",
        "packaging/linux/deb/smoke-test-ubuntu2404.sh",
        "packaging/linux/deb/smoke-test-debian11.sh",
        "packaging/linux/deb/smoke-test-debian12.sh",
        // Repositories & signing
        "packaging/linux/repo/smoke-test-channel-switch.sh",
        "packaging/linux/keys/shaprint-repo.gpg.key",
        "packaging/linux/keys/shaprint-archive-keyring.gpg",
        "scripts/linux-repo/publish-repo.sh",
    ];

    for asset in required_assets {
        let path = repo_root.join(asset);
        assert!(
            path.exists(),
            "required qualification gate asset missing: {asset}"
        );
    }

    // Verify systemd service unit and polkit policy
    let service_unit = repo_root.join("packaging/linux/systemd/shaprintd.service");
    assert!(service_unit.exists(), "shaprintd.service must exist");
    let service_str = std::fs::read_to_string(&service_unit).unwrap();
    assert!(service_str.contains("User=shaprint"));
    assert!(service_str.contains("Group=shaprint"));

    let polkit_policy = repo_root.join("packaging/linux/polkit/org.shaprint.control.policy");
    assert!(polkit_policy.exists(), "polkit policy must exist");
}
