//! Safe binding for ShaPrint's fixed runtime ports.
//!
//! A busy port is reclaimed only after the operating-system socket table identifies every owner as
//! another ShaPrint process. Unknown and unrelated processes are never terminated (ADR 0014).

use std::{collections::HashSet, io, net::SocketAddr, process, sync::Arc, time::Duration};

#[cfg(target_os = "linux")]
use std::path::Path;

use tokio::net::{TcpListener, UdpSocket};

use crate::domain::AppError;

const DEFAULT_RECLAIM_TIMEOUT: Duration = Duration::from_secs(3);
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(50);
const UNKNOWN_PROCESS_NAME: &str = "<unknown>";

/// Socket transport whose local port is being inspected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortTransport {
    Tcp,
    Udp,
}

impl PortTransport {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tcp => "TCP",
            Self::Udp => "UDP",
        }
    }
}

/// One process reported as owning a local socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortOwner {
    pub process_id: u32,
    pub process_name: String,
    #[cfg(target_os = "linux")]
    linux_identity: Option<LinuxProcessIdentity>,
}

#[cfg(target_os = "linux")]
#[derive(Debug, Clone, PartialEq, Eq)]
struct LinuxProcessIdentity {
    executable: String,
    start_time: u64,
}

impl PortOwner {
    pub fn new(process_id: u32, process_name: impl Into<String>) -> Self {
        Self {
            process_id,
            process_name: process_name.into(),
            #[cfg(target_os = "linux")]
            linux_identity: None,
        }
    }

    #[cfg(target_os = "linux")]
    fn with_linux_identity(mut self, identity: Option<LinuxProcessIdentity>) -> Self {
        self.linux_identity = identity;
        self
    }

    pub fn is_shaprint(&self) -> bool {
        let name = self
            .process_name
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or(self.process_name.as_str())
            .trim_end_matches(".exe");
        name.eq_ignore_ascii_case("shaprint")
            || name.eq_ignore_ascii_case("shaprint-desktop")
            || name.eq_ignore_ascii_case("shaprintd")
    }

    fn is_unknown(&self) -> bool {
        self.process_name == UNKNOWN_PROCESS_NAME
    }
}

/// Operating-system seam for finding and terminating socket owners.
///
/// Implementations must return every process that owns a matching endpoint. The binder refuses to
/// reclaim a port if even one reported owner is not a stale ShaPrint process.
pub trait PortOwnerInspector: Send + Sync + 'static {
    fn owners(&self, transport: PortTransport, port: u16) -> io::Result<Vec<PortOwner>>;

    fn terminate(&self, owner: &PortOwner) -> io::Result<()>;

    fn current_process_id(&self) -> u32 {
        process::id()
    }
}

/// Binds runtime listeners and safely reclaims stale ShaPrint-owned ports.
#[derive(Clone)]
pub struct PortBinder {
    inspector: Arc<dyn PortOwnerInspector>,
    reclaim_timeout: Duration,
    poll_interval: Duration,
}

impl Default for PortBinder {
    fn default() -> Self {
        Self::system()
    }
}

impl PortBinder {
    /// Creates a binder backed by the current platform's socket-table inspector.
    pub fn system() -> Self {
        Self::with_inspector(Arc::new(SystemPortOwnerInspector))
    }

    /// Creates a binder using an injected inspector, primarily for lifecycle tests.
    pub fn with_inspector(inspector: Arc<dyn PortOwnerInspector>) -> Self {
        Self {
            inspector,
            reclaim_timeout: DEFAULT_RECLAIM_TIMEOUT,
            poll_interval: DEFAULT_POLL_INTERVAL,
        }
    }

    /// Overrides the maximum time spent waiting for a terminated process to release its socket.
    #[must_use]
    pub fn with_reclaim_timeout(mut self, timeout: Duration) -> Self {
        self.reclaim_timeout = timeout;
        self
    }

    /// Overrides the interval between socket-owner checks.
    #[must_use]
    pub fn with_poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }

    /// Binds a TCP listener, reclaiming a stale ShaPrint listener if necessary.
    pub async fn bind_tcp(&self, address: SocketAddr) -> Result<TcpListener, AppError> {
        match TcpListener::bind(address).await {
            Ok(listener) => Ok(listener),
            Err(error) if error.kind() == io::ErrorKind::AddrInUse => {
                self.reclaim(PortTransport::Tcp, address.port()).await?;
                TcpListener::bind(address)
                    .await
                    .map_err(|error| self.bind_failure(PortTransport::Tcp, address.port(), error))
            }
            Err(error) => Err(self.bind_failure(PortTransport::Tcp, address.port(), error)),
        }
    }

    /// Binds a UDP socket, reclaiming a stale ShaPrint listener if necessary.
    pub async fn bind_udp(&self, address: SocketAddr) -> Result<UdpSocket, AppError> {
        match UdpSocket::bind(address).await {
            Ok(socket) => Ok(socket),
            Err(error) if error.kind() == io::ErrorKind::AddrInUse => {
                self.reclaim(PortTransport::Udp, address.port()).await?;
                UdpSocket::bind(address)
                    .await
                    .map_err(|error| self.bind_failure(PortTransport::Udp, address.port(), error))
            }
            Err(error) => Err(self.bind_failure(PortTransport::Udp, address.port(), error)),
        }
    }

    async fn reclaim(&self, transport: PortTransport, port: u16) -> Result<(), AppError> {
        let owners = self.inspect(transport, port).await?;
        if owners.is_empty() {
            return Err(AppError::internal(format!(
                "{} port {port} is in use, but its process owner could not be identified; no process was terminated",
                transport.as_str()
            )));
        }

        let current_process_id = self.inspector.current_process_id();
        for owner in &owners {
            if owner.process_id == current_process_id || !owner.is_shaprint() {
                return Err(conflict_error(transport, port, owner));
            }
        }

        let terminated_process_ids = owners
            .iter()
            .map(|owner| owner.process_id)
            .collect::<HashSet<_>>();
        for owner in &owners {
            self.terminate(owner, transport, port).await?;
        }

        let deadline = tokio::time::Instant::now() + self.reclaim_timeout;
        loop {
            let remaining = self.inspect(transport, port).await?;
            if remaining.is_empty() {
                return Ok(());
            }
            for owner in &remaining {
                let recently_terminated_unknown =
                    owner.is_unknown() && terminated_process_ids.contains(&owner.process_id);
                if owner.process_id == current_process_id
                    || (!owner.is_shaprint() && !recently_terminated_unknown)
                {
                    return Err(conflict_error(transport, port, owner));
                }
            }
            if tokio::time::Instant::now() >= deadline {
                let owner = &remaining[0];
                return Err(AppError::internal(format!(
                    "stale ShaPrint process '{}' (PID {}) did not release {} port {port} within 3 seconds",
                    owner.process_name,
                    owner.process_id,
                    transport.as_str()
                )));
            }
            tokio::time::sleep(self.poll_interval).await;
        }
    }

    async fn inspect(
        &self,
        transport: PortTransport,
        port: u16,
    ) -> Result<Vec<PortOwner>, AppError> {
        let inspector = Arc::clone(&self.inspector);
        tokio::task::spawn_blocking(move || inspector.owners(transport, port))
            .await
            .map_err(|_| AppError::internal("the port-owner inspection task failed"))?
            .map_err(|error| {
                AppError::internal(format!(
                    "cannot inspect the process using {} port {port}; no process was terminated: {error}",
                    transport.as_str()
                ))
            })
    }

    async fn terminate(
        &self,
        owner: &PortOwner,
        transport: PortTransport,
        port: u16,
    ) -> Result<(), AppError> {
        let inspector = Arc::clone(&self.inspector);
        let owner = owner.clone();
        let process_name = owner.process_name.clone();
        let process_id = owner.process_id;
        tokio::task::spawn_blocking(move || inspector.terminate(&owner))
            .await
            .map_err(|_| AppError::internal("the stale-process termination task failed"))?
            .map_err(|error| {
                AppError::internal(format!(
                    "could not terminate stale ShaPrint process '{}' (PID {}) holding {} port {port}: {error}",
                    process_name,
                    process_id,
                    transport.as_str()
                ))
            })
    }

    fn bind_failure(&self, transport: PortTransport, port: u16, error: io::Error) -> AppError {
        if error.kind() == io::ErrorKind::AddrInUse {
            return AppError::internal(format!(
                "{} port {port} remains in use after stale-process reclamation; no unrelated process was terminated",
                transport.as_str()
            ));
        }
        AppError::internal(format!(
            "cannot bind {} port {port}: {error}",
            transport.as_str()
        ))
    }
}

fn conflict_error(transport: PortTransport, port: u16, owner: &PortOwner) -> AppError {
    AppError::internal(format!(
        "{} port {port} is in use by process '{}' (PID {}); ShaPrint did not terminate it. Close that program or choose another port.",
        transport.as_str(),
        owner.process_name,
        owner.process_id
    ))
}

/// Platform socket-table lookup and process termination.
pub struct SystemPortOwnerInspector;

#[cfg(windows)]
fn powershell_command() -> std::process::Command {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut command = std::process::Command::new("powershell.exe");
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

#[cfg(windows)]
impl PortOwnerInspector for SystemPortOwnerInspector {
    fn owners(&self, transport: PortTransport, port: u16) -> io::Result<Vec<PortOwner>> {
        let query = match transport {
            PortTransport::Tcp => format!(
                "$ids = @(Get-NetTCPConnection -LocalPort {port} -State Listen -ErrorAction SilentlyContinue | Select-Object -ExpandProperty OwningProcess -Unique); "
            ),
            PortTransport::Udp => format!(
                "$ids = @(Get-NetUDPEndpoint -LocalPort {port} -ErrorAction SilentlyContinue | Select-Object -ExpandProperty OwningProcess -Unique); "
            ),
        };
        let script = format!(
            "$ErrorActionPreference = 'Stop'; {query} foreach ($id in $ids) {{ $p = Get-Process -Id $id -ErrorAction SilentlyContinue; $name = if ($null -ne $p) {{ $p.ProcessName }} else {{ '<unknown>' }}; Write-Output ('{{0}}|{{1}}' -f $name, $id) }}"
        );
        let output = powershell_command()
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                &script,
            ])
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(
                String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            ));
        }

        let owners = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| {
                let (process_name, process_id) = line.trim().split_once('|')?;
                Some(PortOwner::new(
                    process_id.parse().ok()?,
                    process_name.to_owned(),
                ))
            })
            .collect::<Vec<_>>();
        Ok(owners)
    }

    fn terminate(&self, owner: &PortOwner) -> io::Result<()> {
        if !owner.is_shaprint() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "refusing to terminate a process not recognized as ShaPrint",
            ));
        }
        let script = format!(
            "$ErrorActionPreference = 'Stop'; $p = Get-Process -Id {} -ErrorAction Stop; if ($p.ProcessName -notmatch '^(?i:shaprint(?:-desktop)?)$') {{ throw 'process identity changed' }}; Stop-Process -Id {} -Force -ErrorAction Stop",
            owner.process_id, owner.process_id
        );
        let output = powershell_command()
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                &script,
            ])
            .output()?;
        if output.status.success() {
            Ok(())
        } else {
            Err(io::Error::other(
                String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            ))
        }
    }
}

#[cfg(target_os = "linux")]
impl PortOwnerInspector for SystemPortOwnerInspector {
    fn owners(&self, transport: PortTransport, port: u16) -> io::Result<Vec<PortOwner>> {
        let socket_inodes = linux_socket_inodes(transport, port)?;
        if socket_inodes.is_empty() {
            return Ok(Vec::new());
        }

        let mut remaining_inodes = socket_inodes.clone();
        let current_pid = process::id();

        // Fast-path: check current process first.
        let mut owns_self = false;
        if let Ok(fds) = std::fs::read_dir(format!("/proc/{current_pid}/fd")) {
            for fd in fds.flatten() {
                if let Ok(target) = std::fs::read_link(fd.path()) {
                    let target_str = target.to_string_lossy();
                    if let Some(inode) = target_str
                        .strip_prefix("socket:[")
                        .and_then(|value| value.strip_suffix(']'))
                        .and_then(|value| value.parse::<u64>().ok())
                    {
                        if remaining_inodes.remove(&inode) {
                            owns_self = true;
                        }
                    }
                }
            }
        }

        let mut owners = Vec::new();
        if owns_self {
            owners.push(
                PortOwner::new(current_pid, linux_process_name(current_pid))
                    .with_linux_identity(linux_process_identity(current_pid)),
            );
            if remaining_inodes.is_empty() {
                return Ok(owners);
            }
        }

        let mut seen_pids = HashSet::new();
        seen_pids.insert(current_pid);

        for entry in std::fs::read_dir("/proc")? {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse().ok())
            else {
                continue;
            };
            if !seen_pids.insert(pid) {
                continue;
            }

            let fd_dir = entry.path().join("fd");
            let fds = match std::fs::read_dir(fd_dir) {
                Ok(fds) => fds,
                Err(_) => continue,
            };
            let mut owns_socket = false;
            for fd in fds.flatten() {
                let target = match std::fs::read_link(fd.path()) {
                    Ok(target) => target,
                    Err(_) => continue,
                };
                let target = target.to_string_lossy();
                let Some(inode) = target
                    .strip_prefix("socket:[")
                    .and_then(|value| value.strip_suffix(']'))
                    .and_then(|value| value.parse::<u64>().ok())
                else {
                    continue;
                };
                if remaining_inodes.remove(&inode) {
                    owns_socket = true;
                }
            }
            if owns_socket {
                owners.push(
                    PortOwner::new(pid, linux_process_name(pid))
                        .with_linux_identity(linux_process_identity(pid)),
                );
                if remaining_inodes.is_empty() {
                    break;
                }
            }
        }

        owners.sort_by_key(|owner| owner.process_id);
        Ok(owners)
    }

    fn terminate(&self, owner: &PortOwner) -> io::Result<()> {
        if owner.process_id == process::id() || !owner.is_shaprint() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "refusing to terminate the current or an unrelated process",
            ));
        }
        let Some(expected_identity) = owner.linux_identity.as_ref() else {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "stale port owner identity is unavailable; refusing to terminate it",
            ));
        };
        // Re-read the executable and start time immediately before signalling. This avoids sending
        // SIGTERM to a reused PID, even when the replacement process has the same name.
        let Some(current_identity) = linux_process_identity(owner.process_id) else {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "stale port owner identity disappeared; refusing to terminate it",
            ));
        };
        if &current_identity != expected_identity
            || !linux_executable_is_shaprint(&current_identity.executable)
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "stale port owner identity changed; refusing to terminate it",
            ));
        }
        let current_name = linux_process_name(owner.process_id);
        if current_name == UNKNOWN_PROCESS_NAME
            || !current_name.eq_ignore_ascii_case(&owner.process_name)
            || !PortOwner::new(owner.process_id, current_name.as_str()).is_shaprint()
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "stale port owner identity changed; refusing to terminate it",
            ));
        }
        let status = process::Command::new("/bin/kill")
            .arg("-TERM")
            .arg(owner.process_id.to_string())
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "kill -TERM failed with status {status}"
            )))
        }
    }
}

#[cfg(target_os = "linux")]
fn linux_socket_inodes(transport: PortTransport, port: u16) -> io::Result<HashSet<u64>> {
    let paths: &[&str] = match transport {
        PortTransport::Tcp => &["/proc/net/tcp", "/proc/net/tcp6"],
        PortTransport::Udp => &["/proc/net/udp", "/proc/net/udp6"],
    };
    let mut inodes = HashSet::new();
    for path in paths {
        let contents = match std::fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        for fields in contents
            .lines()
            .skip(1)
            .map(|line| line.split_whitespace().collect::<Vec<_>>())
        {
            let Some(local_address) = fields.get(1) else {
                continue;
            };
            let Some((_, port_hex)) = local_address.rsplit_once(':') else {
                continue;
            };
            if u16::from_str_radix(port_hex, 16).ok() != Some(port) {
                continue;
            }
            // TCP state 0A is LISTEN. UDP has no listening state and may be represented by 07.
            if transport == PortTransport::Tcp && fields.get(3) != Some(&"0A") {
                continue;
            }
            if let Some(inode) = fields.get(9).and_then(|value| value.parse::<u64>().ok()) {
                inodes.insert(inode);
            }
        }
    }
    Ok(inodes)
}

#[cfg(target_os = "linux")]
fn linux_process_name(pid: u32) -> String {
    std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .map(|name| name.trim().to_owned())
        .ok()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| UNKNOWN_PROCESS_NAME.to_owned())
}

#[cfg(target_os = "linux")]
fn linux_process_identity(pid: u32) -> Option<LinuxProcessIdentity> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let command_end = stat.rfind(')')?;
    let start_time = stat
        .get(command_end + 1..)?
        .split_whitespace()
        .nth(19)?
        .parse()
        .ok()?;
    let executable = std::fs::read_link(format!("/proc/{pid}/exe"))
        .ok()?
        .to_string_lossy()
        .into_owned();
    Some(LinuxProcessIdentity {
        executable,
        start_time,
    })
}

#[cfg(target_os = "linux")]
fn linux_executable_is_shaprint(executable: &str) -> bool {
    Path::new(executable)
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| PortOwner::new(0, name).is_shaprint())
        .unwrap_or(false)
}

#[cfg(not(any(windows, target_os = "linux")))]
impl PortOwnerInspector for SystemPortOwnerInspector {
    fn owners(&self, _transport: PortTransport, _port: u16) -> io::Result<Vec<PortOwner>> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "automatic process inspection is currently supported on Windows and Linux only",
        ))
    }

    fn terminate(&self, _owner: &PortOwner) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "automatic process termination is currently supported on Windows and Linux only",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        net::UdpSocket as StdUdpSocket,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Mutex,
        },
    };

    struct FakeInspector {
        owner: Mutex<Option<PortOwner>>,
        held_tcp: Mutex<Option<std::net::TcpListener>>,
        held_udp: Mutex<Option<StdUdpSocket>>,
        terminations: AtomicUsize,
        current_process_id: u32,
    }

    impl FakeInspector {
        fn new(owner: PortOwner) -> Self {
            Self {
                owner: Mutex::new(Some(owner)),
                held_tcp: Mutex::new(None),
                held_udp: Mutex::new(None),
                terminations: AtomicUsize::new(0),
                current_process_id: process::id(),
            }
        }
    }

    impl PortOwnerInspector for FakeInspector {
        fn owners(&self, _transport: PortTransport, _port: u16) -> io::Result<Vec<PortOwner>> {
            Ok(self
                .owner
                .lock()
                .map_err(|_| io::Error::other("owner lock poisoned"))?
                .clone()
                .into_iter()
                .collect())
        }

        fn terminate(&self, _owner: &PortOwner) -> io::Result<()> {
            self.terminations.fetch_add(1, Ordering::SeqCst);
            self.owner
                .lock()
                .map_err(|_| io::Error::other("owner lock poisoned"))?
                .take();
            self.held_tcp
                .lock()
                .map_err(|_| io::Error::other("TCP lock poisoned"))?
                .take();
            self.held_udp
                .lock()
                .map_err(|_| io::Error::other("UDP lock poisoned"))?
                .take();
            Ok(())
        }

        fn current_process_id(&self) -> u32 {
            self.current_process_id
        }
    }

    struct DelayedReleaseInspector {
        held_tcp: Mutex<Option<std::net::TcpListener>>,
        inspections: AtomicUsize,
        terminations: AtomicUsize,
    }

    impl PortOwnerInspector for DelayedReleaseInspector {
        fn owners(&self, _transport: PortTransport, _port: u16) -> io::Result<Vec<PortOwner>> {
            let inspection = self.inspections.fetch_add(1, Ordering::SeqCst);
            match inspection {
                0 => Ok(vec![PortOwner::new(42, "shaprint.exe")]),
                1 | 2 => Ok(vec![PortOwner::new(42, "<unknown>")]),
                _ => {
                    self.held_tcp
                        .lock()
                        .map_err(|_| io::Error::other("TCP lock poisoned"))?
                        .take();
                    Ok(Vec::new())
                }
            }
        }

        fn terminate(&self, _owner: &PortOwner) -> io::Result<()> {
            self.terminations.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn current_process_id(&self) -> u32 {
            process::id()
        }
    }

    #[tokio::test]
    async fn reclaim_waits_for_socket_release_after_terminated_owner_disappears() {
        let held = std::net::TcpListener::bind("127.0.0.1:0").expect("bind test port");
        let address = held.local_addr().expect("read test port");
        let inspector = Arc::new(DelayedReleaseInspector {
            held_tcp: Mutex::new(Some(held)),
            inspections: AtomicUsize::new(0),
            terminations: AtomicUsize::new(0),
        });
        let binder = PortBinder::with_inspector(inspector.clone())
            .with_reclaim_timeout(Duration::from_millis(100))
            .with_poll_interval(Duration::from_millis(1));

        let listener = binder.bind_tcp(address).await.expect("wait and bind");

        assert_eq!(listener.local_addr().expect("listener address"), address);
        assert_eq!(inspector.terminations.load(Ordering::SeqCst), 1);
        assert!(inspector.inspections.load(Ordering::SeqCst) >= 4);
    }

    #[tokio::test]
    async fn stale_shaprint_tcp_owner_is_terminated_before_binding() {
        let held = std::net::TcpListener::bind("127.0.0.1:0").expect("bind test port");
        let address = held.local_addr().expect("read test port");
        let owner = PortOwner::new(42, "shaprint-desktop");
        let inspector = Arc::new(FakeInspector::new(owner));
        *inspector.held_tcp.lock().expect("TCP lock") = Some(held);
        let binder = PortBinder::with_inspector(inspector.clone());

        let listener = binder.bind_tcp(address).await.expect("reclaim and bind");

        assert_eq!(listener.local_addr().expect("listener address"), address);
        assert_eq!(inspector.terminations.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn stale_shaprint_udp_owner_is_terminated_before_binding() {
        let held = StdUdpSocket::bind("127.0.0.1:0").expect("bind test port");
        let address = held.local_addr().expect("read test port");
        let owner = PortOwner::new(43, "shaprint.exe");
        let inspector = Arc::new(FakeInspector::new(owner));
        *inspector.held_udp.lock().expect("UDP lock") = Some(held);
        let binder = PortBinder::with_inspector(inspector.clone());

        let socket = binder.bind_udp(address).await.expect("reclaim and bind");

        assert_eq!(socket.local_addr().expect("socket address"), address);
        assert_eq!(inspector.terminations.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn unrelated_port_owner_is_never_terminated_and_is_named_in_error() {
        let held = std::net::TcpListener::bind("127.0.0.1:0").expect("bind test port");
        let address = held.local_addr().expect("read test port");
        let owner = PortOwner::new(4512, "other-app");
        let inspector = Arc::new(FakeInspector::new(owner));
        *inspector.held_tcp.lock().expect("TCP lock") = Some(held);
        let binder = PortBinder::with_inspector(inspector.clone());

        let error = binder.bind_tcp(address).await.expect_err("reject conflict");

        assert!(error.message().contains(&address.port().to_string()));
        assert!(error.message().contains("other-app"));
        assert!(error.message().contains("4512"));
        assert_eq!(inspector.terminations.load(Ordering::SeqCst), 0);
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn linux_port_inspection_reports_an_unrelated_current_process_without_signalling_it() {
        let held = std::net::TcpListener::bind("127.0.0.1:0").expect("bind test port");
        let address = held.local_addr().expect("read test port");
        let error = PortBinder::system()
            .bind_tcp(address)
            .await
            .expect_err("the current test process owns the port");

        assert!(error.message().contains("did not terminate it"));
        assert!(error.message().contains(&process::id().to_string()));
        drop(held);
    }

    #[test]
    fn only_known_shaprint_image_names_are_reclaimable() {
        for name in [
            "shaprint",
            "ShaPrint.exe",
            "shaprint-desktop.exe",
            "shaprintd",
        ] {
            assert!(PortOwner::new(17, name).is_shaprint(), "{name}");
        }
        for name in ["not-shaprint", "shaprint-helper", "other.exe"] {
            assert!(!PortOwner::new(17, name).is_shaprint(), "{name}");
        }
    }
}
