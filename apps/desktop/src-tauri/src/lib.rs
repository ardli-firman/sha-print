//! ShaPrint desktop shell.
//!
//! The shell owns the lifetime of the client proxy and server sharing runtimes, shows their live
//! status in a React UI, and stops them cleanly when the runtime closes (#30). The client proxy
//! forwards authenticated jobs over pinned IPPS (#34). Server sharing exposes selected Windows
//! queues over IPPS, authorizes and submits print jobs (#32), supports explicit manual
//! server-certificate trust (#33), and lets a client find nearby servers on the local network
//! (#36).
//!
//! Layout (ADR 0002): `domain` holds types and rules, `application` holds the runtime coordinator
//! and the use cases, `adapters` holds the service and platform implementations, and `ipc` holds
//! the Tauri command adapters and serializable DTOs.

#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]

pub mod adapters;
pub mod application;
pub mod domain;
#[cfg(feature = "desktop")]
pub mod ipc;

use std::path::Path;
#[cfg(feature = "desktop")]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use adapters::client_connections::ClientConnections;
use adapters::discovery::{MdnsAdvertiser, MdnsBrowser};
use adapters::ipps::NetworkChannel;
use adapters::{
    ClientProxyService, FileIdentityStore, IdentityStore, IppsServer, ServerSharingService,
    SystemElevation, DEFAULT_PORT,
};
use application::{
    ClientProxyState, Discovery, DiscoveryBrowser, DiscoveryService, PrintJobSubmitter,
    QueueInstallation, RuntimeCoordinator, ServerAdvertiser, Setup, Sharing, TrustedServerPrinters,
};
use domain::AppError;

/// The state the desktop shell manages, built once at startup.
///
/// Commands read the piece they need — the supervised runtimes, the sharing configuration, the
/// IPPS endpoint, the setup actions, or the client queue installer — so each one states its own
/// dependencies.
pub struct Shell {
    runtime: Arc<RuntimeCoordinator>,
    sharing: Arc<Sharing>,
    endpoint: Arc<IppsServer>,
    setup: Arc<Setup>,
    client_connections: Arc<ClientConnections>,
    network_channel: Arc<NetworkChannel>,
    discovery: Arc<Discovery>,
    queue_installation: Arc<QueueInstallation>,
}

impl Shell {
    /// Builds the shell for the app data directory the identity and settings live in.
    pub fn new(data_dir: &Path) -> Result<Self, AppError> {
        let identity = Arc::new(FileIdentityStore::new(data_dir).load_or_create()?);
        let sharing = Arc::new(Sharing::new(default_printer_catalog()));
        let client_connections = Arc::new(ClientConnections::new(data_dir)?);
        let network_channel = Arc::new(NetworkChannel::open(data_dir)?);
        let endpoint = Arc::new(IppsServer::new(
            DEFAULT_PORT,
            identity,
            Arc::clone(&network_channel),
            default_print_job_submitter(),
        ));
        let setup = Arc::new(Setup::new(Arc::new(SystemElevation::new())));
        let discovery = Arc::new(Discovery::new());
        let browser: Arc<dyn DiscoveryBrowser> = Arc::new(MdnsBrowser::new());
        let advertiser: Arc<dyn ServerAdvertiser> = Arc::new(MdnsAdvertiser::new());

        // The client proxy and discovery opt into autostart (ADR 0001: installed queues must reach
        // the proxy during normal use, and nearby servers must appear without being asked for).
        // Server sharing stays stopped until the user starts it.
        let runtime = Arc::new(RuntimeCoordinator::new(vec![
            Arc::new(ClientProxyService::new(
                Arc::clone(&client_connections),
                Arc::clone(&network_channel),
            )),
            Arc::new(ServerSharingService::new(
                Arc::clone(&sharing),
                Arc::clone(&endpoint),
                advertiser,
            )),
            Arc::new(DiscoveryService::new(Arc::clone(&discovery), browser)),
        ]));

        // Installing a client queue reuses the client trust store for its precondition and the same
        // elevation path as the firewall rule, so a queue is only ever created for an approved
        // server while the proxy that carries its jobs is running (ADR 0005).
        let queue_installation = Arc::new(QueueInstallation::new(
            Arc::clone(&setup),
            Arc::clone(&client_connections) as Arc<dyn TrustedServerPrinters>,
            adapters::queue_installation::platform_installer(),
            Arc::clone(&runtime) as Arc<dyn ClientProxyState>,
        ));

        Ok(Self {
            runtime,
            sharing,
            endpoint,
            setup,
            client_connections,
            network_channel,
            discovery,
            queue_installation,
        })
    }

    pub fn runtime(&self) -> Arc<RuntimeCoordinator> {
        Arc::clone(&self.runtime)
    }

    pub fn sharing(&self) -> Arc<Sharing> {
        Arc::clone(&self.sharing)
    }

    pub fn endpoint(&self) -> Arc<IppsServer> {
        Arc::clone(&self.endpoint)
    }

    pub fn setup(&self) -> Arc<Setup> {
        Arc::clone(&self.setup)
    }

    pub fn client_connections(&self) -> Arc<ClientConnections> {
        Arc::clone(&self.client_connections)
    }

    pub fn network_channel(&self) -> Arc<NetworkChannel> {
        Arc::clone(&self.network_channel)
    }

    pub fn discovery(&self) -> Arc<Discovery> {
        Arc::clone(&self.discovery)
    }

    pub fn queue_installation(&self) -> Arc<QueueInstallation> {
        Arc::clone(&self.queue_installation)
    }
}

/// The printer catalog for the platform this build targets.
#[cfg(windows)]
fn default_printer_catalog() -> Arc<dyn application::LocalPrinterCatalog> {
    Arc::new(adapters::printers::WindowsPrinterCatalog::new())
}

#[cfg(not(windows))]
fn default_printer_catalog() -> Arc<dyn application::LocalPrinterCatalog> {
    Arc::new(adapters::printers::UnsupportedPrinterCatalog::new())
}

#[cfg(windows)]
fn default_print_job_submitter() -> Arc<dyn PrintJobSubmitter> {
    Arc::new(adapters::printers::WindowsPrintJobSubmitter)
}

#[cfg(not(windows))]
fn default_print_job_submitter() -> Arc<dyn PrintJobSubmitter> {
    Arc::new(adapters::printers::UnsupportedPrintJobSubmitter)
}

/// Runs the desktop app until the user closes it.
#[cfg(feature = "desktop")]
pub fn run() -> Result<(), AppError> {
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
        .invoke_handler(tauri::generate_handler![
            ipc::commands::get_runtime_status,
            ipc::commands::start_service,
            ipc::commands::stop_service,
            ipc::commands::list_local_printers,
            ipc::commands::set_shared_printers,
            ipc::commands::get_server_identity,
            ipc::commands::allow_sharing_access,
            ipc::client_connections::inspect_server_connection,
            ipc::client_connections::approve_server_connection,
            ipc::client_connections::list_server_connection_printers,
            ipc::commands::install_printer_queue,
            ipc::server_settings::configure_network_channel,
            ipc::server_settings::get_network_channel_status,
            ipc::discovery::list_nearby_servers,
        ])
        .setup(move |app| {
            use tauri::Manager;

            // The identity and the sharing state live in the app data directory, so the same
            // certificate fingerprint comes back after a restart.
            let data_dir = app.path().app_data_dir().map_err(|error| {
                AppError::internal(format!("cannot locate the app data directory: {error}"))
            })?;
            let shell = Shell::new(&data_dir)?;

            ipc::emitter::forward_status(app.handle().clone(), shell.runtime());
            ipc::emitter::forward_nearby_servers(app.handle().clone(), shell.discovery());
            app.manage(shell.runtime());
            app.manage(shell.sharing());
            app.manage(shell.endpoint());
            app.manage(shell.setup());
            app.manage(shell.client_connections());
            app.manage(shell.network_channel());
            app.manage(shell.discovery());
            app.manage(shell.queue_installation());

            // Start the always-on services in the background: the window paints immediately, then
            // follows their status through the event stream. A service that cannot start is left
            // `failed` for the user to retry instead of taking the shell down.
            let runtime = shell.runtime();
            drop(tauri::async_runtime::spawn(async move {
                if let Err(error) = runtime.start_autostart().await {
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
            use tauri::Manager;

            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                // The window is gone: hold the exit, stop every background task, then exit. The
                // guard ignores the second request raised by our own `exit` call.
                if !stopping.swap(true, Ordering::SeqCst) {
                    api.prevent_exit();
                    let runtime = app
                        .try_state::<Arc<RuntimeCoordinator>>()
                        .map(|runtime| Arc::clone(&runtime));
                    let handle = app.clone();
                    drop(tauri::async_runtime::spawn(async move {
                        if let Some(runtime) = runtime {
                            if let Err(error) = runtime.shutdown().await {
                                log::warn!(
                                    "runtime shutdown did not complete code={} message={}",
                                    error.code_str(),
                                    error
                                );
                            }
                        }
                        handle.exit(0);
                    }));
                }
            }
        });

    Ok(())
}
