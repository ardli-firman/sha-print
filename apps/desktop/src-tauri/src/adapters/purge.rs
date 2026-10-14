//! Safe purge of ShaPrint-owned resources on Linux (ADR 0018, issue #133).
//!
//! Ordinary uninstall preserves state and queues. Confirmed purge requires an explicit operation
//! and removes ShaPrint-owned Client queues, credentials, identity, durable state, service
//! integration, and firewall rules without ever deleting local CUPS printer queues or non-ShaPrint
//! queues.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::adapters::daemon::{
    current_effective_uid, DEFAULT_CONFIG_DIR, DEFAULT_CONFIG_FILE, DEFAULT_CREDENTIAL_DIR,
    DEFAULT_SOCKET_PATH, DEFAULT_STATE_DIR,
};
use crate::adapters::elevation::linux::FirewalldClient;
use crate::adapters::ipps::CLIENT_CREDENTIAL_FILE;
use crate::adapters::printers::cups::{
    query_cups_destinations, CupsDestinationRecord, CupsServerEndpoint,
};
use crate::domain::{AppError, SpoolerRecord, SpoolerRecordClassification};

const LPADMIN_CANDIDATES: [&str; 3] = ["/usr/sbin/lpadmin", "/sbin/lpadmin", "lpadmin"];

fn locate_lpadmin() -> &'static str {
    for candidate in LPADMIN_CANDIDATES {
        if candidate.starts_with('/') && Path::new(candidate).exists() {
            return candidate;
        }
    }
    "lpadmin"
}

/// Options governing explicit purge execution.
#[derive(Debug, Clone)]
pub struct PurgeExecutionOptions {
    pub confirm: bool,
    pub config_dir: PathBuf,
    pub credential_dir: PathBuf,
    pub state_dir: PathBuf,
    pub socket_path: PathBuf,
    pub cups_endpoint: Option<CupsServerEndpoint>,
    pub proxy_port: u16,
    pub firewall_program: Option<PathBuf>,
    pub require_root: bool,
}

impl Default for PurgeExecutionOptions {
    fn default() -> Self {
        Self {
            confirm: false,
            config_dir: PathBuf::from(DEFAULT_CONFIG_DIR),
            credential_dir: PathBuf::from(DEFAULT_CREDENTIAL_DIR),
            state_dir: PathBuf::from(DEFAULT_STATE_DIR),
            socket_path: PathBuf::from(DEFAULT_SOCKET_PATH),
            cups_endpoint: None,
            proxy_port: crate::adapters::CLIENT_PROXY_DEFAULT_PORT,
            firewall_program: None,
            require_root: true,
        }
    }
}

/// Structured summary of all actions taken during purge.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PurgeReportDto {
    pub purged: bool,
    pub removed_client_queues: Vec<String>,
    pub removed_credentials: bool,
    pub removed_identity_and_state: bool,
    pub removed_config: bool,
    pub removed_firewall_rules: bool,
    pub service_stopped: bool,
    pub warnings: Vec<String>,
    pub recovery_actions: Vec<String>,
}

/// Executes confirmed purge of ShaPrint on Linux.
pub async fn execute_purge(options: &PurgeExecutionOptions) -> Result<PurgeReportDto, AppError> {
    execute_purge_with_destinations_and_lpadmin(options, None, None).await
}

/// Internal helper allowing test fixtures to supply mock CUPS destinations and lpadmin paths.
pub async fn execute_purge_with_destinations_and_lpadmin(
    options: &PurgeExecutionOptions,
    mock_destinations: Option<Vec<CupsDestinationRecord>>,
    custom_lpadmin: Option<&Path>,
) -> Result<PurgeReportDto, AppError> {
    if !options.confirm {
        return Err(AppError::invalid_input(
            "purge requires confirmation; pass '--confirm' to proceed",
        ));
    }

    if options.require_root {
        let euid = current_effective_uid().unwrap_or_else(|| {
            std::process::Command::new("id")
                .arg("-u")
                .output()
                .ok()
                .and_then(|out| {
                    String::from_utf8_lossy(&out.stdout)
                        .trim()
                        .parse::<u32>()
                        .ok()
                })
                .unwrap_or(u32::MAX)
        });

        if euid != 0 {
            return Err(AppError::not_authorized(
                "administrative changes require root privileges (run with sudo)",
            ));
        }
    }

    let mut warnings = Vec::new();
    let mut recovery_actions = Vec::new();

    // 1. Stop and disable service integration
    let service_stopped = stop_and_disable_service(&options.socket_path);

    // 2. Remove ShaPrint-owned CUPS client queues
    let removed_client_queues = purge_cups_client_queues(
        options,
        mock_destinations,
        custom_lpadmin,
        &mut warnings,
        &mut recovery_actions,
    );

    // 3. Remove firewall rules
    let removed_firewall_rules =
        purge_firewall_rules(options, &mut warnings, &mut recovery_actions);

    // 4. Remove protected credentials (/etc/shaprint-credentials)
    let removed_credentials = purge_credentials(
        &options.credential_dir,
        &mut warnings,
        &mut recovery_actions,
    );

    // 5. Remove durable state and identity (/var/lib/shaprint)
    let removed_identity_and_state =
        purge_state_and_identity(&options.state_dir, &mut warnings, &mut recovery_actions);

    // 6. Remove non-secret configuration (/etc/shaprint)
    let removed_config = purge_config(&options.config_dir, &mut warnings, &mut recovery_actions);

    // 7. Clean up runtime socket and directory if lingering
    if options.socket_path.exists() {
        let _ = std::fs::remove_file(&options.socket_path);
    }
    if let Some(parent) = options.socket_path.parent() {
        let _ = std::fs::remove_dir(parent);
    }

    Ok(PurgeReportDto {
        purged: true,
        removed_client_queues,
        removed_credentials,
        removed_identity_and_state,
        removed_config,
        removed_firewall_rules,
        service_stopped,
        warnings,
        recovery_actions,
    })
}

fn stop_and_disable_service(socket_path: &Path) -> bool {
    let has_systemctl = Command::new("systemctl")
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success());

    if has_systemctl {
        let _ = Command::new("systemctl")
            .args(["stop", "shaprintd.service"])
            .output();
        let _ = Command::new("systemctl")
            .args(["disable", "--now", "shaprintd.service"])
            .output();
        let _ = Command::new("systemctl").args(["daemon-reload"]).output();
    }

    if socket_path.exists() {
        let _ = std::fs::remove_file(socket_path);
    }
    true
}

fn purge_cups_client_queues(
    options: &PurgeExecutionOptions,
    mock_destinations: Option<Vec<CupsDestinationRecord>>,
    custom_lpadmin: Option<&Path>,
    warnings: &mut Vec<String>,
    recovery_actions: &mut Vec<String>,
) -> Vec<String> {
    let mut removed = Vec::new();
    let endpoint = CupsServerEndpoint::resolve(options.cups_endpoint.as_ref());

    let destinations = match mock_destinations {
        Some(dests) => Ok(dests),
        None => query_cups_destinations(&endpoint),
    };

    let dests = match destinations {
        Ok(list) => list,
        Err(err) => {
            warnings.push(format!("could not query CUPS destinations: {err}"));
            recovery_actions.push(
                "ensure CUPS is running, then verify and remove any lingering ShaPrint client queues with 'lpadmin -x <name>'".to_string(),
            );
            return removed;
        }
    };

    let authorities = [
        format!("127.0.0.1:{}", options.proxy_port),
        format!("localhost:{}", options.proxy_port),
        "127.0.0.1:48632".to_string(),
        "localhost:48632".to_string(),
    ];

    let lpadmin_bin = custom_lpadmin
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| locate_lpadmin().to_string());

    for dest in dests {
        let is_shaprint = authorities.iter().any(|auth| {
            let record = SpoolerRecord::new(&dest.name, &dest.device_uri);
            matches!(
                record.classify(auth),
                SpoolerRecordClassification::RecognisedClientQueue(_)
            )
        });

        if !is_shaprint {
            // Local printer queue or conflicting queue ShaPrint does not own: NEVER REMOVE!
            continue;
        }

        let mut cmd = Command::new(&lpadmin_bin);
        let host_arg = endpoint.lpadmin_host_arg();
        if !host_arg.starts_with('/') && !host_arg.is_empty() {
            cmd.arg("-h").arg(&host_arg);
        }
        cmd.arg("-x").arg(&dest.name);

        match cmd.output() {
            Ok(output) if output.status.success() => {
                removed.push(dest.name);
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                warnings.push(format!(
                    "failed to remove CUPS client queue '{}': {}",
                    dest.name, stderr
                ));
                recovery_actions.push(format!(
                    "remove CUPS client queue manually: lpadmin -x '{}'",
                    dest.name
                ));
            }
            Err(err) => {
                warnings.push(format!(
                    "failed to execute lpadmin to remove queue '{}': {err}",
                    dest.name
                ));
                recovery_actions.push(format!(
                    "remove CUPS client queue manually: lpadmin -x '{}'",
                    dest.name
                ));
            }
        }
    }

    removed
}

fn purge_firewall_rules(
    options: &PurgeExecutionOptions,
    warnings: &mut Vec<String>,
    recovery_actions: &mut Vec<String>,
) -> bool {
    let fw = match &options.firewall_program {
        Some(prog) => FirewalldClient::with_program(prog),
        None => FirewalldClient::new(),
    };

    let firewalld_ok = match fw.remove_inbound_sharing() {
        Ok(()) => true,
        Err(err) => {
            warnings.push(format!("failed to remove firewalld sharing rules: {err}"));
            recovery_actions.push(
                "remove firewall rules manually: firewall-cmd --permanent --remove-port=48631/tcp --remove-port=48633/udp && firewall-cmd --reload".to_string(),
            );
            false
        }
    };

    // Also check UFW if available
    let has_ufw = Command::new("ufw")
        .arg("status")
        .output()
        .is_ok_and(|out| out.status.success());
    if has_ufw {
        let _ = Command::new("ufw")
            .args(["delete", "allow", "48631/tcp"])
            .output();
        let _ = Command::new("ufw")
            .args(["delete", "allow", "48633/udp"])
            .output();
    }

    firewalld_ok
}

fn purge_credentials(
    credential_dir: &Path,
    warnings: &mut Vec<String>,
    recovery_actions: &mut Vec<String>,
) -> bool {
    let cred_file = credential_dir.join(CLIENT_CREDENTIAL_FILE);
    if cred_file.exists() {
        if let Err(err) = std::fs::remove_file(&cred_file) {
            warnings.push(format!(
                "failed to remove credential file '{}': {err}",
                cred_file.display()
            ));
            recovery_actions.push(format!(
                "remove credential file manually: rm -f '{}'",
                cred_file.display()
            ));
        }
    }
    if credential_dir.exists() {
        if let Err(err) = std::fs::remove_dir(credential_dir) {
            warnings.push(format!(
                "failed to remove credential directory '{}': {err}",
                credential_dir.display()
            ));
            recovery_actions.push(format!(
                "remove credential directory manually: rm -rf '{}'",
                credential_dir.display()
            ));
        }
    }
    !cred_file.exists() && !credential_dir.exists()
}

fn purge_state_and_identity(
    state_dir: &Path,
    warnings: &mut Vec<String>,
    recovery_actions: &mut Vec<String>,
) -> bool {
    if !state_dir.exists() {
        return true;
    }
    if let Ok(entries) = std::fs::read_dir(state_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() || path.is_symlink() {
                if let Err(err) = std::fs::remove_file(&path) {
                    warnings.push(format!(
                        "failed to remove state file '{}': {err}",
                        path.display()
                    ));
                }
            } else if path.is_dir() {
                if let Err(err) = std::fs::remove_dir_all(&path) {
                    warnings.push(format!(
                        "failed to remove state directory '{}': {err}",
                        path.display()
                    ));
                }
            }
        }
    }
    if let Err(err) = std::fs::remove_dir(state_dir) {
        if state_dir.exists() {
            warnings.push(format!(
                "failed to remove state directory '{}': {err}",
                state_dir.display()
            ));
            recovery_actions.push(format!(
                "remove state directory manually: rm -rf '{}'",
                state_dir.display()
            ));
        }
    }
    !state_dir.exists()
}

fn purge_config(
    config_dir: &Path,
    warnings: &mut Vec<String>,
    recovery_actions: &mut Vec<String>,
) -> bool {
    let config_file = config_dir.join(DEFAULT_CONFIG_FILE);
    if config_file.exists() {
        if let Err(err) = std::fs::remove_file(&config_file) {
            warnings.push(format!(
                "failed to remove config file '{}': {err}",
                config_file.display()
            ));
            recovery_actions.push(format!(
                "remove config file manually: rm -f '{}'",
                config_file.display()
            ));
        }
    }
    if config_dir.exists() {
        if let Err(err) = std::fs::remove_dir(config_dir) {
            if config_dir.exists() {
                warnings.push(format!(
                    "failed to remove config directory '{}': {err}",
                    config_dir.display()
                ));
                recovery_actions.push(format!(
                    "remove config directory manually: rm -rf '{}'",
                    config_dir.display()
                ));
            }
        }
    }
    !config_file.exists() && !config_dir.exists()
}

/// Formats `PurgeReportDto` for `shaprintctl` human or json output.
pub fn format_purge_report(report: &PurgeReportDto, json: bool) -> Result<String, AppError> {
    if json {
        return serde_json::to_string(report)
            .map_err(|err| AppError::internal(format!("cannot format purge JSON: {err}")));
    }

    let mut out = String::new();
    out.push_str(&format!("purged: {}\n", report.purged));
    let queues = if report.removed_client_queues.is_empty() {
        "none".to_string()
    } else {
        report.removed_client_queues.join(", ")
    };
    out.push_str(&format!("removed-client-queues: {queues}\n"));
    out.push_str(&format!(
        "removed-credentials: {}\n",
        report.removed_credentials
    ));
    out.push_str(&format!(
        "removed-identity-and-state: {}\n",
        report.removed_identity_and_state
    ));
    out.push_str(&format!("removed-config: {}\n", report.removed_config));
    out.push_str(&format!(
        "removed-firewall-rules: {}\n",
        report.removed_firewall_rules
    ));
    out.push_str(&format!("service-stopped: {}\n", report.service_stopped));

    if !report.warnings.is_empty() {
        out.push_str("warnings:\n");
        for warn in &report.warnings {
            out.push_str(&format!("  - {warn}\n"));
        }
    }
    if !report.recovery_actions.is_empty() {
        out.push_str("recovery:\n");
        for action in &report.recovery_actions {
            out.push_str(&format!("  - {action}\n"));
        }
    }

    Ok(out)
}
