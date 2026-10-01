//! Bridges shell state changes to the frontend.
//!
//! The shell emits one event per published snapshot, so the UI renders live status and the live
//! list of nearby servers without polling. Payloads are status and advertisement DTOs only: no
//! credentials, no print job content.

use std::sync::Arc;

use tauri::{AppHandle, Emitter};

use crate::application::{Discovery, RuntimeCoordinator};
use crate::ipc::discovery::{NearbyServersDto, NEARBY_SERVERS_EVENT};
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

/// Forwards every change to the nearby servers to `app` until the app shuts down.
pub fn forward_nearby_servers(app: AppHandle, discovery: Arc<Discovery>) {
    let mut changes = discovery.subscribe();
    let _forwarder = tauri::async_runtime::spawn(async move {
        while changes.changed().await.is_ok() {
            let payload = NearbyServersDto::from(changes.borrow_and_update().as_slice());
            if let Err(error) = app.emit(NEARBY_SERVERS_EVENT, payload) {
                log::warn!("cannot emit nearby servers code=internal message={error}");
            }
        }
    });
}
