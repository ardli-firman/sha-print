//! Tauri commands for the manual client server-review flow.

use crate::{
    adapters::client_connections::{
        ClientConnections, TrustedServer, TrustedServerProbe, TrustedServerStatus,
    },
    ipc::dto::AppErrorDto,
};
use serde::Serialize;
use tauri::State;

pub type SharedClientConnections = std::sync::Arc<ClientConnections>;
pub type SharedPrinterCatalog = std::sync::Arc<dyn crate::application::LocalPrinterCatalog>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecognisedClientQueueDto {
    pub queue_name: String,
    pub server_address: String,
    pub printer_name: String,
}

impl From<crate::domain::RecognisedClientQueue> for RecognisedClientQueueDto {
    fn from(q: crate::domain::RecognisedClientQueue) -> Self {
        Self {
            queue_name: q.queue_name().to_string(),
            server_address: q.server_address().to_string(),
            printer_name: q.printer_name().to_string(),
        }
    }
}

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

#[tauri::command]
pub async fn inspect_server_connection(
    address: String,
    connections: State<'_, SharedClientConnections>,
) -> Result<ServerConnectionReviewDto, AppErrorDto> {
    connections
        .inspect(&address)
        .await
        .map(ServerConnectionReviewDto::from)
        .map_err(|e| AppErrorDto::from(&e))
}
#[tauri::command]
pub async fn approve_server_connection(
    address: String,
    fingerprint: String,
    connections: State<'_, SharedClientConnections>,
) -> Result<ServerConnectionReviewDto, AppErrorDto> {
    connections
        .approve(&address, &fingerprint)
        .await
        .map(ServerConnectionReviewDto::from)
        .map_err(|e| AppErrorDto::from(&e))
}
#[tauri::command]
pub async fn list_server_connection_printers(
    address: String,
    connections: State<'_, SharedClientConnections>,
) -> Result<ServerConnectionPrintersDto, AppErrorDto> {
    connections
        .printers(&address)
        .await
        .map(ServerConnectionPrintersDto::from)
        .map_err(|e| AppErrorDto::from(&e))
}

#[tauri::command]
pub async fn list_trusted_servers(
    connections: State<'_, SharedClientConnections>,
) -> Result<Vec<TrustedServerDto>, AppErrorDto> {
    connections
        .list_trusted_servers()
        .await
        .map(|servers| servers.into_iter().map(TrustedServerDto::from).collect())
        .map_err(|e| AppErrorDto::from(&e))
}

#[tauri::command]
pub async fn probe_trusted_server(
    address: String,
    connections: State<'_, SharedClientConnections>,
) -> Result<TrustedServerProbeDto, AppErrorDto> {
    connections
        .probe_trusted_server(&address)
        .await
        .map(TrustedServerProbeDto::from)
        .map_err(|e| AppErrorDto::from(&e))
}

#[tauri::command]
pub async fn forget_trusted_server(
    address: String,
    connections: State<'_, SharedClientConnections>,
) -> Result<(), AppErrorDto> {
    connections
        .forget_trusted_server(&address)
        .await
        .map_err(|e| AppErrorDto::from(&e))
}

#[tauri::command]
pub async fn list_recognised_client_queues(
    catalog: State<'_, SharedPrinterCatalog>,
) -> Result<Vec<RecognisedClientQueueDto>, AppErrorDto> {
    catalog
        .recognised_client_queues()
        .await
        .map(|queues| {
            queues
                .into_iter()
                .map(RecognisedClientQueueDto::from)
                .collect()
        })
        .map_err(|e| AppErrorDto::from(&e))
}
