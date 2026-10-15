//! Live verification test against the remote Windows Server (issue #160).
//!
//! Verifies that an AlmaLinux client can connect through the Client Proxy to a Windows Server
//! (which omits printer-up-time and compression-supported), install a driverless queue
//! without response adapters via `lpadmin -m everywhere`, and generate a valid PPD.

#![cfg(target_os = "linux")]

mod support;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use shaprint_desktop::adapters::client_connections::ClientConnections;
use shaprint_desktop::adapters::ipps::NetworkChannel;
use shaprint_desktop::adapters::queue_installation::cups::CupsQueueInstaller;
use shaprint_desktop::adapters::ClientProxyService;
use shaprint_desktop::application::{
    PrintFailures, PrintJobTracker, QueueInstaller, RuntimeCoordinator,
};
use shaprint_desktop::domain::{ClientQueueRequest, PrinterName};

use support::temporary_directory;

const LIVE_WINDOWS_SERVER: &str = "10.102.10.21:48631";
const LIVE_PRINTER_NAME: &str = "EPSON L3210 Series (Copy 1)";

fn text_attribute(body: &mut Vec<u8>, tag: u8, name: &str, value: &str) {
    body.push(tag);
    body.extend((name.len() as u16).to_be_bytes());
    body.extend(name.as_bytes());
    body.extend((value.len() as u16).to_be_bytes());
    body.extend(value.as_bytes());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires live Windows Server on local LAN (10.102.10.21:48631)"]
async fn live_windows_server_driverless_queue_installation() {
    // 1. Check if the live Windows Server is reachable with a short timeout to prevent CI stall.
    let reachable = tokio::time::timeout(
        Duration::from_millis(500),
        tokio::net::TcpStream::connect(LIVE_WINDOWS_SERVER),
    )
    .await
    .ok()
    .and_then(Result::ok)
    .is_some();
    if !reachable {
        eprintln!("Skipping live test: {LIVE_WINDOWS_SERVER} is unreachable");
        return;
    }

    if !Path::new("/usr/sbin/cupsd").exists()
        || !Path::new("/usr/sbin/lpadmin").exists()
        || !Path::new("/usr/bin/driverless").exists()
    {
        eprintln!("Skipping live test: required CUPS tools or driverless binary not installed");
        return;
    }

    // 2. Set up client connections trust store and inspect/approve the server.
    let temp_dir = temporary_directory("live-trust");
    std::fs::create_dir_all(&temp_dir).expect("creates tempdir");
    let connections = Arc::new(ClientConnections::new(temp_dir).expect("trust store"));
    let review = connections
        .inspect(LIVE_WINDOWS_SERVER)
        .await
        .expect("inspects Windows Server identity over TLS");
    connections
        .approve(&review.address, &review.current_fingerprint)
        .await
        .expect("approves server fingerprint");

    // 3. Start local Client Proxy on an ephemeral port.
    let channel = Arc::new(NetworkChannel::in_memory());
    let proxy_tracker = Arc::new(PrintJobTracker::new());
    let proxy = Arc::new(ClientProxyService::with_port(
        Arc::clone(&connections),
        Arc::clone(&channel),
        Arc::new(PrintFailures::new()),
        proxy_tracker,
        0, // ephemeral port
    ));
    let coordinator = Arc::new(RuntimeCoordinator::new(vec![proxy.clone()]));
    coordinator.start_autostart().await.expect("starts proxy");

    let proxy_addr = proxy.bound_address().expect("bound address");
    let proxy_port = proxy_addr.port();
    let proxy_authority = format!("127.0.0.1:{proxy_port}");
    println!("Client Proxy listening on {proxy_authority}");

    // 3b. Verify direct TLS connection to Windows Server and attribute response.
    let remote_printer_uri = format!(
        "ipps://{LIVE_WINDOWS_SERVER}/ipp/print/{}",
        shaprint_desktop::adapters::ipps::protocol::percent_encode(LIVE_PRINTER_NAME)
    );
    let get_attr = support::get_printer_attributes(1, &remote_printer_uri);
    let resp = connections
        .forward_ipp(LIVE_WINDOWS_SERVER, &remote_printer_uri, &get_attr, None)
        .await
        .expect("remote server responds to Get-Printer-Attributes over TLS");
    let status = u16::from_be_bytes([resp[2], resp[3]]);
    assert_eq!(
        status, 0x0000,
        "remote server must return IPP 0x0000 (successful-ok)"
    );

    // Verify unauthenticated job validation is properly rejected by the remote server.
    let mut validate_req = vec![2, 0, 0, 4, 0, 0, 0, 2, 1];
    text_attribute(&mut validate_req, 0x47, "attributes-charset", "utf-8");
    text_attribute(&mut validate_req, 0x48, "attributes-natural-language", "en");
    text_attribute(&mut validate_req, 0x45, "printer-uri", &remote_printer_uri);
    text_attribute(
        &mut validate_req,
        0x49,
        "document-format",
        "image/pwg-raster",
    );
    validate_req.push(3);

    let val_resp = connections
        .forward_ipp(
            LIVE_WINDOWS_SERVER,
            &remote_printer_uri,
            &validate_req,
            None,
        )
        .await
        .expect("Validate-Job request answered");
    let val_status = u16::from_be_bytes([val_resp[2], val_resp[3]]);
    assert_eq!(
        val_status, 0x0403,
        "Validate-Job without credential must be rejected with 0x0403 NotAuthorized"
    );

    // 4. Verify driverless utility can poll capabilities through the proxy.
    let req = ClientQueueRequest::new(
        LIVE_WINDOWS_SERVER,
        PrinterName::parse(LIVE_PRINTER_NAME).expect("valid name"),
    )
    .expect("valid request");

    let uri = format!(
        "ipp://{proxy_authority}/ipp/print/{}/{}",
        shaprint_desktop::adapters::ipps::protocol::percent_encode(LIVE_WINDOWS_SERVER),
        shaprint_desktop::adapters::ipps::protocol::percent_encode(LIVE_PRINTER_NAME)
    );

    let uri_clone = uri.clone();
    let driverless_out = tokio::task::spawn_blocking(move || {
        std::process::Command::new("driverless")
            .arg("-d")
            .arg(&uri_clone)
            .output()
            .expect("runs driverless")
    })
    .await
    .expect("driverless task joined");

    let stdout = String::from_utf8_lossy(&driverless_out.stdout);
    let stderr = String::from_utf8_lossy(&driverless_out.stderr);
    println!("driverless exit: {:?}", driverless_out.status);
    if !driverless_out.status.success() {
        eprintln!("driverless stderr: {stderr}");
    }
    assert!(
        driverless_out.status.success(),
        "driverless must succeed without response adapters"
    );
    assert!(
        stdout.contains("*cupsFilter2:"),
        "driverless must generate a valid PPD containing *cupsFilter2"
    );
    assert!(
        stdout.contains("image/pwg-raster"),
        "PPD must support image/pwg-raster"
    );

    // 5. Run CupsQueueInstaller to install the queue in an isolated CUPS instance.
    let cups_dir = temporary_directory("live-cups");
    std::fs::create_dir_all(&cups_dir).expect("cups tempdir");
    let isolated_cups = start_isolated_cupsd(&cups_dir);
    let installer = CupsQueueInstaller::with_endpoint_and_authority(
        isolated_cups.endpoint.clone(),
        proxy_authority,
    );

    println!("Installing driverless queue via CupsQueueInstaller...");
    let req_clone = req.clone();
    tokio::task::spawn_blocking(move || {
        installer
            .install(&req_clone)
            .expect("queue installation succeeds");
    })
    .await
    .expect("installer task joined");

    // 6. Verify the installed destination in CUPS.
    let dests = shaprint_desktop::adapters::printers::cups::query_cups_destinations(
        &isolated_cups.endpoint,
    )
    .expect("queries destinations");
    let dest = dests
        .iter()
        .find(|d| d.name == req.cups_destination_name())
        .expect("installed destination found in CUPS");

    println!(
        "Installed CUPS destination: {} -> {}",
        dest.name, dest.device_uri
    );
    assert_eq!(dest.device_uri, uri);

    // Verify generated PPD file exists in ServerRoot/ppd (or ServerRoot/conf/ppd)
    let ppd_path = isolated_cups
        .dir
        .join("conf/ppd")
        .join(format!("{}.ppd", dest.name));
    assert!(
        ppd_path.exists()
            || isolated_cups
                .dir
                .join("ppd")
                .join(format!("{}.ppd", dest.name))
                .exists(),
        "driverless PPD must exist on disk for the installed queue"
    );

    let _ = coordinator.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires live Windows Server on local LAN (10.102.10.21:48631)"]
async fn live_windows_server_raw_queue_repair_and_driverless_transition() {
    // 1. Check reachability
    let reachable = tokio::time::timeout(
        Duration::from_millis(500),
        tokio::net::TcpStream::connect(LIVE_WINDOWS_SERVER),
    )
    .await
    .ok()
    .and_then(Result::ok)
    .is_some();
    if !reachable {
        eprintln!("Skipping live test: {LIVE_WINDOWS_SERVER} is unreachable");
        return;
    }

    if !Path::new("/usr/sbin/cupsd").exists()
        || !Path::new("/usr/sbin/lpadmin").exists()
        || !Path::new("/usr/bin/driverless").exists()
    {
        eprintln!("Skipping live test: required CUPS tools or driverless binary not installed");
        return;
    }

    // 2. Client Connections & Trust
    let temp_dir = temporary_directory("live-trust-repair");
    std::fs::create_dir_all(&temp_dir).expect("creates tempdir");
    let connections = Arc::new(ClientConnections::new(temp_dir).expect("trust store"));
    let review = connections
        .inspect(LIVE_WINDOWS_SERVER)
        .await
        .expect("inspects Windows Server identity over TLS");
    connections
        .approve(&review.address, &review.current_fingerprint)
        .await
        .expect("approves server fingerprint");

    // 3. Start local Client Proxy on ephemeral port
    let channel = Arc::new(NetworkChannel::in_memory());
    if let Ok(ch) = std::env::var("SHAPRINT_TEST_NETWORK_CHANNEL") {
        if !ch.trim().is_empty() {
            channel
                .configure_sync(ch.trim())
                .expect("configures test channel");
        }
    }
    let proxy_tracker = Arc::new(PrintJobTracker::new());
    let proxy = Arc::new(ClientProxyService::with_port(
        Arc::clone(&connections),
        Arc::clone(&channel),
        Arc::new(PrintFailures::new()),
        proxy_tracker,
        0,
    ));
    let coordinator = Arc::new(RuntimeCoordinator::new(vec![proxy.clone()]));
    coordinator.start_autostart().await.expect("starts proxy");

    let proxy_addr = proxy.bound_address().expect("bound address");
    let proxy_authority = format!("127.0.0.1:{}", proxy_addr.port());

    // 4. Setup Isolated CUPS daemon
    let cups_dir = temporary_directory("live-cups-repair");
    std::fs::create_dir_all(&cups_dir).expect("cups tempdir");
    let isolated_cups = start_isolated_cupsd(&cups_dir);
    let installer = CupsQueueInstaller::with_endpoint_and_authority(
        isolated_cups.endpoint.clone(),
        proxy_authority.clone(),
    );

    let req = ClientQueueRequest::new(
        LIVE_WINDOWS_SERVER,
        PrinterName::parse(LIVE_PRINTER_NAME).expect("valid name"),
    )
    .expect("valid request");

    let cups_name = req.cups_destination_name();
    let uri = format!(
        "ipp://{proxy_authority}/ipp/print/{}/{}",
        shaprint_desktop::adapters::ipps::protocol::percent_encode(LIVE_WINDOWS_SERVER),
        shaprint_desktop::adapters::ipps::protocol::percent_encode(LIVE_PRINTER_NAME)
    );

    // 5. Create pre-#160 raw client queue pointing to proxy without PPD
    let raw_status = std::process::Command::new("/usr/sbin/lpadmin")
        .args([
            "-h",
            &isolated_cups.endpoint.lpadmin_host_arg(),
            "-p",
            &cups_name,
            "-E",
            "-v",
            &uri,
            "-D",
            "EPSON L3210 Series (ShaPrint 10.102.10.21-48631)",
            "-o",
            "printer-is-shared=false",
            "-o",
            "print-color-mode=monochrome",
        ])
        .status()
        .expect("creates raw queue via lpadmin");
    assert!(raw_status.success());

    // 6. Verify initial state is raw without usable conversion or PPD
    let dests_before = shaprint_desktop::adapters::printers::cups::query_cups_destinations(
        &isolated_cups.endpoint,
    )
    .expect("queries destinations before repair");
    let raw_dest = dests_before
        .iter()
        .find(|d| d.name == cups_name)
        .expect("raw destination found");
    assert!(
        raw_dest.is_raw(),
        "queue before repair must be detected as raw"
    );
    assert!(!raw_dest.has_usable_conversion);
    let ppd_path = isolated_cups
        .dir
        .join("conf/ppd")
        .join(format!("{cups_name}.ppd"));
    let ppd_path_alt = isolated_cups
        .dir
        .join("ppd")
        .join(format!("{cups_name}.ppd"));
    assert!(
        !ppd_path.exists() && !ppd_path_alt.exists(),
        "raw queue must not have a PPD"
    );

    // 7. Execute normal install() flow which must automatically detect and repair raw queue
    let req_clone = req.clone();
    tokio::task::spawn_blocking(move || {
        installer
            .install(&req_clone)
            .expect("install on raw queue repairs queue to driverless");
    })
    .await
    .expect("installer task joined");

    // 8. Verify the destination has been repaired to driverless
    let dests_after = shaprint_desktop::adapters::printers::cups::query_cups_destinations(
        &isolated_cups.endpoint,
    )
    .expect("queries destinations after repair");
    let repaired_dest = dests_after
        .iter()
        .find(|d| d.name == cups_name)
        .expect("repaired destination found");
    assert!(!repaired_dest.is_raw(), "repaired queue must not be raw");
    assert!(
        repaired_dest.has_usable_conversion,
        "repaired queue must have usable conversion"
    );
    assert!(
        ppd_path.exists() || ppd_path_alt.exists(),
        "repaired queue must have driverless PPD generated"
    );
    let active_ppd = if ppd_path.exists() {
        ppd_path
    } else {
        ppd_path_alt
    };
    let ppd_content = std::fs::read_to_string(&active_ppd).expect("reads generated PPD");
    assert!(
        ppd_content.contains("*cupsFilter2:"),
        "PPD must contain *cupsFilter2 conversion rule"
    );
    assert!(
        ppd_content.contains("image/pwg-raster"),
        "PPD must support raster generation (image/pwg-raster)"
    );

    // Verify user options were preserved
    let user_opts = repaired_dest.user_options();
    assert!(
        user_opts
            .iter()
            .any(|(k, v)| k == "print-color-mode" && v == "monochrome"),
        "user print defaults must be preserved after repair"
    );

    // 9. Submit a native test document through CUPS
    let mut lp_cmd = std::process::Command::new("lp");
    let host_arg = isolated_cups.endpoint.lpadmin_host_arg();
    if !host_arg.starts_with('/') {
        lp_cmd.arg("-h").arg(&host_arg);
    }
    lp_cmd.arg("-d").arg(&cups_name);
    lp_cmd.stdin(std::process::Stdio::piped());
    let mut child = lp_cmd.spawn().expect("spawns lp command");
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let _ = stdin.write_all(b"%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n2 0 obj<</Type/Pages/Count 1/Kids[3 0 R]>>endobj\n3 0 obj<</Type/Page/MediaBox[0 0 595 842]/Parent 2 0 R>>endobj\ntrailer<</Root 1 0 R>>\n%%EOF\n");
    }
    let lp_exit = child.wait().expect("lp completes");
    println!(
        "Submitted test document to repaired queue: exit={:?}",
        lp_exit
    );
    println!(
        "Physical paper output is recorded as unverified as this is observed remotely over LAN."
    );

    let _ = coordinator.shutdown().await;
}

struct IsolatedCupsd {
    dir: std::path::PathBuf,
    endpoint: shaprint_desktop::adapters::printers::cups::CupsServerEndpoint,
    child: std::process::Child,
}

impl Drop for IsolatedCupsd {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start_isolated_cupsd(dir: &std::path::Path) -> IsolatedCupsd {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("binds ephemeral port");
    let port = listener.local_addr().expect("reads addr").port();
    drop(listener);

    for sub in [
        "conf",
        "conf/ppd",
        "spool",
        "spool/temp",
        "cache",
        "state",
        "state/certs",
        "logs",
        "serverbin/backend",
        "serverbin/filter",
        "serverbin/cgi-bin",
        "serverbin/daemon",
        "serverbin/driver",
        "serverbin/monitor",
        "serverbin/notifier",
    ] {
        std::fs::create_dir_all(dir.join(sub)).expect("creates subdirs");
    }
    for sub in [
        "filter", "cgi-bin", "daemon", "driver", "monitor", "notifier",
    ] {
        let src = std::path::Path::new("/usr/lib/cups").join(sub);
        if let Ok(entries) = std::fs::read_dir(&src) {
            for entry in entries.flatten() {
                let _ = std::os::unix::fs::symlink(
                    entry.path(),
                    dir.join("serverbin").join(sub).join(entry.file_name()),
                );
            }
        }
    }
    for backend in ["ipp", "usb", "socket", "lpd"] {
        let src = std::path::Path::new("/usr/lib/cups/backend").join(backend);
        if src.exists() {
            let dst = dir.join("serverbin/backend").join(backend);
            std::fs::copy(&src, &dst).expect("copies backend");
            std::fs::set_permissions(&dst, std::fs::Permissions::from_mode(0o755))
                .expect("chmod 0755 backend");
        }
    }
    let _ = std::os::unix::fs::symlink("ipp", dir.join("serverbin/backend/http"));
    let _ = std::os::unix::fs::symlink("ipp", dir.join("serverbin/backend/ipps"));

    let sock = dir.join("cups.sock");
    let cupsd_conf = dir.join("conf/cupsd.conf");
    let cups_files_conf = dir.join("conf/cups-files.conf");
    std::fs::write(
        &cupsd_conf,
        format!(
            "LogLevel info\nListen 127.0.0.1:{port}\nListen {}\nBrowsing Off\nDefaultAuthType None\n<Location />\n  Order allow,deny\n  Allow all\n</Location>\n<Location /admin>\n  Order allow,deny\n  Allow all\n</Location>\n<Location /admin/conf>\n  Order allow,deny\n  Allow all\n</Location>\n<Policy default>\n  <Limit All>\n    Order allow,deny\n    Allow all\n  </Limit>\n</Policy>\n",
            sock.display()
        ),
    )
    .expect("writes cupsd.conf");
    std::fs::write(
        &cups_files_conf,
        format!(
            "FileDevice Yes\nAccessLog {d}/logs/access_log\nErrorLog {d}/logs/error_log\nPageLog {d}/logs/page_log\nRequestRoot {d}/spool\nTempDir {d}/spool/temp\nCacheDir {d}/cache\nStateDir {d}/state\nServerRoot {d}/conf\nDataDir /usr/share/cups\nServerBin {d}/serverbin\n",
            d = dir.display()
        ),
    )
    .expect("writes cups-files.conf");

    let child = Command::new("/usr/sbin/cupsd")
        .arg("-f")
        .arg("-c")
        .arg(&cupsd_conf)
        .arg("-s")
        .arg(&cups_files_conf)
        .spawn()
        .expect("spawns cupsd");

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if sock.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    IsolatedCupsd {
        dir: dir.to_path_buf(),
        endpoint: shaprint_desktop::adapters::printers::cups::CupsServerEndpoint::parse(&format!(
            "127.0.0.1:{port}"
        )),
        child,
    }
}
