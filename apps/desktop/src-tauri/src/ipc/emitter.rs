//! Bridges runtime status changes to the frontend.
//!
//! The shell emits one event per published snapshot, so the UI renders live status without
//! polling. Payloads are status DTOs only: no credentials, no print job content.

use std::sync::Arc;

use tauri::{AppHandle, Emitter};

use crate::application::RuntimeCoordinator;
use crate::ipc::dto::{RuntimeStatusDto, RUNTIME_STATUS_EVENT};

/// Forwards every status change to `app` until the app shuts down.
pub fn forward_status(app: AppHandle, runtime: Arc<RuntimeCoordinator>) {
    let mut changes = runtime.subscribe();
    let _forwarder = tauri::async_runtime::spawn(async move {
        // `changed()` resolves on every published snapshot and errors once the coordinator is
        // dropped, which only happens while the process is exiting.
        while changes.changed().await.is_ok() {
            let status = RuntimeStatusDto::from(&*changes.borrow_and_update());
            if let Err(error) = app.emit(RUNTIME_STATUS_EVENT, status) {
                log::warn!("cannot emit runtime status code=internal message={error}");
            }
        }
    });
}
