//! Job-aware desktop updater state and orchestration.
//!
//! Network and installer work are behind traits so scheduling, manual/background error behavior,
//! and restart gating can be exercised without GitHub or a desktop shell.

use std::any::Any;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::{watch, Mutex as AsyncMutex};

use super::PrintJobTracker;

pub const INITIAL_CHECK_DELAY: Duration = Duration::from_secs(20);
pub const UPDATE_CHECK_INTERVAL: Duration = Duration::from_secs(4 * 60 * 60);
pub const RESTART_POLL_INTERVAL: Duration = Duration::from_secs(1);

pub struct UpdateRelease {
    pub version: String,
    pub notes: Option<String>,
    /// Adapter-owned handle for the release response, retained until download completes.
    pub backend_release: Option<Box<dyn Any + Send>>,
}

pub struct UpdatePackage {
    pub version: String,
    pub notes: Option<String>,
    /// Adapter-owned installer state containing the release handle and verified bytes.
    pub installer_data: Option<Box<dyn Any + Send>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateState {
    Idle,
    Checking,
    Available {
        version: String,
        notes: Option<String>,
    },
    Downloading {
        version: String,
    },
    ReadyToRestart {
        version: String,
        restart_requested: bool,
    },
    Failed {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateStatus {
    pub current_version: String,
    pub state: UpdateState,
    pub waiting_for_jobs: bool,
}

#[async_trait]
pub trait UpdateSource: Send + Sync {
    async fn check(&self) -> Result<Option<UpdateRelease>, String>;
    async fn download(&self, release: UpdateRelease) -> Result<UpdatePackage, String>;
}

#[async_trait]
pub trait UpdateInstaller: Send + Sync {
    async fn install(&self, package: UpdatePackage) -> Result<(), String>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckKind {
    Manual,
    Background,
}

struct CoordinatorState {
    status: UpdateStatus,
    package: Option<UpdatePackage>,
}

/// Owns the updater's public state and coordinates user intent with the print drain guard.
pub struct UpdateCoordinator {
    state: Mutex<CoordinatorState>,
    changes: watch::Sender<UpdateStatus>,
    check_gate: AsyncMutex<()>,
    jobs: Arc<PrintJobTracker>,
    current_version: String,
}

impl UpdateCoordinator {
    pub fn new(current_version: impl Into<String>, jobs: Arc<PrintJobTracker>) -> Self {
        let current_version = current_version.into();
        let initial = UpdateStatus {
            current_version: current_version.clone(),
            state: UpdateState::Idle,
            waiting_for_jobs: false,
        };
        let (changes, _) = watch::channel(initial.clone());
        Self {
            state: Mutex::new(CoordinatorState {
                status: initial,
                package: None,
            }),
            changes,
            check_gate: AsyncMutex::new(()),
            jobs,
            current_version,
        }
    }

    pub fn status(&self) -> UpdateStatus {
        self.state
            .lock()
            .map(|state| state.status.clone())
            .unwrap_or(UpdateStatus {
                current_version: self.current_version.clone(),
                state: UpdateState::Idle,
                waiting_for_jobs: false,
            })
    }

    pub fn subscribe(&self) -> watch::Receiver<UpdateStatus> {
        self.changes.subscribe()
    }

    pub async fn check(
        &self,
        source: &dyn UpdateSource,
        kind: CheckKind,
    ) -> Result<UpdateStatus, String> {
        let _check_guard = self.check_gate.lock().await;
        if let Ok(state) = self.state.lock() {
            if matches!(&state.status.state, UpdateState::ReadyToRestart { .. }) {
                return Ok(state.status.clone());
            }
        }
        self.set_state(UpdateState::Checking, false);
        let result: Result<Option<UpdatePackage>, String> = async {
            let Some(release) = source.check().await? else {
                return Ok(None);
            };
            let version = release.version.clone();
            let notes = release.notes.clone();
            self.set_state(
                UpdateState::Available {
                    version: version.clone(),
                    notes: notes.clone(),
                },
                false,
            );
            self.set_state(
                UpdateState::Downloading {
                    version: version.clone(),
                },
                false,
            );
            source.download(release).await.map(Some)
        }
        .await;
        match result {
            Ok(None) => {
                self.clear_package();
                self.set_state(UpdateState::Idle, false);
                Ok(self.status())
            }
            Ok(Some(package)) => {
                let version = package.version.clone();
                if let Ok(mut state) = self.state.lock() {
                    state.package = Some(package);
                }
                self.set_state(
                    UpdateState::ReadyToRestart {
                        version,
                        restart_requested: false,
                    },
                    false,
                );
                Ok(self.status())
            }
            Err(_) => match kind {
                CheckKind::Manual => {
                    let message = "ShaPrint could not check, download, or verify the update. Check your internet connection and try again.".to_owned();
                    self.set_state(
                        UpdateState::Failed {
                            message: message.clone(),
                        },
                        false,
                    );
                    Err(message)
                }
                CheckKind::Background => {
                    log::warn!("background update check failed");
                    self.clear_package();
                    self.set_state(UpdateState::Idle, false);
                    Ok(self.status())
                }
            },
        }
    }

    pub fn request_restart(&self) -> UpdateStatus {
        let next = {
            let Ok(state) = self.state.lock() else {
                return self.status();
            };
            match &state.status.state {
                UpdateState::ReadyToRestart { version, .. } => UpdateState::ReadyToRestart {
                    version: version.clone(),
                    restart_requested: true,
                },
                other => other.clone(),
            }
        };
        self.set_state(next, !self.jobs.is_restart_safe());
        self.status()
    }

    /// Installs only after explicit user intent or a hidden-window idle trigger, and only when the
    /// shared print-job tracker says the active jobs and drain cooldown have cleared.
    pub async fn install_if_requested(
        &self,
        installer: &dyn UpdateInstaller,
    ) -> Result<bool, String> {
        let restart_requested = self
            .state
            .lock()
            .map(|state| {
                matches!(
                    &state.status.state,
                    UpdateState::ReadyToRestart {
                        restart_requested: true,
                        ..
                    }
                )
            })
            .unwrap_or(false);
        if !restart_requested {
            self.update_waiting_flag(false);
            return Ok(false);
        }
        if !self.jobs.is_restart_safe() {
            self.update_waiting_flag(true);
            return Ok(false);
        }
        let package = {
            let Ok(mut state) = self.state.lock() else {
                return Ok(false);
            };
            let requested = matches!(
                state.status.state,
                UpdateState::ReadyToRestart {
                    restart_requested: true,
                    ..
                }
            );
            if !requested {
                return Ok(false);
            }
            state.package.take()
        };
        let Some(package) = package else {
            return Ok(false);
        };
        self.update_waiting_flag(false);
        installer.install(package).await?;
        Ok(true)
    }

    /// Queues an unattended restart only for a hidden window after the print-idle threshold.
    pub fn request_auto_restart_if_idle(
        &self,
        window_hidden: bool,
        quiet_window: Duration,
    ) -> bool {
        if !window_hidden || !self.jobs.is_idle_for(quiet_window) || !self.jobs.is_restart_safe() {
            return false;
        }
        if !matches!(
            self.status().state,
            UpdateState::ReadyToRestart {
                restart_requested: false,
                ..
            }
        ) {
            return false;
        }
        self.request_restart();
        true
    }

    pub fn fail(&self, message: String) {
        self.clear_package();
        self.set_state(UpdateState::Failed { message }, false);
    }

    fn clear_package(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.package = None;
        }
    }

    fn update_waiting_flag(&self, waiting: bool) {
        let status = {
            let Ok(mut state) = self.state.lock() else {
                return;
            };
            if state.status.waiting_for_jobs == waiting {
                return;
            }
            state.status.waiting_for_jobs = waiting;
            state.status.clone()
        };
        self.changes.send_replace(status);
    }

    fn set_state(&self, next: UpdateState, waiting: bool) {
        let status = {
            let Ok(mut state) = self.state.lock() else {
                return;
            };
            state.status.state = next;
            state.status.waiting_for_jobs = waiting;
            state.status.clone()
        };
        self.changes.send_replace(status);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Source(Mutex<Option<Result<Option<UpdateRelease>, String>>>);
    impl Source {
        fn new(result: Result<Option<UpdateRelease>, String>) -> Self {
            Self(Mutex::new(Some(result)))
        }
    }
    #[async_trait]
    impl UpdateSource for Source {
        async fn check(&self) -> Result<Option<UpdateRelease>, String> {
            self.0
                .lock()
                .ok()
                .and_then(|mut result| result.take())
                .unwrap_or_else(|| Err("source already called".into()))
        }
        async fn download(&self, release: UpdateRelease) -> Result<UpdatePackage, String> {
            Ok(UpdatePackage {
                version: release.version,
                notes: release.notes,
                installer_data: None,
            })
        }
    }

    #[derive(Default)]
    struct Installer(AtomicUsize);
    #[async_trait]
    impl UpdateInstaller for Installer {
        async fn install(&self, _: UpdatePackage) -> Result<(), String> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    fn release() -> UpdateRelease {
        UpdateRelease {
            version: "3.2.0".into(),
            notes: None,
            backend_release: None,
        }
    }

    #[tokio::test]
    async fn downloads_into_ready_state_and_manual_restart_waits_for_drain() {
        let jobs = Arc::new(PrintJobTracker::new());
        let coordinator = UpdateCoordinator::new("3.1.1", Arc::clone(&jobs));
        let mut lease = jobs
            .try_acquire_request()
            .expect("tracker accepts requests");
        assert!(lease.mark_print_job());
        let _lease = lease;
        coordinator
            .check(&Source::new(Ok(Some(release()))), CheckKind::Background)
            .await
            .unwrap();
        assert_eq!(
            coordinator.status().state,
            UpdateState::ReadyToRestart {
                version: "3.2.0".into(),
                restart_requested: false,
            }
        );
        coordinator.request_restart();
        assert!(coordinator.status().waiting_for_jobs);
        let installer = Installer::default();
        assert!(!coordinator.install_if_requested(&installer).await.unwrap());
        assert_eq!(installer.0.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn install_without_restart_intent_does_not_mark_jobs_as_waiting() {
        let jobs = Arc::new(PrintJobTracker::new());
        let coordinator = UpdateCoordinator::new("3.1.1", Arc::clone(&jobs));
        let mut lease = jobs
            .try_acquire_request()
            .expect("tracker accepts requests");
        assert!(lease.mark_print_job());

        assert!(!coordinator
            .install_if_requested(&Installer::default())
            .await
            .unwrap());
        assert!(!coordinator.status().waiting_for_jobs);
    }

    #[tokio::test]
    async fn background_check_preserves_a_ready_update_and_queued_restart() {
        let jobs = Arc::new(PrintJobTracker::new());
        let coordinator = UpdateCoordinator::new("3.1.1", Arc::clone(&jobs));
        coordinator
            .check(&Source::new(Ok(Some(release()))), CheckKind::Background)
            .await
            .unwrap();
        coordinator.request_restart();
        let queued_status = coordinator.status();

        let status_after_check = coordinator
            .check(
                &Source::new(Err("GitHub is unreachable".into())),
                CheckKind::Background,
            )
            .await
            .unwrap();
        assert_eq!(status_after_check, queued_status);

        let installer = Installer::default();
        assert!(coordinator.install_if_requested(&installer).await.unwrap());
        assert_eq!(installer.0.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn manual_errors_are_returned_but_background_errors_reset_to_idle() {
        let jobs = Arc::new(PrintJobTracker::new());
        let coordinator = UpdateCoordinator::new("3.1.1", jobs);
        assert_eq!(
            coordinator
                .check(
                    &Source::new(Err("GitHub is unreachable".into())),
                    CheckKind::Manual
                )
                .await,
            Err("ShaPrint could not check, download, or verify the update. Check your internet connection and try again.".into())
        );
        assert_eq!(
            coordinator.status().state,
            UpdateState::Failed {
                message: "ShaPrint could not check, download, or verify the update. Check your internet connection and try again.".into()
            }
        );
        assert!(!format!("{:?}", coordinator.status()).contains("GitHub is unreachable"));
        coordinator
            .check(&Source::new(Err("offline".into())), CheckKind::Background)
            .await
            .unwrap();
        assert_eq!(coordinator.status().state, UpdateState::Idle);
    }

    #[tokio::test]
    async fn hidden_window_auto_restart_requires_a_quiet_period_and_ready_update() {
        let jobs = Arc::new(PrintJobTracker::new());
        let coordinator = UpdateCoordinator::new("3.1.1", Arc::clone(&jobs));
        let quiet_window = Duration::from_millis(25);

        assert!(!coordinator.request_auto_restart_if_idle(true, quiet_window));
        coordinator
            .check(&Source::new(Ok(Some(release()))), CheckKind::Background)
            .await
            .unwrap();
        assert!(!coordinator.request_auto_restart_if_idle(false, quiet_window));
        assert!(!coordinator.request_auto_restart_if_idle(true, quiet_window));
        tokio::time::sleep(quiet_window + Duration::from_millis(5)).await;
        assert!(coordinator.request_auto_restart_if_idle(true, quiet_window));
        assert!(matches!(
            coordinator.status().state,
            UpdateState::ReadyToRestart {
                restart_requested: true,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn requested_restart_installs_once_when_safe() {
        let jobs = Arc::new(PrintJobTracker::new());
        let coordinator = UpdateCoordinator::new("3.1.1", Arc::clone(&jobs));
        coordinator
            .check(&Source::new(Ok(Some(release()))), CheckKind::Manual)
            .await
            .unwrap();
        coordinator.request_restart();
        let installer = Installer::default();
        // Initial state has no completed jobs, so no drain delay is required.
        assert!(coordinator.install_if_requested(&installer).await.unwrap());
        assert!(!coordinator.install_if_requested(&installer).await.unwrap());
        assert_eq!(installer.0.load(Ordering::SeqCst), 1);
    }
}
