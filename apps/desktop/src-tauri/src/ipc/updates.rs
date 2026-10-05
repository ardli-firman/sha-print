//! Tauri updater plugin adapter and the update IPC contract.

use std::sync::Arc;

use async_trait::async_trait;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::application::{
    CheckKind, UpdateCoordinator, UpdateInstaller, UpdatePackage, UpdateRelease, UpdateSource,
    UpdateState, UpdateStatus,
};
use crate::domain::AppError;
use crate::ipc::dto::AppErrorDto;

pub const UPDATE_STATUS_EVENT: &str = "updater://status";

#[derive(Clone)]
pub struct UpdateManager {
    pub coordinator: Arc<UpdateCoordinator>,
    source: Arc<dyn UpdateSource>,
    installer: Arc<dyn UpdateInstaller>,
}

impl UpdateManager {
    pub fn new(
        app: AppHandle,
        current_version: &str,
        jobs: Arc<crate::application::PrintJobTracker>,
    ) -> Self {
        let coordinator = Arc::new(UpdateCoordinator::new(current_version, jobs));
        Self {
            coordinator,
            source: Arc::new(PluginUpdateSource(app.clone())),
            installer: Arc::new(PluginUpdateInstaller),
        }
    }

    pub async fn check(&self, kind: CheckKind) -> Result<UpdateStatusDto, String> {
        let status = self.coordinator.check(self.source.as_ref(), kind).await?;
        Ok(UpdateStatusDto::from(&status))
    }

    pub fn status(&self) -> UpdateStatusDto {
        UpdateStatusDto::from(&self.coordinator.status())
    }

    pub fn request_restart(&self) -> UpdateStatusDto {
        UpdateStatusDto::from(&self.coordinator.request_restart())
    }

    pub async fn install_if_requested(&self) -> Result<bool, String> {
        self.coordinator
            .install_if_requested(self.installer.as_ref())
            .await
    }
}

struct PluginUpdateSource(AppHandle);

#[async_trait]
impl UpdateSource for PluginUpdateSource {
    async fn check(&self) -> Result<Option<UpdateRelease>, String> {
        let updater = self
            .0
            .updater()
            .map_err(|_| "ShaPrint's update service is unavailable.".to_owned())?;
        let Some(update) = updater
            .check()
            .await
            .map_err(|_| "GitHub Releases could not be reached.".to_owned())?
        else {
            return Ok(None);
        };
        Ok(Some(UpdateRelease {
            version: update.version.clone(),
            notes: update.body.clone(),
            backend_release: Some(Box::new(update)),
        }))
    }

    async fn download(&self, release: UpdateRelease) -> Result<UpdatePackage, String> {
        let Some(backend_release) = release.backend_release else {
            return Err("The update response was incomplete.".into());
        };
        let update = backend_release
            .downcast::<Update>()
            .map_err(|_| "The update response was invalid.".to_owned())?;
        let bytes = update.download(|_, _| {}, || {}).await.map_err(|_| {
            "The update could not be downloaded or its signature could not be verified.".to_owned()
        })?;
        Ok(UpdatePackage {
            version: release.version,
            notes: release.notes,
            installer_data: Some(Box::new((*update, bytes))),
        })
    }
}

struct PluginUpdateInstaller;

#[async_trait]
impl UpdateInstaller for PluginUpdateInstaller {
    async fn install(&self, package: UpdatePackage) -> Result<(), String> {
        let Some(installer_data) = package.installer_data else {
            return Err("The downloaded update was unavailable.".into());
        };
        let (update, bytes) = installer_data
            .downcast::<(Update, Vec<u8>)>()
            .map_err(|_| "The downloaded update was invalid.".to_owned())?;
        update
            .install(bytes)
            .map_err(|_| "The verified update could not be installed.".to_owned())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateStatusDto {
    pub current_version: String,
    pub update: UpdateStateDto,
    pub waiting_for_jobs: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum UpdateStateDto {
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

impl From<&UpdateStatus> for UpdateStatusDto {
    fn from(status: &UpdateStatus) -> Self {
        let update = match &status.state {
            UpdateState::Idle => UpdateStateDto::Idle,
            UpdateState::Checking => UpdateStateDto::Checking,
            UpdateState::Available { version, notes } => UpdateStateDto::Available {
                version: version.clone(),
                notes: notes.clone(),
            },
            UpdateState::Downloading { version } => UpdateStateDto::Downloading {
                version: version.clone(),
            },
            UpdateState::ReadyToRestart {
                version,
                restart_requested,
            } => UpdateStateDto::ReadyToRestart {
                version: version.clone(),
                restart_requested: *restart_requested,
            },
            UpdateState::Failed { message } => UpdateStateDto::Failed {
                message: message.clone(),
            },
        };
        Self {
            current_version: status.current_version.clone(),
            update,
            waiting_for_jobs: status.waiting_for_jobs,
        }
    }
}

pub type SharedUpdates = Arc<UpdateManager>;

#[tauri::command]
pub async fn get_update_status(
    updates: State<'_, SharedUpdates>,
) -> Result<UpdateStatusDto, AppErrorDto> {
    Ok(updates.status())
}

#[tauri::command]
pub async fn check_for_updates(
    updates: State<'_, SharedUpdates>,
) -> Result<UpdateStatusDto, AppErrorDto> {
    updates
        .check(CheckKind::Manual)
        .await
        .map_err(|message| AppErrorDto::from(&AppError::internal(message)))
}

#[tauri::command]
pub async fn apply_update_and_restart(
    updates: State<'_, SharedUpdates>,
) -> Result<UpdateStatusDto, AppErrorDto> {
    Ok(updates.request_restart())
}

pub fn forward_status(app: AppHandle, updates: Arc<UpdateCoordinator>) {
    let mut changed = updates.subscribe();
    tauri::async_runtime::spawn(async move {
        while changed.changed().await.is_ok() {
            let payload = UpdateStatusDto::from(&changed.borrow_and_update());
            if let Some(action) = app.try_state::<crate::UpdateTrayAction>() {
                let ready = matches!(&payload.update, UpdateStateDto::ReadyToRestart { .. });
                let label = match &payload.update {
                    UpdateStateDto::ReadyToRestart { version, .. } => {
                        format!("Restart to Update (v{version})")
                    }
                    _ => "Restart to Update".to_owned(),
                };
                if let Err(error) = action.item.set_text(&label) {
                    log::warn!("updater tray action label could not be updated: {error}");
                }
                if let Ok(mut inserted) = action.inserted.lock() {
                    if ready && !*inserted {
                        match action.menu.append(&action.item) {
                            Ok(()) => *inserted = true,
                            Err(error) => {
                                log::warn!("updater tray action could not be added: {error}")
                            }
                        }
                    } else if !ready && *inserted {
                        match action.menu.remove(&action.item) {
                            Ok(()) => *inserted = false,
                            Err(error) => {
                                log::warn!("updater tray action could not be removed: {error}")
                            }
                        }
                    }
                }
            }
            if let Err(error) = app.emit(UPDATE_STATUS_EVENT, payload) {
                log::warn!("updater status event could not be emitted: {error}");
            }
        }
    });
}
