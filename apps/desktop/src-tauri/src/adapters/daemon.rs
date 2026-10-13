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

use crate::application::RuntimeCoordinator;
use crate::domain::{AppError, ErrorCode, ServiceId};
use crate::ipc::dto::{AppErrorDto, RuntimeStatusDto};
use crate::{RuntimePorts, Shell};

/// Default non-secret configuration directory on Linux (ADR 0018).
pub const DEFAULT_CONFIG_DIR: &str = "/etc/shaprint";

/// Default non-secret configuration file name inside [`DEFAULT_CONFIG_DIR`].
pub const DEFAULT_CONFIG_FILE: &str = "config.json";

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
    state_dir: PathBuf,
    socket_path: PathBuf,
    access_policy: PeerAccessPolicy,
    forbid_root: bool,
    ports: RuntimePorts,
}

impl DaemonConfig {
    pub fn new(state_dir: impl Into<PathBuf>, socket_path: impl Into<PathBuf>) -> Self {
        Self {
            config_dir: PathBuf::from(DEFAULT_CONFIG_DIR),
            state_dir: state_dir.into(),
            socket_path: socket_path.into(),
            access_policy: PeerAccessPolicy::RootOrDaemonOwner,
            forbid_root: true,
            ports: RuntimePorts::default(),
        }
    }

    #[must_use]
    pub fn with_config_dir(mut self, config_dir: impl Into<PathBuf>) -> Self {
        self.config_dir = config_dir.into();
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

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
enum ControlRequest {
    Status,
    StartService { id: String },
    StopService { id: String },
    Shutdown,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "kebab-case")]
enum ControlResponse {
    Ok { status: RuntimeStatusDto },
    Err { error: AppErrorDto },
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
        let shell = Arc::new(Shell::with_ports(&config.state_dir, config.ports)?);
        let mut server =
            Self::bind_with_policy(&config.socket_path, shell.runtime(), config.access_policy)
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
        server.shell = Some(shell);
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
                                let conn_cancel = loop_cancel.clone();
                                connections.spawn(async move {
                                    handle_connection(
                                        stream,
                                        conn_runtime,
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
            shell: None,
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
    cancel_tx: watch::Sender<bool>,
    policy: PeerAccessPolicy,
    daemon_uid: Option<u32>,
) {
    let _ = timeout(
        IO_TIMEOUT,
        process_stream(stream, runtime, cancel_tx, policy, daemon_uid),
    )
    .await;
}

async fn process_stream(
    mut stream: UnixStream,
    runtime: Arc<RuntimeCoordinator>,
    cancel_tx: watch::Sender<bool>,
    policy: PeerAccessPolicy,
    daemon_uid: Option<u32>,
) -> Result<(), AppError> {
    let mut stop_daemon = false;
    let outcome = async {
        let peer_cred = stream
            .peer_cred()
            .map_err(|_| AppError::not_authorized("cannot verify local peer credentials"))?;
        match read_json_line::<ControlRequest>(&mut stream, "control request").await? {
            ControlRequest::Status => Ok(RuntimeStatusDto::from(&runtime.status())),
            ControlRequest::StartService { id } => {
                policy.authorize_mutation(peer_cred.uid(), daemon_uid)?;
                let service = ServiceId::parse(&id)?;
                runtime.start(service).await?;
                Ok(RuntimeStatusDto::from(&runtime.status()))
            }
            ControlRequest::StopService { id } => {
                policy.authorize_mutation(peer_cred.uid(), daemon_uid)?;
                let service = ServiceId::parse(&id)?;
                runtime.stop(service).await?;
                Ok(RuntimeStatusDto::from(&runtime.status()))
            }
            ControlRequest::Shutdown => {
                policy.authorize_mutation(peer_cred.uid(), daemon_uid)?;
                runtime.shutdown().await?;
                stop_daemon = true;
                Ok(RuntimeStatusDto::from(&runtime.status()))
            }
        }
    }
    .await;

    let response = match outcome {
        Ok(status) => ControlResponse::Ok { status },
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
        self.send(&ControlRequest::Status).await
    }

    /// Starts a supervised service and returns the updated runtime status.
    pub async fn start_service(&self, id: &str) -> Result<RuntimeStatusDto, AppError> {
        self.send(&ControlRequest::StartService { id: id.to_owned() })
            .await
    }

    /// Stops a supervised service and returns the updated runtime status.
    pub async fn stop_service(&self, id: &str) -> Result<RuntimeStatusDto, AppError> {
        self.send(&ControlRequest::StopService { id: id.to_owned() })
            .await
    }

    /// Stops every supervised service and shuts down `shaprintd`.
    pub async fn shutdown(&self) -> Result<RuntimeStatusDto, AppError> {
        self.send(&ControlRequest::Shutdown).await
    }

    async fn send(&self, request: &ControlRequest) -> Result<RuntimeStatusDto, AppError> {
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
            ControlResponse::Ok { status } => Ok(status),
            ControlResponse::Err { error } => Err(app_error_from_dto(error)),
        }
    }
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

/// Action requested through `shaprintctl`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliCommand {
    Status,
    StartService(ServiceId),
    StopService(ServiceId),
    Shutdown,
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

    let mut iter = arguments.iter().skip(1).map(AsRef::as_ref);
    while let Some(arg) = iter.next() {
        match arg {
            "--config-dir" => {
                let value = iter
                    .next()
                    .ok_or_else(|| AppError::invalid_input("missing value for '--config-dir'"))?;
                config.config_dir = PathBuf::from(value);
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

    let file_config = load_non_secret_config_file(&config.config_dir)?;
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
            other if other.starts_with('-') => {
                return Err(AppError::invalid_input(format!(
                    "unrecognized shaprintctl option '{other}'"
                )));
            }
            other => positional.push(other),
        }
    }

    let command =
        match positional.as_slice() {
            ["status"] => CliCommand::Status,
            ["start", service] => CliCommand::StartService(ServiceId::parse(service)?),
            ["stop"] | ["stop", "daemon"] | ["shutdown"] => CliCommand::Shutdown,
            ["stop", service] => CliCommand::StopService(ServiceId::parse(service)?),
            [] => return Err(AppError::invalid_input(
                "missing command (expected: status, start <service>, stop [service], or shutdown)",
            )),
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
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::dto::ServiceStatusDto;

    #[test]
    fn daemon_args_default_to_standard_linux_paths_and_ports() {
        let defaults = DaemonConfig::default();
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
        };

        let text = format_runtime_status(&dto, false).expect("formats text");
        assert_eq!(text, "client-proxy: running\nserver-sharing: stopped\n");

        let json = format_runtime_status(&dto, true).expect("formats json");
        let parsed: RuntimeStatusDto = serde_json::from_str(&json).expect("valid json");
        assert_eq!(parsed, dto);
    }
}
