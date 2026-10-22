//! One control plane behind every administrative Tauri command.
//!
//! On Windows the window owns the runtimes in-process ([`ControlPlane::Local`]); on Linux the
//! window is a client of the machine daemon ([`ControlPlane::Machine`]): reads go over the same
//! Unix socket `shaprintctl` uses, and mutations run through the polkit-wrapped CLI helper so
//! the GUI itself never runs as root (ADR 0018). Commands never reach into daemon state files
//! on either path, and both paths surface the same stable `ErrorCode`s.

use std::sync::Arc;

#[cfg(unix)]
use std::path::PathBuf;

use crate::domain::{AppError, PrinterName, ServiceId};
use crate::ipc::client_connections::{
    ServerConnectionPrintersDto, ServerConnectionReviewDto, TrustedServerDto, TrustedServerProbeDto,
};
use crate::ipc::dto::{
    ClientQueueDto, DiagnosticsReportDto, LocalPrintersDto, RecognisedClientQueueDto,
    RuntimeStatusDto, ServerIdentityDto,
};
use crate::Shell;

#[cfg(unix)]
use crate::adapters::client_connections::{
    ClientConnections, TrustedServerProbe, TrustedServerStatus,
};
#[cfg(unix)]
use crate::adapters::daemon::{DaemonClient, ServerTrustReviewDto};
#[cfg(unix)]
use crate::adapters::elevated_ctl::{ElevatedCtl, ElevatedMutation, ElevatedOutcome};

/// Environment override for the machine daemon control socket (tests and development).
#[cfg(unix)]
pub const SOCKET_ENV: &str = "SHAPRINT_SOCKET";

/// Environment override keeping Linux on the in-process control plane (development against a
/// build without an installed `shaprintd`). Production Linux builds always control the daemon.
#[cfg(unix)]
pub const CONTROL_ENV: &str = "SHAPRINT_CONTROL";

/// The control plane a command dispatches through.
#[derive(Clone)]
pub enum ControlPlane {
    /// The window supervises the runtimes itself (Windows).
    Local(Arc<Shell>),
    /// The window controls `shaprintd` over its local Unix socket (Linux).
    #[cfg(unix)]
    Machine(MachineControl),
}

/// How the window reaches the print services it controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ControlMode {
    /// In-process runtimes owned by this window.
    Local,
    /// The machine daemon `shaprintd`.
    Machine,
}

impl ControlMode {
    /// The stable string the frontend contract and log lines use.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Machine => "machine",
        }
    }
}

/// Shared state the administrative commands are managed with.
pub type SharedControl = Arc<ControlPlane>;

/// The Linux client half of the machine daemon control plane: unprivileged reads over the Unix
/// socket, polkit-authorized mutations through the elevated `shaprintctl` helper.
#[cfg(unix)]
#[derive(Debug, Clone)]
pub struct MachineControl {
    client: DaemonClient,
    elevated: ElevatedCtl,
}

#[cfg(unix)]
impl MachineControl {
    pub fn new(client: DaemonClient, elevated: ElevatedCtl) -> Self {
        Self { client, elevated }
    }

    pub fn client(&self) -> &DaemonClient {
        &self.client
    }

    async fn run_mutation(&self, mutation: ElevatedMutation) -> Result<ElevatedOutcome, AppError> {
        // An unreachable daemon cannot apply the requested change. Report its recovery
        // before asking the user to authenticate for a helper that would inevitably fail.
        self.client.status().await?;
        self.elevated.run(mutation).await
    }
}

/// The control socket the window talks to: `SHAPRINT_SOCKET` when set, the machine default
/// otherwise.
#[cfg(unix)]
pub fn default_socket_path() -> PathBuf {
    std::env::var_os(SOCKET_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(crate::adapters::daemon::DEFAULT_SOCKET_PATH))
}

/// The control plane this window administers on this build.
///
/// Linux runs the print services as the machine daemon `shaprintd`, so the window is a client of
/// it — `SHAPRINT_CONTROL=local` (development only) keeps the in-process plane instead.
/// Everywhere else the window owns the runtimes itself.
pub fn system_control_plane(shell: &Arc<Shell>) -> SharedControl {
    #[cfg(unix)]
    {
        let local = std::env::var(CONTROL_ENV)
            .map(|value| value.eq_ignore_ascii_case("local"))
            .unwrap_or(false);
        if !local {
            log::info!("control=machine socket={}", default_socket_path().display());
            return Arc::new(ControlPlane::machine(default_socket_path()));
        }
        log::info!("control=local override={CONTROL_ENV}");
    }
    Arc::new(ControlPlane::local(Arc::clone(shell)))
}

impl ControlPlane {
    /// Passive evidence for a selected native Client queue; never invokes an elevated helper.
    pub async fn inspect_local_client_queue(
        &self,
        target: RecognisedClientQueueDto,
    ) -> Result<crate::ipc::dto::LocalTroubleshootingDto, AppError> {
        let parsed = target.parse_target()?;
        match self {
            Self::Local(shell) => Ok(crate::application::LocalTroubleshooting::new(
                shell.printer_catalog(),
                shell.runtime(),
            )
            .with_server_inspection(
                shell.client_connections(),
                shell.network_channel(),
                shell.print_failures(),
            )
            .inspect(parsed)
            .await
            .into()),
            #[cfg(unix)]
            Self::Machine(machine) => machine.client.inspect_local_client_queue(target).await,
        }
    }

    /// The in-process control plane (Windows and tests).
    pub fn local(shell: Arc<Shell>) -> Self {
        Self::Local(shell)
    }

    /// The machine daemon control plane with the default socket and packaged helper paths.
    #[cfg(unix)]
    pub fn machine(socket_path: impl Into<PathBuf>) -> Self {
        let socket_path = socket_path.into();
        Self::Machine(MachineControl::new(
            DaemonClient::new(&socket_path),
            ElevatedCtl::system(&socket_path),
        ))
    }

    /// The machine daemon control plane with explicit client and helper paths (tests).
    #[cfg(unix)]
    pub fn machine_with(client: DaemonClient, elevated: ElevatedCtl) -> Self {
        Self::Machine(MachineControl::new(client, elevated))
    }

    pub fn mode(&self) -> ControlMode {
        match self {
            Self::Local(_) => ControlMode::Local,
            #[cfg(unix)]
            Self::Machine(_) => ControlMode::Machine,
        }
    }

    /// The in-process shell, when this window owns the runtimes (never on Linux).
    pub fn local_shell(&self) -> Option<&Arc<Shell>> {
        match self {
            Self::Local(shell) => Some(shell),
            #[cfg(unix)]
            Self::Machine(_) => None,
        }
    }

    /// Live status of every supervised service.
    pub async fn status(&self) -> Result<RuntimeStatusDto, AppError> {
        match self {
            Self::Local(shell) => Ok(RuntimeStatusDto::from(&shell.runtime().status())),
            #[cfg(unix)]
            Self::Machine(machine) => machine.client.status().await,
        }
    }

    /// Starts one supervised service.
    pub async fn start_service(&self, id: ServiceId) -> Result<RuntimeStatusDto, AppError> {
        match self {
            Self::Local(shell) => {
                shell.runtime().start(id).await?;
                Ok(RuntimeStatusDto::from(&shell.runtime().status()))
            }
            #[cfg(unix)]
            Self::Machine(machine) => match machine
                .run_mutation(ElevatedMutation::StartService(id.as_str().to_owned()))
                .await?
            {
                ElevatedOutcome::Status(status) => Ok(status),
                other => Err(unexpected_outcome("start service", &other)),
            },
        }
    }

    /// Stops one supervised service.
    pub async fn stop_service(&self, id: ServiceId) -> Result<RuntimeStatusDto, AppError> {
        match self {
            Self::Local(shell) => {
                shell.runtime().stop(id).await?;
                Ok(RuntimeStatusDto::from(&shell.runtime().status()))
            }
            #[cfg(unix)]
            Self::Machine(machine) => match machine
                .run_mutation(ElevatedMutation::StopService(id.as_str().to_owned()))
                .await?
            {
                ElevatedOutcome::Status(status) => Ok(status),
                other => Err(unexpected_outcome("stop service", &other)),
            },
        }
    }

    /// Stops every supervised service and the machine daemon itself.
    ///
    /// Only the machine daemon control plane can do this; on Windows the window *is* the
    /// runtime, so quitting the window is how services stop.
    pub async fn shutdown_daemon(&self) -> Result<RuntimeStatusDto, AppError> {
        match self {
            Self::Local(_) => Err(AppError::unsupported(
                "stopping the machine daemon is only available when ShaPrint runs as a system service",
            )),
            #[cfg(unix)]
            Self::Machine(machine) => match machine.run_mutation(ElevatedMutation::StopDaemon).await? {
                ElevatedOutcome::Status(status) => Ok(status),
                other => Err(unexpected_outcome("stop the machine daemon", &other)),
            },
        }
    }

    /// The Server certificate fingerprint clients approve and the port they connect to.
    pub async fn server_identity(&self) -> Result<ServerIdentityDto, AppError> {
        match self {
            Self::Local(shell) => Ok(ServerIdentityDto {
                fingerprint: shell.endpoint().fingerprint().to_string(),
                port: shell.endpoint().port(),
            }),
            #[cfg(unix)]
            Self::Machine(machine) => machine.client.server_identity().await,
        }
    }

    /// Local printer queues and whether each is currently shared.
    pub async fn local_printers(&self) -> Result<LocalPrintersDto, AppError> {
        match self {
            Self::Local(shell) => {
                let printers = shell.sharing().local_printers().await?;
                Ok(LocalPrintersDto::from(printers.as_slice()))
            }
            #[cfg(unix)]
            Self::Machine(machine) => Ok(LocalPrintersDto {
                printers: machine.client.list_local_printers().await?,
            }),
        }
    }

    /// Evaluates system diagnostics across daemon, ports, CUPS, trust, credential, and firewall.
    pub async fn diagnostics(&self) -> Result<DiagnosticsReportDto, AppError> {
        match self {
            Self::Local(shell) => {
                #[cfg(unix)]
                {
                    Ok(crate::adapters::diagnostics::diagnose_daemon_environment(
                        Some(shell),
                        std::path::Path::new(crate::adapters::daemon::DEFAULT_SOCKET_PATH),
                        &crate::RuntimePorts::default(),
                        None,
                        None,
                    )
                    .await)
                }
                #[cfg(not(unix))]
                {
                    Ok(DiagnosticsReportDto {
                        checks: vec![crate::ipc::dto::DiagnosticCheckDto {
                            check: "daemon".to_owned(),
                            status: "ok".to_owned(),
                            code: None,
                            detail: "in-process desktop runtime".to_owned(),
                            recovery: None,
                        }],
                        healthy: true,
                        recent_failure: shell
                            .print_failures()
                            .latest()
                            .as_ref()
                            .map(crate::ipc::dto::PrintFailureDto::from),
                    })
                }
            }
            #[cfg(unix)]
            Self::Machine(machine) => match machine.client.diagnostics().await {
                Ok(report) => Ok(report),
                Err(err) if err.code() == crate::domain::ErrorCode::ServerUnavailable => {
                    Ok(crate::adapters::diagnostics::diagnose_host(
                        machine.client.socket_path(),
                        None,
                        None,
                        None,
                    )
                    .await)
                }
                Err(err) => Err(err),
            },
        }
    }

    /// Replaces the shared printer selection and returns the queues with their new state.
    pub async fn set_shared_printers(
        &self,
        printers: Vec<PrinterName>,
    ) -> Result<LocalPrintersDto, AppError> {
        match self {
            Self::Local(shell) => {
                let updated = shell.sharing().set_shared(printers).await?;
                Ok(LocalPrintersDto::from(updated.as_slice()))
            }
            #[cfg(unix)]
            Self::Machine(machine) => {
                let names: Vec<String> =
                    printers.iter().map(|printer| printer.to_string()).collect();
                match machine
                    .run_mutation(ElevatedMutation::SetSharedPrinters(names))
                    .await?
                {
                    ElevatedOutcome::LocalPrinters(printers) => Ok(LocalPrintersDto { printers }),
                    other => Err(unexpected_outcome("set shared printers", &other)),
                }
            }
        }
    }

    /// Whether a Network Channel verifier exists — never its value.
    pub async fn network_channel_configured(&self) -> Result<bool, AppError> {
        match self {
            Self::Local(shell) => Ok(shell.network_channel().is_configured()),
            #[cfg(unix)]
            Self::Machine(machine) => Ok(machine.client.network_channel_status().await?.configured),
        }
    }

    /// Configures or replaces the machine Network Channel; reports only configured status.
    pub async fn configure_network_channel(&self, channel: &str) -> Result<bool, AppError> {
        match self {
            Self::Local(shell) => shell.network_channel().configure(channel).await,
            #[cfg(unix)]
            Self::Machine(machine) => {
                match machine
                    .run_mutation(ElevatedMutation::SetNetworkChannel(channel.to_owned()))
                    .await?
                {
                    ElevatedOutcome::NetworkChannel(status) => Ok(status.configured),
                    other => Err(unexpected_outcome("configure the Network Channel", &other)),
                }
            }
        }
    }

    /// Reviews a Server's certificate fingerprint before any printer query.
    pub async fn inspect_server(
        &self,
        address: &str,
    ) -> Result<ServerConnectionReviewDto, AppError> {
        match self {
            Self::Local(shell) => shell
                .client_connections()
                .inspect(address)
                .await
                .map(ServerConnectionReviewDto::from),
            #[cfg(unix)]
            Self::Machine(machine) => machine
                .client
                .inspect_server(address)
                .await
                .map(|review| review_dto_from_daemon(&review)),
        }
    }

    /// Approves or reapproves a Server's certificate fingerprint as a machine Trusted Server.
    pub async fn approve_server(
        &self,
        address: &str,
        fingerprint: &str,
    ) -> Result<ServerConnectionReviewDto, AppError> {
        match self {
            Self::Local(shell) => shell
                .client_connections()
                .approve(address, fingerprint)
                .await
                .map(ServerConnectionReviewDto::from),
            #[cfg(unix)]
            Self::Machine(machine) => match machine
                .run_mutation(ElevatedMutation::ApproveServer {
                    address: address.to_owned(),
                    fingerprint: fingerprint.to_owned(),
                })
                .await?
            {
                ElevatedOutcome::Review(review) => Ok(review_dto_from_daemon(&review)),
                other => Err(unexpected_outcome("approve the server", &other)),
            },
        }
    }

    /// Lists every machine-wide Trusted Server.
    pub async fn list_trusted_servers(&self) -> Result<Vec<TrustedServerDto>, AppError> {
        match self {
            Self::Local(shell) => Ok(shell
                .client_connections()
                .list_trusted_servers()
                .await?
                .into_iter()
                .map(TrustedServerDto::from)
                .collect()),
            #[cfg(unix)]
            Self::Machine(machine) => Ok(machine
                .client
                .list_trusted_servers()
                .await?
                .into_iter()
                .map(|entry| TrustedServerDto {
                    address: entry.address,
                    fingerprint: entry.fingerprint,
                })
                .collect()),
        }
    }

    /// Queries the shared printers of an explicitly approved Trusted Server.
    pub async fn list_server_printers(
        &self,
        address: &str,
    ) -> Result<ServerConnectionPrintersDto, AppError> {
        match self {
            Self::Local(shell) => shell
                .client_connections()
                .printers(address)
                .await
                .map(ServerConnectionPrintersDto::from),
            #[cfg(unix)]
            Self::Machine(machine) => {
                machine
                    .client
                    .list_server_printers(address)
                    .await
                    .map(|printers| ServerConnectionPrintersDto {
                        address: printers.address,
                        printers: printers.printers,
                    })
            }
        }
    }

    /// Removes a Server from the machine Trusted Servers.
    pub async fn forget_server(&self, address: &str) -> Result<(), AppError> {
        match self {
            Self::Local(shell) => {
                shell
                    .client_connections()
                    .forget_trusted_server(address)
                    .await
            }
            #[cfg(unix)]
            Self::Machine(machine) => {
                machine
                    .run_mutation(ElevatedMutation::ForgetServer(address.to_owned()))
                    .await?;
                Ok(())
            }
        }
    }

    /// Probes a Trusted Server over unicast TLS without changing any trust.
    pub async fn probe_server(&self, address: &str) -> Result<TrustedServerProbeDto, AppError> {
        match self {
            Self::Local(shell) => shell
                .client_connections()
                .probe_trusted_server(address)
                .await
                .map(TrustedServerProbeDto::from),
            #[cfg(unix)]
            Self::Machine(machine) => probe_via_daemon(machine, address)
                .await
                .map(TrustedServerProbeDto::from),
        }
    }

    /// Lists the recognised ShaPrint Client queues installed in the local spooler.
    pub async fn list_recognised_client_queues(
        &self,
    ) -> Result<Vec<RecognisedClientQueueDto>, AppError> {
        match self {
            Self::Local(shell) => Ok(shell
                .printer_catalog()
                .recognised_client_queues()
                .await?
                .into_iter()
                .map(RecognisedClientQueueDto::from)
                .collect()),
            #[cfg(unix)]
            Self::Machine(machine) => machine.client.list_client_queues().await,
        }
    }

    /// Installs (or updates) a machine-wide Client queue for a shared printer.
    pub async fn install_client_queue(
        &self,
        address: &str,
        printer: &str,
    ) -> Result<ClientQueueDto, AppError> {
        match self {
            Self::Local(shell) => shell
                .queue_installation()
                .install(address, printer)
                .await
                .map(|queue| ClientQueueDto::from(&queue)),
            #[cfg(unix)]
            Self::Machine(machine) => match machine
                .run_mutation(ElevatedMutation::InstallClientQueue {
                    address: address.to_owned(),
                    printer: printer.to_owned(),
                })
                .await?
            {
                ElevatedOutcome::Queue(queue) => Ok(queue),
                other => Err(unexpected_outcome("install the client queue", &other)),
            },
        }
    }

    /// Recreates an existing machine-wide Client queue so it reloads server capabilities.
    pub async fn repair_client_queue(
        &self,
        address: &str,
        printer: &str,
    ) -> Result<ClientQueueDto, AppError> {
        match self {
            Self::Local(shell) => shell
                .queue_installation()
                .repair(address, printer)
                .await
                .map(|queue| ClientQueueDto::from(&queue)),
            #[cfg(unix)]
            Self::Machine(machine) => {
                let parsed_address =
                    crate::adapters::client_connections::ServerAddress::parse(address)?;
                let parsed_printer = crate::domain::PrinterName::parse(printer)?;
                let request = crate::domain::ClientQueueRequest::new(
                    &parsed_address.normalized,
                    parsed_printer,
                )?;
                let target = crate::ipc::dto::RecognisedClientQueueDto {
                    queue_name: request.queue_name().to_string(),
                    server_address: parsed_address.normalized.clone(),
                    printer_name: printer.to_owned(),
                };
                match machine.client.inspect_local_client_queue(target).await {
                    Ok(inspection) => {
                        if !inspection.queue_empty {
                            return Err(AppError::invalid_state(
                                "print jobs are pending or the Client queue state could not be verified; cancel or wait for them to finish before repairing",
                            ));
                        }
                    }
                    Err(err) => {
                        return Err(AppError::invalid_state(format!(
                            "the Client queue state could not be inspected: {err}; repair remains blocked until the queue is known to be empty",
                        )));
                    }
                }
                match machine
                    .run_mutation(ElevatedMutation::RepairClientQueue {
                        address: address.to_owned(),
                        printer: printer.to_owned(),
                    })
                    .await?
                {
                    ElevatedOutcome::Queue(queue) => Ok(queue),
                    other => Err(unexpected_outcome("repair the client queue", &other)),
                }
            }
        }
    }
}

#[cfg(unix)]
fn review_dto_from_daemon(review: &ServerTrustReviewDto) -> ServerConnectionReviewDto {
    ServerConnectionReviewDto {
        address: review.address.clone(),
        current_fingerprint: review.current_fingerprint.clone(),
        previous_fingerprint: review.previous_fingerprint.clone(),
        trusted: review.trusted,
    }
}

/// Probes a Trusted Server using only unprivileged daemon reads, reproducing the in-process
/// probe: offline servers keep their approval, a changed certificate is reported without
/// touching it, and printers are queried only while the pinned identity still matches.
#[cfg(unix)]
async fn probe_via_daemon(
    machine: &MachineControl,
    address: &str,
) -> Result<TrustedServerProbe, AppError> {
    const NOT_APPROVED: &str = "This server is not approved. Inspect its certificate fingerprint and explicitly approve it before probing its status.";

    let normalized = ClientConnections::normalize_address(address)?;
    let approved = machine
        .client
        .list_trusted_servers()
        .await?
        .into_iter()
        .find(|server| server.address == normalized)
        .map(|server| server.fingerprint)
        .ok_or_else(|| AppError::server_not_trusted(NOT_APPROVED))?;

    let review = match machine.client.inspect_server(address).await {
        Ok(review) => review,
        Err(_) => {
            return Ok(TrustedServerProbe {
                address: normalized,
                status: TrustedServerStatus::Offline,
                approved_fingerprint: approved,
                current_fingerprint: None,
                printers: Vec::new(),
            })
        }
    };
    if review.current_fingerprint != approved {
        return Ok(TrustedServerProbe {
            address: normalized,
            status: TrustedServerStatus::IdentityChanged,
            approved_fingerprint: approved,
            current_fingerprint: Some(review.current_fingerprint),
            printers: Vec::new(),
        });
    }
    let current = review.current_fingerprint;
    match machine.client.list_server_printers(address).await {
        Ok(printers) => Ok(TrustedServerProbe {
            address: normalized,
            status: TrustedServerStatus::Online,
            approved_fingerprint: approved,
            current_fingerprint: Some(current),
            printers: printers.printers,
        }),
        Err(_) => Ok(TrustedServerProbe {
            address: normalized,
            status: TrustedServerStatus::Offline,
            approved_fingerprint: approved,
            current_fingerprint: Some(current),
            printers: Vec::new(),
        }),
    }
}

/// Maps a helper result that does not match the mutation's contract to a stable internal error;
/// only reachable if the packaged `shaprintctl` changed shape under us.
#[cfg(unix)]
fn unexpected_outcome(operation: &str, _outcome: &ElevatedOutcome) -> AppError {
    AppError::internal(format!("unexpected result while trying to {operation}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_mode_serializes_for_the_frontend_contract() {
        assert_eq!(
            serde_json::to_value(ControlMode::Local).expect("serializes"),
            serde_json::json!("local")
        );
        assert_eq!(
            serde_json::to_value(ControlMode::Machine).expect("serializes"),
            serde_json::json!("machine")
        );
    }

    /// What the exit handler consults before stopping anything: a machine window owns no local
    /// runtime, so closing it leaves `shaprintd`'s Client proxy and enabled Server running (#130).
    #[cfg(unix)]
    #[test]
    fn closing_a_machine_window_has_no_local_runtime_to_stop() {
        let machine = ControlPlane::machine_with(
            DaemonClient::new("/run/shaprint/shaprintd.sock"),
            ElevatedCtl::system("/run/shaprint/shaprintd.sock"),
        );
        assert_eq!(machine.mode(), ControlMode::Machine);
        assert!(machine.local_shell().is_none());
    }

    /// The local plane still hands the exit handler the in-process runtime it must stop on the
    /// way out, so Windows behavior is unchanged.
    #[test]
    fn closing_a_local_window_stops_the_in_process_runtime() {
        let data_dir =
            std::env::temp_dir().join(format!("shaprint-control-plane-{}", std::process::id()));
        std::fs::create_dir_all(&data_dir).expect("creates the shell data directory");

        let local = ControlPlane::Local(Arc::new(Shell::new(&data_dir).expect("builds the shell")));
        assert_eq!(local.mode(), ControlMode::Local);
        assert!(local.local_shell().is_some());

        let _ = std::fs::remove_dir_all(&data_dir);
    }
}
