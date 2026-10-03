//! Tauri commands for the manual client server-review flow.

use crate::{adapters::client_connections::ClientConnections, ipc::dto::AppErrorDto};
use serde::Serialize;
use tauri::State;

pub type SharedClientConnections = std::sync::Arc<ClientConnections>;

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
