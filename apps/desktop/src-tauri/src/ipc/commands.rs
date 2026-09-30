//! Tauri command adapters.
//!
//! Commands only validate/translate input, call one use case, and map the outcome to a DTO. Every
//! command returns the state its action produced so the UI can render the result immediately; log
//! lines carry the command name, ids, and error codes only — never printer job content, Network
//! Channel values, or key material.
//!
//! `start_service`/`stop_service` are the shell's generic lifecycle actions for every supervised
//! service. The sharing commands cover what a server user configures: which local queues are
//! shared, the identity clients approve, and the one setup action that needs administrator
//! permission.

use std::sync::Arc;

use tauri::State;

use crate::adapters::IppsServer;
use crate::application::{RuntimeCoordinator, Setup, SetupOutcome, Sharing};
use crate::domain::{AppError, PrinterName, ServiceId, SetupAction};
use crate::ipc::dto::{
    AppErrorDto, LocalPrintersDto, RuntimeStatusDto, ServerIdentityDto, SetupOutcomeDto,
};

/// Runtime state shared by every command.
pub type SharedRuntime = Arc<RuntimeCoordinator>;

/// Printer sharing configuration.
pub type SharedSharing = Arc<Sharing>;

/// The IPPS endpoint clients connect to.
pub type SharedEndpoint = Arc<IppsServer>;

/// Setup actions that may need administrator permission.
pub type SharedSetup = Arc<Setup>;

/// Returns the current status of every supervised service.
#[tauri::command]
pub async fn get_runtime_status(
    runtime: State<'_, SharedRuntime>,
) -> Result<RuntimeStatusDto, AppErrorDto> {
    log::info!("command=get_runtime_status");
    Ok(RuntimeStatusDto::from(&runtime.status()))
}

/// Starts one service and returns the resulting status.
#[tauri::command]
pub async fn start_service(
    id: String,
    runtime: State<'_, SharedRuntime>,
) -> Result<RuntimeStatusDto, AppErrorDto> {
    apply(&runtime, &id, Action::Start)
        .await
        .map_err(|error| AppErrorDto::from(&error))
}

/// Stops one service and returns the resulting status.
#[tauri::command]
pub async fn stop_service(
    id: String,
    runtime: State<'_, SharedRuntime>,
) -> Result<RuntimeStatusDto, AppErrorDto> {
    apply(&runtime, &id, Action::Stop)
        .await
        .map_err(|error| AppErrorDto::from(&error))
}

/// Lists the local printer queues and which of them the server shares.
#[tauri::command]
pub async fn list_local_printers(
    sharing: State<'_, SharedSharing>,
) -> Result<LocalPrintersDto, AppErrorDto> {
    log::info!("command=list_local_printers");
    match sharing.local_printers().await {
        Ok(printers) => Ok(LocalPrintersDto::from(printers.as_slice())),
        Err(error) => {
            log::warn!(
                "command=list_local_printers code={} message={}",
                error.code_str(),
                error
            );
            Err(AppErrorDto::from(&error))
        }
    }
}

/// Replaces the shared printer selection and returns the local queues with their new state.
#[tauri::command]
pub async fn set_shared_printers(
    printers: Vec<String>,
    sharing: State<'_, SharedSharing>,
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
    match sharing.set_shared(names).await {
        Ok(local) => {
            log::info!("command=set_shared_printers shared={count} code=ok");
            Ok(LocalPrintersDto::from(local.as_slice()))
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
#[tauri::command]
pub async fn get_server_identity(
    endpoint: State<'_, SharedEndpoint>,
) -> Result<ServerIdentityDto, AppErrorDto> {
    log::info!("command=get_server_identity");
    Ok(ServerIdentityDto {
        fingerprint: endpoint.fingerprint().to_string(),
        port: endpoint.port(),
    })
}

/// Lets clients reach the sharing endpoint through the Windows firewall.
///
/// The only action the shell asks the operating system for that needs administrator permission
/// (ADR 0001); everything else a user does while running the app stays unprivileged.
#[tauri::command]
pub async fn allow_sharing_access(
    setup: State<'_, SharedSetup>,
) -> Result<SetupOutcomeDto, AppErrorDto> {
    let setup = Arc::clone(setup.inner());
    // The prompt and the helper run for as long as the user takes to decide, so they stay off the
    // async runtime.
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        setup.request(SetupAction::AllowInboundSharing)
    })
    .await
    .map_err(|error| {
        AppErrorDto::from(&AppError::internal(format!(
            "the setup action did not finish: {error}"
        )))
    })?;

    match outcome {
        Ok(outcome) => {
            log::info!(
                "command=allow_sharing_access elevated={} code=ok",
                outcome == SetupOutcome::Elevated
            );
            Ok(SetupOutcomeDto {
                elevated: outcome == SetupOutcome::Elevated,
            })
        }
        Err(error) => {
            log::warn!(
                "command=allow_sharing_access code={} message={}",
                error.code_str(),
                error
            );
            Err(AppErrorDto::from(&error))
        }
    }
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
    runtime: &RuntimeCoordinator,
    id: &str,
    action: Action,
) -> Result<RuntimeStatusDto, AppError> {
    let outcome = async {
        let service = ServiceId::parse(id)?;
        match action {
            Action::Start => runtime.start(service).await,
            Action::Stop => runtime.stop(service).await,
        }
    }
    .await;

    match &outcome {
        Ok(()) => log::info!("command={} service_id={} code=ok", action.as_str(), id),
        Err(error) => log::warn!(
            "command={} service_id={} code={}",
            action.as_str(),
            id,
            error.code_str()
        ),
    }

    outcome?;
    Ok(RuntimeStatusDto::from(&runtime.status()))
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
