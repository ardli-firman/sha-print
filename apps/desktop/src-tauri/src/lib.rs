//! ShaPrint desktop shell.
//!
//! Issue #30 scope: a Tauri shell that owns the lifetime of the client proxy and server sharing
//! runtimes, shows their live status in a React UI, and stops them cleanly when the runtime
//! closes. The protocol work those runtimes perform lands in later issues (#31 IPPS sharing,
//! #32 authorized job submission, #34 client proxy forwarding); this crate provides the
//! supervised runtime slots, the typed IPC boundary, and the status pipeline they plug into.
//!
//! Layout (ADR 0002): `domain` holds types and rules, `application` holds the runtime
//! coordinator and its service seam, `adapters` holds the service implementations, and `ipc`
//! holds the Tauri command adapters and serializable DTOs.

#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]

pub mod adapters;
pub mod application;
pub mod domain;
pub mod ipc;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use application::RuntimeCoordinator;
use domain::AppError;

/// Builds the coordinator with the services the desktop shell supervises.
///
/// The client proxy opts into autostart (ADR 0001: installed queues must reach the proxy during
/// normal use). Server sharing stays stopped until the user starts it.
pub fn runtime() -> Arc<RuntimeCoordinator> {
    Arc::new(RuntimeCoordinator::new(vec![
        Arc::new(adapters::ClientProxyService::new()),
        Arc::new(adapters::ServerSharingService::new()),
    ]))
}

/// Runs the desktop app until the user closes it.
pub fn run() -> Result<(), AppError> {
    let runtime = runtime();
    let setup_runtime = Arc::clone(&runtime);
    let shutdown_runtime = Arc::clone(&runtime);
    let stopping = AtomicBool::new(false);

    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .targets([tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::Stdout,
                )])
                .build(),
        )
        .manage(Arc::clone(&runtime))
        .invoke_handler(tauri::generate_handler![
            ipc::commands::get_runtime_status,
            ipc::commands::start_service,
            ipc::commands::stop_service,
        ])
        .setup(move |app| {
            ipc::emitter::forward_status(app.handle().clone(), Arc::clone(&setup_runtime));
            // Start the always-on services in the background: the window paints immediately, then
            // follows their status through the event stream. A service that cannot start is left
            // `failed` for the user to retry instead of taking the shell down.
            drop(tauri::async_runtime::spawn(async move {
                if let Err(error) = setup_runtime.start_autostart().await {
                    log::warn!(
                        "autostart incomplete code={} message={}",
                        error.code_str(),
                        error
                    );
                }
            }));
            Ok(())
        })
        .build(tauri::generate_context!())
        .map_err(|error| AppError::internal(format!("cannot start the desktop shell: {error}")))?
        .run(move |app, event| {
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                // The window is gone: hold the exit, stop every background task, then exit. The
                // guard ignores the second request raised by our own `exit` call.
                if !stopping.swap(true, Ordering::SeqCst) {
                    api.prevent_exit();
                    let runtime = Arc::clone(&shutdown_runtime);
                    let handle = app.clone();
                    drop(tauri::async_runtime::spawn(async move {
                        if let Err(error) = runtime.shutdown().await {
                            log::warn!(
                                "runtime shutdown did not complete code={} message={}",
                                error.code_str(),
                                error
                            );
                        }
                        handle.exit(0);
                    }));
                }
            }
        });

    Ok(())
}
