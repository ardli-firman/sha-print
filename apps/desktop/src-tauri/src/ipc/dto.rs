//! Serializable payloads exchanged with the frontend.
//!
//! These are the only shapes the UI sees; domain types stay free of serialization concerns. Ids,
//! states, and codes are stable strings (ADR 0002), and no payload carries credentials or print
//! job content.

use serde::{Deserialize, Serialize};

use crate::application::LocalPrinter;
use crate::application::{ImportReport, StartupStatus};
use crate::domain::{AppError, PrintFailure, RuntimeStatus, ServiceStatus};

/// Event the shell emits whenever runtime status changes.
pub const RUNTIME_STATUS_EVENT: &str = "runtime://status";

/// Event the shell emits whenever a print failure is reported or dismissed.
pub const PRINT_FAILURE_EVENT: &str = "runtime://print-failure";

/// Event the shell emits when the previous application's settings have been imported.
pub const LEGACY_IMPORT_EVENT: &str = "legacy://import";

/// Status of one supervised service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceStatusDto {
    /// Stable service id, for example `client-proxy`.
    pub id: String,
    /// Stable lifecycle state, for example `running`.
    pub state: String,
    /// Short note about the current state; never contains secrets or job content.
    pub detail: String,
}

impl From<&ServiceStatus> for ServiceStatusDto {
    fn from(status: &ServiceStatus) -> Self {
        Self {
            id: status.id().as_str().to_owned(),
            state: status.state().as_str().to_owned(),
            detail: status.detail().to_owned(),
        }
    }
}

/// Status of every service the shell supervises.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeStatusDto {
    pub services: Vec<ServiceStatusDto>,
}

impl From<&RuntimeStatus> for RuntimeStatusDto {
    fn from(status: &RuntimeStatus) -> Self {
        Self {
            services: status
                .services()
                .iter()
                .map(ServiceStatusDto::from)
                .collect(),
        }
    }
}

/// The latest print failure the shell reported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrintFailureDto {
    /// Which print path failed: `client-forwarding` or `server-submission`.
    pub path: String,
    /// Stable code for the condition, for example `server-unavailable`.
    pub code: String,
    /// What happened; never contains the Network Channel or document contents.
    pub message: String,
    /// The one action that resolves the failure.
    pub recovery: String,
    /// When the failure was observed, as milliseconds since the Unix epoch.
    pub observed_at_ms: u64,
}

impl From<&PrintFailure> for PrintFailureDto {
    fn from(failure: &PrintFailure) -> Self {
        Self {
            path: failure.path().as_str().to_owned(),
            code: failure.code().as_str().to_owned(),
            message: failure.message().to_owned(),
            recovery: failure.recovery().to_owned(),
            observed_at_ms: failure.observed_at_ms(),
        }
    }
}

/// Failure returned to the frontend: a stable code plus a human-readable message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppErrorDto {
    /// Stable error code, for example `invalid-state`.
    pub code: String,
    pub message: String,
}

impl From<&AppError> for AppErrorDto {
    fn from(error: &AppError) -> Self {
        Self {
            code: error.code_str().to_owned(),
            message: error.message().to_owned(),
        }
    }
}

/// One local printer queue as the sharing UI lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalPrinterDto {
    /// Queue name; also the `printer-name` clients see.
    pub name: String,
    /// Whether the server currently shares this queue with clients.
    pub shared: bool,
}

impl From<&LocalPrinter> for LocalPrinterDto {
    fn from(printer: &LocalPrinter) -> Self {
        Self {
            name: printer.name().as_str().to_owned(),
            shared: printer.shared(),
        }
    }
}

/// Every local queue and its sharing state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalPrintersDto {
    pub printers: Vec<LocalPrinterDto>,
}

impl From<&[LocalPrinter]> for LocalPrintersDto {
    fn from(printers: &[LocalPrinter]) -> Self {
        Self {
            printers: printers.iter().map(LocalPrinterDto::from).collect(),
        }
    }
}

/// The server identity a client user approves, and where clients reach it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerIdentityDto {
    /// Uppercase, colon-separated SHA-256 fingerprint of the server certificate.
    pub fingerprint: String,
    /// Port the sharing endpoint listens on.
    pub port: u16,
}

/// What a setup action did. Never carries credentials or permission tokens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetupOutcomeDto {
    /// Whether administrator permission was requested for the action.
    pub elevated: bool,
}

/// Whether ShaPrint starts with this user's Windows login.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartupStatusDto {
    /// Whether this platform registers login startup at all.
    pub supported: bool,
    /// Whether the registration matches this installation.
    pub enabled: bool,
    /// The command a login launch runs; empty when the program could not be located.
    pub command: String,
}

impl From<&StartupStatus> for StartupStatusDto {
    fn from(status: &StartupStatus) -> Self {
        Self {
            supported: status.supported,
            enabled: status.enabled,
            command: status.command.clone(),
        }
    }
}

/// One setting from the previous ShaPrint app that this app does not take.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacySettingDto {
    /// Stable key, for example `client-queues`.
    pub key: String,
    pub label: String,
    /// Why this app does not take it.
    pub reason: String,
}

impl From<&crate::application::SkippedSetting> for LegacySettingDto {
    fn from(setting: &crate::application::SkippedSetting) -> Self {
        Self {
            key: setting.key.clone(),
            label: setting.label.clone(),
            reason: setting.reason.clone(),
        }
    }
}

/// What the move from the previous ShaPrint app did, or would do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyImportReportDto {
    /// Whether the previous app left settings for this user.
    pub found: bool,
    /// Stable channel outcome, for example `imported`.
    pub channel: String,
    /// The sentence the window shows for that outcome; never contains the channel itself.
    pub channel_note: String,
    pub settings: Vec<LegacySettingDto>,
    /// Whether the previous app had queues the user has to select again.
    pub queues_need_reselection: bool,
}

impl From<&ImportReport> for LegacyImportReportDto {
    fn from(report: &ImportReport) -> Self {
        Self {
            found: report.found,
            channel: report.channel.as_str().to_owned(),
            channel_note: report.channel.note().to_owned(),
            settings: report.settings.iter().map(LegacySettingDto::from).collect(),
            queues_need_reselection: report.queues_need_reselection,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ErrorCode, PrinterName, RuntimeStatus, ServiceId};

    fn status(id: ServiceId, state: crate::domain::ServiceState, detail: &str) -> ServiceStatus {
        let mut status = ServiceStatus::stopped(id);
        if state != crate::domain::ServiceState::Stopped {
            let _ = status.transition(crate::domain::ServiceState::Starting);
            let _ = status.transition(state);
        }
        status.set_detail(detail);
        status
    }

    #[test]
    fn runtime_status_payload_shape_is_stable() {
        let runtime = RuntimeStatus::new(vec![
            status(
                ServiceId::ClientProxy,
                crate::domain::ServiceState::Running,
                "running",
            ),
            status(
                ServiceId::ServerSharing,
                crate::domain::ServiceState::Stopped,
                "",
            ),
        ]);

        let payload = serde_json::to_value(RuntimeStatusDto::from(&runtime)).expect("serializes");
        assert_eq!(
            payload,
            serde_json::json!({
                "services": [
                    { "id": "client-proxy", "state": "running", "detail": "running" },
                    { "id": "server-sharing", "state": "stopped", "detail": "" }
                ]
            })
        );
    }

    #[test]
    fn error_payload_shape_is_stable() {
        let error = AppError::new(
            ErrorCode::InvalidState,
            "service 'server-sharing' is not running",
        );
        let payload = serde_json::to_value(AppErrorDto::from(&error)).expect("serializes");
        assert_eq!(
            payload,
            serde_json::json!({
                "code": "invalid-state",
                "message": "service 'server-sharing' is not running"
            })
        );
    }

    #[test]
    fn event_name_matches_the_frontend_contract() {
        assert_eq!(RUNTIME_STATUS_EVENT, "runtime://status");
    }

    #[test]
    fn local_printer_payload_shape_is_stable() {
        let printers = LocalPrintersDto::from(
            &[
                LocalPrinter::new(PrinterName::parse("HP LaserJet").expect("valid name"), true),
                LocalPrinter::new(PrinterName::parse("Zebra").expect("valid name"), false),
            ][..],
        );

        let payload = serde_json::to_value(printers).expect("serializes");
        assert_eq!(
            payload,
            serde_json::json!({
                "printers": [
                    { "name": "HP LaserJet", "shared": true },
                    { "name": "Zebra", "shared": false }
                ]
            })
        );
    }

    #[test]
    fn server_identity_payload_shape_is_stable() {
        let payload = serde_json::to_value(ServerIdentityDto {
            fingerprint: "AA:BB".to_owned(),
            port: 8631,
        })
        .expect("serializes");

        assert_eq!(
            payload,
            serde_json::json!({ "fingerprint": "AA:BB", "port": 8631 })
        );
    }

    #[test]
    fn setup_outcome_payload_shape_is_stable() {
        let payload = serde_json::to_value(SetupOutcomeDto { elevated: true }).expect("serializes");
        assert_eq!(payload, serde_json::json!({ "elevated": true }));
    }

    #[test]
    fn print_failure_payload_shape_is_stable() {
        let queue = PrinterName::parse("Office Printer").expect("a valid queue name");
        let failure =
            crate::domain::PrintFailure::server(ErrorCode::QueueUnavailable, Some(&queue));

        let payload = serde_json::to_value(PrintFailureDto::from(&failure)).expect("serializes");

        assert_eq!(
            payload,
            serde_json::json!({
                "path": "server-submission",
                "code": "queue-unavailable",
                "message": "The job for 'Office Printer' could not be submitted to the Windows printer queue.",
                "recovery": "Check that the printer is switched on and reachable from this computer, then print again.",
                "observed_at_ms": failure.observed_at_ms()
            })
        );
    }

    #[test]
    fn a_dismissed_print_failure_serializes_as_nothing() {
        let payload = serde_json::to_value(Option::<PrintFailureDto>::None).expect("serializes");
        assert_eq!(payload, serde_json::json!(null));
    }

    #[test]
    fn the_print_failure_event_name_matches_the_frontend_contract() {
        assert_eq!(PRINT_FAILURE_EVENT, "runtime://print-failure");
    }

    #[test]
    fn startup_status_payload_shape_is_stable() {
        let payload = serde_json::to_value(StartupStatusDto::from(&StartupStatus {
            supported: true,
            enabled: true,
            command: "\"C:\\ShaPrint\\shaprint-desktop.exe\" --background".to_owned(),
        }))
        .expect("serializes");

        assert_eq!(
            payload,
            serde_json::json!({
                "supported": true,
                "enabled": true,
                "command": "\"C:\\ShaPrint\\shaprint-desktop.exe\" --background"
            })
        );
    }

    #[test]
    fn legacy_import_payload_shape_is_stable() {
        use crate::application::{ChannelOutcome, ImportReport, SkippedSetting};

        let report = ImportReport {
            found: true,
            channel: ChannelOutcome::Imported,
            settings: vec![SkippedSetting {
                key: "client-queues".to_owned(),
                label: "Installed client printers".to_owned(),
                reason: "Printer queues are installed again from this app.".to_owned(),
            }],
            queues_need_reselection: true,
        };

        let payload =
            serde_json::to_value(LegacyImportReportDto::from(&report)).expect("serializes");

        assert_eq!(
            payload,
            serde_json::json!({
                "found": true,
                "channel": "imported",
                "channel_note": "Your Network Channel was imported from the previous ShaPrint app.",
                "settings": [{
                    "key": "client-queues",
                    "label": "Installed client printers",
                    "reason": "Printer queues are installed again from this app."
                }],
                "queues_need_reselection": true
            })
        );
    }

    #[test]
    fn the_legacy_import_event_name_matches_the_frontend_contract() {
        assert_eq!(LEGACY_IMPORT_EVENT, "legacy://import");
    }
}
