//! Local Unix domain socket control plane for the Linux machine daemon (`shaprintd`) and CLI
//! (`shaprintctl`).
//!
//! `shaprintd` supervises the existing `RuntimeCoordinator` outside Tauri and exposes a single
//! local Unix domain socket. Status reads return `RuntimeStatusDto` without secrets or print job
//! content; no TCP administration listener is opened (ADR 0018).

use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::watch;
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::timeout;

use crate::adapters::client_connections::{ConnectionPrinters, ConnectionReview, TrustedServer};
use crate::adapters::ipps::CLIENT_CREDENTIAL_FILE;
use crate::application::RuntimeCoordinator;
use crate::domain::{AppError, ErrorCode, PrinterName, ServiceId};
use crate::ipc::client_connections::RecognisedClientQueueDto;
use crate::ipc::dto::{
    AppErrorDto, ClientQueueDto, LocalPrinterDto, LocalPrintersDto, RuntimeStatusDto,
};
use crate::{RuntimePorts, Shell};

/// Default non-secret configuration directory on Linux (ADR 0018).
pub const DEFAULT_CONFIG_DIR: &str = "/etc/shaprint";

/// Default non-secret configuration file name inside [`DEFAULT_CONFIG_DIR`].
pub const DEFAULT_CONFIG_FILE: &str = "config.json";

/// Default root-protected machine credential directory on Linux (outside `/etc/shaprint`, ADR 0018).
pub const DEFAULT_CREDENTIAL_DIR: &str = "/etc/shaprint-credentials";

/// Default durable state directory on Linux (ADR 0018).
pub const DEFAULT_STATE_DIR: &str = "/var/lib/shaprint";

/// Default runtime control socket path on Linux (ADR 0018).
pub const DEFAULT_SOCKET_PATH: &str = "/run/shaprint/shaprintd.sock";

const MAX_MESSAGE_BYTES: usize = 8 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(15);
const ACCEPT_RETRY_DELAY: Duration = Duration::from_millis(25);

/// Authorization policy applied to kernel-verified `SO_PEERCRED` peer credentials on the local
/// Unix control socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerAccessPolicy {
    /// Allows mutations from root (`uid == 0`, e.g. `sudo shaprintctl`) or the daemon's own
    /// effective UID; all verified local peers may read status.
    RootOrDaemonOwner,
    /// Allows mutations only from root (`uid == 0`); all verified local peers may read status.
    RootOnly,
    /// Allows mutations only from the specified UID; all verified local peers may read status.
    AllowUid(u32),
}

impl PeerAccessPolicy {
    fn authorize_mutation(self, peer_uid: u32, daemon_uid: Option<u32>) -> Result<(), AppError> {
        let allowed = match self {
            Self::RootOrDaemonOwner => peer_uid == 0 || daemon_uid == Some(peer_uid),
            Self::RootOnly => peer_uid == 0,
            Self::AllowUid(expected) => peer_uid == expected,
        };
        if allowed {
            Ok(())
        } else {
            Err(AppError::not_authorized(
                "administrative changes require root privileges (run with sudo)",
            ))
        }
    }
}

/// Reads the current process's effective UID directly from kernel `SO_PEERCRED` on an in-memory
/// socket pair so it remains accurate regardless of `/proc` mounts or `PR_SET_DUMPABLE`.
fn current_effective_uid() -> Option<u32> {
    let (left, _right) = UnixStream::pair().ok()?;
    left.peer_cred().ok().map(|cred| cred.uid())
}

/// Configuration for launching `shaprintd`.
#[derive(Debug, Clone)]
pub struct DaemonConfig {
    config_dir: PathBuf,
    credential_dir: Option<PathBuf>,
    state_dir: PathBuf,
    socket_path: PathBuf,
    access_policy: PeerAccessPolicy,
    forbid_root: bool,
    ports: RuntimePorts,
    cups_endpoint: Option<crate::adapters::printers::cups::CupsServerEndpoint>,
    firewall_program: Option<PathBuf>,
}

impl DaemonConfig {
    pub fn new(state_dir: impl Into<PathBuf>, socket_path: impl Into<PathBuf>) -> Self {
        Self {
            config_dir: PathBuf::from(DEFAULT_CONFIG_DIR),
            credential_dir: None,
            state_dir: state_dir.into(),
            socket_path: socket_path.into(),
            access_policy: PeerAccessPolicy::RootOrDaemonOwner,
            forbid_root: true,
            ports: RuntimePorts::default(),
            cups_endpoint: None,
            firewall_program: None,
        }
    }

    #[must_use]
    pub fn with_cups_endpoint(
        mut self,
        endpoint: crate::adapters::printers::cups::CupsServerEndpoint,
    ) -> Self {
        self.cups_endpoint = Some(endpoint);
        self
    }

    #[must_use]
    pub fn with_firewall_program(mut self, program: impl Into<PathBuf>) -> Self {
        self.firewall_program = Some(program.into());
        self
    }

    #[must_use]
    pub fn with_config_dir(mut self, config_dir: impl Into<PathBuf>) -> Self {
        self.config_dir = config_dir.into();
        self
    }

    #[must_use]
    pub fn with_credential_dir(mut self, credential_dir: impl Into<PathBuf>) -> Self {
        self.credential_dir = Some(credential_dir.into());
        self
    }

    #[must_use]
    pub fn with_access_policy(mut self, policy: PeerAccessPolicy) -> Self {
        self.access_policy = policy;
        self
    }

    #[must_use]
    pub fn with_forbid_root(mut self, forbid_root: bool) -> Self {
        self.forbid_root = forbid_root;
        self
    }

    #[must_use]
    pub fn with_ports(mut self, ports: RuntimePorts) -> Self {
        self.ports = ports;
        self
    }

    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    pub fn credential_dir(&self) -> PathBuf {
        if let Some(dir) = &self.credential_dir {
            return dir.clone();
        }
        if self.config_dir == Path::new(DEFAULT_CONFIG_DIR) {
            PathBuf::from(DEFAULT_CREDENTIAL_DIR)
        } else {
            self.config_dir
                .parent()
                .map(|parent| parent.join("shaprint-credentials"))
                .unwrap_or_else(|| PathBuf::from(DEFAULT_CREDENTIAL_DIR))
        }
    }

    pub fn state_dir(&self) -> &Path {
        &self.state_dir
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub fn access_policy(&self) -> PeerAccessPolicy {
        self.access_policy
    }

    pub fn forbid_root(&self) -> bool {
        self.forbid_root
    }

    pub fn ports(&self) -> &RuntimePorts {
        &self.ports
    }
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self::new(DEFAULT_STATE_DIR, DEFAULT_SOCKET_PATH)
    }
}

/// Safe Network Channel configured status reported over the Unix socket; never carries the secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkChannelStatusDto {
    pub configured: bool,
}

/// Server certificate fingerprint review returned by `shaprintctl trust inspect` and `approve`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerTrustReviewDto {
    pub address: String,
    pub current_fingerprint: String,
    pub previous_fingerprint: Option<String>,
    pub trusted: bool,
}

impl From<ConnectionReview> for ServerTrustReviewDto {
    fn from(review: ConnectionReview) -> Self {
        Self {
            address: review.address,
            current_fingerprint: review.current_fingerprint,
            previous_fingerprint: review.previous_fingerprint,
            trusted: review.trusted,
        }
    }
}

/// One Trusted Server stored in the machine trust store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustedServerEntryDto {
    pub address: String,
    pub fingerprint: String,
}

impl From<TrustedServer> for TrustedServerEntryDto {
    fn from(server: TrustedServer) -> Self {
        Self {
            address: server.address,
            fingerprint: server.fingerprint,
        }
    }
}

/// Shared printers reported by a Trusted Server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustedServerPrintersDto {
    pub address: String,
    pub printers: Vec<String>,
}

impl From<ConnectionPrinters> for TrustedServerPrintersDto {
    fn from(value: ConnectionPrinters) -> Self {
        Self {
            address: value.address,
            printers: value.printers,
        }
    }
}

/// Recognised ShaPrint Client queue installed in the local CUPS spooler.
pub type InstalledClientQueueDto = RecognisedClientQueueDto;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
enum ControlRequest {
    Status,
    StartService {
        id: String,
    },
    StopService {
        id: String,
    },
    Shutdown,
    NetworkChannelStatus,
    SetNetworkChannel {
        channel: String,
    },
    InspectServer {
        address: String,
    },
    ApproveServer {
        address: String,
        fingerprint: String,
    },
    ForgetServer {
        address: String,
    },
    ListTrustedServers,
    ListServerPrinters {
        address: String,
    },
    InstallClientQueue {
        address: String,
        printer: String,
    },
    RepairClientQueue {
        address: String,
        printer: String,
    },
    ListClientQueues,
    ListLocalPrinters,
    SetSharedPrinters {
        printers: Vec<String>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "kebab-case")]
enum ControlResponse {
    Ok {
        status: RuntimeStatusDto,
    },
    NetworkChannel {
        network_channel: NetworkChannelStatusDto,
    },
    ServerReview {
        review: ServerTrustReviewDto,
    },
    TrustedServers {
        servers: Vec<TrustedServerEntryDto>,
    },
    ServerPrinters {
        printers: TrustedServerPrintersDto,
    },
    Forgotten {
        address: String,
    },
    ClientQueue {
        queue: ClientQueueDto,
    },
    ClientQueues {
        queues: Vec<InstalledClientQueueDto>,
    },
    LocalPrinters {
        printers: Vec<LocalPrinterDto>,
    },
    Err {
        error: AppErrorDto,
    },
}

/// Running `shaprintd` control-plane server over a local Unix domain socket.
pub struct DaemonServer {
    socket_path: PathBuf,
    shell: Option<Arc<Shell>>,
    runtime: Arc<RuntimeCoordinator>,
    cancel_tx: watch::Sender<bool>,
    accept_task: JoinHandle<()>,
}

impl DaemonServer {
    /// Builds `Shell`, binds the local Unix domain socket first so a duplicate instance cannot
    /// disturb active listeners, and then restores persisted sharing state and starts autostart
    /// services.
    pub async fn launch(config: DaemonConfig) -> Result<Self, AppError> {
        Self::launch_with_effective_uid(config, current_effective_uid()).await
    }

    /// Launches `shaprintd` while checking `effective_uid` against the non-root policy.
    pub async fn launch_with_effective_uid(
        config: DaemonConfig,
        effective_uid: Option<u32>,
    ) -> Result<Self, AppError> {
        if config.forbid_root && effective_uid == Some(0) {
            return Err(AppError::invalid_state(
                "shaprintd must not run as root; start it under the unprivileged 'shaprint' account",
            ));
        }
        std::fs::create_dir_all(&config.state_dir).map_err(|error| {
            AppError::internal(format!(
                "cannot create daemon state directory '{}': {error}",
                config.state_dir.display()
            ))
        })?;
        std::fs::set_permissions(&config.state_dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| {
                AppError::internal(format!(
                    "cannot set permissions on daemon state directory '{}': {error}",
                    config.state_dir.display()
                ))
            })?;
        let credential_file = config.credential_dir().join(CLIENT_CREDENTIAL_FILE);
        let shell = Arc::new(Shell::with_linux_options(
            &config.state_dir,
            config.ports,
            Some(credential_file),
            config.cups_endpoint,
            config.firewall_program,
        )?);
        let server = Self::bind_internal(
            &config.socket_path,
            shell.runtime(),
            Some(Arc::clone(&shell)),
            config.access_policy,
        )
        .await?;
        if let Err(error) = shell.restore().await {
            log::warn!(
                "cannot restore server sharing code={} message={}",
                error.code_str(),
                error
            );
        }
        if let Err(error) = shell.runtime().start_autostart().await {
            log::warn!(
                "autostart incomplete code={} message={}",
                error.code_str(),
                error
            );
        }

        log::info!(
            "shaprintd started socket={} state_dir={}",
            config.socket_path.display(),
            config.state_dir.display()
        );
        Ok(server)
    }

    /// Binds a local Unix socket supervising an existing `RuntimeCoordinator` with the default
    /// peer credential policy.
    pub async fn bind(
        socket_path: impl Into<PathBuf>,
        runtime: Arc<RuntimeCoordinator>,
    ) -> Result<Self, AppError> {
        Self::bind_with_policy(socket_path, runtime, PeerAccessPolicy::RootOrDaemonOwner).await
    }

    /// Binds a local Unix socket supervising an existing `RuntimeCoordinator` with an explicit
    /// peer credential policy.
    pub async fn bind_with_policy(
        socket_path: impl Into<PathBuf>,
        runtime: Arc<RuntimeCoordinator>,
        policy: PeerAccessPolicy,
    ) -> Result<Self, AppError> {
        Self::bind_internal(socket_path, runtime, None, policy).await
    }

    async fn bind_internal(
        socket_path: impl Into<PathBuf>,
        runtime: Arc<RuntimeCoordinator>,
        shell: Option<Arc<Shell>>,
        policy: PeerAccessPolicy,
    ) -> Result<Self, AppError> {
        let socket_path = socket_path.into();
        if let Some(parent) = socket_path.parent() {
            if !parent.as_os_str().is_empty() {
                let existed = parent.exists();
                std::fs::create_dir_all(parent).map_err(|error| {
                    AppError::internal(format!(
                        "cannot create socket directory '{}': {error}",
                        parent.display()
                    ))
                })?;
                if !existed {
                    std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o755))
                        .map_err(|error| {
                            AppError::internal(format!(
                                "cannot set permissions on socket directory '{}': {error}",
                                parent.display()
                            ))
                        })?;
                }
            }
        }
        cleanup_stale_socket(&socket_path).await?;

        let listener = UnixListener::bind(&socket_path).map_err(|error| {
            AppError::internal(format!(
                "cannot bind control socket '{}': {error}",
                socket_path.display()
            ))
        })?;
        // Allow local users to connect for read-only status while SO_PEERCRED gates mutations.
        std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o666)).map_err(
            |error| {
                AppError::internal(format!(
                    "cannot set permissions on control socket '{}': {error}",
                    socket_path.display()
                ))
            },
        )?;

        let daemon_uid = current_effective_uid();
        let (cancel_tx, mut cancel_rx) = watch::channel(false);
        let loop_runtime = Arc::clone(&runtime);
        let loop_shell = shell.clone();
        let loop_cancel = cancel_tx.clone();
        let accept_task = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    changed = cancel_rx.changed() => {
                        if changed.is_err() || *cancel_rx.borrow() {
                            break;
                        }
                    }
                    Some(_) = connections.join_next(), if !connections.is_empty() => {}
                    accepted = listener.accept() => {
                        match accepted {
                            Ok((stream, _addr)) => {
                                let conn_runtime = Arc::clone(&loop_runtime);
                                let conn_shell = loop_shell.clone();
                                let conn_cancel = loop_cancel.clone();
                                connections.spawn(async move {
                                    handle_connection(
                                        stream,
                                        conn_runtime,
                                        conn_shell,
                                        conn_cancel,
                                        policy,
                                        daemon_uid,
                                    )
                                    .await;
                                });
                            }
                            Err(error) => {
                                log::warn!("control socket accept error: {error}");
                                tokio::time::sleep(ACCEPT_RETRY_DELAY).await;
                            }
                        }
                    }
                }
            }
            // Drain all in-flight control requests before returning so no connection task can
            // start a service after `wait()` shuts down the runtime coordinator.
            while connections.join_next().await.is_some() {}
        });

        Ok(Self {
            socket_path,
            shell,
            runtime,
            cancel_tx,
            accept_task,
        })
    }

    pub fn shell(&self) -> Option<&Shell> {
        self.shell.as_deref()
    }

    pub fn runtime(&self) -> &Arc<RuntimeCoordinator> {
        &self.runtime
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Returns a handle that can trigger clean daemon shutdown from a signal handler.
    pub fn shutdown_handle(&self) -> DaemonShutdownHandle {
        DaemonShutdownHandle {
            cancel_tx: self.cancel_tx.clone(),
        }
    }

    /// Stops the control socket, shuts down every supervised runtime service, and removes the
    /// socket file.
    pub async fn shutdown(self) -> Result<(), AppError> {
        let _ = self.cancel_tx.send_replace(true);
        self.wait().await
    }

    /// Waits until shutdown is requested (either over the control socket or via `shutdown`),
    /// ensures every supervised service is stopped, and removes the socket file.
    pub async fn wait(self) -> Result<(), AppError> {
        let _ = self.accept_task.await;
        let result = self.runtime.shutdown().await;
        let _ = std::fs::remove_file(&self.socket_path);
        log::info!("shaprintd stopped socket={}", self.socket_path.display());
        result
    }
}

async fn cleanup_stale_socket(socket_path: &Path) -> Result<(), AppError> {
    let metadata = match std::fs::symlink_metadata(socket_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(AppError::internal(format!(
                "cannot inspect control socket '{}': {error}",
                socket_path.display()
            )))
        }
    };

    if !metadata.file_type().is_socket() {
        return Err(AppError::invalid_input(format!(
            "control socket path '{}' exists and is not a Unix socket",
            socket_path.display()
        )));
    }

    if UnixStream::connect(socket_path).await.is_ok() {
        return Err(AppError::invalid_state(format!(
            "shaprintd is already listening on '{}'",
            socket_path.display()
        )));
    }

    std::fs::remove_file(socket_path).map_err(|error| {
        AppError::internal(format!(
            "cannot remove stale socket '{}': {error}",
            socket_path.display()
        ))
    })
}

async fn handle_connection(
    stream: UnixStream,
    runtime: Arc<RuntimeCoordinator>,
    shell: Option<Arc<Shell>>,
    cancel_tx: watch::Sender<bool>,
    policy: PeerAccessPolicy,
    daemon_uid: Option<u32>,
) {
    let _ = timeout(
        IO_TIMEOUT,
        process_stream(stream, runtime, shell, cancel_tx, policy, daemon_uid),
    )
    .await;
}

fn require_shell(shell: &Option<Arc<Shell>>) -> Result<&Arc<Shell>, AppError> {
    shell.as_ref().ok_or_else(|| {
        AppError::invalid_state(
            "shaprintd is running without machine credential and trust storage attached",
        )
    })
}

fn runtime_status_dto(
    runtime: &RuntimeCoordinator,
    shell: &Option<Arc<Shell>>,
) -> RuntimeStatusDto {
    let mut dto = RuntimeStatusDto::from(&runtime.status());
    if let Some(shell) = shell {
        dto.network_channel_configured = Some(shell.network_channel().is_configured());
    }
    dto
}

async fn process_stream(
    mut stream: UnixStream,
    runtime: Arc<RuntimeCoordinator>,
    shell: Option<Arc<Shell>>,
    cancel_tx: watch::Sender<bool>,
    policy: PeerAccessPolicy,
    daemon_uid: Option<u32>,
) -> Result<(), AppError> {
    let mut stop_daemon = false;
    let outcome: Result<ControlResponse, AppError> = async {
        let peer_cred = stream
            .peer_cred()
            .map_err(|_| AppError::not_authorized("cannot verify local peer credentials"))?;
        match read_json_line::<ControlRequest>(&mut stream, "control request").await? {
            ControlRequest::Status => Ok(ControlResponse::Ok {
                status: runtime_status_dto(&runtime, &shell),
            }),
            ControlRequest::StartService { id } => {
                policy.authorize_mutation(peer_cred.uid(), daemon_uid)?;
                let service = ServiceId::parse(&id)?;
                runtime.start(service).await?;
                Ok(ControlResponse::Ok {
                    status: runtime_status_dto(&runtime, &shell),
                })
            }
            ControlRequest::StopService { id } => {
                policy.authorize_mutation(peer_cred.uid(), daemon_uid)?;
                let service = ServiceId::parse(&id)?;
                runtime.stop(service).await?;
                Ok(ControlResponse::Ok {
                    status: runtime_status_dto(&runtime, &shell),
                })
            }
            ControlRequest::Shutdown => {
                policy.authorize_mutation(peer_cred.uid(), daemon_uid)?;
                runtime.shutdown().await?;
                stop_daemon = true;
                Ok(ControlResponse::Ok {
                    status: runtime_status_dto(&runtime, &shell),
                })
            }
            ControlRequest::NetworkChannelStatus => {
                let shell = require_shell(&shell)?;
                Ok(ControlResponse::NetworkChannel {
                    network_channel: NetworkChannelStatusDto {
                        configured: shell.network_channel().is_configured(),
                    },
                })
            }
            ControlRequest::SetNetworkChannel { channel } => {
                policy.authorize_mutation(peer_cred.uid(), daemon_uid)?;
                let shell = require_shell(&shell)?;
                let configured = shell.network_channel().configure(&channel).await?;
                log::info!("configured machine Network Channel configured={configured}");
                Ok(ControlResponse::NetworkChannel {
                    network_channel: NetworkChannelStatusDto { configured },
                })
            }
            ControlRequest::InspectServer { address } => {
                let shell = require_shell(&shell)?;
                let review = shell.client_connections().inspect(&address).await?;
                log::info!(
                    "inspected server certificate address={} trusted={}",
                    review.address,
                    review.trusted
                );
                Ok(ControlResponse::ServerReview {
                    review: ServerTrustReviewDto::from(review),
                })
            }
            ControlRequest::ApproveServer {
                address,
                fingerprint,
            } => {
                policy.authorize_mutation(peer_cred.uid(), daemon_uid)?;
                let shell = require_shell(&shell)?;
                let review = shell
                    .client_connections()
                    .approve(&address, &fingerprint)
                    .await?;
                log::info!(
                    "approved trusted server address={} fingerprint={}",
                    review.address,
                    review.current_fingerprint
                );
                Ok(ControlResponse::ServerReview {
                    review: ServerTrustReviewDto::from(review),
                })
            }
            ControlRequest::ForgetServer { address } => {
                policy.authorize_mutation(peer_cred.uid(), daemon_uid)?;
                let shell = require_shell(&shell)?;
                let parsed = crate::adapters::client_connections::ServerAddress::parse(&address)?;
                shell
                    .client_connections()
                    .forget_trusted_server(&address)
                    .await?;
                log::info!("forgot trusted server address={}", parsed.normalized);
                Ok(ControlResponse::Forgotten {
                    address: parsed.normalized,
                })
            }
            ControlRequest::ListTrustedServers => {
                let shell = require_shell(&shell)?;
                let servers = shell
                    .client_connections()
                    .list_trusted_servers()
                    .await?
                    .into_iter()
                    .map(TrustedServerEntryDto::from)
                    .collect();
                Ok(ControlResponse::TrustedServers { servers })
            }
            ControlRequest::ListServerPrinters { address } => {
                let shell = require_shell(&shell)?;
                let printers = shell.client_connections().printers(&address).await?;
                Ok(ControlResponse::ServerPrinters {
                    printers: TrustedServerPrintersDto::from(printers),
                })
            }
            ControlRequest::InstallClientQueue { address, printer } => {
                policy.authorize_mutation(peer_cred.uid(), daemon_uid)?;
                let shell = require_shell(&shell)?;
                let queue = shell
                    .queue_installation()
                    .install(&address, &printer)
                    .await?;
                log::info!(
                    "installed client queue name={} server={} printer={}",
                    queue.name(),
                    queue.request().server_address(),
                    queue.request().printer()
                );
                Ok(ControlResponse::ClientQueue {
                    queue: ClientQueueDto::from(&queue),
                })
            }
            ControlRequest::RepairClientQueue { address, printer } => {
                policy.authorize_mutation(peer_cred.uid(), daemon_uid)?;
                let shell = require_shell(&shell)?;
                let queue = shell
                    .queue_installation()
                    .repair(&address, &printer)
                    .await?;
                log::info!(
                    "repaired client queue name={} server={} printer={}",
                    queue.name(),
                    queue.request().server_address(),
                    queue.request().printer()
                );
                Ok(ControlResponse::ClientQueue {
                    queue: ClientQueueDto::from(&queue),
                })
            }
            ControlRequest::ListClientQueues => {
                let shell = require_shell(&shell)?;
                let queues = shell
                    .printer_catalog()
                    .recognised_client_queues()
                    .await?
                    .into_iter()
                    .map(InstalledClientQueueDto::from)
                    .collect();
                Ok(ControlResponse::ClientQueues { queues })
            }
            ControlRequest::ListLocalPrinters => {
                let shell = require_shell(&shell)?;
                let printers = shell.sharing().local_printers().await?;
                let dto = LocalPrintersDto::from(printers.as_slice());
                Ok(ControlResponse::LocalPrinters {
                    printers: dto.printers,
                })
            }
            ControlRequest::SetSharedPrinters { printers } => {
                policy.authorize_mutation(peer_cred.uid(), daemon_uid)?;
                let shell = require_shell(&shell)?;
                let parsed: Vec<PrinterName> = printers
                    .iter()
                    .map(|name| PrinterName::parse(name))
                    .collect::<Result<Vec<_>, _>>()?;
                let updated = shell.sharing().set_shared(parsed).await?;
                log::info!("updated shared printers count={}", updated.len());
                let dto = LocalPrintersDto::from(updated.as_slice());
                Ok(ControlResponse::LocalPrinters {
                    printers: dto.printers,
                })
            }
        }
    }
    .await;

    let response = match outcome {
        Ok(response) => response,
        Err(error) => ControlResponse::Err {
            error: AppErrorDto::from(&error),
        },
    };
    let write_result = write_json_line(&mut stream, &response).await;
    if stop_daemon {
        let _ = cancel_tx.send_replace(true);
    }
    write_result
}

async fn read_json_line<T: DeserializeOwned>(
    stream: &mut UnixStream,
    label: &str,
) -> Result<T, AppError> {
    let mut reader = BufReader::new(stream).take((MAX_MESSAGE_BYTES + 1) as u64);
    let mut line = String::new();
    let bytes = reader
        .read_line(&mut line)
        .await
        .map_err(|error| AppError::internal(format!("cannot read {label}: {error}")))?;
    if bytes == 0 {
        return Err(AppError::invalid_input(format!("empty {label}")));
    }
    if line.len() > MAX_MESSAGE_BYTES {
        return Err(AppError::invalid_input(format!("{label} is too large")));
    }
    serde_json::from_str::<T>(line.trim())
        .map_err(|error| AppError::invalid_input(format!("invalid {label}: {error}")))
}

async fn write_json_line<T: Serialize>(
    stream: &mut UnixStream,
    payload: &T,
) -> Result<(), AppError> {
    let mut encoded = serde_json::to_vec(payload)
        .map_err(|error| AppError::internal(format!("cannot encode control payload: {error}")))?;
    encoded.push(b'\n');
    stream
        .write_all(&encoded)
        .await
        .map_err(|error| AppError::internal(format!("cannot write control payload: {error}")))?;
    stream
        .flush()
        .await
        .map_err(|error| AppError::internal(format!("cannot flush control payload: {error}")))
}

/// Client for communicating with `shaprintd` over its local Unix domain socket.
#[derive(Debug, Clone)]
pub struct DaemonClient {
    socket_path: PathBuf,
}

impl DaemonClient {
    pub fn new(socket_path: impl Into<PathBuf>) -> Self {
        Self {
            socket_path: socket_path.into(),
        }
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Queries the current status of all supervised services from `shaprintd`.
    pub async fn status(&self) -> Result<RuntimeStatusDto, AppError> {
        match self.send(&ControlRequest::Status).await? {
            ControlResponse::Ok { status } => Ok(status),
            other => Err(unexpected_response(&other)),
        }
    }

    /// Starts a supervised service and returns the updated runtime status.
    pub async fn start_service(&self, id: &str) -> Result<RuntimeStatusDto, AppError> {
        match self
            .send(&ControlRequest::StartService { id: id.to_owned() })
            .await?
        {
            ControlResponse::Ok { status } => Ok(status),
            other => Err(unexpected_response(&other)),
        }
    }

    /// Stops a supervised service and returns the updated runtime status.
    pub async fn stop_service(&self, id: &str) -> Result<RuntimeStatusDto, AppError> {
        match self
            .send(&ControlRequest::StopService { id: id.to_owned() })
            .await?
        {
            ControlResponse::Ok { status } => Ok(status),
            other => Err(unexpected_response(&other)),
        }
    }

    /// Stops every supervised service and shuts down `shaprintd`.
    pub async fn shutdown(&self) -> Result<RuntimeStatusDto, AppError> {
        match self.send(&ControlRequest::Shutdown).await? {
            ControlResponse::Ok { status } => Ok(status),
            other => Err(unexpected_response(&other)),
        }
    }

    /// Queries whether the machine Network Channel is configured without returning its value.
    pub async fn network_channel_status(&self) -> Result<NetworkChannelStatusDto, AppError> {
        match self.send(&ControlRequest::NetworkChannelStatus).await? {
            ControlResponse::NetworkChannel { network_channel } => Ok(network_channel),
            other => Err(unexpected_response(&other)),
        }
    }

    /// Configures or replaces the machine Network Channel and returns the safe configured status.
    pub async fn configure_network_channel(
        &self,
        channel: &str,
    ) -> Result<NetworkChannelStatusDto, AppError> {
        match self
            .send(&ControlRequest::SetNetworkChannel {
                channel: channel.to_owned(),
            })
            .await?
        {
            ControlResponse::NetworkChannel { network_channel } => Ok(network_channel),
            other => Err(unexpected_response(&other)),
        }
    }

    /// Inspects a Server's presented TLS certificate fingerprint before any printer query.
    pub async fn inspect_server(&self, address: &str) -> Result<ServerTrustReviewDto, AppError> {
        match self
            .send(&ControlRequest::InspectServer {
                address: address.to_owned(),
            })
            .await?
        {
            ControlResponse::ServerReview { review } => Ok(review),
            other => Err(unexpected_response(&other)),
        }
    }

    /// Approves or reapproves a Server's certificate fingerprint as a machine Trusted Server.
    pub async fn approve_server(
        &self,
        address: &str,
        fingerprint: &str,
    ) -> Result<ServerTrustReviewDto, AppError> {
        match self
            .send(&ControlRequest::ApproveServer {
                address: address.to_owned(),
                fingerprint: fingerprint.to_owned(),
            })
            .await?
        {
            ControlResponse::ServerReview { review } => Ok(review),
            other => Err(unexpected_response(&other)),
        }
    }

    /// Removes a Server from machine Trusted Servers.
    pub async fn forget_server(&self, address: &str) -> Result<String, AppError> {
        match self
            .send(&ControlRequest::ForgetServer {
                address: address.to_owned(),
            })
            .await?
        {
            ControlResponse::Forgotten { address } => Ok(address),
            other => Err(unexpected_response(&other)),
        }
    }

    /// Lists all machine-wide Trusted Servers and their pinned fingerprints.
    pub async fn list_trusted_servers(&self) -> Result<Vec<TrustedServerEntryDto>, AppError> {
        match self.send(&ControlRequest::ListTrustedServers).await? {
            ControlResponse::TrustedServers { servers } => Ok(servers),
            other => Err(unexpected_response(&other)),
        }
    }

    /// Queries shared printers on an explicitly approved Trusted Server.
    pub async fn list_server_printers(
        &self,
        address: &str,
    ) -> Result<TrustedServerPrintersDto, AppError> {
        match self
            .send(&ControlRequest::ListServerPrinters {
                address: address.to_owned(),
            })
            .await?
        {
            ControlResponse::ServerPrinters { printers } => Ok(printers),
            other => Err(unexpected_response(&other)),
        }
    }

    /// Installs (or updates) a machine-wide Client queue for a Trusted Server's shared printer.
    pub async fn install_client_queue(
        &self,
        address: &str,
        printer: &str,
    ) -> Result<ClientQueueDto, AppError> {
        match self
            .send(&ControlRequest::InstallClientQueue {
                address: address.to_owned(),
                printer: printer.to_owned(),
            })
            .await?
        {
            ControlResponse::ClientQueue { queue } => Ok(queue),
            other => Err(unexpected_response(&other)),
        }
    }

    /// Repairs (recreates) an existing machine-wide Client queue.
    pub async fn repair_client_queue(
        &self,
        address: &str,
        printer: &str,
    ) -> Result<ClientQueueDto, AppError> {
        match self
            .send(&ControlRequest::RepairClientQueue {
                address: address.to_owned(),
                printer: printer.to_owned(),
            })
            .await?
        {
            ControlResponse::ClientQueue { queue } => Ok(queue),
            other => Err(unexpected_response(&other)),
        }
    }

    /// Lists all recognised ShaPrint Client queues installed in the local spooler.
    pub async fn list_client_queues(&self) -> Result<Vec<InstalledClientQueueDto>, AppError> {
        match self.send(&ControlRequest::ListClientQueues).await? {
            ControlResponse::ClientQueues { queues } => Ok(queues),
            other => Err(unexpected_response(&other)),
        }
    }

    /// Lists all eligible local printer queues and whether each is currently shared by the Server.
    pub async fn list_local_printers(&self) -> Result<Vec<LocalPrinterDto>, AppError> {
        match self.send(&ControlRequest::ListLocalPrinters).await? {
            ControlResponse::LocalPrinters { printers } => Ok(printers),
            other => Err(unexpected_response(&other)),
        }
    }

    /// Persists the Server's selected shared printers and returns the updated local printer list.
    pub async fn set_shared_printers(
        &self,
        printers: &[String],
    ) -> Result<Vec<LocalPrinterDto>, AppError> {
        match self
            .send(&ControlRequest::SetSharedPrinters {
                printers: printers.to_vec(),
            })
            .await?
        {
            ControlResponse::LocalPrinters { printers } => Ok(printers),
            other => Err(unexpected_response(&other)),
        }
    }

    async fn send(&self, request: &ControlRequest) -> Result<ControlResponse, AppError> {
        let mut stream = timeout(IO_TIMEOUT, UnixStream::connect(&self.socket_path))
            .await
            .map_err(|_| AppError::timeout("connecting to shaprintd timed out"))?
            .map_err(|error| {
                AppError::server_unavailable(format!(
                    "cannot connect to shaprintd at '{}': {error}",
                    self.socket_path.display()
                ))
            })?;

        timeout(IO_TIMEOUT, write_json_line(&mut stream, request))
            .await
            .map_err(|_| AppError::timeout("sending request to shaprintd timed out"))??;

        let response: ControlResponse = timeout(
            IO_TIMEOUT,
            read_json_line(&mut stream, "shaprintd response"),
        )
        .await
        .map_err(|_| AppError::timeout("waiting for shaprintd response timed out"))??;

        match response {
            ControlResponse::Err { error } => Err(app_error_from_dto(error)),
            other => Ok(other),
        }
    }
}

fn unexpected_response(response: &ControlResponse) -> AppError {
    AppError::internal(format!("unexpected shaprintd response: {response:?}"))
}

/// Handle used by signal handlers to request a clean `DaemonServer` shutdown.
#[derive(Debug, Clone)]
pub struct DaemonShutdownHandle {
    cancel_tx: watch::Sender<bool>,
}

impl DaemonShutdownHandle {
    pub fn request_shutdown(&self) {
        let _ = self.cancel_tx.send_replace(true);
    }
}

fn app_error_from_dto(dto: AppErrorDto) -> AppError {
    let code = ErrorCode::ALL
        .into_iter()
        .find(|code| code.as_str() == dto.code)
        .unwrap_or(ErrorCode::Internal);
    AppError::new(code, dto.message)
}

/// Permitted non-argument sources for reading the Network Channel secret in `shaprintctl`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretInputSource {
    /// Interactive hidden prompt when stdin is a terminal, or standard input otherwise.
    PromptOrStdin,
    /// Standard input (`--stdin`).
    Stdin,
    /// Open file descriptor number (`--fd <fd>`).
    FileDescriptor(i32),
    /// System credential file path (`--credential-file <path>`).
    CredentialFile(PathBuf),
}

/// Action requested through `shaprintctl`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliCommand {
    Status,
    StartService(ServiceId),
    StopService(ServiceId),
    Shutdown,
    NetworkChannelStatus,
    SetNetworkChannel(SecretInputSource),
    InspectServer {
        address: String,
    },
    ApproveServer {
        address: String,
        fingerprint: String,
    },
    ForgetServer {
        address: String,
    },
    ListTrustedServers,
    ListServerPrinters {
        address: String,
    },
    InstallClientQueue {
        address: String,
        printer: String,
    },
    RepairClientQueue {
        address: String,
        printer: String,
    },
    ListClientQueues,
    ListLocalPrinters,
    SetSharedPrinters {
        printers: Vec<String>,
    },
}

/// Parsed command-line options for `shaprintctl`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliOptions {
    pub socket_path: PathBuf,
    pub json: bool,
    pub command: CliCommand,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct NonSecretDaemonConfigFile {
    #[serde(default)]
    state_dir: Option<PathBuf>,
    #[serde(default)]
    socket_path: Option<PathBuf>,
    #[serde(default)]
    proxy_port: Option<u16>,
    #[serde(default)]
    server_port: Option<u16>,
    #[serde(default)]
    discovery_ports: Option<Vec<u16>>,
    #[serde(default)]
    require_root: Option<bool>,
    #[serde(default)]
    forbid_root: Option<bool>,
}

fn load_non_secret_config_file(config_dir: &Path) -> Result<NonSecretDaemonConfigFile, AppError> {
    let config_file = config_dir.join(DEFAULT_CONFIG_FILE);
    let bytes = match std::fs::read(&config_file) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(NonSecretDaemonConfigFile::default())
        }
        Err(error) => {
            return Err(AppError::internal(format!(
                "cannot read daemon configuration '{}': {error}",
                config_file.display()
            )))
        }
    };
    serde_json::from_slice::<NonSecretDaemonConfigFile>(&bytes).map_err(|error| {
        AppError::invalid_input(format!(
            "invalid non-secret daemon configuration '{}': {error}",
            config_file.display()
        ))
    })
}

/// Parses command-line arguments for `shaprintd`.
pub fn parse_daemon_args<S: AsRef<str>>(arguments: &[S]) -> Result<DaemonConfig, AppError> {
    let mut config = DaemonConfig::default();
    let mut cli = NonSecretDaemonConfigFile::default();
    let mut explicit_config_dir = false;

    let mut iter = arguments.iter().skip(1).map(AsRef::as_ref);
    while let Some(arg) = iter.next() {
        match arg {
            "--config-dir" => {
                let value = iter
                    .next()
                    .ok_or_else(|| AppError::invalid_input("missing value for '--config-dir'"))?;
                config.config_dir = PathBuf::from(value);
                explicit_config_dir = true;
            }
            "--state-dir" => {
                let value = iter
                    .next()
                    .ok_or_else(|| AppError::invalid_input("missing value for '--state-dir'"))?;
                cli.state_dir = Some(PathBuf::from(value));
            }
            "--socket" => {
                let value = iter
                    .next()
                    .ok_or_else(|| AppError::invalid_input("missing value for '--socket'"))?;
                cli.socket_path = Some(PathBuf::from(value));
            }
            "--proxy-port" => {
                let value = iter
                    .next()
                    .ok_or_else(|| AppError::invalid_input("missing value for '--proxy-port'"))?;
                cli.proxy_port = Some(parse_port(value, "--proxy-port")?);
            }
            "--server-port" => {
                let value = iter
                    .next()
                    .ok_or_else(|| AppError::invalid_input("missing value for '--server-port'"))?;
                cli.server_port = Some(parse_port(value, "--server-port")?);
            }
            "--discovery-port" => {
                let value = iter.next().ok_or_else(|| {
                    AppError::invalid_input("missing value for '--discovery-port'")
                })?;
                cli.discovery_ports = Some(vec![parse_port(value, "--discovery-port")?]);
            }
            "--require-root" => {
                cli.require_root = Some(true);
            }
            "--forbid-root" => {
                cli.forbid_root = Some(true);
            }
            other => {
                return Err(AppError::invalid_input(format!(
                    "unrecognized shaprintd option '{other}'"
                )));
            }
        }
    }

    let file_config = if explicit_config_dir {
        load_non_secret_config_file(&config.config_dir)?
    } else {
        NonSecretDaemonConfigFile::default()
    };
    if let Some(state_dir) = cli.state_dir.or(file_config.state_dir) {
        config.state_dir = state_dir;
    }
    if let Some(socket_path) = cli.socket_path.or(file_config.socket_path) {
        config.socket_path = socket_path;
    }

    let mut ports = RuntimePorts::default();
    if let Some(port) = cli.proxy_port.or(file_config.proxy_port) {
        ports.proxy_port = port;
    }
    if let Some(port) = cli.server_port.or(file_config.server_port) {
        ports.server_port = port;
    }
    if let Some(discovery_ports) = cli.discovery_ports.or(file_config.discovery_ports) {
        ports.discovery_ports = discovery_ports;
    }
    if cli
        .require_root
        .or(file_config.require_root)
        .unwrap_or(false)
    {
        config.access_policy = PeerAccessPolicy::RootOnly;
    }
    if let Some(forbid_root) = cli.forbid_root.or(file_config.forbid_root) {
        config.forbid_root = forbid_root;
    }

    Ok(config.with_ports(ports))
}

fn parse_port(value: &str, flag: &str) -> Result<u16, AppError> {
    value
        .parse::<u16>()
        .map_err(|_| AppError::invalid_input(format!("invalid port '{value}' for '{flag}'")))
}

/// Parses command-line arguments for `shaprintctl`.
pub fn parse_cli_args<S: AsRef<str>>(arguments: &[S]) -> Result<CliOptions, AppError> {
    let mut socket_path = PathBuf::from(DEFAULT_SOCKET_PATH);
    let mut json = false;
    let mut stdin_flag = false;
    let mut fd_arg: Option<i32> = None;
    let mut credential_file_arg: Option<PathBuf> = None;
    let mut fingerprint_arg: Option<String> = None;
    let mut positional = Vec::new();

    let mut iter = arguments.iter().skip(1).map(AsRef::as_ref);
    while let Some(arg) = iter.next() {
        match arg {
            "--socket" => {
                let value = iter
                    .next()
                    .ok_or_else(|| AppError::invalid_input("missing value for '--socket'"))?;
                socket_path = PathBuf::from(value);
            }
            "--json" => {
                json = true;
            }
            "--stdin" => {
                stdin_flag = true;
            }
            "--fd" => {
                let value = iter
                    .next()
                    .ok_or_else(|| AppError::invalid_input("missing value for '--fd'"))?;
                let parsed = value
                    .parse::<i32>()
                    .ok()
                    .filter(|fd| *fd >= 0)
                    .ok_or_else(|| {
                        AppError::invalid_input(format!(
                            "invalid file descriptor '{value}' for '--fd'"
                        ))
                    })?;
                fd_arg = Some(parsed);
            }
            "--credential-file" => {
                let value = iter.next().ok_or_else(|| {
                    AppError::invalid_input("missing value for '--credential-file'")
                })?;
                credential_file_arg = Some(PathBuf::from(value));
            }
            "--fingerprint" => {
                let value = iter
                    .next()
                    .ok_or_else(|| AppError::invalid_input("missing value for '--fingerprint'"))?;
                fingerprint_arg = Some(value.to_owned());
            }
            other
                if other == "--network-channel"
                    || other.starts_with("--network-channel=")
                    || other == "--channel"
                    || other.starts_with("--channel=")
                    || other == "--value"
                    || other.starts_with("--value=")
                    || other == "--secret"
                    || other.starts_with("--secret=") =>
            {
                return Err(AppError::invalid_input(
                    "the Network Channel must not be passed as a command-line argument; use a hidden prompt, --stdin, --fd, or --credential-file",
                ));
            }
            other if other.starts_with('-') => {
                let option_name = other.split('=').next().unwrap_or(other);
                return Err(AppError::invalid_input(format!(
                    "unrecognized shaprintctl option '{option_name}'"
                )));
            }
            other => positional.push(other),
        }
    }

    let secret_sources_count = usize::from(stdin_flag)
        + usize::from(fd_arg.is_some())
        + usize::from(credential_file_arg.is_some());
    if secret_sources_count > 1 {
        return Err(AppError::invalid_input(
            "specify at most one of '--stdin', '--fd', or '--credential-file'",
        ));
    }

    let is_set_network_channel = matches!(positional.as_slice(), ["network-channel", "set", ..]);
    if !is_set_network_channel && secret_sources_count > 0 {
        return Err(AppError::invalid_input(
            "'--stdin', '--fd', and '--credential-file' are only valid with 'network-channel set'",
        ));
    }
    if fingerprint_arg.is_some() && !matches!(positional.as_slice(), ["trust", "approve", ..]) {
        return Err(AppError::invalid_input(
            "'--fingerprint' is only valid with 'trust approve'",
        ));
    }

    let command = match positional.as_slice() {
        ["status"] => CliCommand::Status,
        ["start", service] => CliCommand::StartService(ServiceId::parse(service)?),
        ["stop"] | ["stop", "daemon"] | ["shutdown"] => CliCommand::Shutdown,
        ["stop", service] => CliCommand::StopService(ServiceId::parse(service)?),
        ["network-channel"] | ["network-channel", "status"] => CliCommand::NetworkChannelStatus,
        ["network-channel", "set"] => {
            let source = if stdin_flag {
                SecretInputSource::Stdin
            } else if let Some(fd) = fd_arg {
                SecretInputSource::FileDescriptor(fd)
            } else if let Some(path) = credential_file_arg {
                SecretInputSource::CredentialFile(path)
            } else {
                SecretInputSource::PromptOrStdin
            };
            CliCommand::SetNetworkChannel(source)
        }
        ["network-channel", "set", ..] => {
            return Err(AppError::invalid_input(
                "the Network Channel must not be passed as a command-line argument; use a hidden prompt, --stdin, --fd, or --credential-file",
            ));
        }
        ["trust", "list"] | ["trusted-servers"] => CliCommand::ListTrustedServers,
        ["trust", "inspect", address] | ["inspect", address] => CliCommand::InspectServer {
            address: (*address).to_owned(),
        },
        ["trust", "approve", address] => {
            let fingerprint = fingerprint_arg.ok_or_else(|| {
                AppError::invalid_input(
                    "missing certificate fingerprint for 'trust approve' (pass --fingerprint <SHA256>)",
                )
            })?;
            CliCommand::ApproveServer {
                address: (*address).to_owned(),
                fingerprint,
            }
        }
        ["trust", "approve", address, fingerprint] => {
            if fingerprint_arg.is_some() {
                return Err(AppError::invalid_input(
                    "specify the fingerprint either positionally or with '--fingerprint', not both",
                ));
            }
            CliCommand::ApproveServer {
                address: (*address).to_owned(),
                fingerprint: (*fingerprint).to_owned(),
            }
        }
        ["trust", "forget", address] | ["forget", address] => CliCommand::ForgetServer {
            address: (*address).to_owned(),
        },
        ["trust", "printers", address] | ["printers", address] => CliCommand::ListServerPrinters {
            address: (*address).to_owned(),
        },
        ["queue", "install", address, printer] => CliCommand::InstallClientQueue {
            address: (*address).to_owned(),
            printer: (*printer).to_owned(),
        },
        ["queue", "repair", address, printer] => CliCommand::RepairClientQueue {
            address: (*address).to_owned(),
            printer: (*printer).to_owned(),
        },
        ["queue", "list"] | ["queues"] => CliCommand::ListClientQueues,
        ["sharing"] | ["sharing", "printers"] | ["sharing", "list"] => {
            CliCommand::ListLocalPrinters
        }
        ["sharing", "clear"] => CliCommand::SetSharedPrinters {
            printers: Vec::new(),
        },
        ["sharing", "set", rest @ ..] => {
            if rest.is_empty() {
                return Err(AppError::invalid_input(
                    "specify at least one local printer name for 'sharing set' (or use 'sharing clear')",
                ));
            }
            CliCommand::SetSharedPrinters {
                printers: rest.iter().map(|s| (*s).to_owned()).collect(),
            }
        }
        [] => {
            return Err(AppError::invalid_input(
                "missing command (expected: status, start <service>, stop [service], shutdown, network-channel, trust, printers, queue, or sharing)",
            ))
        }
        other => {
            return Err(AppError::invalid_input(format!(
                "invalid command '{}'",
                other.join(" ")
            )))
        }
    };

    Ok(CliOptions {
        socket_path,
        json,
        command,
    })
}

/// Reads and trims a single line secret from a buffered reader, rejecting empty values.
pub fn read_secret_from_reader<R: std::io::Read>(reader: R) -> Result<String, AppError> {
    use std::io::BufRead;

    let mut line = String::new();
    let mut buffered = std::io::BufReader::new(reader.take((MAX_MESSAGE_BYTES + 1) as u64));
    buffered.read_line(&mut line).map_err(|error| {
        AppError::internal(format!("cannot read Network Channel input: {error}"))
    })?;
    if line.len() > MAX_MESSAGE_BYTES {
        return Err(AppError::invalid_input(
            "Network Channel input is too large",
        ));
    }
    let trimmed = line.trim_end_matches(['\r', '\n']);
    if trimmed.is_empty() {
        return Err(AppError::invalid_input("Network Channel cannot be empty"));
    }
    Ok(trimmed.to_owned())
}

/// Reads the Network Channel value from the configured [`SecretInputSource`].
pub fn read_secret_input(source: &SecretInputSource) -> Result<String, AppError> {
    match source {
        SecretInputSource::Stdin => read_secret_from_reader(std::io::stdin().lock()),
        SecretInputSource::CredentialFile(path) => {
            let file = std::fs::File::open(path).map_err(|error| {
                AppError::invalid_input(format!(
                    "cannot open credential file '{}': {error}",
                    path.display()
                ))
            })?;
            read_secret_from_reader(file)
        }
        SecretInputSource::FileDescriptor(fd) => read_secret_from_fd(*fd),
        SecretInputSource::PromptOrStdin => {
            use std::io::IsTerminal;
            if std::io::stdin().is_terminal() {
                read_secret_from_tty_hidden_prompt("Network Channel: ")
            } else {
                read_secret_from_reader(std::io::stdin().lock())
            }
        }
    }
}

#[cfg(unix)]
fn read_secret_from_fd(fd: i32) -> Result<String, AppError> {
    use std::io::Read;
    // Duplicate the file descriptor via `/dev/fd/<fd>` or `/proc/self/fd/<fd>` so we can read it
    // safely without `unsafe` raw-fd ownership assumptions.
    let proc_path = PathBuf::from(format!("/proc/self/fd/{fd}"));
    let dev_path = PathBuf::from(format!("/dev/fd/{fd}"));
    let path = if proc_path.exists() {
        proc_path
    } else {
        dev_path
    };
    let file = std::fs::File::open(&path).map_err(|error| {
        AppError::invalid_input(format!("cannot read from file descriptor {fd}: {error}"))
    })?;
    read_secret_from_reader(file.take((MAX_MESSAGE_BYTES + 1) as u64))
}

#[cfg(unix)]
fn read_secret_from_tty_hidden_prompt(prompt: &str) -> Result<String, AppError> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut tty_out = std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/tty")
        .map_err(|error| AppError::internal(format!("cannot open /dev/tty for prompt: {error}")))?;
    tty_out
        .write_all(prompt.as_bytes())
        .and_then(|_| tty_out.flush())
        .map_err(|error| AppError::internal(format!("cannot write prompt to /dev/tty: {error}")))?;

    let tty_in_for_stty = std::fs::File::open("/dev/tty")
        .map_err(|error| AppError::internal(format!("cannot open /dev/tty: {error}")))?;
    let stty_status = Command::new("stty")
        .arg("-echo")
        .stdin(Stdio::from(tty_in_for_stty))
        .status()
        .map_err(|error| AppError::internal(format!("cannot disable terminal echo: {error}")))?;
    if !stty_status.success() {
        return Err(AppError::internal(
            "cannot disable terminal echo for hidden prompt",
        ));
    }

    struct RestoreEcho;
    impl Drop for RestoreEcho {
        fn drop(&mut self) {
            if let Ok(tty) = std::fs::File::open("/dev/tty") {
                let _ = Command::new("stty")
                    .arg("echo")
                    .stdin(Stdio::from(tty))
                    .status();
            }
            if let Ok(mut out) = std::fs::OpenOptions::new().write(true).open("/dev/tty") {
                let _ = out.write_all(b"\n");
                let _ = out.flush();
            }
        }
    }
    let _restore = RestoreEcho;

    let tty_in = std::fs::File::open("/dev/tty")
        .map_err(|error| AppError::internal(format!("cannot read from /dev/tty: {error}")))?;
    read_secret_from_reader(tty_in)
}

/// Formats `RuntimeStatusDto` for `shaprintctl` output.
pub fn format_runtime_status(status: &RuntimeStatusDto, json: bool) -> Result<String, AppError> {
    if json {
        return serde_json::to_string(status)
            .map_err(|error| AppError::internal(format!("cannot format status JSON: {error}")));
    }
    let mut output = String::new();
    for service in &status.services {
        if service.detail.is_empty() || service.detail == service.state {
            output.push_str(&format!("{}: {}\n", service.id, service.state));
        } else {
            output.push_str(&format!(
                "{}: {} ({})\n",
                service.id, service.state, service.detail
            ));
        }
    }
    if let Some(configured) = status.network_channel_configured {
        let state = if configured {
            "configured"
        } else {
            "unconfigured"
        };
        output.push_str(&format!("network-channel: {state}\n"));
    }
    Ok(output)
}

/// Executes a parsed [`CliOptions`] command against `shaprintd` and returns formatted output.
pub async fn execute_cli_command(options: &CliOptions) -> Result<String, AppError> {
    let client = DaemonClient::new(&options.socket_path);
    match &options.command {
        CliCommand::Status => {
            let status = client.status().await?;
            format_runtime_status(&status, options.json)
        }
        CliCommand::StartService(id) => {
            if *id == ServiceId::ServerSharing
                && options.socket_path == Path::new(DEFAULT_SOCKET_PATH)
                && current_effective_uid() == Some(0)
            {
                let fw = crate::adapters::elevation::linux::FirewalldClient::new();
                if matches!(fw.inbound_sharing_allowed(), Ok(false)) {
                    let _ = fw.allow_inbound_sharing();
                }
            }
            let status = client.start_service(id.as_str()).await?;
            format_runtime_status(&status, options.json)
        }
        CliCommand::StopService(id) => {
            let status = client.stop_service(id.as_str()).await?;
            format_runtime_status(&status, options.json)
        }
        CliCommand::Shutdown => {
            let status = client.shutdown().await?;
            format_runtime_status(&status, options.json)
        }
        CliCommand::NetworkChannelStatus => {
            let nc = client.network_channel_status().await?;
            format_network_channel_status(&nc, options.json)
        }
        CliCommand::SetNetworkChannel(source) => {
            let secret = read_secret_input(source)?;
            // When `shaprintctl` runs as root on the default machine socket, persist the Client
            // credential directly into `/etc/shaprint-credentials/network-channel` (`0640
            // root:shaprint` or `0600`) so it lives in root-owned machine credential storage that
            // the unprivileged `shaprint` daemon can read while `/etc/shaprint` remains
            // strictly non-secret (ADR 0018). If the subsequent IPC call fails, restore the
            // previous credential file state so Client and Server never diverge.
            let root_cred = if options.socket_path == Path::new(DEFAULT_SOCKET_PATH)
                && current_effective_uid() == Some(0)
            {
                let cred_path = Path::new(DEFAULT_CREDENTIAL_DIR).join(CLIENT_CREDENTIAL_FILE);
                let previous = std::fs::read_to_string(&cred_path).ok();
                crate::adapters::ipps::persist_root_machine_credential(&cred_path, &secret)?;
                Some((cred_path, previous))
            } else {
                None
            };
            match client.configure_network_channel(&secret).await {
                Ok(nc) => format_network_channel_status(&nc, options.json),
                Err(error) => {
                    if let Some((cred_path, previous)) = root_cred {
                        match previous {
                            Some(prev) => {
                                let _ = crate::adapters::ipps::persist_root_machine_credential(
                                    &cred_path,
                                    prev.trim_end_matches(['\r', '\n']),
                                );
                            }
                            None => {
                                let _ = std::fs::remove_file(&cred_path);
                            }
                        }
                    }
                    Err(error)
                }
            }
        }
        CliCommand::InspectServer { address } => {
            let review = client.inspect_server(address).await?;
            format_server_review(&review, options.json)
        }
        CliCommand::ApproveServer {
            address,
            fingerprint,
        } => {
            let review = client.approve_server(address, fingerprint).await?;
            format_server_review(&review, options.json)
        }
        CliCommand::ForgetServer { address } => {
            let forgotten = client.forget_server(address).await?;
            if options.json {
                format_json_line(
                    &serde_json::json!({
                        "address": forgotten,
                        "trusted": false,
                    }),
                    "forget",
                )
            } else {
                Ok(format!("forgotten: {forgotten}\n"))
            }
        }
        CliCommand::ListTrustedServers => {
            let servers = client.list_trusted_servers().await?;
            if options.json {
                format_json_line(&servers, "trusted servers")
            } else {
                let mut out = String::new();
                for server in &servers {
                    out.push_str(&format!("{} {}\n", server.address, server.fingerprint));
                }
                Ok(out)
            }
        }
        CliCommand::ListServerPrinters { address } => {
            let printers = client.list_server_printers(address).await?;
            if options.json {
                format_json_line(&printers, "server printers")
            } else {
                let mut out = String::new();
                for printer in &printers.printers {
                    out.push_str(&format!("{printer}\n"));
                }
                Ok(out)
            }
        }
        CliCommand::InstallClientQueue { address, printer } => {
            let queue = client.install_client_queue(address, printer).await?;
            format_client_queue(&queue, options.json)
        }
        CliCommand::RepairClientQueue { address, printer } => {
            let queue = client.repair_client_queue(address, printer).await?;
            format_client_queue(&queue, options.json)
        }
        CliCommand::ListClientQueues => {
            let queues = client.list_client_queues().await?;
            if options.json {
                format_json_line(&queues, "client queues")
            } else {
                let mut out = String::new();
                for q in &queues {
                    out.push_str(&format!(
                        "{} -> {} ({})\n",
                        q.queue_name, q.printer_name, q.server_address
                    ));
                }
                Ok(out)
            }
        }
        CliCommand::ListLocalPrinters => {
            let printers = client.list_local_printers().await?;
            format_local_printers(&printers, options.json)
        }
        CliCommand::SetSharedPrinters { printers } => {
            let updated = client.set_shared_printers(printers).await?;
            format_local_printers(&updated, options.json)
        }
    }
}

fn format_client_queue(queue: &ClientQueueDto, json: bool) -> Result<String, AppError> {
    if json {
        format_json_line(queue, "client queue")
    } else {
        let cups_destination = crate::domain::PrinterName::parse(&queue.printer_name)
            .and_then(|printer| {
                crate::domain::ClientQueueRequest::new(&queue.server_address, printer)
            })
            .map(|req| req.cups_destination_name())
            .unwrap_or_else(|_| queue.queue_name.clone());
        Ok(format!(
            "cups_destination: {cups_destination}\nqueue_name: {}\nserver_address: {}\nprinter_name: {}\nuri: {}\n",
            queue.queue_name, queue.server_address, queue.printer_name, queue.uri
        ))
    }
}

fn format_local_printers(printers: &[LocalPrinterDto], json: bool) -> Result<String, AppError> {
    if json {
        format_json_line(printers, "local printers")
    } else {
        let mut out = String::new();
        for printer in printers {
            let status = if printer.shared { "shared" } else { "unshared" };
            out.push_str(&format!("{}: {status}\n", printer.name));
        }
        Ok(out)
    }
}

fn format_json_line<T: Serialize + ?Sized>(value: &T, label: &str) -> Result<String, AppError> {
    let mut payload = serde_json::to_string(value)
        .map_err(|error| AppError::internal(format!("cannot format {label} JSON: {error}")))?;
    payload.push('\n');
    Ok(payload)
}

fn format_network_channel_status(
    status: &NetworkChannelStatusDto,
    json: bool,
) -> Result<String, AppError> {
    if json {
        format_json_line(status, "network-channel")
    } else if status.configured {
        Ok("network-channel: configured\n".to_owned())
    } else {
        Ok("network-channel: unconfigured\n".to_owned())
    }
}

fn format_server_review(review: &ServerTrustReviewDto, json: bool) -> Result<String, AppError> {
    if json {
        format_json_line(review, "server review")
    } else {
        let mut out = format!(
            "address: {}\ncurrent_fingerprint: {}\ntrusted: {}\n",
            review.address, review.current_fingerprint, review.trusted
        );
        if let Some(prev) = &review.previous_fingerprint {
            out.push_str(&format!("previous_fingerprint: {prev}\n"));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::dto::ServiceStatusDto;

    #[test]
    fn daemon_args_default_to_standard_linux_paths_and_ports() {
        let defaults = parse_daemon_args(&["shaprintd"]).expect("parses default args");
        assert_eq!(defaults.config_dir(), Path::new(DEFAULT_CONFIG_DIR));
        assert_eq!(defaults.state_dir(), Path::new(DEFAULT_STATE_DIR));
        assert_eq!(defaults.socket_path(), Path::new(DEFAULT_SOCKET_PATH));
        assert_eq!(
            defaults.access_policy(),
            PeerAccessPolicy::RootOrDaemonOwner
        );
        assert!(defaults.forbid_root());
        assert_eq!(defaults.ports(), &RuntimePorts::default());
    }

    #[test]
    fn cli_args_parse_status_service_actions_and_shutdown() {
        let status = parse_cli_args(&["shaprintctl", "--json", "status"]).expect("status");
        assert!(status.json);
        assert_eq!(status.command, CliCommand::Status);

        let start =
            parse_cli_args(&["shaprintctl", "start", "client-proxy"]).expect("start service");
        assert_eq!(
            start.command,
            CliCommand::StartService(ServiceId::ClientProxy)
        );

        let stop_service =
            parse_cli_args(&["shaprintctl", "stop", "server-sharing"]).expect("stop service");
        assert_eq!(
            stop_service.command,
            CliCommand::StopService(ServiceId::ServerSharing)
        );

        let stop_daemon = parse_cli_args(&["shaprintctl", "stop"]).expect("stop daemon");
        assert_eq!(stop_daemon.command, CliCommand::Shutdown);
    }

    #[test]
    fn status_formatting_renders_human_and_json_views() {
        let dto = RuntimeStatusDto {
            services: vec![
                ServiceStatusDto {
                    id: "client-proxy".to_owned(),
                    state: "running".to_owned(),
                    detail: "running".to_owned(),
                },
                ServiceStatusDto {
                    id: "server-sharing".to_owned(),
                    state: "stopped".to_owned(),
                    detail: String::new(),
                },
            ],
            network_channel_configured: None,
        };

        let text = format_runtime_status(&dto, false).expect("formats text");
        assert_eq!(text, "client-proxy: running\nserver-sharing: stopped\n");

        let json = format_runtime_status(&dto, true).expect("formats json");
        let parsed: RuntimeStatusDto = serde_json::from_str(&json).expect("valid json");
        assert_eq!(parsed, dto);
    }

    #[test]
    fn cli_args_parse_network_channel_and_trust_commands_and_reject_secret_arguments() {
        let nc_status =
            parse_cli_args(&["shaprintctl", "network-channel", "status"]).expect("nc status");
        assert_eq!(nc_status.command, CliCommand::NetworkChannelStatus);

        let nc_prompt =
            parse_cli_args(&["shaprintctl", "network-channel", "set"]).expect("nc prompt");
        assert_eq!(
            nc_prompt.command,
            CliCommand::SetNetworkChannel(SecretInputSource::PromptOrStdin)
        );

        let nc_stdin = parse_cli_args(&["shaprintctl", "network-channel", "set", "--stdin"])
            .expect("nc stdin");
        assert_eq!(
            nc_stdin.command,
            CliCommand::SetNetworkChannel(SecretInputSource::Stdin)
        );

        let nc_fd =
            parse_cli_args(&["shaprintctl", "network-channel", "set", "--fd", "3"]).expect("nc fd");
        assert_eq!(
            nc_fd.command,
            CliCommand::SetNetworkChannel(SecretInputSource::FileDescriptor(3))
        );

        let nc_cred = parse_cli_args(&[
            "shaprintctl",
            "network-channel",
            "set",
            "--credential-file",
            "/run/credentials/shaprintd.service/network-channel",
        ])
        .expect("nc credential file");
        assert_eq!(
            nc_cred.command,
            CliCommand::SetNetworkChannel(SecretInputSource::CredentialFile(PathBuf::from(
                "/run/credentials/shaprintd.service/network-channel"
            )))
        );

        // Passing the Network Channel value on the command line must be rejected so it never leaks
        // into process listings or shell history.
        for forbidden in [
            &["shaprintctl", "network-channel", "set", "inline-secret"][..],
            &[
                "shaprintctl",
                "network-channel",
                "set",
                "--value",
                "inline-secret",
            ][..],
            &["shaprintctl", "--network-channel", "inline-secret"][..],
            &["shaprintctl", "--network-channel=inline-secret"][..],
            &[
                "shaprintctl",
                "network-channel",
                "set",
                "--channel=inline-secret",
            ][..],
            &[
                "shaprintctl",
                "network-channel",
                "set",
                "--secret=inline-secret",
            ][..],
            &[
                "shaprintctl",
                "network-channel",
                "set",
                "--unknown-flag=inline-secret",
            ][..],
        ] {
            let err = parse_cli_args(forbidden).expect_err("must reject inline secret on CLI");
            assert_eq!(err.code(), ErrorCode::InvalidInput);
            assert!(
                !err.message().contains("inline-secret"),
                "error message must never echo an inline secret value: {}",
                err.message()
            );
        }

        let inspect = parse_cli_args(&["shaprintctl", "trust", "inspect", "10.0.0.5:48631"])
            .expect("inspect");
        assert_eq!(
            inspect.command,
            CliCommand::InspectServer {
                address: "10.0.0.5:48631".to_owned()
            }
        );

        let approve_flag = parse_cli_args(&[
            "shaprintctl",
            "trust",
            "approve",
            "10.0.0.5:48631",
            "--fingerprint",
            "AA:BB",
        ])
        .expect("approve with flag");
        assert_eq!(
            approve_flag.command,
            CliCommand::ApproveServer {
                address: "10.0.0.5:48631".to_owned(),
                fingerprint: "AA:BB".to_owned(),
            }
        );

        let approve_positional =
            parse_cli_args(&["shaprintctl", "trust", "approve", "10.0.0.5:48631", "AA:BB"])
                .expect("approve positional");
        assert_eq!(
            approve_positional.command,
            CliCommand::ApproveServer {
                address: "10.0.0.5:48631".to_owned(),
                fingerprint: "AA:BB".to_owned(),
            }
        );

        let forget =
            parse_cli_args(&["shaprintctl", "trust", "forget", "10.0.0.5:48631"]).expect("forget");
        assert_eq!(
            forget.command,
            CliCommand::ForgetServer {
                address: "10.0.0.5:48631".to_owned()
            }
        );

        let list = parse_cli_args(&["shaprintctl", "trust", "list"]).expect("trust list");
        assert_eq!(list.command, CliCommand::ListTrustedServers);

        let printers =
            parse_cli_args(&["shaprintctl", "printers", "10.0.0.5:48631"]).expect("printers");
        assert_eq!(
            printers.command,
            CliCommand::ListServerPrinters {
                address: "10.0.0.5:48631".to_owned()
            }
        );
    }

    #[test]
    fn secret_input_reader_reads_from_reader_fd_and_credential_file_and_rejects_empty() {
        use std::io::Cursor;

        let from_reader =
            read_secret_from_reader(Cursor::new(b"my-channel-value\r\n")).expect("reads line");
        assert_eq!(from_reader, "my-channel-value");

        let empty_err =
            read_secret_from_reader(Cursor::new(b"\n")).expect_err("rejects empty secret");
        assert_eq!(empty_err.code(), ErrorCode::InvalidInput);
    }

    #[test]
    fn cli_args_parse_queue_and_sharing_commands() {
        let install = parse_cli_args(&[
            "shaprintctl",
            "queue",
            "install",
            "10.0.0.5:48631",
            "Office Printer",
        ])
        .expect("queue install");
        assert_eq!(
            install.command,
            CliCommand::InstallClientQueue {
                address: "10.0.0.5:48631".to_owned(),
                printer: "Office Printer".to_owned(),
            }
        );

        let repair = parse_cli_args(&[
            "shaprintctl",
            "queue",
            "repair",
            "10.0.0.5:48631",
            "Office Printer",
        ])
        .expect("queue repair");
        assert_eq!(
            repair.command,
            CliCommand::RepairClientQueue {
                address: "10.0.0.5:48631".to_owned(),
                printer: "Office Printer".to_owned(),
            }
        );

        let queue_list = parse_cli_args(&["shaprintctl", "queue", "list"]).expect("queue list");
        assert_eq!(queue_list.command, CliCommand::ListClientQueues);

        let sharing_printers =
            parse_cli_args(&["shaprintctl", "sharing", "printers"]).expect("sharing printers");
        assert_eq!(sharing_printers.command, CliCommand::ListLocalPrinters);

        let sharing_set = parse_cli_args(&[
            "shaprintctl",
            "sharing",
            "set",
            "Driverless_IPP",
            "USB_LaserJet",
        ])
        .expect("sharing set");
        assert_eq!(
            sharing_set.command,
            CliCommand::SetSharedPrinters {
                printers: vec!["Driverless_IPP".to_owned(), "USB_LaserJet".to_owned()],
            }
        );

        let sharing_clear =
            parse_cli_args(&["shaprintctl", "sharing", "clear"]).expect("sharing clear");
        assert_eq!(
            sharing_clear.command,
            CliCommand::SetSharedPrinters {
                printers: Vec::new(),
            }
        );
    }
}
