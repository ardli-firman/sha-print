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
#[cfg(test)]
mod test_support;

use std::path::Path;
#[cfg(feature = "desktop")]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use adapters::client_connections::ClientConnections;
use adapters::discovery::{MdnsAdvertiser, MdnsBrowser};
use adapters::ipps::NetworkChannel;
#[cfg(windows)]
use adapters::legacy::FileLegacySettings;
use adapters::{
    ClientProxyService, FileIdentityStore, IdentityStore, IppsServer, ServerSharingService,
    SystemElevation, DEFAULT_PORT,
};
#[cfg(feature = "desktop")]
use application::{close_action, CloseAction};
use application::{
    ChannelStore, ClientProxyState, Discovery, DiscoveryBrowser, DiscoveryService, LegacyImport,
    LegacySettingsSource, PrintFailures, PrintJobSubmitter, PrintJobTracker, QueueInstallation,
    RuntimeCoordinator, ServerAdvertiser, Setup, Sharing, Startup, StartupRegistration,
    TrustedServerPrinters,
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
    startup: Arc<Startup>,
    legacy_import: Arc<LegacyImport>,
    client_connections: Arc<ClientConnections>,
    network_channel: Arc<NetworkChannel>,
    discovery: Arc<Discovery>,
    print_failures: Arc<PrintFailures>,
    job_tracker: Arc<PrintJobTracker>,
    queue_installation: Arc<QueueInstallation>,
}

impl Shell {
    /// Builds the shell for the app data directory the identity and settings live in.
    pub fn new(data_dir: &Path) -> Result<Self, AppError> {
        let identity = Arc::new(FileIdentityStore::new(data_dir).load_or_create()?);
        let sharing = Arc::new(Sharing::with_persistence(
            default_printer_catalog(),
            Some(data_dir),
        ));
        let client_connections = Arc::new(ClientConnections::new(data_dir)?);
        let network_channel = Arc::new(NetworkChannel::open(data_dir)?);
        // Both print paths report here, so the window has one place to answer "why did my print
        // not come out?" (#39).
        let print_failures = Arc::new(PrintFailures::new());
        let job_tracker = Arc::new(PrintJobTracker::new());
        let endpoint = Arc::new(IppsServer::new(
            DEFAULT_PORT,
            identity,
            Arc::clone(&network_channel),
            default_print_job_submitter(),
            Arc::clone(&print_failures),
            Arc::clone(&job_tracker),
        ));
        let setup = Arc::new(Setup::new(Arc::new(SystemElevation::new())));
        let startup = Arc::new(Startup::new(default_startup_registration(), Some(data_dir)));
        // Moving from the previous .NET application takes the Network Channel and nothing else;
        // printer queues are always selected again (#41).
        let legacy_import = Arc::new(LegacyImport::new(
            default_legacy_settings(),
            Arc::clone(&network_channel) as Arc<dyn ChannelStore>,
            Some(data_dir),
        ));
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
                Arc::clone(&print_failures),
                Arc::clone(&job_tracker),
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
            startup,
            legacy_import,
            client_connections,
            network_channel,
            discovery,
            print_failures,
            job_tracker,
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

    pub fn startup(&self) -> Arc<Startup> {
        Arc::clone(&self.startup)
    }

    pub fn legacy_import(&self) -> Arc<LegacyImport> {
        Arc::clone(&self.legacy_import)
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

    pub fn print_failures(&self) -> Arc<PrintFailures> {
        Arc::clone(&self.print_failures)
    }

    pub fn job_tracker(&self) -> Arc<PrintJobTracker> {
        Arc::clone(&self.job_tracker)
    }

    pub fn queue_installation(&self) -> Arc<QueueInstallation> {
        Arc::clone(&self.queue_installation)
    }

    /// Restores the persistent sharing state against available local queues.
    pub async fn restore(&self) -> Result<(), AppError> {
        self.sharing.restore().await
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

/// The login-startup registration for the platform this build targets.
#[cfg(windows)]
fn default_startup_registration() -> Arc<dyn StartupRegistration> {
    Arc::new(adapters::startup::WindowsStartup::new())
}

#[cfg(not(windows))]
fn default_startup_registration() -> Arc<dyn StartupRegistration> {
    Arc::new(adapters::startup::UnsupportedStartup)
}

/// Where the previous .NET ShaPrint application kept this user's settings.
#[cfg(windows)]
fn default_legacy_settings() -> Arc<dyn LegacySettingsSource> {
    match FileLegacySettings::locate() {
        Some(settings) => Arc::new(settings),
        None => Arc::new(adapters::legacy::NoLegacySettings),
    }
}

#[cfg(not(windows))]
fn default_legacy_settings() -> Arc<dyn LegacySettingsSource> {
    Arc::new(adapters::legacy::NoLegacySettings)
}

/// What the shell keeps alive while its main window is closed (#40).
#[cfg(feature = "desktop")]
struct ShellLifecycle {
    /// Whether a tray icon exists to bring the window back and to quit from.
    tray_ready: bool,
}

#[cfg(feature = "desktop")]
#[derive(Clone)]
pub(crate) struct UpdateTrayAction {
    pub(crate) item: tauri::menu::MenuItem<tauri::Wry>,
    pub(crate) menu: tauri::menu::Menu<tauri::Wry>,
    pub(crate) inserted: Arc<std::sync::Mutex<bool>>,
}

/// Runs the desktop app until the user quits it.
///
/// `background` is set by a login launch: the proxy and discovery come up, the tray icon appears,
/// and the main window stays hidden until the user asks for it (issue #40).
#[cfg(feature = "desktop")]
pub fn run(background: bool) -> Result<(), AppError> {
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
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            ipc::commands::get_runtime_status,
            ipc::commands::start_service,
            ipc::commands::stop_service,
            ipc::commands::get_print_failures,
            ipc::commands::dismiss_print_failure,
            ipc::commands::list_local_printers,
            ipc::commands::set_shared_printers,
            ipc::commands::get_server_identity,
            ipc::commands::allow_sharing_access,
            ipc::commands::get_startup_status,
            ipc::commands::set_startup_enabled,
            ipc::commands::get_legacy_import_report,
            ipc::commands::import_legacy_settings,
            ipc::client_connections::inspect_server_connection,
            ipc::client_connections::approve_server_connection,
            ipc::client_connections::list_server_connection_printers,
            ipc::commands::install_printer_queue,
            ipc::server_settings::configure_network_channel,
            ipc::server_settings::get_network_channel_status,
            ipc::discovery::list_nearby_servers,
            ipc::commands::get_drain_status,
            ipc::updates::get_update_status,
            ipc::updates::check_for_updates,
            ipc::updates::apply_update_and_restart,
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
            ipc::emitter::forward_print_failures(app.handle().clone(), shell.print_failures());
            ipc::emitter::forward_legacy_import(app.handle().clone(), shell.legacy_import());
            app.manage(shell.runtime());
            app.manage(shell.sharing());
            app.manage(shell.endpoint());
            app.manage(shell.setup());
            app.manage(shell.client_connections());
            app.manage(shell.network_channel());
            app.manage(shell.discovery());
            app.manage(shell.print_failures());
            app.manage(shell.startup());
            app.manage(shell.legacy_import());
            app.manage(shell.queue_installation());
            let job_tracker = shell.job_tracker();
            app.manage(Arc::clone(&job_tracker));
            let updates = Arc::new(ipc::updates::UpdateManager::new(
                app.handle().clone(),
                env!("CARGO_PKG_VERSION"),
                Arc::clone(&job_tracker),
            ));
            ipc::updates::forward_status(app.handle().clone(), Arc::clone(&updates.coordinator));
            app.manage(Arc::clone(&updates));
            start_update_workers(app.handle().clone(), updates, Arc::clone(&job_tracker));

            // The tray is what makes the app reachable after its window is closed, so its
            // availability decides what closing the window means.
            let tray_ready = install_tray(app.handle());
            app.manage(ShellLifecycle { tray_ready });

            // Starting with Windows is the product default, applied once: after the user has made a
            // choice — including "off" — a launch leaves the registration alone.
            if let Err(error) = shell.startup().apply_default() {
                log::warn!(
                    "login startup is not registered code={} message={}",
                    error.code_str(),
                    error
                );
            }

            // A login launch stays out of the way: the proxy is up, and the window waits in the
            // tray until the user opens it. With no tray there would be no way back to the window
            // at all, so it is shown instead of leaving an unreachable process behind.
            if !background {
                show_main_window(app.handle());
            } else if !tray_ready {
                log::warn!("no tray is available; showing the window so ShaPrint stays reachable");
                show_main_window(app.handle());
            }

            // Start the always-on services in the background: the window paints immediately, then
            // follows their status through the event stream. A service that cannot start is left
            // `failed` for the user to retry instead of taking the shell down. Server sharing is
            // not an autostart service, so a login launch never starts sharing on its own.
            //
            // The import runs first, so the client proxy already has the Network Channel the user
            // brought over from the previous application (#41).
            let runtime = shell.runtime();
            let legacy_import = shell.legacy_import();
            let sharing = shell.sharing();
            drop(tauri::async_runtime::spawn(async move {
                match legacy_import.apply().await {
                    Ok(report) => log::info!(
                        "legacy import found={} channel={} queues_need_reselection={}",
                        report.found,
                        report.channel.as_str(),
                        report.queues_need_reselection
                    ),
                    Err(error) => log::warn!(
                        "legacy import did not finish code={} message={}",
                        error.code_str(),
                        error
                    ),
                }
                if let Err(error) = sharing.restore().await {
                    log::warn!(
                        "cannot restore server sharing code={} message={}",
                        error.code_str(),
                        error
                    );
                }
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
        .on_window_event(|window, event| {
            use tauri::Manager;

            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let tray_ready = window
                    .try_state::<ShellLifecycle>()
                    .is_some_and(|lifecycle| lifecycle.tray_ready);
                if close_action(tray_ready) == CloseAction::HideToTray {
                    // Closing the window is not quitting: the proxy must stay reachable for
                    // installed queues, and Quit lives in the tray menu.
                    api.prevent_close();
                    if let Err(error) = window.hide() {
                        log::warn!("cannot hide the main window message={error}");
                    }
                }
            }
        })
        .build(tauri::generate_context!())
        .map_err(|error| AppError::internal(format!("cannot start the desktop shell: {error}")))?
        .run(move |app, event| {
            use tauri::Manager;

            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                // The user quit, or the window closed with no tray to hide in: hold the exit, stop
                // every background task, then exit. The guard ignores the second request raised by
                // our own `exit` call.
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

#[cfg(feature = "desktop")]
fn start_update_workers(
    app: tauri::AppHandle,
    updates: Arc<ipc::updates::UpdateManager>,
    jobs: Arc<PrintJobTracker>,
) {
    use application::{
        UpdateState, INITIAL_CHECK_DELAY, RESTART_POLL_INTERVAL, UPDATE_CHECK_INTERVAL,
    };
    use tauri::Manager;

    let polling_manager = Arc::clone(&updates);
    drop(tauri::async_runtime::spawn(async move {
        tokio::time::sleep(INITIAL_CHECK_DELAY).await;
        loop {
            if let Err(error) = polling_manager
                .check(application::CheckKind::Background)
                .await
            {
                log::warn!("background update check failed; ShaPrint will retry later");
                let _ = error;
            }
            tokio::time::sleep(UPDATE_CHECK_INTERVAL).await;
        }
    }));

    let restart_app = app.clone();
    let manager = Arc::clone(&updates);
    let runtime = app
        .try_state::<Arc<RuntimeCoordinator>>()
        .map(|state| Arc::clone(state.inner()));
    let sharing = app
        .try_state::<Arc<Sharing>>()
        .map(|state| Arc::clone(state.inner()));
    drop(tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(RESTART_POLL_INTERVAL).await;
            let hidden = restart_app
                .get_webview_window("main")
                .and_then(|window| window.is_visible().ok())
                .is_some_and(|visible| !visible);
            manager
                .coordinator
                .request_auto_restart_if_idle(hidden, application::QUIET_WINDOW);
            if matches!(
                manager.coordinator.status().state,
                UpdateState::ReadyToRestart {
                    restart_requested: true,
                    ..
                }
            ) {
                let Some(_restart_permit) = jobs.try_begin_restart() else {
                    continue;
                };
                if let Some(runtime) = &runtime {
                    if let Err(_error) = runtime.shutdown().await {
                        manager.coordinator.fail(
                            "ShaPrint could not stop its print services cleanly. No update was installed.".to_owned(),
                        );
                        restore_services_after_failed_update(Some(runtime), sharing.as_ref()).await;
                        continue;
                    }
                }
                match manager.install_if_requested().await {
                    Ok(true) => restart_app.restart(),
                    Ok(false) => {}
                    Err(_error) => {
                        manager.coordinator.fail(
                            "The verified update could not be installed. ShaPrint's print services were restarted.".to_owned(),
                        );
                        restore_services_after_failed_update(runtime.as_ref(), sharing.as_ref())
                            .await;
                    }
                }
            }
        }
    }));
}

#[cfg(feature = "desktop")]
async fn restore_services_after_failed_update(
    runtime: Option<&Arc<RuntimeCoordinator>>,
    sharing: Option<&Arc<Sharing>>,
) {
    if let Some(sharing) = sharing {
        if sharing.restore().await.is_err() {
            log::warn!("server sharing could not be restored after updater failure");
        }
    }
    if let Some(runtime) = runtime {
        if runtime.start_autostart().await.is_err() {
            log::warn!("print services could not be restored after updater failure");
        }
    }
}

/// Brings the main window back, creating nothing if it was closed.
#[cfg(feature = "desktop")]
fn show_main_window(app: &tauri::AppHandle) {
    use tauri::Manager;

    let Some(window) = app.get_webview_window("main") else {
        log::warn!("the main window is gone; ShaPrint is running in the tray only");
        return;
    };
    if let Err(error) = window.show() {
        log::warn!("cannot show the main window message={error}");
    }
    let _ = window.unminimize();
    if let Err(error) = window.set_focus() {
        log::warn!("cannot focus the main window message={error}");
    }
}

/// Builds the tray icon with its menu.
///
/// Returns whether the app can still be reached once its window is closed: a tray that could not
/// be created leaves the window as the only way in, which is what closing then means.
#[cfg(feature = "desktop")]
fn install_tray(app: &tauri::AppHandle) -> bool {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
    use tauri::Manager;

    // Without a bundled icon the tray entry would be invisible on Windows, which is worse than no
    // tray: the user could not find the app again after closing its window.
    let Some(icon) = app.default_window_icon().cloned() else {
        log::warn!("no window icon is bundled; ShaPrint will exit when its window closes");
        return false;
    };

    let build_menu = || -> Result<Menu<tauri::Wry>, tauri::Error> {
        let open = MenuItem::with_id(app, "open", "Open ShaPrint", true, None::<&str>)?;
        let update = MenuItem::with_id(
            app,
            "restart-update",
            "Restart to Update",
            true,
            None::<&str>,
        )?;
        let quit = MenuItem::with_id(app, "quit", "Quit ShaPrint", true, None::<&str>)?;
        let menu = Menu::with_items(app, &[&open, &quit])?;
        app.manage(UpdateTrayAction {
            item: update,
            menu: menu.clone(),
            inserted: Arc::new(std::sync::Mutex::new(false)),
        });
        Ok(menu)
    };
    let menu = match build_menu() {
        Ok(menu) => menu,
        Err(error) => {
            log::warn!("cannot build the tray menu message={error}");
            return false;
        }
    };

    let tray = TrayIconBuilder::new()
        .icon(icon)
        .tooltip("ShaPrint")
        .menu(&menu)
        // The menu is the confirmed way to quit; a left click just brings the window back.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().0.as_str() {
            "open" => show_main_window(app),
            "restart-update" => {
                if let Some(updates) = app.try_state::<ipc::updates::SharedUpdates>() {
                    updates.request_restart();
                }
            }
            "quit" => app.exit(0),
            other => log::warn!("unknown tray menu action={other}"),
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app);

    match tray {
        Ok(_) => true,
        Err(error) => {
            log::warn!("cannot create the tray icon message={error}");
            false
        }
    }
}
