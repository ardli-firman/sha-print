//! Bridges shell state changes to the frontend.
//!
//! The shell emits one event per published snapshot, so the UI renders live status, print failures,
//! the import from the previous application, and the live list of nearby servers without polling.
//! Payloads are status, failure, and advertisement DTOs only: no credentials, no print job content.

use std::sync::Arc;

use tauri::{AppHandle, Emitter};

use crate::application::{Discovery, LegacyImport, PrintFailures, RuntimeCoordinator};
use crate::ipc::discovery::{NearbyServersDto, NEARBY_SERVERS_EVENT};
use crate::ipc::dto::{
    LegacyImportReportDto, PrintFailureDto, RuntimeStatusDto, LEGACY_IMPORT_EVENT,
    PRINT_FAILURE_EVENT, RUNTIME_STATUS_EVENT,
};

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

/// Forwards every print failure and dismissal to `app` until the app shuts down.
pub fn forward_print_failures(app: AppHandle, failures: Arc<PrintFailures>) {
    let mut changes = failures.subscribe();
    let _forwarder = tauri::async_runtime::spawn(async move {
        while changes.changed().await.is_ok() {
            let payload = changes
                .borrow_and_update()
                .as_ref()
                .map(PrintFailureDto::from);
            if let Err(error) = app.emit(PRINT_FAILURE_EVENT, payload) {
                log::warn!("cannot emit print failure code=internal message={error}");
            }
        }
    });
}

/// Forwards every completed import from the previous application to `app`.
pub fn forward_legacy_import(app: AppHandle, legacy: Arc<LegacyImport>) {
    let mut changes = legacy.subscribe();
    let _forwarder = tauri::async_runtime::spawn(async move {
        while changes.changed().await.is_ok() {
            let report = changes.borrow_and_update().clone();
            // Nothing has been imported yet is not news for the window.
            let Some(report) = report else {
                continue;
            };
            let payload = LegacyImportReportDto::from(&report);
            if let Err(error) = app.emit(LEGACY_IMPORT_EVENT, payload) {
                log::warn!("cannot emit the legacy import result code=internal message={error}");
            }
        }
    });
}
