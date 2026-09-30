//! The setup use case: request administrator permission exactly where the domain says it is
//! required, and nothing else.

use std::sync::Arc;

use crate::domain::{AppError, SetupAction};

/// Runs a configuration action with administrator rights.
///
/// The adapter is platform-specific: Windows raises a UAC prompt for the action, other platforms
/// report that they cannot (ADR 0001 keeps Linux a later phase).
pub trait ElevationBroker: Send + Sync + 'static {
    /// Performs `action` elevated. Callers check [`SetupAction::requires_elevation`] first.
    fn elevate(&self, action: SetupAction) -> Result<(), AppError>;
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
        self.broker.elevate(action)?;
        Ok(SetupOutcome::Elevated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Records the actions it was asked to elevate.
    #[derive(Default)]
    struct RecordingBroker {
        elevated: Mutex<Vec<SetupAction>>,
    }

    impl RecordingBroker {
        fn elevated(&self) -> Vec<SetupAction> {
            self.elevated
                .lock()
                .map(|actions| actions.clone())
                .unwrap_or_default()
        }
    }

    impl ElevationBroker for RecordingBroker {
        fn elevate(&self, action: SetupAction) -> Result<(), AppError> {
            self.elevated
                .lock()
                .map_err(|_| AppError::internal("broker lock is poisoned"))?
                .push(action);
            Ok(())
        }
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
    fn a_refused_prompt_is_reported_to_the_caller() {
        struct RefusingBroker;

        impl ElevationBroker for RefusingBroker {
            fn elevate(&self, _action: SetupAction) -> Result<(), AppError> {
                Err(AppError::unsupported(
                    "administrator permission was not granted",
                ))
            }
        }

        let setup = Setup::new(Arc::new(RefusingBroker));
        let error = setup
            .request(SetupAction::AllowInboundSharing)
            .expect_err("refused");

        assert_eq!(error.code(), crate::domain::ErrorCode::Unsupported);
    }
}
