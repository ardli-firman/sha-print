//! IPC commands for configuring the machine's Network Channel verifier.

use tauri::State;

use crate::ipc::control::SharedControl;
use crate::ipc::dto::AppErrorDto;

/// Configures or replaces the Network Channel and returns only safe configured status.
///
/// Machine mode passes the value to the elevated CLI helper on its standard input under the
/// `org.shaprint.control.network-channel` polkit action; declining it keeps the old channel.
#[tauri::command]
pub async fn configure_network_channel(
    network_channel: String,
    control: State<'_, SharedControl>,
) -> Result<bool, AppErrorDto> {
    control
        .configure_network_channel(&network_channel)
        .await
        .map_err(|error| AppErrorDto::from(&error))
}

/// Reports whether a Network Channel verifier exists, never its value.
#[tauri::command]
pub async fn get_network_channel_status(
    control: State<'_, SharedControl>,
) -> Result<bool, AppErrorDto> {
    control
        .network_channel_configured()
        .await
        .map_err(|error| AppErrorDto::from(&error))
}
