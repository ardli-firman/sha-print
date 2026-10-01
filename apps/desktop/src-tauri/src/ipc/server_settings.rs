//! IPC commands for configuring the server's Network Channel verifier.

use std::sync::Arc;

use tauri::State;

use crate::adapters::ipps::NetworkChannel;
use crate::ipc::dto::AppErrorDto;

/// Configures or replaces the server Network Channel and returns only safe configured status.
#[tauri::command]
pub async fn configure_network_channel(
    network_channel: String,
    channel: State<'_, Arc<NetworkChannel>>,
) -> Result<bool, AppErrorDto> {
    channel
        .configure(&network_channel)
        .await
        .map_err(|error| AppErrorDto::from(&error))
}

/// Reports whether a Network Channel verifier exists, never its value.
#[tauri::command]
pub fn get_network_channel_status(
    channel: State<'_, Arc<NetworkChannel>>,
) -> Result<bool, AppErrorDto> {
    Ok(channel.is_configured())
}
