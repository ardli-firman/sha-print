//! The setup use case: request administrator permission exactly where the domain says it is
//! required, and turn the helper's classified failure into a message the user can act on.

use std::sync::Arc;

use crate::domain::{AppError, ClientQueueRequest, SetupAction, SetupFailure};

/// Runs a configuration action with administrator rights.
///
/// The adapter is platform-specific: Windows raises a UAC prompt for the action, other platforms
/// report that they cannot (ADR 0001 keeps Linux a later phase).
pub trait ElevationBroker: Send + Sync + 'static {
    /// Performs `action` elevated. Callers check [`SetupAction::requires_elevation`] first.
    fn elevate(&self, action: SetupAction) -> Result<(), SetupFailure>;

    /// Installs (or repairs) the native Windows queue for `request` elevated.
    ///
    /// Queue installation always changes machine-wide spooler state, so it is its own operation
    /// rather than a parameterless action: the helper needs the queue name and its destination.
    fn install_queue(&self, request: &ClientQueueRequest) -> Result<(), SetupFailure>;
}

/// What the shell did for a configuration action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupOutcome {
    /// The action ran as the logged-in user; no permission was requested.
    Routine,
    /// Administrator permission was requested and the action ran elevated.
    Elevated,
}

/// Routes configuration actions to the elevation broker, but only the ones that need it.
pub struct Setup {
    broker: Arc<dyn ElevationBroker>,
}

impl Setup {
    pub fn new(broker: Arc<dyn ElevationBroker>) -> Self {
        Self { broker }
    }

    /// Performs `action`, requesting administrator permission only when the action changes
    /// machine-wide state.
    pub fn request(&self, action: SetupAction) -> Result<SetupOutcome, AppError> {
        if !action.requires_elevation() {
            return Ok(SetupOutcome::Routine);
        }

        log::info!("requesting elevation action={}", action.as_str());
        self.broker.elevate(action).map_err(|failure| {
            log::warn!(
                "elevated action failed action={} reason={}",
                action.as_str(),
                failure.kind().id()
            );
            failure.for_action(action)
        })?;
        Ok(SetupOutcome::Elevated)
    }

    /// Installs (or repairs) the native Windows queue for `request`.
    ///
    /// A spooler queue is machine-wide state, so [`SetupAction::InstallPrinter`] is classified as
    /// requiring elevation and this call always prompts.
    pub fn install_queue(&self, request: &ClientQueueRequest) -> Result<(), AppError> {
        log::info!(
            "requesting elevation action={}",
            SetupAction::InstallPrinter.as_str()
        );
        self.broker.install_queue(request).map_err(|failure| {
            log::warn!(
                "queue install failed queue={} reason={}",
                request.queue_name(),
                failure.kind().id()
            );
            failure.for_queue_install(request)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ClientQueueName, ErrorCode, PrinterName, SetupFailureKind};
    use std::sync::Mutex;

    /// Records what it was asked to elevate.
    #[derive(Default)]
    struct RecordingBroker {
        elevated: Mutex<Vec<SetupAction>>,
        installed: Mutex<Vec<ClientQueueRequest>>,
        outcome: Mutex<Option<SetupFailureKind>>,
    }

    impl RecordingBroker {
        fn with_failure(kind: SetupFailureKind) -> Self {
            Self {
                outcome: Mutex::new(Some(kind)),
                ..Self::default()
            }
        }

        fn elevated(&self) -> Vec<SetupAction> {
            self.elevated
                .lock()
                .map(|actions| actions.clone())
                .unwrap_or_default()
        }

        fn installed(&self) -> Vec<ClientQueueRequest> {
            self.installed
                .lock()
                .map(|requests| requests.clone())
                .unwrap_or_default()
        }

        fn refusal(&self) -> Option<SetupFailure> {
            self.outcome
                .lock()
                .ok()
                .and_then(|kind| *kind)
                .map(|kind| SetupFailure::new(kind, "recorded failure"))
        }
    }

    impl ElevationBroker for RecordingBroker {
        fn elevate(&self, action: SetupAction) -> Result<(), SetupFailure> {
            if let Some(failure) = self.refusal() {
                return Err(failure);
            }
            self.elevated
                .lock()
                .map_err(|_| SetupFailure::new(SetupFailureKind::Other, "broker lock is poisoned"))?
                .push(action);
            Ok(())
        }

        fn install_queue(&self, request: &ClientQueueRequest) -> Result<(), SetupFailure> {
            if let Some(failure) = self.refusal() {
                return Err(failure);
            }
            self.installed
                .lock()
                .map_err(|_| SetupFailure::new(SetupFailureKind::Other, "broker lock is poisoned"))?
                .push(request.clone());
            Ok(())
        }
    }

    fn queue_request() -> ClientQueueRequest {
        ClientQueueRequest::new(
            "10.0.0.5:8631",
            PrinterName::parse("Office Printer").expect("valid printer name"),
        )
        .expect("valid request")
    }

    #[test]
    fn routine_configuration_never_asks_for_administrator_permission() {
        let broker = Arc::new(RecordingBroker::default());
        let setup = Setup::new(Arc::clone(&broker) as Arc<dyn ElevationBroker>);

        for action in SetupAction::ALL
            .into_iter()
            .filter(|action| !action.requires_elevation())
        {
            assert_eq!(
                setup.request(action).expect("routine action"),
                SetupOutcome::Routine
            );
        }

        assert!(broker.elevated().is_empty());
        assert!(broker.installed().is_empty());
    }

    #[test]
    fn machine_wide_configuration_asks_for_administrator_permission() {
        let broker = Arc::new(RecordingBroker::default());
        let setup = Setup::new(Arc::clone(&broker) as Arc<dyn ElevationBroker>);

        assert_eq!(
            setup
                .request(SetupAction::AllowInboundSharing)
                .expect("elevated action"),
            SetupOutcome::Elevated
        );
        assert_eq!(broker.elevated(), vec![SetupAction::AllowInboundSharing]);
    }

    #[test]
    fn installing_a_queue_asks_for_administrator_permission_with_the_request() {
        let broker = Arc::new(RecordingBroker::default());
        let setup = Setup::new(Arc::clone(&broker) as Arc<dyn ElevationBroker>);

        let request = queue_request();
        setup.install_queue(&request).expect("installs elevated");

        assert_eq!(broker.installed(), vec![request]);
        // Installing a queue is its own elevated operation, not a parameterless action.
        assert!(broker.elevated().is_empty());
    }

    #[test]
    fn a_refused_prompt_is_reported_to_the_caller() {
        let setup = Setup::new(Arc::new(RecordingBroker::with_failure(
            SetupFailureKind::PermissionDenied,
        )));
        let error = setup
            .request(SetupAction::AllowInboundSharing)
            .expect_err("refused");

        assert_eq!(error.code(), ErrorCode::Unsupported);
        assert!(error.message().contains("administrator permission"));
        // The raw helper text never reaches the user.
        assert!(!error.message().contains("recorded failure"));
    }

    #[test]
    fn a_failed_install_names_the_queue_and_the_advice() {
        let setup = Setup::new(Arc::new(RecordingBroker::with_failure(
            SetupFailureKind::SpoolerUnavailable,
        )));
        let error = setup.install_queue(&queue_request()).expect_err("refused");

        assert_eq!(error.code(), ErrorCode::InvalidState);
        assert!(error.message().contains(
            ClientQueueName::parse("Office Printer (ShaPrint 10.0.0.5-8631)")
                .expect("valid queue name")
                .as_str()
        ));
        assert!(error.message().contains("Print Spooler"));
    }
}
