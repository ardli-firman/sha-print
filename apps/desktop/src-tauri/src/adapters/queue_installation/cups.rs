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

use crate::adapters::printers::cups::{query_cups_destinations, CupsServerEndpoint};
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

        if let Some(existing) = destinations.iter().find(|d| d.name == cups_name) {
            let record = SpoolerRecord::new(&existing.name, &existing.device_uri);
            match record.classify(&self.proxy_authority) {
                SpoolerRecordClassification::RecognisedClientQueue(_) => {
                    if recreate_existing {
                        self.delete_destination(&endpoint, &cups_name)?;
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

        self.configure_destination(&endpoint, &cups_name, &uri, human_description)
    }

    fn delete_destination(
        &self,
        endpoint: &CupsServerEndpoint,
        cups_name: &str,
    ) -> Result<(), SetupFailure> {
        let mut cmd = Command::new(locate_lpadmin());
        let host_arg = endpoint.lpadmin_host_arg();
        if !host_arg.starts_with('/') {
            cmd.arg("-h").arg(&host_arg);
        }
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
    ) -> Result<(), SetupFailure> {
        // First try IPP Everywhere (`-m everywhere`) so CUPS queries the running local proxy for
        // printer capabilities and media sizes; fall back to raw/generic queue creation if the
        // local CUPS build does not have the everywhere generator helper available.
        let everywhere = self.run_lpadmin(endpoint, cups_name, uri, description, true)?;
        if everywhere.status.success() {
            return Ok(());
        }
        let fallback = self.run_lpadmin(endpoint, cups_name, uri, description, false)?;
        if fallback.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&fallback.stderr);
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
        let mut cmd = Command::new(locate_lpadmin());
        let host_arg = endpoint.lpadmin_host_arg();
        if !host_arg.starts_with('/') {
            cmd.arg("-h").arg(&host_arg);
        }
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
    {
        SetupFailureKind::SpoolerUnavailable
    } else {
        SetupFailureKind::Other
    };
    SetupFailure::new(kind, format!("lpadmin failed: {stderr}"))
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

    fn request(server: &str, printer: &str) -> ClientQueueRequest {
        ClientQueueRequest::new(
            server,
            PrinterName::parse(printer).expect("valid printer name"),
        )
        .expect("valid request")
    }

    #[test]
    fn cups_installer_creates_queue_is_idempotent_and_refuses_non_shaprint_conflict() {
        use std::process::{Child, Command};
        use std::time::{Duration, Instant};

        if !Path::new("/usr/sbin/cupsd").exists() || !Path::new("/usr/sbin/lpadmin").exists() {
            return;
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

        let cupsd = IsolatedCupsd::start();
        let installer = CupsQueueInstaller::with_endpoint_and_authority(
            cupsd.endpoint.clone(),
            "127.0.0.1:48632",
        );
        let req = request("10.0.0.5:48631", "Office Printer");

        // 1. First install creates the queue with deterministic CUPS-safe identifier and readable description.
        installer.install(&req).expect("first install succeeds");
        let dests = query_cups_destinations(&cupsd.endpoint).expect("queries cups destinations");
        assert_eq!(dests.len(), 1);
        assert_eq!(dests[0].name, req.cups_destination_name());
        assert_eq!(
            dests[0].device_uri,
            "ipp://127.0.0.1:48632/ipp/print/10.0.0.5%3A48631/Office%20Printer"
        );
        assert_eq!(
            dests[0].printer_info,
            "Office Printer (ShaPrint 10.0.0.5-48631)"
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
    }
}
