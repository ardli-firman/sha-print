//! Bridges shell state changes to the frontend.
//!
//! The shell emits one event per published snapshot, so the UI renders live status, print failures,
//! the import from the previous application, and the live list of nearby servers without polling.
//! Payloads are status, failure, and advertisement DTOs only: no credentials, no print job content.

use std::sync::Arc;

use tauri::{AppHandle, Emitter};

use crate::application::{Discovery, LegacyImport, PrintFailures, RuntimeCoordinator};
use crate::ipc::control::SharedControl;
use crate::ipc::discovery::{NearbyServersDto, NEARBY_SERVERS_EVENT};
use crate::ipc::dto::{
    LegacyImportReportDto, PrintFailureDto, RuntimeStatusDto, ServiceStatusDto,
    LEGACY_IMPORT_EVENT, PRINT_FAILURE_EVENT, RUNTIME_STATUS_EVENT,
};

/// Forwards every status change to `app` until the app shuts down.
pub fn forward_status(app: AppHandle, runtime: Arc<RuntimeCoordinator>) {
    let mut changes = runtime.subscribe();
    let _forwarder = tauri::async_runtime::spawn(async move {
        // `changed()` resolves on every published snapshot and errors once the coordinator is
        // dropped, which only happens while the process is exiting.
        while changes.changed().await.is_ok() {
            let status = RuntimeStatusDto::from(&*changes.borrow_and_update());
            emit_status(&app, &status);
        }
    });
}

/// Publishes the machine daemon's status while this window only supervises its own browsing.
///
/// In machine mode there is no in-process coordinator to subscribe to: the daemon is a separate
/// root process reached over its local socket, so its status is polled once a second and emitted
/// only when it changes. While the daemon is unreachable every service is reported stopped with
/// the reason, so the window never renders a stale snapshot of a dead machine.
pub fn forward_daemon_status(app: AppHandle, control: SharedControl) {
    const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

    let _forwarder = tauri::async_runtime::spawn(async move {
        let mut last: Option<RuntimeStatusDto> = None;
        loop {
            match control.status().await {
                Ok(status) => {
                    if last.as_ref() != Some(&status) {
                        emit_status(&app, &status);
                        last = Some(status);
                    }
                }
                Err(error) => {
                    let status = daemon_unreachable_status();
                    if last.as_ref() != Some(&status) {
                        log::warn!(
                            "machine daemon status unavailable code={} message={}",
                            error.code_str(),
                            error
                        );
                        emit_status(&app, &status);
                        last = Some(status);
                    }
                }
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    });
}

fn emit_status(app: &AppHandle, status: &RuntimeStatusDto) {
    if let Err(error) = app.emit(RUNTIME_STATUS_EVENT, status) {
        log::warn!("cannot emit runtime status code=internal message={error}");
    }
}

/// The snapshot shown while the daemon cannot be reached: its services run as its tasks, so with
/// `shaprintd` down none of them runs. A transient read failure shows the same snapshot — the
/// detail names the real reason instead of claiming the services were stopped.
fn daemon_unreachable_status() -> RuntimeStatusDto {
    RuntimeStatusDto {
        services: crate::domain::ServiceId::ALL
            .iter()
            .map(|id| ServiceStatusDto {
                id: id.as_str().to_owned(),
                state: "stopped".to_owned(),
                detail: "the machine daemon is not reachable".to_owned(),
            })
            .collect(),
        network_channel_configured: None,
    }
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
