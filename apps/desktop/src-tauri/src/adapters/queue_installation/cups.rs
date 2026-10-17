//! Native CUPS Client queue installation on Linux (ADR 0018, issue #127).
//!
//! Installs a machine-wide CUPS Client queue whose `device-uri` targets the local Client proxy
//! (`ipp://127.0.0.1:<proxy_port>/ipp/print/<server>/<printer>`), uses the deterministic
//! CUPS-safe destination identifier (`ClientQueueRequest::cups_destination_name`), and sets the
//! human-readable description (`printer-info`) to `ClientQueueRequest::queue_name()`.
//!
//! Before modifying CUPS, inspects existing destinations through `libcups`:
//! - If a destination with the same name exists and is a recognised ShaPrint Client queue, it is
//!   updated idempotently (or recreated on repair).
//! - If a destination with the same name exists and is NOT a recognised ShaPrint Client queue,
//!   installation refuses with `SetupFailureKind::ExistingQueueConflict` and never overwrites it.

use std::path::Path;
use std::process::Command;

use crate::adapters::printers::cups::{
    query_cups_active_jobs, query_cups_destinations, query_cups_printer_ppd, CupsServerEndpoint,
};
use crate::application::QueueInstaller;
use crate::domain::{
    AppError, ClientQueueRequest, SetupFailure, SetupFailureKind, SpoolerRecord,
    SpoolerRecordClassification,
};

const LPADMIN_CANDIDATES: [&str; 3] = ["/usr/sbin/lpadmin", "/sbin/lpadmin", "lpadmin"];

fn locate_lpadmin() -> &'static str {
    for candidate in LPADMIN_CANDIDATES {
        if candidate.starts_with('/') && Path::new(candidate).exists() {
            return candidate;
        }
    }
    "lpadmin"
}

/// Installs and repairs machine-wide CUPS Client queues on Linux.
#[derive(Debug, Clone)]
pub struct CupsQueueInstaller {
    endpoint: Option<CupsServerEndpoint>,
    proxy_authority: String,
}

impl Default for CupsQueueInstaller {
    fn default() -> Self {
        Self::new()
    }
}

impl CupsQueueInstaller {
    pub fn new() -> Self {
        Self {
            endpoint: None,
            proxy_authority: super::proxy_authority(),
        }
    }

    pub fn with_proxy_authority(proxy_authority: impl Into<String>) -> Self {
        Self {
            endpoint: None,
            proxy_authority: proxy_authority.into(),
        }
    }

    pub fn with_endpoint_and_authority(
        endpoint: CupsServerEndpoint,
        proxy_authority: impl Into<String>,
    ) -> Self {
        Self {
            endpoint: Some(endpoint),
            proxy_authority: proxy_authority.into(),
        }
    }

    fn resolve_endpoint(&self) -> CupsServerEndpoint {
        CupsServerEndpoint::resolve(self.endpoint.as_ref())
    }

    fn apply_queue(
        &self,
        request: &ClientQueueRequest,
        recreate_existing: bool,
    ) -> Result<(), SetupFailure> {
        let uri = self.queue_uri(request).map_err(|error| {
            SetupFailure::new(SetupFailureKind::InvalidRequest, error.message())
        })?;
        let endpoint = self.resolve_endpoint();
        let destinations = query_cups_destinations(&endpoint).map_err(|error| {
            SetupFailure::new(SetupFailureKind::SpoolerUnavailable, error.message())
        })?;

        let cups_name = request.cups_destination_name();
        let human_description = request.queue_name().as_str();
        let mut preserved_options = Vec::new();
        let is_new_queue = !destinations.iter().any(|d| d.name == cups_name);

        if let Some(existing) = destinations.iter().find(|d| d.name == cups_name) {
            let record = SpoolerRecord::new(&existing.name, &existing.device_uri);
            match record.classify(&self.proxy_authority) {
                SpoolerRecordClassification::RecognisedClientQueue(q) => {
                    // Refuse conflicting destination
                    if q.server_address() != request.server_address()
                        || q.printer_name() != request.printer()
                    {
                        return Err(SetupFailure::new(
                            SetupFailureKind::ExistingQueueConflict,
                            format!(
                                "existing CUPS queue '{cups_name}' points to conflicting destination server='{}' printer='{}'",
                                q.server_address(),
                                q.printer_name()
                            ),
                        ));
                    }

                    // Leave healthy Client queues intact if recreate was not explicitly requested.
                    if existing.has_usable_conversion && !recreate_existing {
                        return Ok(());
                    }

                    // Existing queue is either raw (no usable conversion) or explicit repair requested.
                    // Check for active print jobs to ensure no pending jobs are discarded.
                    let active_jobs =
                        query_cups_active_jobs(&endpoint, &cups_name).map_err(|e| {
                            SetupFailure::new(SetupFailureKind::SpoolerUnavailable, e.message())
                        })?;
                    if active_jobs > 0 {
                        return Err(SetupFailure::new(
                            SetupFailureKind::QueueBusy,
                            format!(
                                "CUPS queue '{cups_name}' has {active_jobs} pending print jobs; cancel or wait for them to finish before repairing",
                            ),
                        ));
                    }

                    // Preserve user print defaults where valid.
                    preserved_options = existing.user_options();
                    if !has_user_pagesize_or_media(&preserved_options) {
                        if let Ok(old_ppd) = query_cups_printer_ppd(&endpoint, &cups_name) {
                            if let Some(valid_ps) =
                                extract_valid_default_pagesize_from_ppd(&old_ppd)
                            {
                                preserved_options.push(("PageSize".to_string(), valid_ps));
                            }
                        }
                    }
                }
                _ => {
                    return Err(SetupFailure::new(
                        SetupFailureKind::ExistingQueueConflict,
                        format!(
                            "existing CUPS queue '{cups_name}' points to '{}' and is not owned by ShaPrint",
                            existing.device_uri
                        ),
                    ));
                }
            }
        }

        self.configure_destination(&endpoint, &cups_name, &uri, human_description, is_new_queue)?;

        let has_user_media = has_user_pagesize_or_media(&preserved_options);
        if !has_user_media {
            self.auto_configure_default_pagesize(&endpoint, &cups_name);
        }

        self.apply_user_options(&endpoint, &cups_name, &preserved_options);
        Ok(())
    }

    fn auto_configure_default_pagesize(&self, endpoint: &CupsServerEndpoint, cups_name: &str) {
        let pagesize = query_cups_printer_ppd(endpoint, cups_name)
            .ok()
            .and_then(|ppd| resolve_default_pagesize_from_ppd(&ppd))
            .or_else(|| {
                let mut cmd = Command::new("lpoptions");
                let host_arg = endpoint.lpadmin_host_arg();
                if !host_arg.starts_with('/') {
                    cmd.arg("-h").arg(&host_arg);
                }
                cmd.arg("-p").arg(cups_name).arg("-l");
                let out = cmd.output().ok()?;
                if out.status.success() {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    resolve_default_pagesize_from_lpoptions(&stdout)
                } else {
                    None
                }
            });

        if let Some(ps) = pagesize {
            let mut cmd = self.lpadmin_command(endpoint);
            cmd.arg("-p")
                .arg(cups_name)
                .arg("-o")
                .arg(format!("PageSize={ps}"));
            let _ = cmd.output();
        }
    }

    fn lpadmin_command(&self, endpoint: &CupsServerEndpoint) -> Command {
        let mut cmd = Command::new(locate_lpadmin());
        let host_arg = endpoint.lpadmin_host_arg();
        if !host_arg.starts_with('/') {
            cmd.arg("-h").arg(&host_arg);
        }
        cmd
    }

    fn apply_user_options(
        &self,
        endpoint: &CupsServerEndpoint,
        cups_name: &str,
        options: &[(String, String)],
    ) {
        if options.is_empty() {
            return;
        }
        let mut cmd = self.lpadmin_command(endpoint);
        cmd.arg("-p").arg(cups_name);
        for (key, val) in options {
            cmd.arg("-o").arg(format!("{key}={val}"));
        }
        let _ = cmd.output();
    }

    fn delete_destination(
        &self,
        endpoint: &CupsServerEndpoint,
        cups_name: &str,
    ) -> Result<(), SetupFailure> {
        let mut cmd = self.lpadmin_command(endpoint);
        let output = cmd.arg("-x").arg(cups_name).output().map_err(|error| {
            SetupFailure::new(
                SetupFailureKind::Other,
                format!("cannot run lpadmin -x '{cups_name}': {error}"),
            )
        })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(classify_lpadmin_failure(stderr.trim()));
        }
        Ok(())
    }

    fn configure_destination(
        &self,
        endpoint: &CupsServerEndpoint,
        cups_name: &str,
        uri: &str,
        description: &str,
        is_new_queue: bool,
    ) -> Result<(), SetupFailure> {
        // Enforce driverless IPP Everywhere (`-m everywhere`). Never fall back to an unfiltered
        // raw queue if capability negotiation or PPD generation fails (issue #160).
        let everywhere = self.run_lpadmin(endpoint, cups_name, uri, description, true)?;
        if everywhere.status.success() {
            return Ok(());
        }
        // Only remove incomplete queue if this was a fresh installation.
        // Never delete or destroy a pre-existing queue on repair failure.
        if is_new_queue {
            let _ = self.delete_destination(endpoint, cups_name);
        }
        let stderr = String::from_utf8_lossy(&everywhere.stderr);
        Err(classify_lpadmin_failure(stderr.trim()))
    }

    fn run_lpadmin(
        &self,
        endpoint: &CupsServerEndpoint,
        cups_name: &str,
        uri: &str,
        description: &str,
        everywhere: bool,
    ) -> Result<std::process::Output, SetupFailure> {
        let mut cmd = self.lpadmin_command(endpoint);
        cmd.arg("-p")
            .arg(cups_name)
            .arg("-E")
            .arg("-v")
            .arg(uri)
            .arg("-D")
            .arg(description)
            .arg("-o")
            .arg("printer-is-shared=false");
        if everywhere {
            cmd.arg("-m").arg("everywhere");
        }
        cmd.output().map_err(|error| {
            SetupFailure::new(
                SetupFailureKind::Other,
                format!("cannot execute lpadmin: {error}"),
            )
        })
    }
}

fn classify_lpadmin_failure(stderr: &str) -> SetupFailure {
    let lower = stderr.to_ascii_lowercase();
    let kind = if lower.contains("forbidden")
        || lower.contains("not authorized")
        || lower.contains("unauthorized")
        || lower.contains("permission denied")
    {
        SetupFailureKind::PermissionDenied
    } else if lower.contains("unable to connect")
        || lower.contains("service unavailable")
        || lower.contains("bad file descriptor")
        || lower.contains("connection refused")
        || lower.contains("host is down")
    {
        SetupFailureKind::SpoolerUnavailable
    } else if lower.contains("unable to create ppd")
        || lower.contains("unable to copy ppd")
        || lower.contains("does not support required ipp attributes")
        || lower.contains("printer does not support")
        || lower.contains("not supported")
        || lower.contains("bad-request")
        || lower.contains("client-error")
        || lower.contains("server-error")
    {
        SetupFailureKind::QueueRejected
    } else {
        SetupFailureKind::Other
    };
    SetupFailure::new(kind, format!("lpadmin failed: {stderr}"))
}

fn has_user_pagesize_or_media(options: &[(String, String)]) -> bool {
    options
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("pagesize") || k.eq_ignore_ascii_case("media"))
}

/// Parses the declared `*DefaultPageSize:` and all available `*PageSize ` choices from PPD content.
pub fn parse_ppd_pagesizes(ppd_content: &str) -> (Option<String>, Vec<String>) {
    let mut declared_default = None;
    let mut available_sizes = Vec::new();

    for line in ppd_content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("*DefaultPageSize:") {
            let val = rest.trim();
            if !val.is_empty() && !val.eq_ignore_ascii_case("unknown") {
                declared_default = Some(val.to_owned());
            }
        } else if let Some(rest) = trimmed.strip_prefix("*PageSize ") {
            let rest = rest.trim_start();
            let name: String = rest
                .chars()
                .take_while(|c| *c != '/' && *c != ':' && !c.is_whitespace())
                .collect();
            if !name.is_empty() && !available_sizes.contains(&name) {
                available_sizes.push(name);
            }
        }
    }

    (declared_default, available_sizes)
}

/// Extracts the declared `*DefaultPageSize` from a PPD only if it exactly matches an available
/// `*PageSize` option in that PPD.
pub fn extract_valid_default_pagesize_from_ppd(ppd_content: &str) -> Option<String> {
    let (declared_default, available_sizes) = parse_ppd_pagesizes(ppd_content);
    let def = declared_default?;
    available_sizes
        .into_iter()
        .find(|s| s.eq_ignore_ascii_case(&def))
}

/// Identifies available `PageSize` options in the generated PPD and selects a valid default
/// page size (resolving `.Borderless` suffixes where needed under modern cups-filters 2.0).
pub fn resolve_default_pagesize_from_ppd(ppd_content: &str) -> Option<String> {
    let (declared_default, available_sizes) = parse_ppd_pagesizes(ppd_content);

    if available_sizes.is_empty() {
        return None;
    }

    // 1. If declared default exists:
    if let Some(ref def) = declared_default {
        // 1a. If declared default is directly an available choice:
        if let Some(matched) = available_sizes.iter().find(|s| s.eq_ignore_ascii_case(def)) {
            return Some(matched.clone());
        }
        // 1b. If declared default with `.Borderless` suffix is an available choice:
        let borderless = format!("{def}.Borderless");
        if let Some(matched) = available_sizes
            .iter()
            .find(|s| s.eq_ignore_ascii_case(&borderless))
        {
            return Some(matched.clone());
        }
        // 1c. If declared default ended with `.Borderless`, try stripping it:
        if let Some(stripped) = def.strip_suffix(".Borderless") {
            if let Some(matched) = available_sizes
                .iter()
                .find(|s| s.eq_ignore_ascii_case(stripped))
            {
                return Some(matched.clone());
            }
        }
    }

    // 2. Common default candidates in preference order:
    const FALLBACK_CANDIDATES: [&str; 4] = ["A4.Borderless", "A4", "Letter.Borderless", "Letter"];
    for candidate in FALLBACK_CANDIDATES {
        if let Some(matched) = available_sizes
            .iter()
            .find(|s| s.eq_ignore_ascii_case(candidate))
        {
            return Some(matched.clone());
        }
    }

    // 3. Fallback to the first available page size in the PPD:
    available_sizes.into_iter().next()
}

/// Parses the output of `lpoptions -l` to extract available page sizes and select a valid default.
pub fn resolve_default_pagesize_from_lpoptions(lpoptions_output: &str) -> Option<String> {
    for line in lpoptions_output.lines() {
        if line.starts_with("PageSize/") || line.starts_with("PageSize:") {
            let Some((_, choices)) = line.split_once(':') else {
                continue;
            };
            let mut available_sizes = Vec::new();
            let mut active_default = None;
            for choice in choices.split_whitespace() {
                if let Some(active) = choice.strip_prefix('*') {
                    if !active.is_empty() {
                        active_default = Some(active.to_owned());
                    }
                }
                let name = choice.trim_start_matches('*');
                if !name.is_empty() && !available_sizes.contains(&name.to_owned()) {
                    available_sizes.push(name.to_owned());
                }
            }
            if let Some(active) = active_default {
                return Some(active);
            }
            if available_sizes.is_empty() {
                return None;
            }
            const FALLBACK_CANDIDATES: [&str; 4] =
                ["A4.Borderless", "A4", "Letter.Borderless", "Letter"];
            for candidate in FALLBACK_CANDIDATES {
                if let Some(matched) = available_sizes
                    .iter()
                    .find(|s| s.eq_ignore_ascii_case(candidate))
                {
                    return Some(matched.clone());
                }
            }
            return available_sizes.into_iter().next();
        }
    }
    None
}

impl QueueInstaller for CupsQueueInstaller {
    fn queue_uri(&self, request: &ClientQueueRequest) -> Result<String, AppError> {
        super::queue_uri(&self.proxy_authority, request)
    }

    fn install(&self, request: &ClientQueueRequest) -> Result<(), SetupFailure> {
        self.apply_queue(request, false)
    }

    fn repair(&self, request: &ClientQueueRequest) -> Result<(), SetupFailure> {
        self.apply_queue(request, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::PrinterName;

    use std::process::{Child, Command};
    use std::time::{Duration, Instant};

    fn request(server: &str, printer: &str) -> ClientQueueRequest {
        ClientQueueRequest::new(
            server,
            PrinterName::parse(printer).expect("valid printer name"),
        )
        .expect("valid request")
    }

    struct IsolatedCupsd {
        dir: std::path::PathBuf,
        endpoint: CupsServerEndpoint,
        child: Child,
    }

    impl IsolatedCupsd {
        fn start() -> Self {
            let listener =
                std::net::TcpListener::bind("127.0.0.1:0").expect("binds ephemeral port");
            let port = listener.local_addr().expect("reads addr").port();
            drop(listener);

            let dir = crate::test_support::temporary_directory("cups-installer-test");
            for sub in [
                "conf",
                "spool",
                "spool/temp",
                "cache",
                "state",
                "state/certs",
                "logs",
                "ppd",
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
                    use std::os::unix::fs::PermissionsExt;
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

            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline {
                if sock.exists() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }

            Self {
                dir,
                endpoint: CupsServerEndpoint::parse(&format!("127.0.0.1:{port}")),
                child,
            }
        }
    }

    impl Drop for IsolatedCupsd {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    struct MockIppPrinter {
        port: u16,
        shutdown: std::sync::Arc<std::sync::atomic::AtomicBool>,
        handle: Option<std::thread::JoinHandle<()>>,
    }

    impl MockIppPrinter {
        fn start(server: &str, printer_name: &str) -> Self {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("binds mock ipp");
            let port = listener.local_addr().expect("reads addr").port();
            listener.set_nonblocking(true).expect("nonblocking");
            let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let shutdown_clone = std::sync::Arc::clone(&shutdown);

            let uri = format!(
                "ipp://127.0.0.1:{}/ipp/print/{}/{}",
                port,
                crate::adapters::ipps::protocol::percent_encode(server),
                crate::adapters::ipps::protocol::percent_encode(printer_name)
            );

            let resp_template = crate::adapters::ipps::protocol::response(
                1,
                crate::adapters::ipps::protocol::IPP_VERSION_2_0,
                crate::adapters::ipps::protocol::Status::Ok,
                &[crate::adapters::ipps::protocol::PrinterEntry {
                    name: printer_name.to_owned(),
                    uri,
                    accepting_jobs: true,
                }],
            );

            let handle = std::thread::spawn(move || {
                use std::io::{Read, Write};
                use std::sync::atomic::Ordering;
                while !shutdown_clone.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            let _ = stream.set_nonblocking(false);
                            let mut req_buf = Vec::new();
                            loop {
                                let mut buf = [0u8; 4096];
                                let n = match stream.read(&mut buf) {
                                    Ok(0) => break,
                                    Ok(n) => n,
                                    Err(_) => break,
                                };
                                req_buf.extend_from_slice(&buf[..n]);
                                if req_buf
                                    .windows(23)
                                    .any(|w| w.eq_ignore_ascii_case(b"expect: 100-continue"))
                                {
                                    let _ = stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n");
                                    let _ = stream.flush();
                                    if let Some(pos) = req_buf.windows(23).position(|w| {
                                        w.eq_ignore_ascii_case(b"expect: 100-continue")
                                    }) {
                                        req_buf[pos..pos + 23].fill(0);
                                    }
                                }
                                if let Some(pos) = req_buf.windows(4).position(|w| w == b"\r\n\r\n")
                                {
                                    let ipp = &req_buf[pos + 4..];
                                    if ipp.len() >= 8 {
                                        let mut resp_bytes = resp_template.clone();
                                        resp_bytes[4..8].copy_from_slice(&ipp[4..8]);
                                        let http_resp = format!(
                                            "HTTP/1.1 200 OK\r\nContent-Type: application/ipp\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                            resp_bytes.len()
                                        );
                                        let _ = stream.write_all(http_resp.as_bytes());
                                        let _ = stream.write_all(&resp_bytes);
                                        let _ = stream.flush();
                                        break;
                                    }
                                }
                            }
                        }
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        Err(_) => break,
                    }
                }
            });

            Self {
                port,
                shutdown,
                handle: Some(handle),
            }
        }
    }

    impl Drop for MockIppPrinter {
        fn drop(&mut self) {
            self.shutdown
                .store(true, std::sync::atomic::Ordering::Relaxed);
            if let Some(h) = self.handle.take() {
                let _ = h.join();
            }
        }
    }

    fn query_cups_active_pagesize(
        endpoint: &CupsServerEndpoint,
        cups_name: &str,
        home_dir: &Path,
    ) -> Option<String> {
        let mut cmd = Command::new("lpoptions");
        let host_arg = endpoint.lpadmin_host_arg();
        if !host_arg.starts_with('/') {
            cmd.arg("-h").arg(&host_arg);
        }
        cmd.env("HOME", home_dir);
        cmd.arg("-p").arg(cups_name).arg("-l");
        let out = cmd.output().ok()?;
        if !out.status.success() {
            return None;
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        for line in stdout.lines() {
            if line.starts_with("PageSize/") || line.starts_with("PageSize:") {
                let (_, choices) = line.split_once(':')?;
                for choice in choices.split_whitespace() {
                    if let Some(active) = choice.strip_prefix('*') {
                        return Some(active.to_owned());
                    }
                }
            }
        }
        None
    }

    #[test]
    fn new_queue_automatically_configures_valid_default_pagesize() {
        if !Path::new("/usr/sbin/cupsd").exists() || !Path::new("/usr/sbin/lpadmin").exists() {
            return;
        }

        let cupsd = IsolatedCupsd::start();
        let req = request("10.0.0.5:48631", "Office Printer");
        let mock = MockIppPrinter::start("10.0.0.5:48631", "Office Printer");
        let proxy_authority = format!("127.0.0.1:{}", mock.port);
        let installer = CupsQueueInstaller::with_endpoint_and_authority(
            cupsd.endpoint.clone(),
            proxy_authority,
        );

        installer.install(&req).expect("install succeeds");

        let active_pagesize =
            query_cups_active_pagesize(&cupsd.endpoint, &req.cups_destination_name(), &cupsd.dir)
                .expect("newly installed driverless queue must have an active default PageSize");
        assert!(
            active_pagesize == "A4.Borderless" || active_pagesize == "A4",
            "active default PageSize must be valid, got {active_pagesize}"
        );
    }

    #[test]
    fn repair_preserves_user_defined_pagesize_and_media() {
        if !Path::new("/usr/sbin/cupsd").exists() || !Path::new("/usr/sbin/lpadmin").exists() {
            return;
        }

        let cupsd = IsolatedCupsd::start();
        let req = request("10.0.0.5:48631", "Office Printer");
        let mock = MockIppPrinter::start("10.0.0.5:48631", "Office Printer");
        let proxy_authority = format!("127.0.0.1:{}", mock.port);
        let installer = CupsQueueInstaller::with_endpoint_and_authority(
            cupsd.endpoint.clone(),
            proxy_authority,
        );

        // 1. Install queue
        installer.install(&req).expect("install succeeds");
        let cups_name = req.cups_destination_name();

        // Verify initial default is A4.Borderless
        let init_pagesize = query_cups_active_pagesize(&cupsd.endpoint, &cups_name, &cupsd.dir)
            .expect("active PageSize");
        assert_eq!(init_pagesize, "A4.Borderless");

        // 2. User configures Letter.Borderless via lpadmin
        let _ = Command::new("/usr/sbin/lpadmin")
            .args([
                "-h",
                &cupsd.endpoint.lpadmin_host_arg(),
                "-p",
                &cups_name,
                "-o",
                "PageSize=Letter.Borderless",
            ])
            .status()
            .expect("sets PageSize=Letter.Borderless");

        let user_pagesize = query_cups_active_pagesize(&cupsd.endpoint, &cups_name, &cupsd.dir)
            .expect("active PageSize");
        assert_eq!(user_pagesize, "Letter.Borderless");

        // 3. Repair the queue
        installer.repair(&req).expect("repair succeeds");

        // 4. Assert user-defined PageSize was preserved intact
        let repaired_pagesize = query_cups_active_pagesize(&cupsd.endpoint, &cups_name, &cupsd.dir)
            .expect("active PageSize after repair");
        assert_eq!(
            repaired_pagesize, "Letter.Borderless",
            "repair must preserve user-defined PageSize"
        );
    }

    #[test]
    fn repair_unconfigured_broken_driverless_queue_configures_default_pagesize() {
        if !Path::new("/usr/sbin/cupsd").exists() || !Path::new("/usr/sbin/lpadmin").exists() {
            return;
        }

        let cupsd = IsolatedCupsd::start();
        let req = request("10.0.0.5:48631", "Office Printer");
        let mock = MockIppPrinter::start("10.0.0.5:48631", "Office Printer");
        let proxy_authority = format!("127.0.0.1:{}", mock.port);
        let installer = CupsQueueInstaller::with_endpoint_and_authority(
            cupsd.endpoint.clone(),
            proxy_authority,
        );

        let cups_name = req.cups_destination_name();
        let uri = format!(
            "ipp://127.0.0.1:{}/ipp/print/10.0.0.5%3A48631/Office%20Printer",
            mock.port
        );

        // Reproduce the pre-#165 unconfigured queue: create with lpadmin -m everywhere but no PageSize configured
        let status = Command::new("/usr/sbin/lpadmin")
            .args([
                "-h",
                &cupsd.endpoint.lpadmin_host_arg(),
                "-p",
                &cups_name,
                "-E",
                "-v",
                &uri,
                "-D",
                "Office Printer (ShaPrint 10.0.0.5-48631)",
                "-o",
                "printer-is-shared=false",
                "-m",
                "everywhere",
            ])
            .status()
            .expect("creates driverless queue without PageSize option");
        assert!(status.success());

        // Verify initial state has NO active default PageSize (pre-#165 bug)
        let init_active = query_cups_active_pagesize(&cupsd.endpoint, &cups_name, &cupsd.dir);
        assert_eq!(
            init_active, None,
            "pre-#165 unconfigured queue must have no active PageSize"
        );

        // Repair the queue
        installer.repair(&req).expect("repair succeeds");

        // Assert that the repaired queue now receives an active default PageSize
        let repaired_active = query_cups_active_pagesize(&cupsd.endpoint, &cups_name, &cupsd.dir)
            .expect("repaired queue must now have an active PageSize");
        assert!(
            repaired_active == "A4.Borderless" || repaired_active == "A4",
            "active default PageSize must be valid, got {repaired_active}"
        );
    }

    #[test]
    fn print_job_without_explicit_media_options_succeeds_through_filter() {
        if !Path::new("/usr/sbin/cupsd").exists()
            || !Path::new("/usr/sbin/lpadmin").exists()
            || !Path::new("/usr/sbin/cupsfilter").exists()
        {
            return;
        }

        let cupsd = IsolatedCupsd::start();
        let req = request("10.0.0.5:48631", "Office Printer");
        let mock = MockIppPrinter::start("10.0.0.5:48631", "Office Printer");
        let proxy_authority = format!("127.0.0.1:{}", mock.port);
        let installer = CupsQueueInstaller::with_endpoint_and_authority(
            cupsd.endpoint.clone(),
            proxy_authority,
        );

        installer.install(&req).expect("install succeeds");
        let cups_name = req.cups_destination_name();

        let ppd_path = cupsd.dir.join("conf/ppd").join(format!("{cups_name}.ppd"));
        let ppd_path_alt = cupsd.dir.join("ppd").join(format!("{cups_name}.ppd"));
        let final_ppd = if ppd_path.exists() {
            ppd_path
        } else {
            ppd_path_alt
        };
        assert!(final_ppd.exists(), "PPD must exist");

        // Create a minimal PDF test file
        let pdf_file = cupsd.dir.join("test_doc.pdf");
        std::fs::write(
            &pdf_file,
            b"%PDF-1.4\n1 0 obj<</Type/Catalog>>endobj\ntrailer<</Root 1 0 R>>\n%%EOF",
        )
        .expect("writes test pdf");

        // Run cupsfilter without explicit media options (-o media=... or -o PageSize=...)
        let mut cmd = Command::new("/usr/sbin/cupsfilter");
        cmd.env("HOME", &cupsd.dir);
        cmd.arg("-p")
            .arg(&final_ppd)
            .arg("-m")
            .arg("application/vnd.cups-raster")
            .arg(&pdf_file);

        let out = cmd.output().expect("runs cupsfilter");
        assert!(
            out.status.success(),
            "cupsfilter must succeed without explicit media options; stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn test_resolve_default_pagesize_from_ppd_borderless() {
        let ppd = r#"*OpenUI *PageSize: PickOne
*DefaultPageSize: A4
*PageSize 4x6.Borderless: "<</PageSize[288 432]>>setpagedevice"
*PageSize A4.Borderless: "<</PageSize[595 842]>>setpagedevice"
*PageSize Letter.Borderless: "<</PageSize[612 792]>>setpagedevice"
*CloseUI: *PageSize"#;

        assert_eq!(
            resolve_default_pagesize_from_ppd(ppd),
            Some("A4.Borderless".to_string())
        );
        assert_eq!(extract_valid_default_pagesize_from_ppd(ppd), None);

        let user_ppd = ppd.replace(
            "*DefaultPageSize: A4",
            "*DefaultPageSize: Letter.Borderless",
        );
        assert_eq!(
            extract_valid_default_pagesize_from_ppd(&user_ppd),
            Some("Letter.Borderless".to_string())
        );
        assert_eq!(
            resolve_default_pagesize_from_ppd(&user_ppd),
            Some("Letter.Borderless".to_string())
        );
    }

    #[test]
    fn test_resolve_default_pagesize_from_ppd_standard() {
        let ppd = r#"*OpenUI *PageSize: PickOne
*DefaultPageSize: A4
*PageSize A4/A4: "<</PageSize[595 842]>>setpagedevice"
*PageSize Letter/US Letter: "<</PageSize[612 792]>>setpagedevice"
*CloseUI: *PageSize"#;

        assert_eq!(
            resolve_default_pagesize_from_ppd(ppd),
            Some("A4".to_string())
        );
        assert_eq!(
            extract_valid_default_pagesize_from_ppd(ppd),
            Some("A4".to_string())
        );
    }

    #[test]
    fn test_resolve_default_pagesize_from_lpoptions() {
        let lpoptions_with_active = r#"Resolution/Output Resolution: *300dpi 600dpi
PageSize/Media Size: 4x6.Borderless *Letter.Borderless A4.Borderless
Duplex/2-Sided Printing: *None DuplexNoTumble DuplexTumble"#;

        assert_eq!(
            resolve_default_pagesize_from_lpoptions(lpoptions_with_active),
            Some("Letter.Borderless".to_string()),
            "must prioritize active choice marked with *"
        );

        let lpoptions_unselected = r#"Resolution/Output Resolution: *300dpi 600dpi
MalformedLineWithoutColon
PageSize/Media Size: 4x6.Borderless A4.Borderless Letter.Borderless
Duplex/2-Sided Printing: *None DuplexNoTumble DuplexTumble"#;

        assert_eq!(
            resolve_default_pagesize_from_lpoptions(lpoptions_unselected),
            Some("A4.Borderless".to_string()),
            "must fall back to candidate when no active choice is set and ignore malformed lines"
        );
    }

    #[test]
    fn cups_installer_creates_queue_is_idempotent_and_refuses_non_shaprint_conflict() {
        if !Path::new("/usr/sbin/cupsd").exists() || !Path::new("/usr/sbin/lpadmin").exists() {
            return;
        }

        let cupsd = IsolatedCupsd::start();
        let req = request("10.0.0.5:48631", "Office Printer");
        let mock = MockIppPrinter::start("10.0.0.5:48631", "Office Printer");
        let proxy_authority = format!("127.0.0.1:{}", mock.port);
        let installer = CupsQueueInstaller::with_endpoint_and_authority(
            cupsd.endpoint.clone(),
            proxy_authority,
        );

        // 1. First install creates the queue with deterministic CUPS-safe identifier and readable description.
        installer.install(&req).expect("first install succeeds");
        let dests = query_cups_destinations(&cupsd.endpoint).expect("queries cups destinations");
        assert_eq!(dests.len(), 1);
        assert_eq!(dests[0].name, req.cups_destination_name());
        assert_eq!(
            dests[0].device_uri,
            format!(
                "ipp://127.0.0.1:{}/ipp/print/10.0.0.5%3A48631/Office%20Printer",
                mock.port
            )
        );
        assert_eq!(
            dests[0].printer_info,
            "Office Printer (ShaPrint 10.0.0.5-48631)"
        );
        assert!(
            cupsd
                .dir
                .join("conf/ppd")
                .join(format!("{}.ppd", req.cups_destination_name()))
                .exists()
                || cupsd
                    .dir
                    .join("ppd")
                    .join(format!("{}.ppd", req.cups_destination_name()))
                    .exists(),
            "driverless PPD must be generated by lpadmin -m everywhere"
        );

        // 2. Reinstalling the same destination is idempotent and does not create duplicates.
        installer
            .install(&req)
            .expect("second install is idempotent");
        installer.repair(&req).expect("repair succeeds");
        let dests_after =
            query_cups_destinations(&cupsd.endpoint).expect("queries cups destinations");
        assert_eq!(dests_after.len(), 1);
        assert_eq!(dests_after[0].name, req.cups_destination_name());

        // 3. A conflicting non-ShaPrint queue with the same CUPS name is never overwritten.
        let conflict_req = request("10.0.0.9:48631", "Reception Printer");
        let conflict_name = conflict_req.cups_destination_name();
        let status = Command::new("/usr/sbin/lpadmin")
            .args([
                "-h",
                &cupsd.endpoint.lpadmin_host_arg(),
                "-p",
                &conflict_name,
                "-E",
                "-v",
                "file:/dev/null",
                "-D",
                "Unrelated Local Queue",
            ])
            .status()
            .expect("creates conflicting non-ShaPrint queue");
        assert!(status.success());

        let err = installer
            .install(&conflict_req)
            .expect_err("refuses to overwrite non-ShaPrint queue");
        assert_eq!(err.kind(), SetupFailureKind::ExistingQueueConflict);

        let verify_untouched =
            query_cups_destinations(&cupsd.endpoint).expect("queries cups destinations");
        let untouched = verify_untouched
            .iter()
            .find(|d| d.name == conflict_name)
            .expect("conflicting queue still exists");
        assert_eq!(untouched.device_uri, "file:/dev/null");
        assert_eq!(untouched.printer_info, "Unrelated Local Queue");

        // 4. Failed driverless setup reports classified failure and NEVER falls back to an unfiltered raw queue.
        let failing_req = request("10.0.0.5:48631", "Failing Queue");
        let failing_installer = CupsQueueInstaller::with_endpoint_and_authority(
            cupsd.endpoint.clone(),
            "127.0.0.1:49999",
        );
        let err = failing_installer
            .install(&failing_req)
            .expect_err("install must fail when driverless setup fails");
        assert!(matches!(
            err.kind(),
            SetupFailureKind::QueueRejected | SetupFailureKind::SpoolerUnavailable
        ));
        let dests_failed =
            query_cups_destinations(&cupsd.endpoint).expect("queries cups destinations");
        assert!(
            !dests_failed
                .iter()
                .any(|d| d.name == failing_req.cups_destination_name()),
            "failed queue installation must never leave an unfiltered raw queue in CUPS"
        );
    }

    #[test]
    fn test_mock_ipp_printer_with_driverless() {
        if !Path::new("/usr/bin/driverless").exists() {
            return;
        }
        let mock = MockIppPrinter::start("10.0.0.5:48631", "Office Printer");
        let uri = format!(
            "ipp://127.0.0.1:{}/ipp/print/10.0.0.5%3A48631/Office%20Printer",
            mock.port
        );
        let out = Command::new("driverless")
            .arg("-d")
            .arg(&uri)
            .output()
            .expect("runs driverless");
        assert!(out.status.success());
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("*cupsFilter2:"));
    }

    #[test]
    fn raw_client_queue_is_detected_and_repaired_to_driverless_preserving_user_defaults() {
        if !Path::new("/usr/sbin/cupsd").exists() || !Path::new("/usr/sbin/lpadmin").exists() {
            return;
        }

        let cupsd = IsolatedCupsd::start();
        let req = request("10.0.0.5:48631", "Office Printer");
        let mock = MockIppPrinter::start("10.0.0.5:48631", "Office Printer");
        let proxy_authority = format!("127.0.0.1:{}", mock.port);
        let installer = CupsQueueInstaller::with_endpoint_and_authority(
            cupsd.endpoint.clone(),
            proxy_authority,
        );

        let cups_name = req.cups_destination_name();
        let uri = format!(
            "ipp://127.0.0.1:{}/ipp/print/10.0.0.5%3A48631/Office%20Printer",
            mock.port
        );

        // 1. Reproduce the pre-#160 raw queue: create queue without -m everywhere
        let status = Command::new("/usr/sbin/lpadmin")
            .args([
                "-h",
                &cupsd.endpoint.lpadmin_host_arg(),
                "-p",
                &cups_name,
                "-E",
                "-v",
                &uri,
                "-D",
                "Office Printer (ShaPrint 10.0.0.5-48631)",
                "-o",
                "printer-is-shared=false",
                "-o",
                "print-color-mode=monochrome",
            ])
            .status()
            .expect("creates raw queue with user options");
        assert!(status.success());

        // Verify initial state is raw without PPD and with user options
        let dests_init =
            query_cups_destinations(&cupsd.endpoint).expect("queries cups destinations");
        assert_eq!(dests_init.len(), 1);
        let raw_dest = &dests_init[0];
        assert_eq!(raw_dest.name, cups_name);
        assert!(!raw_dest.has_usable_conversion);
        assert!(raw_dest.is_raw());
        let user_opts = raw_dest.user_options();
        assert!(user_opts
            .iter()
            .any(|(k, v)| k == "print-color-mode" && v == "monochrome"));

        let ppd_path = cupsd.dir.join("conf/ppd").join(format!("{cups_name}.ppd"));
        let ppd_path_alt = cupsd.dir.join("ppd").join(format!("{cups_name}.ppd"));
        assert!(
            !ppd_path.exists() && !ppd_path_alt.exists(),
            "raw queue must not have a PPD"
        );

        // 2. Normal installation flow detects the raw queue and idempotently repairs it
        installer
            .install(&req)
            .expect("install on raw queue must trigger automatic repair to driverless");

        let dests_repaired =
            query_cups_destinations(&cupsd.endpoint).expect("queries cups destinations");
        assert_eq!(dests_repaired.len(), 1);
        let repaired = &dests_repaired[0];
        assert_eq!(repaired.name, cups_name);
        assert_eq!(repaired.device_uri, uri);
        assert!(
            repaired.has_usable_conversion,
            "repaired queue must have usable conversion"
        );
        assert!(!repaired.is_raw());
        assert!(
            ppd_path.exists() || ppd_path_alt.exists(),
            "repaired queue must now have a generated driverless PPD"
        );

        // Verify user defaults were preserved
        let opts_after = repaired.user_options();
        assert!(
            opts_after
                .iter()
                .any(|(k, v)| k == "print-color-mode" && v == "monochrome"),
            "print-color-mode default must be preserved"
        );

        // 3. Subsequent install is idempotent and leaves the healthy queue intact
        installer
            .install(&req)
            .expect("subsequent install on healthy queue must succeed idempotently");

        // 4. Explicit repair also succeeds cleanly
        installer
            .repair(&req)
            .expect("explicit repair succeeds cleanly");
    }

    #[test]
    fn repair_defers_when_pending_print_jobs_exist() {
        if !Path::new("/usr/sbin/cupsd").exists() || !Path::new("/usr/sbin/lpadmin").exists() {
            return;
        }

        let cupsd = IsolatedCupsd::start();
        let req = request("10.0.0.5:48631", "Office Printer");
        let mock = MockIppPrinter::start("10.0.0.5:48631", "Office Printer");
        let proxy_authority = format!("127.0.0.1:{}", mock.port);
        let installer = CupsQueueInstaller::with_endpoint_and_authority(
            cupsd.endpoint.clone(),
            proxy_authority,
        );

        // Install the queue
        installer.install(&req).expect("first install succeeds");
        let cups_name = req.cups_destination_name();

        // Stop the printer queue so submitted jobs remain pending in spooler
        let stop_status = Command::new("/usr/sbin/cupsdisable")
            .args(["-h", &cupsd.endpoint.lpadmin_host_arg(), &cups_name])
            .status();
        let stopped = stop_status.map(|s| s.success()).unwrap_or(false);
        if !stopped {
            // Alternatively stop via lpadmin -p cups_name -u
            let _ = Command::new("/usr/sbin/lpadmin")
                .args([
                    "-h",
                    &cupsd.endpoint.lpadmin_host_arg(),
                    "-p",
                    &cups_name,
                    "-r",
                    "hold",
                ])
                .status();
        }

        // Submit a job using lp
        let mut lp = Command::new("lp");
        let host_arg = cupsd.endpoint.lpadmin_host_arg();
        if !host_arg.starts_with('/') {
            lp.arg("-h").arg(&host_arg);
        }
        lp.arg("-d").arg(&cups_name);
        lp.stdin(std::process::Stdio::piped());
        if let Ok(mut child) = lp.spawn() {
            use std::io::Write;
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(
                    b"%PDF-1.4\n1 0 obj<</Type/Catalog>>endobj\ntrailer<</Root 1 0 R>>\n%%EOF",
                );
            }
            let _ = child.wait();
        }

        let active = query_cups_active_jobs(&cupsd.endpoint, &cups_name).unwrap_or(0);
        if active > 0 {
            // Repair must fail with QueueBusy and NOT discard the job
            let err = installer
                .repair(&req)
                .expect_err("repair must fail when pending jobs exist");
            assert_eq!(err.kind(), SetupFailureKind::QueueBusy);

            // Job must still exist
            let active_after = query_cups_active_jobs(&cupsd.endpoint, &cups_name).unwrap_or(0);
            assert_eq!(active_after, active, "pending jobs must not be discarded");
        }
    }

    #[test]
    fn conflicting_destination_is_rejected_without_overwriting() {
        if !Path::new("/usr/sbin/cupsd").exists() || !Path::new("/usr/sbin/lpadmin").exists() {
            return;
        }

        let cupsd = IsolatedCupsd::start();
        let req1 = request("10.0.0.5:48631", "Office Printer");
        let mock = MockIppPrinter::start("10.0.0.5:48631", "Office Printer");
        let proxy_authority = format!("127.0.0.1:{}", mock.port);
        let installer = CupsQueueInstaller::with_endpoint_and_authority(
            cupsd.endpoint.clone(),
            proxy_authority.clone(),
        );

        // Install valid queue
        installer.install(&req1).expect("first install succeeds");

        // Request with different server but derived to conflicting queue
        let _req2 = request("10.0.0.99:48631", "Office Printer");
        let cups_name = req1.cups_destination_name();

        // Suppose someone manually pointed existing cups_name to another ShaPrint destination
        let other_uri =
            format!("ipp://{proxy_authority}/ipp/print/10.0.0.99%3A48631/Other%20Printer");
        let _ = Command::new("/usr/sbin/lpadmin")
            .args([
                "-h",
                &cupsd.endpoint.lpadmin_host_arg(),
                "-p",
                &cups_name,
                "-v",
                &other_uri,
            ])
            .status();

        let err = installer
            .install(&req1)
            .expect_err("conflicting destination must be rejected");
        assert_eq!(err.kind(), SetupFailureKind::ExistingQueueConflict);
    }
}
