//! Configuration actions a user can ask the shell for, which of them need administrator rights, and
//! how a failed elevated action is reported.
//!
//! ADR 0001: setup may request administrator permission once, while routine operation runs as the
//! logged-in user. Keeping the classification in the domain — instead of at each call site — is
//! what makes "elevation only where it is required" checkable.
//!
//! The elevated helper is a separate process of this same executable that reports only an exit code
//! (ADR 0005). [`SetupFailureKind`] is that exit code's meaning: it turns "the helper failed" into
//! the action the user can take.

use std::fmt;

use crate::domain::{AppError, ClientQueueRequest, ErrorCode};

/// A configuration action the shell can perform for the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SetupAction {
    /// Let clients reach this server through the Windows firewall: the IPPS endpoint and the
    /// discovery ports a nearby-server query arrives on. Machine-wide state, so Windows asks for
    /// administrator permission.
    AllowInboundSharing,
    /// Choose which local queues are shared. Per-user configuration.
    SelectPrinters,
    /// Start sharing the selected queues. Per-user configuration.
    StartSharing,
    /// Stop sharing. Per-user configuration.
    StopSharing,
    /// Show the certificate fingerprint clients approve. Read-only.
    ShowCertificateFingerprint,
    /// Install (or repair) the native Windows queue for a printer shared by a trusted server.
    /// Machine-wide spooler state, so Windows asks for administrator permission.
    InstallPrinter,
}

impl SetupAction {
    /// Every action, in a fixed order.
    pub const ALL: [SetupAction; 6] = [
        SetupAction::AllowInboundSharing,
        SetupAction::SelectPrinters,
        SetupAction::StartSharing,
        SetupAction::StopSharing,
        SetupAction::ShowCertificateFingerprint,
        SetupAction::InstallPrinter,
    ];

    /// The stable name used by logs and by the elevated helper's command line.
    pub const fn as_str(self) -> &'static str {
        match self {
            SetupAction::AllowInboundSharing => "allow-inbound-sharing",
            SetupAction::SelectPrinters => "select-printers",
            SetupAction::StartSharing => "start-sharing",
            SetupAction::StopSharing => "stop-sharing",
            SetupAction::ShowCertificateFingerprint => "show-certificate-fingerprint",
            SetupAction::InstallPrinter => "install-printer",
        }
    }

    /// Parses an action received from the command line of an elevated helper.
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|action| action.as_str() == value)
    }

    /// Whether the action changes machine-wide state and therefore needs administrator rights.
    ///
    /// Only inbound access and printer installation do: the firewall rule and a spooler queue
    /// outlive the user's session and apply to all users. Selecting queues, starting and stopping
    /// sharing, and showing the fingerprint are per-user actions and never prompt (ADR 0001).
    pub const fn requires_elevation(self) -> bool {
        matches!(
            self,
            SetupAction::AllowInboundSharing | SetupAction::InstallPrinter
        )
    }
}

/// Why an elevated setup action failed.
///
/// The kinds are stable: the helper's exit codes and the messages the user sees both derive from
/// them, and they never carry credentials, Network Channel values, or print job content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SetupFailureKind {
    /// This platform or build cannot perform the action.
    Unsupported,
    /// The user declined the prompt, or Windows denied the change.
    PermissionDenied,
    /// The request data was not usable.
    InvalidRequest,
    /// The Windows Print Spooler is not available.
    SpoolerUnavailable,
    /// Windows refused the printer queue or its IPP port.
    QueueRejected,
    /// A printer with the derived name already exists and points somewhere else.
    ExistingQueueConflict,
    /// A required step did not finish in time.
    TimedOut,
    /// Anything else.
    Other,
}

/// Exit code the helper uses for the first failure kind. Codes start above the values the C runtime
/// reserves for abnormal termination, so a crash cannot be mistaken for a classified failure.
const FIRST_FAILURE_EXIT_CODE: i32 = 10;

impl SetupFailureKind {
    /// Every kind, in a fixed order.
    pub const ALL: [SetupFailureKind; 8] = [
        SetupFailureKind::Unsupported,
        SetupFailureKind::PermissionDenied,
        SetupFailureKind::InvalidRequest,
        SetupFailureKind::SpoolerUnavailable,
        SetupFailureKind::QueueRejected,
        SetupFailureKind::ExistingQueueConflict,
        SetupFailureKind::TimedOut,
        SetupFailureKind::Other,
    ];

    /// The stable id used by logs and by the helper's own diagnostics.
    pub const fn id(self) -> &'static str {
        match self {
            SetupFailureKind::Unsupported => "unsupported",
            SetupFailureKind::PermissionDenied => "permission-denied",
            SetupFailureKind::InvalidRequest => "invalid-request",
            SetupFailureKind::SpoolerUnavailable => "spooler-unavailable",
            SetupFailureKind::QueueRejected => "queue-rejected",
            SetupFailureKind::ExistingQueueConflict => "existing-queue-conflict",
            SetupFailureKind::TimedOut => "timed-out",
            SetupFailureKind::Other => "other",
        }
    }

    /// Reads the kind an elevated helper reported by its stable id.
    pub fn from_id(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.id() == value)
    }

    /// The exit code the elevated helper reports for this kind.
    pub const fn exit_code(self) -> i32 {
        FIRST_FAILURE_EXIT_CODE
            + match self {
                SetupFailureKind::Unsupported => 0,
                SetupFailureKind::PermissionDenied => 1,
                SetupFailureKind::InvalidRequest => 2,
                SetupFailureKind::SpoolerUnavailable => 3,
                SetupFailureKind::QueueRejected => 4,
                SetupFailureKind::ExistingQueueConflict => 5,
                SetupFailureKind::TimedOut => 6,
                SetupFailureKind::Other => 7,
            }
    }

    /// Reads a helper exit code back into the kind it stands for. `None` for success and for codes
    /// the shell does not classify.
    pub fn from_exit_code(code: i32) -> Option<Self> {
        if code == 0 {
            return None;
        }
        Self::ALL.into_iter().find(|kind| kind.exit_code() == code)
    }

    /// The stable error code the UI receives.
    pub const fn error_code(self) -> ErrorCode {
        match self {
            // A declined prompt already reports `unsupported` (ADR 0001), so both stay put.
            SetupFailureKind::Unsupported | SetupFailureKind::PermissionDenied => {
                ErrorCode::Unsupported
            }
            SetupFailureKind::InvalidRequest => ErrorCode::InvalidInput,
            SetupFailureKind::SpoolerUnavailable
            | SetupFailureKind::QueueRejected
            | SetupFailureKind::ExistingQueueConflict => ErrorCode::InvalidState,
            SetupFailureKind::TimedOut => ErrorCode::Timeout,
            SetupFailureKind::Other => ErrorCode::Internal,
        }
    }

    /// What the user can do about it. Never empty, never a secret.
    pub const fn advice(self) -> &'static str {
        match self {
            SetupFailureKind::Unsupported => {
                "this build cannot perform that setup action; run ShaPrint on Windows 10 or later."
            }
            SetupFailureKind::PermissionDenied => {
                "administrator permission was not granted. Approve the Windows prompt, then try again."
            }
            SetupFailureKind::InvalidRequest => {
                "the request was not valid. Review the server connection and the selected printer, then try again."
            }
            SetupFailureKind::SpoolerUnavailable => {
                "the Windows Print Spooler service is not running. Start it (services.msc), then try again."
            }
            SetupFailureKind::QueueRejected => {
                "Windows refused the printer queue. Confirm the client proxy is running and that the server still shares this printer, then try again."
            }
            SetupFailureKind::ExistingQueueConflict => {
                "a printer with that name already exists in Windows and does not point at this remote printer. Remove it in Windows printer settings, then try again."
            }
            SetupFailureKind::TimedOut => {
                "the Windows Print Spooler did not answer in time. Check the Print Spooler service, then try again."
            }
            SetupFailureKind::Other => {
                "the setup helper failed. Restart ShaPrint and try again; if it keeps failing, reinstall the printer driver."
            }
        }
    }
}

/// A setup failure: the reason, plus detail the helper keeps for diagnosis.
///
/// The detail stays inside the helper process (it goes to standard error, ADR 0005); the app only
/// transports the kind, so raw spooler text never reaches the UI, logs, or IPC payloads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupFailure {
    kind: SetupFailureKind,
    detail: String,
}

impl SetupFailure {
    pub fn new(kind: SetupFailureKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }

    pub fn kind(&self) -> SetupFailureKind {
        self.kind
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }

    /// The error the user sees when `action` failed for this reason.
    ///
    /// Naming the action keeps the message honest for every elevated action, including one added
    /// later.
    pub fn for_action(&self, action: SetupAction) -> AppError {
        let advice = self.kind.advice();
        AppError::new(
            self.kind.error_code(),
            match action {
                SetupAction::AllowInboundSharing => {
                    format!("Could not let clients through the Windows firewall: {advice}")
                }
                SetupAction::InstallPrinter => {
                    format!("Could not install the Windows printer queue: {advice}")
                }
                other => format!("Could not complete {}: {advice}", other.as_str()),
            },
        )
    }

    /// The error the user sees when this queue could not be installed.
    pub fn for_queue_install(&self, request: &ClientQueueRequest) -> AppError {
        AppError::new(
            self.kind.error_code(),
            format!(
                "Could not install the Windows queue \"{}\" for printer \"{}\": {}",
                request.queue_name(),
                request.printer(),
                self.kind.advice()
            ),
        )
    }
}

impl fmt::Display for SetupFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.kind.id(), self.detail)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::PrinterName;

    #[test]
    fn only_machine_wide_configuration_requires_elevation() {
        let elevating: Vec<&str> = SetupAction::ALL
            .into_iter()
            .filter(|action| action.requires_elevation())
            .map(SetupAction::as_str)
            .collect();

        assert_eq!(elevating, vec!["allow-inbound-sharing", "install-printer"]);
    }

    #[test]
    fn action_names_are_stable_and_parse_back() {
        let names: Vec<&str> = SetupAction::ALL
            .iter()
            .map(|action| action.as_str())
            .collect();
        assert_eq!(
            names,
            vec![
                "allow-inbound-sharing",
                "select-printers",
                "start-sharing",
                "stop-sharing",
                "show-certificate-fingerprint",
                "install-printer",
            ]
        );
        for action in SetupAction::ALL {
            assert_eq!(SetupAction::parse(action.as_str()), Some(action));
        }
        assert_eq!(SetupAction::parse("install-scanner"), None);
    }

    fn queue_request() -> ClientQueueRequest {
        ClientQueueRequest::new(
            "10.0.0.5:8631",
            PrinterName::parse("Office Printer").expect("valid printer name"),
        )
        .expect("valid request")
    }

    #[test]
    fn failure_kinds_round_trip_through_their_exit_codes() {
        assert_eq!(SetupFailureKind::from_exit_code(0), None);
        let mut codes = Vec::new();
        for kind in SetupFailureKind::ALL {
            let code = kind.exit_code();
            assert!(
                code >= FIRST_FAILURE_EXIT_CODE,
                "{code} is below the reserved range"
            );
            assert!(!codes.contains(&code), "duplicate exit code {code}");
            codes.push(code);
            assert_eq!(SetupFailureKind::from_exit_code(code), Some(kind));
            assert_eq!(SetupFailureKind::from_id(kind.id()), Some(kind));
            assert!(!kind.advice().is_empty());
            assert!(!kind.id().is_empty());
        }
        assert_eq!(SetupFailureKind::from_exit_code(1), None);
        assert_eq!(SetupFailureKind::from_exit_code(9), None);
        assert_eq!(SetupFailureKind::from_id("no-such-kind"), None);
    }

    #[test]
    fn failure_kinds_report_distinct_advice() {
        let mut advice: Vec<&str> = SetupFailureKind::ALL
            .iter()
            .map(|kind| kind.advice())
            .collect();
        let total = advice.len();
        advice.sort_unstable();
        advice.dedup();
        assert_eq!(advice.len(), total, "two kinds share one piece of advice");
    }

    #[test]
    fn a_failed_install_message_names_the_queue_and_the_advice() {
        let failure = SetupFailure::new(SetupFailureKind::SpoolerUnavailable, "spooler is down");
        let error = failure.for_queue_install(&queue_request());

        assert_eq!(error.code(), ErrorCode::InvalidState);
        assert!(error
            .message()
            .contains("Office Printer (ShaPrint 10.0.0.5-8631)"));
        assert!(error.message().contains("Office Printer"));
        assert!(error.message().contains("Print Spooler"));
        assert!(!error.message().contains("spooler is down"));
    }

    #[test]
    fn a_failed_firewall_message_repeats_the_advice() {
        let failure = SetupFailure::new(SetupFailureKind::PermissionDenied, "declined");
        let error = failure.for_action(SetupAction::AllowInboundSharing);

        assert_eq!(error.code(), ErrorCode::Unsupported);
        assert!(error.message().contains("Windows firewall"));
        assert!(error
            .message()
            .contains(SetupFailureKind::PermissionDenied.advice()));
    }

    #[test]
    fn a_failed_action_message_names_that_action_and_nothing_else() {
        let failure = SetupFailure::new(SetupFailureKind::Other, "recorded failure");

        // The shared mapper must not describe the wrong action.
        let error = failure.for_action(SetupAction::InstallPrinter);
        assert!(error.message().contains("printer queue"));
        assert!(!error.message().contains("firewall"));

        let error = failure.for_action(SetupAction::StartSharing);
        assert!(error.message().contains("start-sharing"));
        assert!(!error.message().contains("firewall"));
    }

    #[test]
    fn a_failure_keeps_its_detail_for_the_helper_log() {
        let failure = SetupFailure::new(SetupFailureKind::Other, "exit code 66");
        assert_eq!(failure.kind(), SetupFailureKind::Other);
        assert_eq!(failure.detail(), "exit code 66");
        assert_eq!(failure.to_string(), "other: exit code 66");
    }
}
