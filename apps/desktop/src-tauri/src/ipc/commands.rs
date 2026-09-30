//! Tauri command adapters.
//!
//! Commands only validate/translate input, call one coordinator method, and map the outcome to a
//! DTO. Every command returns the full status snapshot so the UI can render the result of the
//! action immediately; log lines carry the command name, service id, and error code only.
//!
//! `start_service`/`stop_service` are the shell's generic lifecycle actions for every supervised
//! service; sharing-specific configuration (selected queues, advertisement) arrives with #31.

use std::sync::Arc;

use tauri::State;

use crate::application::RuntimeCoordinator;
use crate::domain::{AppError, ServiceId};
use crate::ipc::dto::{AppErrorDto, RuntimeStatusDto};

/// Runtime state shared by every command.
pub type SharedRuntime = Arc<RuntimeCoordinator>;

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
