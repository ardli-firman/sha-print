//! Tauri command and event for the servers a client can see on the local network.
//!
//! The list is a hint the UI offers for review; approving a server still goes through the same
//! commands as a manually entered address, because discovery proves nothing about identity
//! (ADR 0004).

use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::application::Discovery;
use crate::domain::NearbyServer;

/// The discovery cache the shell shares with the window.
pub type SharedDiscovery = Arc<Discovery>;

/// Event the shell emits whenever the nearby servers change.
pub const NEARBY_SERVERS_EVENT: &str = "discovery://servers";

/// One nearby server as the UI lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NearbyServerDto {
    /// Label the server advertises for itself.
    pub name: String,
    /// Address to review and approve before any printer is listed; also what tells two servers
    /// apart when they advertise the same label.
    pub address: String,
    /// The printers the server says it shares.
    pub printers: Vec<String>,
}

impl From<&NearbyServer> for NearbyServerDto {
    fn from(server: &NearbyServer) -> Self {
        Self {
            name: server.name().to_owned(),
            address: server.address().to_owned(),
            printers: server
                .printers()
                .iter()
                .map(|printer| printer.as_str().to_owned())
                .collect(),
        }
    }
}

/// Every nearby server the client can currently see.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NearbyServersDto {
    pub servers: Vec<NearbyServerDto>,
}

impl From<&[NearbyServer]> for NearbyServersDto {
    fn from(servers: &[NearbyServer]) -> Self {
        Self {
            servers: servers.iter().map(NearbyServerDto::from).collect(),
        }
    }
}

/// Lists the servers currently visible on the local network.
///
/// Reading the cache cannot fail: a discovery problem is reported as the discovery service's own
/// status, not as an error on every read.
#[tauri::command]
pub async fn list_nearby_servers(
    discovery: State<'_, SharedDiscovery>,
) -> Result<NearbyServersDto, crate::ipc::dto::AppErrorDto> {
    log::info!("command=list_nearby_servers");
    Ok(NearbyServersDto::from(
        discovery.nearby_servers().as_slice(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::PrinterName;

    #[test]
    fn nearby_server_payload_shape_is_stable() {
        let server = NearbyServer::new(
            "DESKTOP-ABC",
            "192.0.2.10:8631",
            vec![PrinterName::parse("Zebra").expect("valid printer name")],
        )
        .expect("a valid nearby server");

        let payload =
            serde_json::to_value(NearbyServersDto::from(&[server][..])).expect("serializes");

        assert_eq!(
            payload,
            serde_json::json!({
                "servers": [{
                    "name": "DESKTOP-ABC",
                    "address": "192.0.2.10:8631",
                    "printers": ["Zebra"]
                }]
            })
        );
    }

    #[test]
    fn event_name_matches_the_frontend_contract() {
        assert_eq!(NEARBY_SERVERS_EVENT, "discovery://servers");
    }
}
