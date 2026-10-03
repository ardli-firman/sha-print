//! Service identity, lifecycle states, and the status snapshot the shell publishes to the UI.

use crate::domain::{sanitize_text, AppError, ErrorCode};

/// Longest status detail the shell publishes; longer text is truncated.
const DETAIL_LIMIT: usize = 120;

/// A background runtime the desktop shell owns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ServiceId {
    /// Local proxy that forwards print jobs from installed Windows queues to a trusted server.
    ClientProxy,
    /// IPPS sharing of the local printer queues the user selected.
    ServerSharing,
    /// Browsing the local network for servers that share printers with this client.
    ServerDiscovery,
}

impl ServiceId {
    /// Every service the shell supervises, in the order it reports them.
    pub const ALL: [ServiceId; 3] = [
        ServiceId::ClientProxy,
        ServiceId::ServerSharing,
        ServiceId::ServerDiscovery,
    ];

    /// The stable id used by IPC payloads, log lines, and the UI.
    pub const fn as_str(self) -> &'static str {
        match self {
            ServiceId::ClientProxy => "client-proxy",
            ServiceId::ServerSharing => "server-sharing",
            ServiceId::ServerDiscovery => "server-discovery",
        }
    }

    /// Parses an id received over IPC.
    pub fn parse(value: &str) -> Result<Self, AppError> {
        Self::ALL
            .into_iter()
            .find(|id| id.as_str() == value)
            .ok_or_else(|| {
                AppError::new(
                    ErrorCode::UnknownService,
                    format!("unknown service '{value}'"),
                )
            })
    }
}

/// Lifecycle state of a supervised service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ServiceState {
    /// Not started, or stopped after having run.
    Stopped,
    /// Startup is in progress and the service has not reported ready yet.
    Starting,
    /// The service reported ready and is doing its work.
    Running,
    /// Shutdown was requested and the shell is waiting for the task to end.
    Stopping,
    /// The service ended on its own or failed to start.
    Failed,
}

impl ServiceState {
    /// Every state, in lifecycle order.
    pub const ALL: [ServiceState; 5] = [
        ServiceState::Stopped,
        ServiceState::Starting,
        ServiceState::Running,
        ServiceState::Stopping,
        ServiceState::Failed,
    ];

    /// The stable state name used by IPC payloads and the UI.
    pub const fn as_str(self) -> &'static str {
        match self {
            ServiceState::Stopped => "stopped",
            ServiceState::Starting => "starting",
            ServiceState::Running => "running",
            ServiceState::Stopping => "stopping",
            ServiceState::Failed => "failed",
        }
    }

    /// Whether a task exists that the shell still has to stop.
    pub const fn is_live(self) -> bool {
        matches!(
            self,
            ServiceState::Starting | ServiceState::Running | ServiceState::Stopping
        )
    }
}

/// Legal lifecycle transitions. Anything else is a bug or a rejected user action.
const fn can_transition(from: ServiceState, to: ServiceState) -> bool {
    use ServiceState::{Failed, Running, Starting, Stopped, Stopping};
    matches!(
        (from, to),
        (Stopped, Starting)
            | (Starting, Running)
            | (Starting, Stopping)
            | (Starting, Failed)
            | (Running, Stopping)
            | (Running, Failed)
            | (Stopping, Stopped)
            | (Stopping, Failed)
            | (Failed, Starting)
    )
}

/// Status of one supervised service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceStatus {
    id: ServiceId,
    state: ServiceState,
    detail: String,
}

impl ServiceStatus {
    /// A service that has not been started yet.
    pub(crate) fn stopped(id: ServiceId) -> Self {
        Self {
            id,
            state: ServiceState::Stopped,
            detail: String::new(),
        }
    }

    pub fn id(&self) -> ServiceId {
        self.id
    }

    pub fn state(&self) -> ServiceState {
        self.state
    }

    /// Short human-readable note about the current state; never contains secrets or job content.
    pub fn detail(&self) -> &str {
        &self.detail
    }

    /// Applies `next` when the lifecycle allows it.
    pub(crate) fn transition(&mut self, next: ServiceState) -> Result<(), AppError> {
        if !can_transition(self.state, next) {
            return Err(AppError::invalid_state(format!(
                "service '{}' cannot move from {} to {}",
                self.id.as_str(),
                self.state.as_str(),
                next.as_str()
            )));
        }
        self.state = next;
        Ok(())
    }

    /// Replaces the detail, stripping control characters and truncating long text so a single
    /// misbehaving service cannot corrupt UI rendering or log lines.
    pub(crate) fn set_detail(&mut self, detail: &str) {
        self.detail = sanitize_text(detail, DETAIL_LIMIT);
    }
}

/// The status snapshots the shell publishes; ordered by [`ServiceId::ALL`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeStatus {
    services: Vec<ServiceStatus>,
}

impl RuntimeStatus {
    pub(crate) fn new(services: Vec<ServiceStatus>) -> Self {
        Self { services }
    }

    pub fn services(&self) -> &[ServiceStatus] {
        &self.services
    }

    pub fn service(&self, id: ServiceId) -> Option<&ServiceStatus> {
        self.services.iter().find(|status| status.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_ids_are_stable_and_parse_back() {
        let ids: Vec<&str> = ServiceId::ALL.iter().map(|id| id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["client-proxy", "server-sharing", "server-discovery"]
        );
        for id in ServiceId::ALL {
            assert_eq!(ServiceId::parse(id.as_str()).ok(), Some(id));
        }
    }

    #[test]
    fn unknown_service_id_is_rejected() {
        let error = ServiceId::parse("scanner").unwrap_err();
        assert_eq!(error.code(), ErrorCode::UnknownService);
        assert_eq!(error.message(), "unknown service 'scanner'");
    }

    #[test]
    fn state_names_are_stable() {
        let states: Vec<&str> = ServiceState::ALL
            .iter()
            .map(|state| state.as_str())
            .collect();
        assert_eq!(
            states,
            vec!["stopped", "starting", "running", "stopping", "failed"]
        );
    }

    #[test]
    fn lifecycle_allows_the_expected_transitions() {
        let mut status = ServiceStatus::stopped(ServiceId::ClientProxy);
        assert_eq!(status.state(), ServiceState::Stopped);

        status.transition(ServiceState::Starting).unwrap();
        status.transition(ServiceState::Running).unwrap();
        status.transition(ServiceState::Stopping).unwrap();
        status.transition(ServiceState::Stopped).unwrap();
        status.transition(ServiceState::Starting).unwrap();
        status.transition(ServiceState::Failed).unwrap();
        status.transition(ServiceState::Starting).unwrap();
    }

    #[test]
    fn illegal_transitions_are_rejected() {
        let mut status = ServiceStatus::stopped(ServiceId::ServerSharing);
        let error = status.transition(ServiceState::Stopped).unwrap_err();
        assert_eq!(error.code(), ErrorCode::InvalidState);
        assert_eq!(
            error.message(),
            "service 'server-sharing' cannot move from stopped to stopped"
        );

        status.transition(ServiceState::Starting).unwrap();
        let error = status.transition(ServiceState::Starting).unwrap_err();
        assert_eq!(error.code(), ErrorCode::InvalidState);

        status.transition(ServiceState::Running).unwrap();
        let error = status.transition(ServiceState::Stopped).unwrap_err();
        assert_eq!(error.code(), ErrorCode::InvalidState);
    }

    #[test]
    fn detail_strips_control_characters() {
        let mut status = ServiceStatus::stopped(ServiceId::ClientProxy);
        status.set_detail("started\r\nlistening\tnow");
        assert_eq!(status.detail(), "started  listening now");
    }

    #[test]
    fn detail_is_truncated_to_the_publish_limit() {
        let mut status = ServiceStatus::stopped(ServiceId::ClientProxy);
        status.set_detail(&"a".repeat(500));
        assert_eq!(status.detail().chars().count(), DETAIL_LIMIT + 1);
        assert!(status.detail().ends_with('…'));
    }

    #[test]
    fn runtime_status_looks_up_services_by_id() {
        let status = RuntimeStatus::new(vec![
            ServiceStatus::stopped(ServiceId::ClientProxy),
            ServiceStatus::stopped(ServiceId::ServerSharing),
        ]);
        assert_eq!(status.services().len(), 2);
        assert_eq!(
            status
                .service(ServiceId::ServerSharing)
                .map(ServiceStatus::state),
            Some(ServiceState::Stopped)
        );
    }
}
