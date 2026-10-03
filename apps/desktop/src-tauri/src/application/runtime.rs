//! The service seam: what a background runtime must provide to be supervised by the shell.

use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::watch;

use crate::application::StatusRegistry;
use crate::domain::{AppError, ServiceId, ServiceState};

/// Cancellation handle handed to a service. The shell cancels it when it stops or shuts down the
/// service, so a service never has to outlive the app.
#[derive(Clone, Debug)]
pub struct Shutdown {
    cancelled: watch::Receiver<bool>,
}

impl Shutdown {
    /// Creates the cancellation pair: the sender stays with the coordinator, the handle with the
    /// service.
    pub(crate) fn channel() -> (watch::Sender<bool>, Self) {
        let (sender, cancelled) = watch::channel(false);
        (sender, Self { cancelled })
    }

    /// Resolves once the shell cancels this service. A dropped coordinator counts as cancelled so
    /// services cannot be orphaned.
    pub async fn cancelled(&mut self) {
        loop {
            if *self.cancelled.borrow_and_update() {
                return;
            }
            if self.cancelled.changed().await.is_err() {
                return;
            }
        }
    }
}

/// Reporting handle: the only way a service changes what the shell and the UI can see.
#[derive(Clone, Debug)]
pub struct ServiceReporter {
    id: ServiceId,
    registry: Arc<StatusRegistry>,
}

impl ServiceReporter {
    pub(crate) fn new(id: ServiceId, registry: Arc<StatusRegistry>) -> Self {
        Self { id, registry }
    }

    pub fn id(&self) -> ServiceId {
        self.id
    }

    /// Reports that the service is up and serving. Call once, after startup prerequisites succeed.
    pub fn ready(&self) -> Result<(), AppError> {
        self.registry.transition(self.id, ServiceState::Running)?;
        self.registry.set_detail(self.id, "running")
    }

    /// Replaces the short status note shown in the UI.
    pub fn detail(&self, detail: &str) -> Result<(), AppError> {
        self.registry.set_detail(self.id, detail)
    }
}

/// Everything a supervised service may use while it runs.
#[derive(Clone, Debug)]
pub struct ServiceContext {
    shutdown: Shutdown,
    reporter: ServiceReporter,
}

impl ServiceContext {
    pub(crate) fn new(shutdown: Shutdown, reporter: ServiceReporter) -> Self {
        Self { shutdown, reporter }
    }

    /// Handle to the reporter for this service.
    pub fn reporter(&self) -> ServiceReporter {
        self.reporter.clone()
    }

    /// Handle to this service's cancellation signal.
    pub fn shutdown(&self) -> Shutdown {
        self.shutdown.clone()
    }

    /// Resolves when the shell asks this service to stop.
    pub async fn cancelled(&self) {
        self.shutdown.clone().cancelled().await;
    }
}

/// A background runtime the shell owns.
///
/// Implementations run until the shell cancels them and must return promptly once
/// [`ServiceContext::cancelled`] resolves; the coordinator reports a timeout if they do not.
#[async_trait]
pub trait RuntimeService: Send + Sync + 'static {
    fn id(&self) -> ServiceId;

    /// Whether the shell starts this service when the app launches.
    fn autostart(&self) -> bool;

    /// Whether the service can start right now.
    ///
    /// A service that needs user configuration reports it here: the shell hands the error straight
    /// to the caller instead of driving the lifecycle into `failed` for something the user can fix
    /// in one step.
    fn preflight(&self) -> Result<(), AppError> {
        Ok(())
    }

    /// Runs the service until cancelled, reporting readiness through the context.
    async fn run(&self, context: ServiceContext) -> Result<(), AppError>;

    /// Called when the service has successfully started and reported ready.
    fn started(&self) {}

    /// Called when the service has been explicitly stopped by the user.
    fn stopped(&self) {}
}
