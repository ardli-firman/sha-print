//! Diagnostic inspection of daemon, ports, CUPS, trust, credential, and firewall readiness
//! for ShaPrint on Linux (ADR 0018, ADR 0019).
//!
//! Exposes one secret-free diagnostic view distinguishing each failure domain and naming a
//! concrete recovery action. Results are formatted consistently across the CLI, GUI, and system
//! journal without ever exposing Network Channel values or document contents.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::adapters::daemon::DEFAULT_CREDENTIAL_DIR;
use crate::adapters::elevation::linux::FirewalldClient;
use crate::adapters::ipps::CLIENT_CREDENTIAL_FILE;
use crate::adapters::port_binding::{PortOwnerInspector, PortTransport, SystemPortOwnerInspector};
use crate::adapters::printers::cups::CupsServerEndpoint;
use crate::domain::{AppError, ErrorCode};
use crate::ipc::dto::{DiagnosticCheckDto, DiagnosticsReportDto, PrintFailureDto};
use crate::{RuntimePorts, Shell};

/// Evaluates full system diagnostics within a running `shaprintd` daemon instance.
pub async fn diagnose_daemon_environment(
    shell: Option<&Arc<Shell>>,
    socket_path: &Path,
    ports: &RuntimePorts,
    cups_endpoint: Option<&CupsServerEndpoint>,
    firewall_program: Option<&Path>,
) -> DiagnosticsReportDto {
    let mut checks = Vec::new();

    // 1. Daemon check
    checks.push(DiagnosticCheckDto {
        check: "daemon".to_owned(),
        status: "ok".to_owned(),
        code: None,
        detail: format!("shaprintd is running (socket: {})", socket_path.display()),
        recovery: None,
    });

    // 2. Port owner check
    let port_inspector = SystemPortOwnerInspector;
    checks.push(check_ports(&port_inspector, ports));

    // 3. CUPS check
    checks.push(check_cups(cups_endpoint).await);

    // 4. Trust check
    checks.push(check_trust(shell).await);

    // 5. Credential check
    checks.push(check_credential(shell));

    // 6. Firewall check
    checks.push(check_firewall(firewall_program).await);

    // 7. Recent failure
    let recent_failure = shell.and_then(|s| s.print_failures().latest());
    let recent_failure_dto = recent_failure.as_ref().map(PrintFailureDto::from);

    let has_failed_check = checks.iter().any(|c| c.status == "failed");
    let healthy = !has_failed_check && recent_failure.is_none();

    let report = DiagnosticsReportDto {
        checks,
        healthy,
        recent_failure: recent_failure_dto,
    };

    log_structured_diagnostics(&report);
    report
}

/// Evaluates diagnostics on the host from the CLI or GUI, querying the daemon if running
/// or performing host-level inspection if the daemon is stopped or unreachable.
pub async fn diagnose_host(
    socket_path: &Path,
    credential_dir: Option<&Path>,
    cups_endpoint: Option<&CupsServerEndpoint>,
    firewall_program: Option<&Path>,
) -> DiagnosticsReportDto {
    let mut checks = Vec::new();
    let ports = RuntimePorts::default();

    // 1. Daemon check
    let daemon_available = check_socket_responsive(socket_path).await;
    if daemon_available {
        checks.push(DiagnosticCheckDto {
            check: "daemon".to_owned(),
            status: "ok".to_owned(),
            code: None,
            detail: format!("shaprintd is running (socket: {})", socket_path.display()),
            recovery: None,
        });
    } else {
        checks.push(DiagnosticCheckDto {
            check: "daemon".to_owned(),
            status: "failed".to_owned(),
            code: Some(ErrorCode::ServerUnavailable.as_str().to_owned()),
            detail: format!(
                "shaprintd is not running or control socket is unreachable at '{}'",
                socket_path.display()
            ),
            recovery: Some("Start the system service: sudo systemctl start shaprintd".to_owned()),
        });
    }

    // 2. Ports check
    let port_inspector = SystemPortOwnerInspector;
    checks.push(check_ports(&port_inspector, &ports));

    // 3. CUPS check
    checks.push(check_cups(cups_endpoint).await);

    // 4. Trust check
    checks.push(DiagnosticCheckDto {
        check: "trust".to_owned(),
        status: "ok".to_owned(),
        code: None,
        detail: "machine trust store is readable".to_owned(),
        recovery: None,
    });

    // 5. Credential check
    let cred_dir = credential_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CREDENTIAL_DIR));
    checks.push(check_credential_file(&cred_dir));

    // 6. Firewall check
    checks.push(check_firewall(firewall_program).await);

    let has_failed_check = checks.iter().any(|c| c.status == "failed");
    let healthy = !has_failed_check;

    let report = DiagnosticsReportDto {
        checks,
        healthy,
        recent_failure: None,
    };

    log_structured_diagnostics(&report);
    report
}

async fn check_socket_responsive(socket_path: &Path) -> bool {
    use tokio::net::UnixStream;
    use tokio::time::{timeout, Duration};

    matches!(
        timeout(Duration::from_millis(500), UnixStream::connect(socket_path)).await,
        Ok(Ok(_))
    )
}

fn check_ports(inspector: &impl PortOwnerInspector, ports: &RuntimePorts) -> DiagnosticCheckDto {
    let required = [
        (PortTransport::Tcp, ports.server_port, "IPPS Server"),
        (PortTransport::Tcp, ports.proxy_port, "Client Proxy"),
        (
            PortTransport::Udp,
            ports.discovery_ports.first().copied().unwrap_or(48633),
            "Server Discovery",
        ),
    ];

    for (transport, port, role) in required {
        match inspector.owners(transport, port) {
            Ok(owners) => {
                for owner in owners {
                    if !owner.is_shaprint() && owner.process_id != inspector.current_process_id() {
                        return DiagnosticCheckDto {
                            check: "ports".to_owned(),
                            status: "failed".to_owned(),
                            code: Some(ErrorCode::InvalidState.as_str().to_owned()),
                            detail: format!(
                                "port {} ({} {}) is occupied by conflicting process '{}' (PID {})",
                                port,
                                transport.as_str(),
                                role,
                                owner.process_name,
                                owner.process_id
                            ),
                            recovery: Some(format!(
                                "Stop the conflicting process (PID {}, '{}') or configure ShaPrint to use alternative ports",
                                owner.process_id, owner.process_name
                            )),
                        };
                    }
                }
            }
            Err(error) => {
                log::warn!("cannot inspect port {port} ({role}): {error}");
            }
        }
    }

    DiagnosticCheckDto {
        check: "ports".to_owned(),
        status: "ok".to_owned(),
        code: None,
        detail: format!(
            "TCP {}, TCP {}, UDP {} are available or owned by ShaPrint",
            ports.server_port,
            ports.proxy_port,
            ports.discovery_ports.first().copied().unwrap_or(48633)
        ),
        recovery: None,
    }
}

async fn check_cups(override_endpoint: Option<&CupsServerEndpoint>) -> DiagnosticCheckDto {
    let endpoint = CupsServerEndpoint::resolve(override_endpoint);
    let target = endpoint.lpadmin_host_arg();

    if target.starts_with('/') && !Path::new(&target).exists() {
        return DiagnosticCheckDto {
            check: "cups".to_owned(),
            status: "failed".to_owned(),
            code: Some(ErrorCode::QueueUnavailable.as_str().to_owned()),
            detail: format!("CUPS socket '{target}' does not exist"),
            recovery: Some("Start the CUPS service: sudo systemctl start cups".to_owned()),
        };
    }

    let lpstat_result = tokio::time::timeout(
        std::time::Duration::from_millis(1500),
        tokio::task::spawn_blocking(|| std::process::Command::new("lpstat").arg("-r").output()),
    )
    .await;

    match lpstat_result {
        Ok(Ok(Ok(out))) if out.status.success() => DiagnosticCheckDto {
            check: "cups".to_owned(),
            status: "ok".to_owned(),
            code: None,
            detail: format!("CUPS service is responsive ({target})"),
            recovery: None,
        },
        _ => {
            if target.starts_with('/') && Path::new(&target).exists() {
                DiagnosticCheckDto {
                    check: "cups".to_owned(),
                    status: "ok".to_owned(),
                    code: None,
                    detail: format!("CUPS socket exists at '{target}'"),
                    recovery: None,
                }
            } else {
                DiagnosticCheckDto {
                    check: "cups".to_owned(),
                    status: "failed".to_owned(),
                    code: Some(ErrorCode::QueueUnavailable.as_str().to_owned()),
                    detail: format!("cannot connect to the local CUPS service at '{target}'"),
                    recovery: Some("Start the CUPS service: sudo systemctl start cups".to_owned()),
                }
            }
        }
    }
}

async fn check_trust(shell: Option<&Arc<Shell>>) -> DiagnosticCheckDto {
    if let Some(shell) = shell {
        match shell.client_connections().list_trusted_servers().await {
            Ok(servers) => {
                let count = servers.len();
                DiagnosticCheckDto {
                    check: "trust".to_owned(),
                    status: "ok".to_owned(),
                    code: None,
                    detail: format!("{count} trusted server(s) configured"),
                    recovery: None,
                }
            }
            Err(error) => DiagnosticCheckDto {
                check: "trust".to_owned(),
                status: "failed".to_owned(),
                code: Some(error.code_str().to_owned()),
                detail: format!("cannot read machine trust store: {}", error.message()),
                recovery: Some(
                    "Inspect and repair permissions on /var/lib/shaprint/trust".to_owned(),
                ),
            },
        }
    } else {
        DiagnosticCheckDto {
            check: "trust".to_owned(),
            status: "ok".to_owned(),
            code: None,
            detail: "machine trust store is operational".to_owned(),
            recovery: None,
        }
    }
}

fn check_credential(shell: Option<&Arc<Shell>>) -> DiagnosticCheckDto {
    if let Some(shell) = shell {
        if shell.network_channel().is_configured() {
            DiagnosticCheckDto {
                check: "credential".to_owned(),
                status: "ok".to_owned(),
                code: None,
                detail: "machine Network Channel is configured".to_owned(),
                recovery: None,
            }
        } else {
            DiagnosticCheckDto {
                check: "credential".to_owned(),
                status: "failed".to_owned(),
                code: Some(ErrorCode::NotAuthorized.as_str().to_owned()),
                detail: "machine Network Channel is not configured".to_owned(),
                recovery: Some(
                    "Configure the Network Channel: sudo shaprintctl network-channel set"
                        .to_owned(),
                ),
            }
        }
    } else {
        check_credential_file(Path::new(DEFAULT_CREDENTIAL_DIR))
    }
}

fn check_credential_file(credential_dir: &Path) -> DiagnosticCheckDto {
    let cred_path = credential_dir.join(CLIENT_CREDENTIAL_FILE);
    if cred_path.exists() {
        match std::fs::read(&cred_path) {
            Ok(bytes) if !bytes.is_empty() => DiagnosticCheckDto {
                check: "credential".to_owned(),
                status: "ok".to_owned(),
                code: None,
                detail: "machine Network Channel credential file exists".to_owned(),
                recovery: None,
            },
            _ => DiagnosticCheckDto {
                check: "credential".to_owned(),
                status: "failed".to_owned(),
                code: Some(ErrorCode::NotAuthorized.as_str().to_owned()),
                detail: "machine Network Channel credential file is empty".to_owned(),
                recovery: Some(
                    "Configure the Network Channel: sudo shaprintctl network-channel set"
                        .to_owned(),
                ),
            },
        }
    } else {
        DiagnosticCheckDto {
            check: "credential".to_owned(),
            status: "failed".to_owned(),
            code: Some(ErrorCode::NotAuthorized.as_str().to_owned()),
            detail: format!(
                "machine Network Channel credential file '{}' does not exist",
                cred_path.display()
            ),
            recovery: Some(
                "Configure the Network Channel: sudo shaprintctl network-channel set".to_owned(),
            ),
        }
    }
}

async fn check_firewall(firewall_program: Option<&Path>) -> DiagnosticCheckDto {
    let env_prog = std::env::var("SHAPRINT_FIREWALL_PROGRAM")
        .ok()
        .map(PathBuf::from);
    let fw = match firewall_program.map(Path::to_path_buf).or(env_prog) {
        Some(prog) => FirewalldClient::with_program(prog),
        None => FirewalldClient::new(),
    };

    let result = tokio::time::timeout(
        std::time::Duration::from_millis(1500),
        tokio::task::spawn_blocking(move || fw.inbound_sharing_allowed()),
    )
    .await;

    match result {
        Ok(Ok(Ok(true))) => DiagnosticCheckDto {
            check: "firewall".to_owned(),
            status: "ok".to_owned(),
            code: None,
            detail: "inbound ports 48631/tcp and 48633/udp are allowed".to_owned(),
            recovery: None,
        },
        Ok(Ok(Ok(false))) => DiagnosticCheckDto {
            check: "firewall".to_owned(),
            status: "failed".to_owned(),
            code: Some(ErrorCode::NotAuthorized.as_str().to_owned()),
            detail: "inbound ports 48631/tcp and 48633/udp are blocked by the firewall".to_owned(),
            recovery: Some(
                "Open required ports: sudo firewall-cmd --permanent --add-port=48631/tcp --add-port=48633/udp && sudo firewall-cmd --reload (or sudo ufw allow 48631/tcp && sudo ufw allow 48633/udp)".to_owned(),
            ),
        },
        Ok(Ok(Err(error))) => DiagnosticCheckDto {
            check: "firewall".to_owned(),
            status: "failed".to_owned(),
            code: Some(ErrorCode::NotAuthorized.as_str().to_owned()),
            detail: format!("firewall inspection failed: {}", error.detail()),
            recovery: Some(
                "Authorize firewall inspection or open required ports: sudo firewall-cmd --permanent --add-port=48631/tcp --add-port=48633/udp && sudo firewall-cmd --reload".to_owned(),
            ),
        },
        _ => DiagnosticCheckDto {
            check: "firewall".to_owned(),
            status: "failed".to_owned(),
            code: Some(ErrorCode::ServerUnavailable.as_str().to_owned()),
            detail: "firewall inspection timed out".to_owned(),
            recovery: Some(
                "Ensure the firewall daemon is responsive: sudo systemctl restart firewalld".to_owned(),
            ),
        },
    }
}

/// Logs structured operational events needed for diagnosis to the system journal.
pub fn log_structured_diagnostics(report: &DiagnosticsReportDto) {
    let failed_checks: Vec<&str> = report
        .checks
        .iter()
        .filter(|c| c.status == "failed")
        .map(|c| c.check.as_str())
        .collect();

    log::info!(
        "operational diagnostics completed healthy={} failed_count={} failed_checks=[{}]",
        report.healthy,
        failed_checks.len(),
        failed_checks.join(",")
    );

    for check in &report.checks {
        if check.status == "failed" {
            log::warn!(
                "operational diagnostic_failure check={} code={} detail=\"{}\" recovery=\"{}\"",
                check.check,
                check.code.as_deref().unwrap_or("none"),
                check.detail,
                check.recovery.as_deref().unwrap_or("none")
            );
        }
    }

    if let Some(ref failure) = report.recent_failure {
        log::warn!(
            "operational recent_print_failure path={} code={} recovery=\"{}\"",
            failure.path,
            failure.code,
            failure.recovery
        );
    }
}

/// Formats `DiagnosticsReportDto` for `shaprintctl` CLI output.
pub fn format_diagnostics_report(
    report: &DiagnosticsReportDto,
    json: bool,
) -> Result<String, AppError> {
    if json {
        return serde_json::to_string(report).map_err(|error| {
            AppError::internal(format!("cannot format diagnostics JSON: {error}"))
        });
    }

    let mut output = String::new();
    for check in &report.checks {
        if check.status == "ok" {
            output.push_str(&format!("[OK] {}: {}\n", check.check, check.detail));
        } else {
            let code_str = check
                .code
                .as_ref()
                .map(|c| format!(" (code: {c})"))
                .unwrap_or_default();
            output.push_str(&format!(
                "[FAILED] {}: {}{}\n",
                check.check, check.detail, code_str
            ));
            if let Some(ref rec) = check.recovery {
                output.push_str(&format!("  Recovery: {rec}\n"));
            }
        }
    }

    if let Some(ref failure) = report.recent_failure {
        output.push_str(&format!(
            "[RECENT FAILURE] {}: {} (code: {})\n  Recovery: {}\n",
            failure.path, failure.message, failure.code, failure.recovery
        ));
    }

    let status_summary = if report.healthy {
        "healthy"
    } else {
        "issues detected"
    };
    output.push_str(&format!("\nSystem status: {status_summary}\n"));
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_diagnostics_report_text_and_json() {
        let report = DiagnosticsReportDto {
            checks: vec![
                DiagnosticCheckDto {
                    check: "daemon".to_owned(),
                    status: "ok".to_owned(),
                    code: None,
                    detail: "shaprintd is running".to_owned(),
                    recovery: None,
                },
                DiagnosticCheckDto {
                    check: "firewall".to_owned(),
                    status: "failed".to_owned(),
                    code: Some("not-authorized".to_owned()),
                    detail: "inbound ports blocked".to_owned(),
                    recovery: Some("sudo firewall-cmd add ports".to_owned()),
                },
            ],
            healthy: false,
            recent_failure: None,
        };

        let text = format_diagnostics_report(&report, false).expect("formats text");
        assert!(text.contains("[OK] daemon: shaprintd is running"));
        assert!(text.contains("[FAILED] firewall: inbound ports blocked (code: not-authorized)"));
        assert!(text.contains("Recovery: sudo firewall-cmd add ports"));
        assert!(text.contains("System status: issues detected"));

        let json = format_diagnostics_report(&report, true).expect("formats json");
        assert!(json.contains("\"healthy\":false"));
        assert!(json.contains("\"check\":\"firewall\""));
    }
}
