//! Tauri command adapters.
//!
//! Commands only validate/translate input, call one use case, and map the outcome to a DTO. Every
//! command returns the state its action produced so the UI can render the result immediately; log
//! lines carry the command name, ids, and error codes only — never printer job content, Network
//! Channel values, or key material.
//!
//! `start_service`/`stop_service` are the shell's generic lifecycle actions for every supervised
//! service. The sharing commands cover what a server user configures: which local queues are
//! shared and the identity clients approve. Inbound access is checked inside Server Sharing start.
//! `install_printer_queue` is the client counterpart: it creates the Windows queue a shared printer
//! prints through (issue #35).

use std::sync::Arc;

use tauri::State;

use crate::application::{LegacyImport, PrintFailures, PrintJobTracker, Startup};
use crate::domain::{AppError, PrinterName, ServiceId};
use crate::ipc::control::{ControlMode, ControlPlane, SharedControl};
use crate::ipc::dto::{
    AppErrorDto, ClientQueueDto, DiagnosticsReportDto, LegacyImportReportDto, LocalPrintersDto,
    PrintFailureDto, RuntimeStatusDto, ServerIdentityDto, StartupStatusDto,
};

/// The latest failed print attempt, as the window reports it.
pub type SharedPrintFailures = Arc<PrintFailures>;

/// Reads local queue evidence only. Errors carry a stable code and no native raw text.
#[tauri::command]
pub async fn inspect_local_client_queue(
    control: State<'_, SharedControl>,
    target: crate::ipc::dto::RecognisedClientQueueDto,
) -> Result<crate::ipc::dto::LocalTroubleshootingDto, AppErrorDto> {
    control
        .inspect_local_client_queue(target)
        .await
        .map_err(|error| AppErrorDto {
            code: error.code_str().to_owned(),
            message: "Local queue checks could not be completed. Check ShaPrint and try again."
                .to_owned(),
        })
}

/// Session actions return only classified evidence, never driver text or page bytes.
#[tauri::command]
pub async fn refresh_test_print_session(
    control: State<'_, SharedControl>,
    session_id: u64,
) -> Result<crate::ipc::dto::LocalTroubleshootingDto, AppErrorDto> {
    control
        .refresh_test_print_session(session_id)
        .await
        .map_err(|error| AppErrorDto::from(&error))
}
#[tauri::command]
pub async fn send_test_print(
    control: State<'_, SharedControl>,
    session_id: u64,
) -> Result<crate::ipc::dto::LocalTroubleshootingDto, AppErrorDto> {
    control
        .send_test_print(session_id)
        .await
        .map_err(|error| AppErrorDto::from(&error))
}
#[tauri::command]
pub fn confirm_test_print(
    control: State<'_, SharedControl>,
    session_id: u64,
    result: String,
) -> Result<crate::ipc::dto::LocalTroubleshootingDto, AppErrorDto> {
    control
        .confirm_test_print(session_id, &result)
        .map_err(|error| AppErrorDto::from(&error))
}
#[tauri::command]
pub fn close_test_print_session(
    control: State<'_, SharedControl>,
    session_id: u64,
) -> Result<(), AppErrorDto> {
    control
        .close_test_print_session(session_id)
        .map_err(|error| AppErrorDto::from(&error))
}

/// Whether ShaPrint starts with Windows, and the one action that changes it.
pub type SharedStartup = Arc<Startup>;

/// Moving supported settings from the previous .NET ShaPrint application.
pub type SharedLegacyImport = Arc<LegacyImport>;

/// In-flight print job tracker and drain guard.
pub type SharedPrintJobTracker = Arc<PrintJobTracker>;

/// Current drain guard and restart readiness status.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DrainStatusDto {
    pub active_jobs: usize,
    pub is_restart_safe: bool,
    pub is_idle_for_quiet_window: bool,
}

/// Returns whether restart is safe and whether in-flight print jobs are active.
#[tauri::command]
pub async fn get_drain_status(
    tracker: State<'_, SharedPrintJobTracker>,
) -> Result<DrainStatusDto, AppErrorDto> {
    log::info!("command=get_drain_status");
    Ok(DrainStatusDto {
        active_jobs: tracker.active_count(),
        is_restart_safe: tracker.is_restart_safe(),
        is_idle_for_quiet_window: tracker.is_idle_for_quiet_window(),
    })
}

/// Returns the current status of every supervised service.
///
/// On Linux this is the machine daemon's status, read over its local socket; on Windows it is
/// the in-process runtime this window owns.
#[tauri::command]
pub async fn get_runtime_status(
    control: State<'_, SharedControl>,
) -> Result<RuntimeStatusDto, AppErrorDto> {
    log::info!("command=get_runtime_status");
    control.status().await.map_err(|error| {
        log::warn!(
            "command=get_runtime_status code={} message={}",
            error.code_str(),
            error
        );
        AppErrorDto::from(&error)
    })
}

/// Returns the diagnostic report across daemon, ports, CUPS, trust, credential, and firewall.
#[tauri::command]
pub async fn get_diagnostics(
    control: State<'_, SharedControl>,
) -> Result<DiagnosticsReportDto, AppErrorDto> {
    log::info!("command=get_diagnostics");
    control.diagnostics().await.map_err(|error| {
        log::warn!(
            "command=get_diagnostics code={} message={}",
            error.code_str(),
            error
        );
        AppErrorDto::from(&error)
    })
}

/// Returns the latest print failure, or `null` when none is outstanding.
#[tauri::command]
pub async fn get_print_failures(
    failures: State<'_, SharedPrintFailures>,
) -> Result<Option<PrintFailureDto>, AppErrorDto> {
    log::info!("command=get_print_failures");
    Ok(failures.latest().as_ref().map(PrintFailureDto::from))
}

/// Dismisses the latest print failure and returns the resulting surface.
#[tauri::command]
pub async fn dismiss_print_failure(
    failures: State<'_, SharedPrintFailures>,
) -> Result<Option<PrintFailureDto>, AppErrorDto> {
    log::info!("command=dismiss_print_failure");
    failures.clear();
    Ok(failures.latest().as_ref().map(PrintFailureDto::from))
}

/// Starts one service and returns the resulting status.
///
/// In machine mode this asks the daemon over its socket's elevated CLI helper, so the polkit
/// decision is made by the same rules `shaprintctl start` runs under.
#[tauri::command]
pub async fn start_service(
    id: String,
    control: State<'_, SharedControl>,
) -> Result<RuntimeStatusDto, AppErrorDto> {
    apply(&control, &id, Action::Start)
        .await
        .map_err(|error| AppErrorDto::from(&error))
}

/// Stops one service and returns the resulting status.
#[tauri::command]
pub async fn stop_service(
    id: String,
    control: State<'_, SharedControl>,
) -> Result<RuntimeStatusDto, AppErrorDto> {
    apply(&control, &id, Action::Stop)
        .await
        .map_err(|error| AppErrorDto::from(&error))
}

/// Reports what the previous ShaPrint app left, and what this app took from it.
///
/// The last import this app ran wins; before any import this describes what one would do. No
/// payload carries the Network Channel itself.
#[tauri::command]
pub async fn get_legacy_import_report(
    legacy: State<'_, SharedLegacyImport>,
) -> Result<LegacyImportReportDto, AppErrorDto> {
    let legacy = Arc::clone(legacy.inner());
    // Reading the previous app's files, and opening its protected channel, are synchronous Windows
    // calls, so they stay off the async runtime.
    let report = tauri::async_runtime::spawn_blocking(move || match legacy.report() {
        Some(report) => Ok(report),
        None => legacy.inspect(),
    })
    .await
    .map_err(|error| {
        AppErrorDto::from(&AppError::internal(format!(
            "the previous app's settings could not be read: {error}"
        )))
    })?
    .map_err(|error| AppErrorDto::from(&error))?;

    log::info!(
        "command=get_legacy_import_report found={} channel={}",
        report.found,
        report.channel.as_str()
    );
    Ok(LegacyImportReportDto::from(&report))
}

/// Runs the import again and returns what it did. Importing twice changes nothing.
#[tauri::command]
pub async fn import_legacy_settings(
    legacy: State<'_, SharedLegacyImport>,
) -> Result<LegacyImportReportDto, AppErrorDto> {
    let legacy = Arc::clone(legacy.inner());
    let report = legacy.apply().await.map_err(|error| {
        log::warn!(
            "command=import_legacy_settings code={} message={}",
            error.code_str(),
            error
        );
        AppErrorDto::from(&error)
    })?;
    log::info!(
        "command=import_legacy_settings found={} channel={}",
        report.found,
        report.channel.as_str()
    );
    Ok(LegacyImportReportDto::from(&report))
}

/// Lists the local printer queues and which of them the server shares.
#[tauri::command]
pub async fn list_local_printers(
    control: State<'_, SharedControl>,
) -> Result<LocalPrintersDto, AppErrorDto> {
    log::info!("command=list_local_printers");
    control.local_printers().await.map_err(|error| {
        log::warn!(
            "command=list_local_printers code={} message={}",
            error.code_str(),
            error
        );
        AppErrorDto::from(&error)
    })
}

/// Replaces the shared printer selection and returns the local queues with their new state.
///
/// In machine mode the selection change runs through the elevated CLI helper under the
/// `org.shaprint.control.sharing` polkit action; declining it leaves the selection untouched.
#[tauri::command]
pub async fn set_shared_printers(
    printers: Vec<String>,
    control: State<'_, SharedControl>,
) -> Result<LocalPrintersDto, AppErrorDto> {
    let names = parse_names(&printers).map_err(|error| {
        log::warn!(
            "command=set_shared_printers code={} message={}",
            error.code_str(),
            error
        );
        AppErrorDto::from(&error)
    })?;

    let count = names.len();
    match control.set_shared_printers(names).await {
        Ok(local) => {
            log::info!("command=set_shared_printers shared={count} code=ok");
            Ok(local)
        }
        Err(error) => {
            log::warn!(
                "command=set_shared_printers code={} message={}",
                error.code_str(),
                error
            );
            Err(AppErrorDto::from(&error))
        }
    }
}

/// Returns the certificate fingerprint clients approve and the port they connect to.
///
/// In machine mode this is the *daemon's* Server identity, because that is the endpoint remote
/// clients actually reach — never this window's local certificate.
#[tauri::command]
pub async fn get_server_identity(
    control: State<'_, SharedControl>,
) -> Result<ServerIdentityDto, AppErrorDto> {
    log::info!("command=get_server_identity");
    control.server_identity().await.map_err(|error| {
        log::warn!(
            "command=get_server_identity code={} message={}",
            error.code_str(),
            error
        );
        AppErrorDto::from(&error)
    })
}

/// Reports whether ShaPrint starts with this user's Windows login.
#[tauri::command]
pub async fn get_startup_status(
    startup: State<'_, SharedStartup>,
) -> Result<StartupStatusDto, AppErrorDto> {
    let startup = Arc::clone(startup.inner());
    // Reading the registration is a synchronous Windows API, so it stays off the async runtime.
    let status = tauri::async_runtime::spawn_blocking(move || startup.status())
        .await
        .map_err(|error| {
            AppErrorDto::from(&AppError::internal(format!(
                "the startup check did not finish: {error}"
            )))
        })?;
    log::info!(
        "command=get_startup_status supported={} enabled={}",
        status.supported,
        status.enabled
    );
    Ok(StartupStatusDto::from(&status))
}

/// Turns starting with Windows on or off and returns the resulting state.
#[tauri::command]
pub async fn set_startup_enabled(
    enabled: bool,
    startup: State<'_, SharedStartup>,
) -> Result<StartupStatusDto, AppErrorDto> {
    let startup = Arc::clone(startup.inner());
    // The registration write is a synchronous Windows API, so it stays off the async runtime.
    let outcome = tauri::async_runtime::spawn_blocking(move || startup.set_enabled(enabled))
        .await
        .map_err(|error| {
            AppErrorDto::from(&AppError::internal(format!(
                "the startup change did not finish: {error}"
            )))
        })?;

    match outcome {
        Ok(status) => {
            log::info!("command=set_startup_enabled enabled={}", status.enabled);
            Ok(StartupStatusDto::from(&status))
        }
        Err(error) => {
            log::warn!(
                "command=set_startup_enabled code={} message={}",
                error.code_str(),
                error
            );
            Err(AppErrorDto::from(&error))
        }
    }
}

/// Installs the native queue for a printer a trusted server shares.
///
/// The use case checks every precondition — an approved server that currently shares the printer,
/// and a running local proxy — before the one elevation this needs (UAC on Windows, the
/// `org.shaprint.control.queue` polkit action in machine mode).
#[tauri::command]
pub async fn install_printer_queue(
    server_address: String,
    printer_name: String,
    control: State<'_, SharedControl>,
) -> Result<ClientQueueDto, AppErrorDto> {
    match control
        .install_client_queue(&server_address, &printer_name)
        .await
    {
        Ok(queue) => {
            log::info!(
                "command=install_printer_queue queue={} code=ok",
                queue.queue_name
            );
            Ok(queue)
        }
        Err(error) => {
            log::warn!(
                "command=install_printer_queue code={} message={}",
                error.code_str(),
                error
            );
            Err(AppErrorDto::from(&error))
        }
    }
}

/// Recreates an existing native queue so it reloads the Server's IPP capabilities.
#[tauri::command]
pub async fn repair_printer_queue(
    server_address: String,
    printer_name: String,
    control: State<'_, SharedControl>,
) -> Result<ClientQueueDto, AppErrorDto> {
    match control
        .repair_client_queue(&server_address, &printer_name)
        .await
    {
        Ok(queue) => {
            log::info!(
                "command=repair_printer_queue queue={} code=ok",
                queue.queue_name
            );
            Ok(queue)
        }
        Err(error) => {
            log::warn!(
                "command=repair_printer_queue code={} message={}",
                error.code_str(),
                error
            );
            Err(AppErrorDto::from(&error))
        }
    }
}

/// Reports which control plane this window administers: `local` (the in-process runtimes owned
/// by this process) or `machine` (the `shaprintd` system daemon over its local socket).
#[tauri::command]
pub async fn get_control_mode(
    control: State<'_, SharedControl>,
) -> Result<ControlMode, AppErrorDto> {
    let mode = control.mode();
    log::info!("command=get_control_mode mode={}", mode.as_str());
    Ok(mode)
}

/// Stops every machine service and the daemon itself; machine control plane only.
///
/// Runs through the elevated CLI helper under the `org.shaprint.control.stop` polkit action, the
/// same rule `shaprintctl stop` authorizes under. Declining it leaves the daemon running.
#[tauri::command]
pub async fn shutdown_daemon(
    control: State<'_, SharedControl>,
) -> Result<RuntimeStatusDto, AppErrorDto> {
    log::info!("command=shutdown_daemon");
    control.shutdown_daemon().await.map_err(|error| {
        log::warn!(
            "command=shutdown_daemon code={} message={}",
            error.code_str(),
            error
        );
        AppErrorDto::from(&error)
    })
}

/// Opens Windows Printers & scanners settings or control panel through a constrained desktop action.
#[tauri::command]
pub async fn open_printers_settings() -> Result<(), AppErrorDto> {
    tauri::async_runtime::spawn_blocking(|| {
        #[cfg(windows)]
        {
            use std::process::Command;
            // First try modern Windows Settings uri
            let modern = Command::new("cmd")
                .args(["/c", "start", "ms-settings:printers"])
                .status();
            if let Ok(status) = modern {
                if status.success() {
                    return Ok(());
                }
            }
            // Fallback to classic control panel printers folder
            let classic = Command::new("control.exe")
                .arg("printers")
                .status()
                .map_err(|e| AppError::internal(format!("cannot open Windows printers: {e}")))?;
            if classic.success() {
                Ok(())
            } else {
                Err(AppError::internal("cannot open Windows printers dialog"))
            }
        }
        #[cfg(not(windows))]
        {
            Err(AppError::unsupported(
                "Windows settings is only available on Windows",
            ))
        }
    })
    .await
    .map_err(|e| AppErrorDto::from(&AppError::internal(format!("task panicked: {e}"))))?
    .map_err(|e| AppErrorDto::from(&e))
}

/// Turns IPC strings into validated printer names.
fn parse_names(printers: &[String]) -> Result<Vec<PrinterName>, AppError> {
    printers
        .iter()
        .map(|name| PrinterName::parse(name))
        .collect()
}

#[derive(Debug, Clone, Copy)]
enum Action {
    Start,
    Stop,
}

impl Action {
    const fn as_str(self) -> &'static str {
        match self {
            Action::Start => "start_service",
            Action::Stop => "stop_service",
        }
    }
}

async fn apply(
    control: &ControlPlane,
    id: &str,
    action: Action,
) -> Result<RuntimeStatusDto, AppError> {
    let outcome = async {
        let service = ServiceId::parse(id)?;
        match action {
            Action::Start => control.start_service(service).await,
            Action::Stop => control.stop_service(service).await,
        }
    }
    .await;

    match &outcome {
        Ok(_) => log::info!("command={} service_id={} code=ok", action.as_str(), id),
        Err(error) => log::warn!(
            "command={} service_id={} code={}",
            action.as_str(),
            id,
            error.code_str()
        ),
    }

    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ErrorCode;

    #[test]
    fn printer_names_from_the_ui_are_validated() {
        assert_eq!(
            parse_names(&["HP LaserJet".to_owned()])
                .expect("valid")
                .len(),
            1
        );
        assert!(parse_names(&[]).expect("empty selection").is_empty());

        let error = parse_names(&["  ".to_owned()]).expect_err("rejected");
        assert_eq!(error.code(), ErrorCode::InvalidInput);
    }
}
