//! Safe binding for ShaPrint's fixed runtime ports.
//!
//! A busy port is reclaimed only after the operating-system socket table identifies every owner as
//! another ShaPrint process. Unknown and unrelated processes are never terminated (ADR 0014).

use std::{collections::HashSet, io, net::SocketAddr, process, sync::Arc, time::Duration};

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
    fn as_str(self) -> &'static str {
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
}

impl PortOwner {
    pub fn new(process_id: u32, process_name: impl Into<String>) -> Self {
        Self {
            process_id,
            process_name: process_name.into(),
        }
    }

    fn is_shaprint(&self) -> bool {
        let name = self
            .process_name
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or(self.process_name.as_str())
            .trim_end_matches(".exe");
        name.eq_ignore_ascii_case("shaprint") || name.eq_ignore_ascii_case("shaprint-desktop")
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
struct SystemPortOwnerInspector;

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

#[cfg(not(windows))]
impl PortOwnerInspector for SystemPortOwnerInspector {
    fn owners(&self, _transport: PortTransport, _port: u16) -> io::Result<Vec<PortOwner>> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "automatic process inspection is currently supported on Windows only",
        ))
    }

    fn terminate(&self, _owner: &PortOwner) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "automatic process termination is currently supported on Windows only",
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

    #[test]
    fn only_known_shaprint_image_names_are_reclaimable() {
        for name in ["shaprint", "ShaPrint.exe", "shaprint-desktop.exe"] {
            assert!(PortOwner::new(17, name).is_shaprint(), "{name}");
        }
        for name in ["not-shaprint", "shaprint-helper", "other.exe"] {
            assert!(!PortOwner::new(17, name).is_shaprint(), "{name}");
        }
    }
}
