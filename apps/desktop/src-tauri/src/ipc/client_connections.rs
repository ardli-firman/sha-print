//! Tauri commands for the manual client server-review flow.

use crate::{
    adapters::client_connections::{TrustedServer, TrustedServerProbe, TrustedServerStatus},
    ipc::control::SharedControl,
    ipc::dto::AppErrorDto,
};
use serde::Serialize;
use tauri::State;

pub use crate::ipc::dto::RecognisedClientQueueDto;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ServerConnectionReviewDto {
    pub address: String,
    pub current_fingerprint: String,
    pub previous_fingerprint: Option<String>,
    pub trusted: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ServerConnectionPrintersDto {
    pub address: String,
    pub printers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TrustedServerDto {
    pub address: String,
    pub fingerprint: String,
}

impl From<TrustedServer> for TrustedServerDto {
    fn from(server: TrustedServer) -> Self {
        Self {
            address: server.address,
            fingerprint: server.fingerprint,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustedServerStatusDto {
    Online,
    Offline,
    IdentityChanged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TrustedServerProbeDto {
    pub address: String,
    pub status: TrustedServerStatusDto,
    pub approved_fingerprint: String,
    pub current_fingerprint: Option<String>,
    pub printers: Vec<String>,
}

impl From<TrustedServerProbe> for TrustedServerProbeDto {
    fn from(probe: TrustedServerProbe) -> Self {
        Self {
            address: probe.address,
            status: match probe.status {
                TrustedServerStatus::Online => TrustedServerStatusDto::Online,
                TrustedServerStatus::Offline => TrustedServerStatusDto::Offline,
                TrustedServerStatus::IdentityChanged => TrustedServerStatusDto::IdentityChanged,
            },
            approved_fingerprint: probe.approved_fingerprint,
            current_fingerprint: probe.current_fingerprint,
            printers: probe.printers,
        }
    }
}

impl From<crate::adapters::client_connections::ConnectionReview> for ServerConnectionReviewDto {
    fn from(v: crate::adapters::client_connections::ConnectionReview) -> Self {
        Self {
            address: v.address,
            current_fingerprint: v.current_fingerprint,
            previous_fingerprint: v.previous_fingerprint,
            trusted: v.trusted,
        }
    }
}
impl From<crate::adapters::client_connections::ConnectionPrinters> for ServerConnectionPrintersDto {
    fn from(v: crate::adapters::client_connections::ConnectionPrinters) -> Self {
        Self {
            address: v.address,
            printers: v.printers,
        }
    }
}

/// Reviews a Server's certificate fingerprint before any printer query.
///
/// Machine mode reads this from the daemon over its local socket: the approval under review is
/// the machine's trust store, not a per-window copy.
#[tauri::command]
pub async fn inspect_server_connection(
    address: String,
    control: State<'_, SharedControl>,
) -> Result<ServerConnectionReviewDto, AppErrorDto> {
    control
        .inspect_server(&address)
        .await
        .map_err(|e| AppErrorDto::from(&e))
}

/// Approves (or reapproves) a Server's certificate fingerprint.
///
/// Machine mode runs this through the elevated CLI helper under the
/// `org.shaprint.control.trust` polkit action; declining it leaves the trust store unchanged.
#[tauri::command]
pub async fn approve_server_connection(
    address: String,
    fingerprint: String,
    control: State<'_, SharedControl>,
) -> Result<ServerConnectionReviewDto, AppErrorDto> {
    control
        .approve_server(&address, &fingerprint)
        .await
        .map_err(|e| AppErrorDto::from(&e))
}

/// Lists the shared printers of an explicitly trusted Server.
#[tauri::command]
pub async fn list_server_connection_printers(
    address: String,
    control: State<'_, SharedControl>,
) -> Result<ServerConnectionPrintersDto, AppErrorDto> {
    control
        .list_server_printers(&address)
        .await
        .map_err(|e| AppErrorDto::from(&e))
}

/// Lists every machine-wide Trusted Server.
#[tauri::command]
pub async fn list_trusted_servers(
    control: State<'_, SharedControl>,
) -> Result<Vec<TrustedServerDto>, AppErrorDto> {
    control
        .list_trusted_servers()
        .await
        .map_err(|e| AppErrorDto::from(&e))
}

/// Probes a Trusted Server over unicast TLS without changing any approval.
#[tauri::command]
pub async fn probe_trusted_server(
    address: String,
    control: State<'_, SharedControl>,
) -> Result<TrustedServerProbeDto, AppErrorDto> {
    control
        .probe_server(&address)
        .await
        .map_err(|e| AppErrorDto::from(&e))
}

/// Removes a Server from the machine Trusted Servers.
#[tauri::command]
pub async fn forget_trusted_server(
    address: String,
    control: State<'_, SharedControl>,
) -> Result<(), AppErrorDto> {
    control
        .forget_server(&address)
        .await
        .map_err(|e| AppErrorDto::from(&e))
}

/// Lists the recognised ShaPrint Client queues installed in the local spooler.
#[tauri::command]
pub async fn list_recognised_client_queues(
    control: State<'_, SharedControl>,
) -> Result<Vec<RecognisedClientQueueDto>, AppErrorDto> {
    control
        .list_recognised_client_queues()
        .await
        .map_err(|e| AppErrorDto::from(&e))
}
