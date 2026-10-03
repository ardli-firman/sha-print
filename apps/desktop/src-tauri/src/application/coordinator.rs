//! The runtime coordinator: owns every supervised service, publishes their status, and stops
//! them cleanly.
//!
//! One coordinator instance is shared by the Tauri command adapters, the status emitter, and the
//! shutdown hook, so the UI always reads the same view of the runtime.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::{timeout_at, Instant};

use crate::application::{RuntimeService, ServiceContext, ServiceReporter, Shutdown};
use crate::domain::{AppError, ErrorCode, RuntimeStatus, ServiceId, ServiceState, ServiceStatus};

/// How long a service may take to report ready before the shell reports a timeout.
pub const START_TIMEOUT: Duration = Duration::from_secs(10);

/// How long shutdown waits for a task to end before reporting a timeout.
pub const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

/// The published status snapshot plus the channel subscribers listen on.
#[derive(Debug)]
pub(crate) struct StatusRegistry {
    statuses: Mutex<Vec<ServiceStatus>>,
    changes: watch::Sender<RuntimeStatus>,
}

impl StatusRegistry {
    /// Seeds one `stopped` entry per id, in the order services are reported.
    pub(crate) fn new(ids: &[ServiceId]) -> Self {
        let statuses: Vec<ServiceStatus> =
            ids.iter().copied().map(ServiceStatus::stopped).collect();
        let (changes, _) = watch::channel(RuntimeStatus::new(statuses.clone()));
        Self {
            statuses: Mutex::new(statuses),
            changes,
        }
    }

    pub(crate) fn snapshot(&self) -> RuntimeStatus {
        match self.statuses.lock() {
            Ok(statuses) => RuntimeStatus::new(statuses.clone()),
            // A poisoned lock means another thread panicked; report the last published snapshot
            // rather than propagating a failure the UI cannot act on.
            Err(poisoned) => RuntimeStatus::new(poisoned.into_inner().clone()),
        }
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<RuntimeStatus> {
        self.changes.subscribe()
    }

    pub(crate) fn status(&self, id: ServiceId) -> Option<ServiceStatus> {
        self.lock()
            .ok()?
            .iter()
            .find(|status| status.id() == id)
            .cloned()
    }

    pub(crate) fn transition(&self, id: ServiceId, next: ServiceState) -> Result<(), AppError> {
        let mut statuses = self.lock()?;
        let status = entry(&mut statuses, id)?;
        status.transition(next)?;
        self.publish(&statuses);
        Ok(())
    }

    pub(crate) fn set_detail(&self, id: ServiceId, detail: &str) -> Result<(), AppError> {
        let mut statuses = self.lock()?;
        let status = entry(&mut statuses, id)?;
        status.set_detail(detail);
        self.publish(&statuses);
        Ok(())
    }

    /// Records a state the shell does not request, such as a service ending or a stop timing out.
    /// A rejected transition is logged instead of propagated: the published status stays the
    /// source of truth and the caller still reports the failure it observed.
    pub(crate) fn record(&self, id: ServiceId, next: ServiceState, detail: &str) {
        if let Err(error) = self.transition(id, next) {
            log::warn!(
                "cannot record service state id={} message={}",
                id.as_str(),
                error
            );
            return;
        }
        if let Err(error) = self.set_detail(id, detail) {
            log::warn!(
                "cannot update service detail id={} message={}",
                id.as_str(),
                error
            );
        }
    }

    fn publish(&self, statuses: &[ServiceStatus]) {
        self.changes
            .send_replace(RuntimeStatus::new(statuses.to_vec()));
    }

    fn lock(&self) -> Result<MutexGuard<'_, Vec<ServiceStatus>>, AppError> {
        self.statuses
            .lock()
            .map_err(|_| AppError::internal("runtime status lock is poisoned"))
    }
}

fn entry(statuses: &mut [ServiceStatus], id: ServiceId) -> Result<&mut ServiceStatus, AppError> {
    statuses
        .iter_mut()
        .find(|status| status.id() == id)
        .ok_or_else(|| AppError::unknown_service(format!("unknown service '{}'", id.as_str())))
}

/// A spawned service task plus the handle that cancels it.
#[derive(Debug)]
struct RunningService {
    cancel: watch::Sender<bool>,
    task: JoinHandle<()>,
}

/// Owns the supervised services and their status.
pub struct RuntimeCoordinator {
    registry: Arc<StatusRegistry>,
    services: BTreeMap<ServiceId, Arc<dyn RuntimeService>>,
    running: Mutex<BTreeMap<ServiceId, RunningService>>,
    start_timeout: Duration,
    shutdown_timeout: Duration,
}

impl RuntimeCoordinator {
    /// Creates a coordinator for the given services and seeds their status as `stopped`.
    pub fn new(services: Vec<Arc<dyn RuntimeService>>) -> Self {
        let mut supervised = BTreeMap::new();
        for service in services {
            let id = service.id();
            if supervised.insert(id, service).is_some() {
                log::warn!("ignoring duplicate service id={}", id.as_str());
            }
        }
        let ids: Vec<ServiceId> = ServiceId::ALL
            .into_iter()
            .filter(|id| supervised.contains_key(id))
            .collect();

        Self {
            registry: Arc::new(StatusRegistry::new(&ids)),
            services: supervised,
            running: Mutex::new(BTreeMap::new()),
            start_timeout: START_TIMEOUT,
            shutdown_timeout: SHUTDOWN_TIMEOUT,
        }
    }

    /// Overrides how long a service may take to report ready. Used by tests.
    #[must_use]
    pub fn with_start_timeout(mut self, timeout: Duration) -> Self {
        self.start_timeout = timeout;
        self
    }

    /// Overrides how long stop and shutdown wait for a task. Used by tests.
    #[must_use]
    pub fn with_shutdown_timeout(mut self, timeout: Duration) -> Self {
        self.shutdown_timeout = timeout;
        self
    }

    /// The current status of every supervised service.
    pub fn status(&self) -> RuntimeStatus {
        self.registry.snapshot()
    }

    /// Subscribes to status changes; the receiver also holds the current snapshot.
    pub fn subscribe(&self) -> watch::Receiver<RuntimeStatus> {
        self.registry.subscribe()
    }

    /// Starts every service that opts into autostart.
    ///
    /// A service that fails is reported and left `failed`; the remaining services still start, so
    /// one broken runtime cannot keep the others, or the shell, from working.
    pub async fn start_autostart(&self) -> Result<(), AppError> {
        let mut failure = None;
        for (id, service) in &self.services {
            if !service.autostart() {
                continue;
            }
            if let Err(error) = self.start(*id).await {
                log::warn!(
                    "autostart failed id={} code={}",
                    id.as_str(),
                    error.code_str()
                );
                if failure.is_none() {
                    failure = Some(error);
                }
            }
        }
        match failure {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// Starts one service and waits until it reports ready.
    pub async fn start(&self, id: ServiceId) -> Result<(), AppError> {
        let service = self.service(id)?;

        // A precondition the user can still fix (no shared printers selected, for example) is
        // reported with its own code and leaves the lifecycle untouched.
        service.preflight()?;

        // A failed service is reset first, so a retry from the UI is a single action.
        if self.state(id)? == ServiceState::Failed {
            self.stop(id).await?;
        }

        let (cancel, shutdown) = Shutdown::channel();
        {
            let mut running = self.lock_running()?;
            if running.contains_key(&id) {
                return Err(AppError::invalid_state(format!(
                    "service '{}' is already active",
                    id.as_str()
                )));
            }
            self.registry.transition(id, ServiceState::Starting)?;
            self.registry.set_detail(id, "starting")?;

            let context = ServiceContext::new(
                shutdown,
                ServiceReporter::new(id, Arc::clone(&self.registry)),
            );
            let registry = Arc::clone(&self.registry);
            let task = tokio::spawn(supervise(Arc::clone(&service), context, registry));
            running.insert(id, RunningService { cancel, task });
        }

        match self.await_ready(id).await {
            Ok(()) => {
                service.started();
                log::info!("service started id={}", id.as_str());
                Ok(())
            }
            Err(error) => {
                log::warn!(
                    "service failed to start id={} code={}",
                    id.as_str(),
                    error.code_str()
                );
                self.discard(id, &error).await;
                Err(error)
            }
        }
    }

    /// Stops one service and waits for its task to end.
    pub async fn stop(&self, id: ServiceId) -> Result<(), AppError> {
        self.service(id)?;
        let state = self.state(id)?;
        if state == ServiceState::Stopped {
            return Err(AppError::invalid_state(format!(
                "service '{}' is not running",
                id.as_str()
            )));
        }

        let entry = self.lock_running()?.remove(&id);
        let Some(entry) = entry else {
            // No task is left to cancel (the service failed on its own); keep that visible.
            self.settle_stopped(id);
            return Ok(());
        };

        if state.is_live() {
            self.registry.transition(id, ServiceState::Stopping)?;
        }
        let _ = entry.cancel.send_replace(true);

        match timeout_at(Instant::now() + self.shutdown_timeout, entry.task).await {
            Ok(_) => {
                self.settle_stopped(id);
                if let Ok(service) = self.service(id) {
                    service.stopped();
                }
                log::info!("service stopped id={}", id.as_str());
                Ok(())
            }
            Err(_) => {
                self.registry.record(
                    id,
                    ServiceState::Failed,
                    &format!(
                        "did not stop within the deadline [{}]",
                        ErrorCode::Timeout.as_str()
                    ),
                );
                Err(AppError::timeout(format!(
                    "service '{}' did not stop in time",
                    id.as_str()
                )))
            }
        }
    }

    /// Stops every service the shell started; safe to call more than once.
    pub async fn shutdown(&self) -> Result<(), AppError> {
        let entries: Vec<(ServiceId, RunningService)> = {
            let mut running = self.lock_running()?;
            std::mem::take(&mut *running).into_iter().collect()
        };
        if entries.is_empty() {
            return Ok(());
        }
        log::info!("runtime shutdown services={}", entries.len());

        for (id, entry) in &entries {
            if self.state(*id)?.is_live() {
                self.registry.transition(*id, ServiceState::Stopping)?;
            }
            let _ = entry.cancel.send_replace(true);
        }

        let deadline = Instant::now() + self.shutdown_timeout;
        let mut stuck = Vec::new();
        for (id, entry) in entries {
            if timeout_at(deadline, entry.task).await.is_err() {
                self.registry.record(
                    id,
                    ServiceState::Failed,
                    "did not stop within the deadline [timeout]",
                );
                stuck.push(id);
            } else {
                self.settle_stopped(id);
            }
        }

        if stuck.is_empty() {
            Ok(())
        } else {
            let names: Vec<&str> = stuck.iter().map(|id| id.as_str()).collect();
            Err(AppError::timeout(format!(
                "services did not stop in time: {}",
                names.join(", ")
            )))
        }
    }

    /// Waits until the service reports ready, fails during startup, or the deadline passes.
    async fn await_ready(&self, id: ServiceId) -> Result<(), AppError> {
        let mut changes = self.registry.subscribe();
        let deadline = Instant::now() + self.start_timeout;
        loop {
            match self.state(id)? {
                ServiceState::Running => return Ok(()),
                ServiceState::Failed => {
                    let detail = self
                        .registry
                        .status(id)
                        .map(|status| status.detail().to_owned())
                        .unwrap_or_default();
                    return Err(AppError::internal(format!(
                        "service '{}' failed to start: {}",
                        id.as_str(),
                        detail
                    )));
                }
                _ => {}
            }
            match timeout_at(deadline, changes.changed()).await {
                Ok(Ok(())) => {}
                Ok(Err(_)) => {
                    return Err(AppError::internal("runtime status channel closed"));
                }
                Err(_) => {
                    return Err(AppError::timeout(format!(
                        "service '{}' did not report ready in time",
                        id.as_str()
                    )));
                }
            }
        }
    }

    /// Tears down a service that could not start, keeping the failure visible in its status.
    async fn discard(&self, id: ServiceId, error: &AppError) {
        let entry = match self.lock_running() {
            Ok(mut running) => running.remove(&id),
            Err(_) => None,
        };
        if let Some(entry) = entry {
            let _ = entry.cancel.send_replace(true);
            let _ = timeout_at(Instant::now() + self.shutdown_timeout, entry.task).await;
        }
        if matches!(self.state(id), Ok(ServiceState::Starting)) {
            self.registry.record(
                id,
                ServiceState::Failed,
                &format!("{error} [{}]", error.code_str()),
            );
        }
    }

    /// Moves a live service to `stopped`; failures stay visible for the user to retry.
    fn settle_stopped(&self, id: ServiceId) {
        if matches!(self.state(id), Ok(state) if state.is_live()) {
            self.registry.record(id, ServiceState::Stopped, "stopped");
        }
    }

    fn state(&self, id: ServiceId) -> Result<ServiceState, AppError> {
        self.registry
            .status(id)
            .map(|status| status.state())
            .ok_or_else(|| AppError::unknown_service(format!("unknown service '{}'", id.as_str())))
    }

    fn service(&self, id: ServiceId) -> Result<Arc<dyn RuntimeService>, AppError> {
        self.services
            .get(&id)
            .cloned()
            .ok_or_else(|| AppError::unknown_service(format!("unknown service '{}'", id.as_str())))
    }

    fn lock_running(
        &self,
    ) -> Result<MutexGuard<'_, BTreeMap<ServiceId, RunningService>>, AppError> {
        self.running
            .lock()
            .map_err(|_| AppError::internal("runtime lock is poisoned"))
    }
}

/// Runs one service and records how its task ended: cancellation ends in `stopped`, an error or
/// an unexpected return ends in `failed`.
async fn supervise(
    service: Arc<dyn RuntimeService>,
    context: ServiceContext,
    registry: Arc<StatusRegistry>,
) {
    let id = service.id();
    let outcome = service.run(context).await;
    let state = registry.status(id).map(|status| status.state());

    match (outcome, state) {
        (Ok(()), Some(ServiceState::Stopping)) => {
            registry.record(id, ServiceState::Stopped, "stopped");
        }
        (Ok(()), _) => {
            log::warn!("service stopped unexpectedly id={}", id.as_str());
            registry.record(id, ServiceState::Failed, "stopped unexpectedly [internal]");
        }
        (Err(error), _) => {
            log::warn!(
                "service failed id={} code={} message={}",
                id.as_str(),
                error.code_str(),
                error
            );
            registry.record(
                id,
                ServiceState::Failed,
                &format!("{error} [{}]", error.code_str()),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ErrorCode;
    use async_trait::async_trait;

    /// Behaviour a fake service can have in coordinator tests.
    enum Behaviour {
        /// Reports ready, then waits for cancellation.
        Idle,
        /// Fails during startup.
        Fails,
        /// Waits forever without reporting ready.
        NeverReady,
        /// Rejects startup before the lifecycle starts, for example because the user has not
        /// configured it yet.
        Blocked,
        /// Reports ready, then ignores cancellation.
        Stuck,
    }

    struct FakeService {
        id: ServiceId,
        autostart: bool,
        behaviour: Behaviour,
    }

    impl FakeService {
        fn new(id: ServiceId, autostart: bool, behaviour: Behaviour) -> Self {
            Self {
                id,
                autostart,
                behaviour,
            }
        }
    }

    #[async_trait]
    impl RuntimeService for FakeService {
        fn id(&self) -> ServiceId {
            self.id
        }

        fn autostart(&self) -> bool {
            self.autostart
        }

        fn preflight(&self) -> Result<(), AppError> {
            match self.behaviour {
                Behaviour::Blocked => Err(AppError::invalid_state(
                    "select at least one printer to share before starting",
                )),
                _ => Ok(()),
            }
        }

        async fn run(&self, context: ServiceContext) -> Result<(), AppError> {
            match self.behaviour {
                Behaviour::Fails => Err(AppError::internal("printer adapter unavailable")),
                Behaviour::NeverReady => {
                    context.cancelled().await;
                    Ok(())
                }
                Behaviour::Blocked => {
                    context.reporter().ready()?;
                    context.cancelled().await;
                    Ok(())
                }
                Behaviour::Idle => {
                    context.reporter().ready()?;
                    context.cancelled().await;
                    Ok(())
                }
                Behaviour::Stuck => {
                    context.reporter().ready()?;
                    std::future::pending::<()>().await;
                    Ok(())
                }
            }
        }
    }

    fn coordinator(services: Vec<FakeService>) -> RuntimeCoordinator {
        RuntimeCoordinator::new(
            services
                .into_iter()
                .map(|service| Arc::new(service) as Arc<dyn RuntimeService>)
                .collect(),
        )
    }

    fn state(coordinator: &RuntimeCoordinator, id: ServiceId) -> ServiceState {
        coordinator
            .status()
            .service(id)
            .map(|status| status.state())
            .unwrap_or_else(|| panic!("no status for {}", id.as_str()))
    }

    fn detail(coordinator: &RuntimeCoordinator, id: ServiceId) -> String {
        coordinator
            .status()
            .service(id)
            .map(|status| status.detail().to_owned())
            .unwrap_or_default()
    }

    #[tokio::test]
    async fn new_reports_every_service_as_stopped() {
        let runtime = coordinator(vec![
            FakeService::new(ServiceId::ClientProxy, true, Behaviour::Idle),
            FakeService::new(ServiceId::ServerSharing, false, Behaviour::Idle),
        ]);

        let status = runtime.status();
        assert_eq!(status.services().len(), 2);
        assert_eq!(
            state(&runtime, ServiceId::ClientProxy),
            ServiceState::Stopped
        );
        assert_eq!(
            state(&runtime, ServiceId::ServerSharing),
            ServiceState::Stopped
        );
    }

    #[tokio::test]
    async fn autostart_starts_only_services_that_opt_in() {
        let runtime = coordinator(vec![
            FakeService::new(ServiceId::ClientProxy, true, Behaviour::Idle),
            FakeService::new(ServiceId::ServerSharing, false, Behaviour::Idle),
        ]);

        runtime.start_autostart().await.expect("autostart succeeds");

        assert_eq!(
            state(&runtime, ServiceId::ClientProxy),
            ServiceState::Running
        );
        assert_eq!(
            state(&runtime, ServiceId::ServerSharing),
            ServiceState::Stopped
        );
        runtime.shutdown().await.expect("shutdown succeeds");
    }

    #[tokio::test]
    async fn autostart_reports_a_failure_without_blocking_the_other_services() {
        let runtime = coordinator(vec![
            FakeService::new(ServiceId::ClientProxy, true, Behaviour::Fails),
            FakeService::new(ServiceId::ServerSharing, true, Behaviour::Idle),
        ]);

        let error = runtime
            .start_autostart()
            .await
            .expect_err("reports the failure");

        assert_eq!(
            state(&runtime, ServiceId::ClientProxy),
            ServiceState::Failed
        );
        assert_eq!(
            state(&runtime, ServiceId::ServerSharing),
            ServiceState::Running
        );
        assert!(error.message().contains("printer adapter unavailable"));
        runtime.shutdown().await.expect("shutdown succeeds");
    }

    #[tokio::test]
    async fn stop_returns_a_service_to_stopped() {
        let runtime = coordinator(vec![FakeService::new(
            ServiceId::ServerSharing,
            false,
            Behaviour::Idle,
        )]);

        runtime
            .start(ServiceId::ServerSharing)
            .await
            .expect("starts");
        runtime.stop(ServiceId::ServerSharing).await.expect("stops");

        assert_eq!(
            state(&runtime, ServiceId::ServerSharing),
            ServiceState::Stopped
        );
        assert_eq!(detail(&runtime, ServiceId::ServerSharing), "stopped");
    }

    #[tokio::test]
    async fn starting_an_active_service_is_rejected() {
        let runtime = coordinator(vec![FakeService::new(
            ServiceId::ClientProxy,
            true,
            Behaviour::Idle,
        )]);

        runtime.start(ServiceId::ClientProxy).await.expect("starts");
        let error = runtime
            .start(ServiceId::ClientProxy)
            .await
            .expect_err("rejected");

        assert_eq!(error.code(), ErrorCode::InvalidState);
        assert_eq!(
            state(&runtime, ServiceId::ClientProxy),
            ServiceState::Running
        );
        runtime.shutdown().await.expect("shutdown succeeds");
    }

    #[tokio::test]
    async fn stopping_a_stopped_service_is_rejected() {
        let runtime = coordinator(vec![FakeService::new(
            ServiceId::ClientProxy,
            false,
            Behaviour::Idle,
        )]);

        let error = runtime
            .stop(ServiceId::ClientProxy)
            .await
            .expect_err("rejected");

        assert_eq!(error.code(), ErrorCode::InvalidState);
        assert_eq!(error.message(), "service 'client-proxy' is not running");
    }

    #[tokio::test]
    async fn services_the_shell_does_not_own_are_rejected() {
        let runtime = coordinator(vec![]);

        let error = runtime
            .start(ServiceId::ClientProxy)
            .await
            .expect_err("rejected");

        assert_eq!(error.code(), ErrorCode::UnknownService);
    }

    #[tokio::test]
    async fn a_service_that_fails_to_start_is_reported_as_failed() {
        let runtime = coordinator(vec![FakeService::new(
            ServiceId::ClientProxy,
            true,
            Behaviour::Fails,
        )]);

        let error = runtime
            .start(ServiceId::ClientProxy)
            .await
            .expect_err("fails");

        assert_eq!(
            state(&runtime, ServiceId::ClientProxy),
            ServiceState::Failed
        );
        assert_eq!(
            detail(&runtime, ServiceId::ClientProxy),
            "printer adapter unavailable [internal]"
        );
        assert!(error.message().contains("printer adapter unavailable"));
    }

    #[tokio::test]
    async fn a_failed_service_can_be_started_again() {
        let runtime = coordinator(vec![FakeService::new(
            ServiceId::ClientProxy,
            false,
            Behaviour::Fails,
        )]);

        runtime
            .start(ServiceId::ClientProxy)
            .await
            .expect_err("fails");
        let error = runtime
            .start(ServiceId::ClientProxy)
            .await
            .expect_err("retry fails too");

        // A retry must reach the service again instead of being rejected as an illegal transition.
        assert_ne!(error.code(), ErrorCode::InvalidState);
        assert_eq!(
            state(&runtime, ServiceId::ClientProxy),
            ServiceState::Failed
        );
    }

    #[tokio::test]
    async fn a_service_with_an_unmet_precondition_reports_it_without_starting() {
        let runtime = coordinator(vec![FakeService::new(
            ServiceId::ServerSharing,
            false,
            Behaviour::Blocked,
        )]);

        let error = runtime
            .start(ServiceId::ServerSharing)
            .await
            .expect_err("rejected");

        assert_eq!(error.code(), ErrorCode::InvalidState);
        assert_eq!(
            error.message(),
            "select at least one printer to share before starting"
        );
        // The user's configuration problem is not a service failure: the state stays untouched so
        // fixing the configuration and pressing Start again works.
        assert_eq!(
            state(&runtime, ServiceId::ServerSharing),
            ServiceState::Stopped
        );
    }

    #[tokio::test]
    async fn a_service_that_never_reports_ready_times_out() {
        let runtime = coordinator(vec![FakeService::new(
            ServiceId::ClientProxy,
            true,
            Behaviour::NeverReady,
        )])
        .with_start_timeout(Duration::from_millis(50));

        let error = runtime
            .start(ServiceId::ClientProxy)
            .await
            .expect_err("times out");

        assert_eq!(error.code(), ErrorCode::Timeout);
        assert_eq!(
            state(&runtime, ServiceId::ClientProxy),
            ServiceState::Failed
        );
    }

    #[tokio::test]
    async fn shutdown_stops_every_service_and_can_run_again() {
        let runtime = coordinator(vec![
            FakeService::new(ServiceId::ClientProxy, true, Behaviour::Idle),
            FakeService::new(ServiceId::ServerSharing, false, Behaviour::Idle),
        ]);
        runtime.start_autostart().await.expect("autostart succeeds");
        runtime
            .start(ServiceId::ServerSharing)
            .await
            .expect("starts");

        runtime.shutdown().await.expect("shutdown succeeds");

        assert_eq!(
            state(&runtime, ServiceId::ClientProxy),
            ServiceState::Stopped
        );
        assert_eq!(
            state(&runtime, ServiceId::ServerSharing),
            ServiceState::Stopped
        );
        runtime.shutdown().await.expect("second shutdown succeeds");
    }

    #[tokio::test]
    async fn shutdown_reports_a_task_that_ignores_cancellation() {
        let runtime = coordinator(vec![FakeService::new(
            ServiceId::ClientProxy,
            true,
            Behaviour::Stuck,
        )])
        .with_shutdown_timeout(Duration::from_millis(50));
        runtime.start_autostart().await.expect("autostart succeeds");

        let error = runtime.shutdown().await.expect_err("reports a timeout");

        assert_eq!(error.code(), ErrorCode::Timeout);
        assert_eq!(
            state(&runtime, ServiceId::ClientProxy),
            ServiceState::Failed
        );
        assert_eq!(
            detail(&runtime, ServiceId::ClientProxy),
            "did not stop within the deadline [timeout]"
        );
    }

    #[tokio::test]
    async fn subscribers_receive_every_status_change() {
        let runtime = coordinator(vec![FakeService::new(
            ServiceId::ClientProxy,
            true,
            Behaviour::Idle,
        )]);
        let mut changes = runtime.subscribe();

        runtime.start_autostart().await.expect("autostart succeeds");
        changes.changed().await.expect("publishes a change");

        let published = changes.borrow_and_update().clone();
        assert_eq!(
            published
                .service(ServiceId::ClientProxy)
                .map(|status| status.state()),
            Some(ServiceState::Running)
        );
        runtime.shutdown().await.expect("shutdown succeeds");
    }
}
