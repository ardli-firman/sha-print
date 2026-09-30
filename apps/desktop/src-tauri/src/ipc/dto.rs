//! Serializable payloads exchanged with the frontend.
//!
//! These are the only shapes the UI sees; domain types stay free of serialization concerns. Ids,
//! states, and codes are stable strings (ADR 0002), and no payload carries credentials or print
//! job content.

use serde::{Deserialize, Serialize};

use crate::domain::{AppError, RuntimeStatus, ServiceStatus};

/// Event the shell emits whenever runtime status changes.
pub const RUNTIME_STATUS_EVENT: &str = "runtime://status";

/// Status of one supervised service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceStatusDto {
    /// Stable service id, for example `client-proxy`.
    pub id: String,
    /// Stable lifecycle state, for example `running`.
    pub state: String,
    /// Short note about the current state; never contains secrets or job content.
    pub detail: String,
}

impl From<&ServiceStatus> for ServiceStatusDto {
    fn from(status: &ServiceStatus) -> Self {
        Self {
            id: status.id().as_str().to_owned(),
            state: status.state().as_str().to_owned(),
            detail: status.detail().to_owned(),
        }
    }
}

/// Status of every service the shell supervises.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeStatusDto {
    pub services: Vec<ServiceStatusDto>,
}

impl From<&RuntimeStatus> for RuntimeStatusDto {
    fn from(status: &RuntimeStatus) -> Self {
        Self {
            services: status
                .services()
                .iter()
                .map(ServiceStatusDto::from)
                .collect(),
        }
    }
}

/// Failure returned to the frontend: a stable code plus a human-readable message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppErrorDto {
    /// Stable error code, for example `invalid-state`.
    pub code: String,
    pub message: String,
}

impl From<&AppError> for AppErrorDto {
    fn from(error: &AppError) -> Self {
        Self {
            code: error.code_str().to_owned(),
            message: error.message().to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ErrorCode, RuntimeStatus, ServiceId};

    fn status(id: ServiceId, state: crate::domain::ServiceState, detail: &str) -> ServiceStatus {
        let mut status = ServiceStatus::stopped(id);
        if state != crate::domain::ServiceState::Stopped {
            let _ = status.transition(crate::domain::ServiceState::Starting);
            let _ = status.transition(state);
        }
        status.set_detail(detail);
        status
    }

    #[test]
    fn runtime_status_payload_shape_is_stable() {
        let runtime = RuntimeStatus::new(vec![
            status(
                ServiceId::ClientProxy,
                crate::domain::ServiceState::Running,
                "running",
            ),
            status(
                ServiceId::ServerSharing,
                crate::domain::ServiceState::Stopped,
                "",
            ),
        ]);

        let payload = serde_json::to_value(RuntimeStatusDto::from(&runtime)).expect("serializes");
        assert_eq!(
            payload,
            serde_json::json!({
                "services": [
                    { "id": "client-proxy", "state": "running", "detail": "running" },
                    { "id": "server-sharing", "state": "stopped", "detail": "" }
                ]
            })
        );
    }

    #[test]
    fn error_payload_shape_is_stable() {
        let error = AppError::new(
            ErrorCode::InvalidState,
            "service 'server-sharing' is not running",
        );
        let payload = serde_json::to_value(AppErrorDto::from(&error)).expect("serializes");
        assert_eq!(
            payload,
            serde_json::json!({
                "code": "invalid-state",
                "message": "service 'server-sharing' is not running"
            })
        );
    }

    #[test]
    fn event_name_matches_the_frontend_contract() {
        assert_eq!(RUNTIME_STATUS_EVENT, "runtime://status");
    }
}
