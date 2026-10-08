//! Native queue installation through the Windows PrintManagement module.
//!
//! `Add-Printer -IppURL` creates the queue and its IPP port in one step, which is the same command
//! the client-proxy native smoke test uses (`tests/client_proxy_smoke.ps1`). ShaPrint only supplies
//! a queue name and a loopback URL, so no driver INF, no third-party installer, and no user input
//! reaches the command line.
//!
//! The values travel through the environment instead of the script text, so a printer name or
//! address can never change what PowerShell runs (ADR 0005). The script answers on standard output
//! with a stable token, and the helper turns that token into the exit code the app reads; the
//! localized Windows text stays on standard error for diagnosis.
//!
//! This module is compiled for Windows and under `test` on every platform, so its argument
//! construction, token parsing, and failure classification are verified without a Windows machine.
//! Only spawning the command and hiding its console window are Windows-specific.

use std::process::Command;

use crate::application::QueueInstaller;
use crate::domain::{AppError, ClientQueueRequest, SetupFailure, SetupFailureKind};

/// The PowerShell executable Windows ships.
pub const PROGRAM: &str = "powershell.exe";

/// Flags that keep the helper's PowerShell run unattended.
pub const ARGUMENTS: [&str; 3] = ["-NoProfile", "-NonInteractive", "-Command"];

/// Environment variables the script reads. Values never appear in the script text.
pub const QUEUE_NAME_VARIABLE: &str = "SHAPRINT_QUEUE_NAME";
pub const QUEUE_IPP_URL_VARIABLE: &str = "SHAPRINT_QUEUE_IPP_URL";

/// Prefix of the single stable line the script writes when it cannot create the queue.
pub const FAILURE_MARKER: &str = "SHAPRINT-FAIL";

/// Longest slice of PowerShell's own diagnostics the helper keeps.
const MAX_DETAIL: usize = 400;

/// The script that creates the queue, updates an older ShaPrint client queue, or reports a conflict.
///
/// When an existing queue's port points to an older ShaPrint loopback proxy URL (including legacy port
/// 8632 or legacy server port 8631), it automatically updates/replaces the queue with the new loopback
/// proxy port and target server address (ADR 0014). When an existing queue points to an unrelated
/// non-ShaPrint destination, it leaves the queue untouched and reports an existing-queue conflict.
pub const SCRIPT: &str = r#"$ErrorActionPreference = 'Stop'
if (-not (Get-Command -Name Add-Printer -ErrorAction SilentlyContinue)) {
    Write-Output 'SHAPRINT-FAIL unsupported'
    exit 1
}
$spooler = Get-Service -Name Spooler -ErrorAction SilentlyContinue
if ($null -eq $spooler -or $spooler.Status -ne 'Running') {
    Write-Output 'SHAPRINT-FAIL spooler-unavailable'
    exit 1
}
$name = $env:SHAPRINT_QUEUE_NAME
$url = $env:SHAPRINT_QUEUE_IPP_URL
if ([string]::IsNullOrWhiteSpace($name) -or [string]::IsNullOrWhiteSpace($url)) {
    Write-Output 'SHAPRINT-FAIL invalid-request'
    exit 1
}
try {
    $existing = Get-Printer -Name $name -ErrorAction SilentlyContinue
    if ($existing) {
        $port = "$($existing.PortName)"
        if ($port -eq $url -or $port -eq $url.Replace('ipp://', 'http://') -or $port -like 'WSD-*') {
            exit 0
        }
        if ($port -match '^(?i)(ipp|http)://(127\.0\.0\.1|localhost|\[::1\])(:\d+)?/ipp/print/') {
            Remove-Printer -Name $name -ErrorAction Stop
            Add-Printer -Name $name -IppURL $url -ErrorAction Stop
            exit 0
        }
        Write-Output 'SHAPRINT-FAIL existing-queue-conflict'
        [Console]::Error.WriteLine("the queue '$name' already exists and points at '$port'")
        exit 1
    }
    Add-Printer -Name $name -IppURL $url -ErrorAction Stop
} catch {
    Write-Output 'SHAPRINT-FAIL queue-rejected'
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
}
"#;

/// Creates native Windows queues through the spooler.
#[derive(Debug, Default)]
pub struct WindowsQueueInstaller;

impl WindowsQueueInstaller {
    pub fn new() -> Self {
        Self
    }
}

impl QueueInstaller for WindowsQueueInstaller {
    fn queue_uri(&self, request: &ClientQueueRequest) -> Result<String, AppError> {
        super::queue_uri(&super::proxy_authority(), request)
    }

    fn install(&self, request: &ClientQueueRequest) -> Result<(), SetupFailure> {
        let uri = super::queue_uri(&super::proxy_authority(), request).map_err(|error| {
            SetupFailure::new(SetupFailureKind::InvalidRequest, error.message())
        })?;

        let mut command = Command::new(PROGRAM);
        command
            .args(ARGUMENTS)
            .arg(SCRIPT)
            .env(QUEUE_NAME_VARIABLE, request.queue_name().as_str())
            .env(QUEUE_IPP_URL_VARIABLE, &uri);
        // A GUI helper must not flash a console window for the command it runs.
        hide_console(&mut command);

        let output = command.output().map_err(|error| {
            SetupFailure::new(
                SetupFailureKind::Other,
                format!("cannot run {PROGRAM}: {error}"),
            )
        })?;

        if let Some(kind) = reported_failure(&String::from_utf8_lossy(&output.stdout)) {
            return Err(SetupFailure::new(
                kind,
                detail(&String::from_utf8_lossy(&output.stderr)),
            ));
        }
        if output.status.success() {
            return Ok(());
        }
        Err(unreported_failure(
            output.status,
            &String::from_utf8_lossy(&output.stderr),
        ))
    }
}

/// The failure to report when the script produced no recognisable token.
///
/// PowerShell can die before it prints one — a syntactically fine script that hits an unexpected
/// terminating error still leaves its own explanation on standard error, and dropping it would leave
/// the caller with an exit code and nothing to diagnose.
fn unreported_failure(status: impl std::fmt::Display, stderr: &str) -> SetupFailure {
    SetupFailure::new(
        SetupFailureKind::Other,
        format!("{PROGRAM} exited with {status}: {}", detail(stderr)),
    )
}

/// Reads the failure kind the script reported on standard output.
///
/// Matching a token instead of the error text keeps the classification working on a Windows with
/// any display language.
pub fn reported_failure(stdout: &str) -> Option<SetupFailureKind> {
    stdout.lines().find_map(|line| {
        let rest = line.trim().strip_prefix(FAILURE_MARKER)?;
        SetupFailureKind::from_id(rest.trim())
    })
}

/// Trims PowerShell's diagnostics to something worth keeping.
fn detail(stderr: &str) -> String {
    let text = stderr.trim();
    if text.is_empty() {
        return "the setup command reported no detail".to_owned();
    }
    text.chars().take(MAX_DETAIL).collect()
}

/// Keeps PowerShell from opening a console window.
fn hide_console(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = command;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only the test that runs where `powershell.exe` is absent needs a request; on Windows every
    /// test works from the script text, and an unused helper would fail the lint gate.
    #[cfg(not(windows))]
    fn request() -> ClientQueueRequest {
        ClientQueueRequest::new(
            "10.0.0.5:8631",
            crate::domain::PrinterName::parse("Office Printer").expect("valid printer name"),
        )
        .expect("valid request")
    }

    #[test]
    fn the_script_reports_every_failure_kind_it_can_classify() {
        let reported: Vec<SetupFailureKind> = SCRIPT
            .lines()
            .filter_map(|line| {
                line.trim()
                    .strip_prefix("Write-Output '")
                    .and_then(|rest| rest.strip_suffix('\''))
                    .and_then(|token| token.strip_prefix(FAILURE_MARKER))
                    .and_then(|kind| SetupFailureKind::from_id(kind.trim()))
            })
            .collect();

        assert_eq!(
            reported,
            vec![
                SetupFailureKind::Unsupported,
                SetupFailureKind::SpoolerUnavailable,
                SetupFailureKind::InvalidRequest,
                SetupFailureKind::ExistingQueueConflict,
                SetupFailureKind::QueueRejected,
            ]
        );
    }

    #[test]
    fn the_script_creates_an_ipp_queue_and_reads_its_values_from_the_environment() {
        assert!(SCRIPT.contains("Add-Printer -Name $name -IppURL $url"));
        assert!(SCRIPT.contains(QUEUE_NAME_VARIABLE));
        assert!(SCRIPT.contains(QUEUE_IPP_URL_VARIABLE));
        // No user-supplied value is interpolated into the script text.
        assert!(!SCRIPT.contains("Office Printer"));
        assert_ne!(QUEUE_NAME_VARIABLE, QUEUE_IPP_URL_VARIABLE);
    }

    #[test]
    fn the_script_updates_older_shaprint_loopback_queue_and_rejects_unrelated_queue() {
        // Re-installing an existing queue pointing to an older ShaPrint loopback proxy port
        // replaces/updates the queue (ADR 0014), while unrelated non-ShaPrint queues report a conflict.
        let identical_check = SCRIPT
            .find("$port -eq $url")
            .expect("compares the existing destination for exact match");
        let loopback_match = SCRIPT
            .find("if ($port -match '^(?i)(ipp|http)://(127\\.0\\.0\\.1|localhost|\\[::1\\])(:\\d+)?/ipp/print/')")
            .expect("identifies ShaPrint loopback proxy destinations");
        let remove = SCRIPT
            .find("Remove-Printer -Name $name")
            .expect("removes older ShaPrint loopback queue");
        let re_add = SCRIPT
            .find("Add-Printer -Name $name -IppURL $url")
            .expect("re-creates updated queue");
        let conflict = SCRIPT
            .find("existing-queue-conflict")
            .expect("reports conflict for unrelated queues");

        assert!(identical_check < loopback_match);
        assert!(loopback_match < remove);
        assert!(remove < re_add);
        assert!(re_add < conflict);
    }

    #[test]
    fn a_reported_token_becomes_the_failure_kind() {
        for kind in SetupFailureKind::ALL {
            let stdout = format!("some noise\n{FAILURE_MARKER} {}\n", kind.id());
            assert_eq!(reported_failure(&stdout), Some(kind));
            let padded = format!("  {FAILURE_MARKER}\t{}\r\n", kind.id());
            assert_eq!(reported_failure(&padded), Some(kind));
        }
    }

    #[test]
    fn unrelated_output_is_not_a_failure() {
        assert_eq!(reported_failure(""), None);
        assert_eq!(reported_failure("Add-Printer completed\n"), None);
        assert_eq!(reported_failure("SHAPRINT-FAIL\n"), None);
        assert_eq!(reported_failure("SHAPRINT-FAIL not-a-kind\n"), None);
    }

    #[test]
    fn the_helper_keeps_only_a_bounded_slice_of_powershells_detail() {
        assert_eq!(detail("   "), "the setup command reported no detail");
        assert_eq!(detail(" boom \n"), "boom");
        let long = detail(&"x".repeat(MAX_DETAIL * 2));
        assert_eq!(long.chars().count(), MAX_DETAIL);
    }

    #[test]
    fn an_unclassified_exit_keeps_powershells_own_diagnostic() {
        // Without a token the exit code alone says nothing; the stderr text is the only clue left.
        let failure = unreported_failure(
            "exit code: 1",
            "  The term 'Add-Printer' is not recognized\n",
        );

        assert_eq!(failure.kind(), SetupFailureKind::Other);
        assert!(failure.detail().contains("exit code: 1"));
        assert!(failure
            .detail()
            .contains("The term 'Add-Printer' is not recognized"));
    }

    #[test]
    fn the_command_is_unattended() {
        assert_eq!(PROGRAM, "powershell.exe");
        assert!(ARGUMENTS.contains(&"-NonInteractive"));
        assert!(ARGUMENTS.contains(&"-NoProfile"));
    }

    /// On a machine without `powershell.exe` the failure is classified, not a panic or a hang.
    #[cfg(not(windows))]
    #[test]
    fn a_missing_powershell_is_reported_as_an_unclassified_failure() {
        let failure = WindowsQueueInstaller::new()
            .install(&request())
            .expect_err("no powershell on this platform");
        assert_eq!(failure.kind(), SetupFailureKind::Other);
        assert!(failure.detail().contains(PROGRAM));
    }
}
