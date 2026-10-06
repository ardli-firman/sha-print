//! Installing a native Windows queue for a printer shared by a trusted server (issue #35).
//!
//! The use case keeps the queue's destination honest. It installs a queue only for a printer the
//! approved server is sharing right now, and only while the local proxy that will carry the job is
//! running, so the user never ends up with a queue that silently cannot print.

use std::sync::Arc;

use async_trait::async_trait;

use crate::application::{RuntimeCoordinator, Setup};
use crate::domain::{
    AppError, ClientQueueName, ClientQueueRequest, PrinterName, ServiceId, ServiceState,
    SetupFailure,
};

/// The shared printers of a server the user explicitly approved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedPrinters {
    /// The canonical `host:port` form the trust store approved.
    pub address: String,
    /// The queues that server is sharing right now.
    pub printers: Vec<PrinterName>,
}

/// Reads the printers a trusted server currently shares.
///
/// The adapter reuses the client trust flow, so an unapproved address, a server whose certificate
/// changed, or an unreachable server is an error here and no queue is installed for it.
#[async_trait]
pub trait TrustedServerPrinters: Send + Sync + 'static {
    async fn shared_printers(&self, address: &str) -> Result<TrustedPrinters, AppError>;
}

/// Reports whether the local client proxy is running.
pub trait ClientProxyState: Send + Sync + 'static {
    fn is_running(&self) -> bool;
}

/// Reports whether the client has configured a Network Channel for authorized print submission.
pub trait ChannelState: Send + Sync + 'static {
    fn is_configured(&self) -> bool;
}

/// Creates native Windows queues and reports where they send print jobs.
pub trait QueueInstaller: Send + Sync + 'static {
    /// Whether this build has a real queue installer.
    fn is_available(&self) -> bool {
        true
    }

    /// The IPP URI a queue installed for `request` routes through.
    fn queue_uri(&self, request: &ClientQueueRequest) -> Result<String, AppError>;

    /// Installs (or repairs) the queue. The app runs this inside the elevated helper, so a failure
    /// is a classified [`SetupFailure`] rather than free-form text.
    fn install(&self, request: &ClientQueueRequest) -> Result<(), SetupFailure>;
}

impl ClientProxyState for RuntimeCoordinator {
    fn is_running(&self) -> bool {
        self.status().services().iter().any(|service| {
            service.id() == ServiceId::ClientProxy && service.state() == ServiceState::Running
        })
    }
}

/// A native queue the shell installed, and the destination it uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientQueue {
    request: ClientQueueRequest,
    uri: String,
}

impl ClientQueue {
    pub fn new(request: ClientQueueRequest, uri: String) -> Self {
        Self { request, uri }
    }

    pub fn name(&self) -> &ClientQueueName {
        self.request.queue_name()
    }

    pub fn uri(&self) -> &str {
        &self.uri
    }

    pub fn request(&self) -> &ClientQueueRequest {
        &self.request
    }
}

/// Installs native Windows queues for the printers a trusted server shares.
pub struct QueueInstallation {
    setup: Arc<Setup>,
    servers: Arc<dyn TrustedServerPrinters>,
    installer: Arc<dyn QueueInstaller>,
    proxy: Arc<dyn ClientProxyState>,
    channel: Arc<dyn ChannelState>,
}

impl QueueInstallation {
    pub fn new(
        setup: Arc<Setup>,
        servers: Arc<dyn TrustedServerPrinters>,
        installer: Arc<dyn QueueInstaller>,
        proxy: Arc<dyn ClientProxyState>,
        channel: Arc<dyn ChannelState>,
    ) -> Self {
        Self {
            setup,
            servers,
            installer,
            proxy,
            channel,
        }
    }

    /// Installs the native queue for `printer`, shared by the trusted server at `server_address`.
    ///
    /// Every precondition is settled before the UAC prompt, so the user is not asked for
    /// administrator permission for a queue that cannot work.
    pub async fn install(
        &self,
        server_address: &str,
        printer: &str,
    ) -> Result<ClientQueue, AppError> {
        if !self.installer.is_available() {
            return Err(AppError::unsupported(
                "installing a native Windows printer queue is only supported on Windows",
            ));
        }
        let printer = PrinterName::parse(printer)?;
        if !self.proxy.is_running() {
            return Err(AppError::invalid_state(
                "the local print proxy is not running. Start it, then install the queue so the printer can reach the server.",
            ));
        }
        if !self.channel.is_configured() {
            return Err(AppError::invalid_state(
                "a Network Channel is not configured. Set the shared Network Channel before installing the queue.",
            ));
        }
        let trusted = self.servers.shared_printers(server_address).await?;
        if !trusted.printers.contains(&printer) {
            return Err(AppError::invalid_input(format!(
                "the trusted server {} is not sharing a printer named \"{}\". Show its shared printers again and choose one of them.",
                trusted.address, printer
            )));
        }

        let request = ClientQueueRequest::new(&trusted.address, printer)?;
        let uri = self.installer.queue_uri(&request)?;
        self.install_elevated(&request).await?;
        log::info!(
            "installed native client queue={} server={}",
            request.queue_name(),
            request.server_address()
        );
        Ok(ClientQueue::new(request, uri))
    }

    /// Runs the installation in the elevated helper without blocking the async runtime.
    async fn install_elevated(&self, request: &ClientQueueRequest) -> Result<(), AppError> {
        let setup = Arc::clone(&self.setup);
        let request = request.clone();
        tokio::task::spawn_blocking(move || setup.install_queue(&request))
            .await
            .map_err(|error| {
                AppError::internal(format!("the setup action did not finish: {error}"))
            })?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ElevationBroker;
    use crate::domain::{ErrorCode, SetupAction, SetupFailureKind};
    use std::sync::Mutex;

    /// Records the queue installations it was asked to elevate, and can refuse.
    #[derive(Default)]
    struct RecordingBroker {
        installed: Mutex<Vec<ClientQueueRequest>>,
        outcome: Option<SetupFailureKind>,
    }

    impl RecordingBroker {
        fn refusing(kind: SetupFailureKind) -> Arc<Self> {
            Arc::new(Self {
                installed: Mutex::new(Vec::new()),
                outcome: Some(kind),
            })
        }

        fn installed(&self) -> Vec<ClientQueueRequest> {
            self.installed
                .lock()
                .map(|requests| requests.clone())
                .unwrap_or_default()
        }

        fn refusal(&self) -> Option<SetupFailure> {
            self.outcome
                .map(|kind| SetupFailure::new(kind, "recorded failure"))
        }
    }

    impl ElevationBroker for RecordingBroker {
        fn elevate(&self, _action: SetupAction) -> Result<(), SetupFailure> {
            Err(SetupFailure::new(
                SetupFailureKind::Other,
                "no parameterless action is part of this flow",
            ))
        }

        fn install_queue(&self, request: &ClientQueueRequest) -> Result<(), SetupFailure> {
            if let Some(failure) = self.refusal() {
                return Err(failure);
            }
            self.installed
                .lock()
                .map_err(|_| SetupFailure::new(SetupFailureKind::Other, "lock poisoned"))?
                .push(request.clone());
            Ok(())
        }
    }

    /// Serves a fixed printer list, or a fixed error, and counts the queries.
    struct FakeServers {
        address: String,
        printers: Vec<PrinterName>,
        error: Option<&'static str>,
        queries: Mutex<usize>,
    }

    impl FakeServers {
        fn sharing(address: &str, printers: &[&str]) -> Arc<Self> {
            Arc::new(Self {
                address: address.to_owned(),
                printers: printers
                    .iter()
                    .map(|name| PrinterName::parse(name).expect("valid printer name"))
                    .collect(),
                error: None,
                queries: Mutex::new(0),
            })
        }

        fn failing(error: &'static str) -> Arc<Self> {
            Arc::new(Self {
                address: String::new(),
                printers: Vec::new(),
                error: Some(error),
                queries: Mutex::new(0),
            })
        }

        fn queries(&self) -> usize {
            self.queries.lock().map(|count| *count).unwrap_or_default()
        }
    }

    #[async_trait]
    impl TrustedServerPrinters for FakeServers {
        async fn shared_printers(&self, _address: &str) -> Result<TrustedPrinters, AppError> {
            if let Ok(mut queries) = self.queries.lock() {
                *queries += 1;
            }
            match self.error {
                Some(message) => Err(AppError::invalid_state(message)),
                None => Ok(TrustedPrinters {
                    address: self.address.clone(),
                    printers: self.printers.clone(),
                }),
            }
        }
    }

    /// Builds a fixed queue URI and counts any install attempt.
    ///
    /// Installing runs in the elevated helper, never in the app process, so a queue install reaching
    /// this fake is a boundary violation the tests can catch.
    struct FakeInstaller {
        available: bool,
        installs: Mutex<usize>,
    }

    impl FakeInstaller {
        fn new() -> Arc<Self> {
            Self::with_availability(true)
        }

        fn unavailable() -> Arc<Self> {
            Self::with_availability(false)
        }

        fn with_availability(available: bool) -> Arc<Self> {
            Arc::new(Self {
                available,
                installs: Mutex::new(0),
            })
        }

        fn installs(&self) -> usize {
            self.installs.lock().map(|count| *count).unwrap_or_default()
        }
    }

    impl QueueInstaller for FakeInstaller {
        fn is_available(&self) -> bool {
            self.available
        }

        fn queue_uri(&self, request: &ClientQueueRequest) -> Result<String, AppError> {
            // Deliberately not a plausible destination: the real URI comes from the single builder
            // the installers and the integration tests share, so this fake only shows that the use
            // case hands the installer's answer through unchanged.
            Ok(format!(
                "stub://{}/{}",
                request.server_address(),
                request.printer()
            ))
        }

        fn install(&self, _request: &ClientQueueRequest) -> Result<(), SetupFailure> {
            if let Ok(mut installs) = self.installs.lock() {
                *installs += 1;
            }
            Err(SetupFailure::new(
                SetupFailureKind::Other,
                "the app must install a queue through the elevated helper",
            ))
        }
    }

    struct FakeProxy {
        running: bool,
    }

    impl ClientProxyState for FakeProxy {
        fn is_running(&self) -> bool {
            self.running
        }
    }

    struct FakeChannel {
        configured: bool,
    }

    impl ChannelState for FakeChannel {
        fn is_configured(&self) -> bool {
            self.configured
        }
    }

    fn installation(
        broker: Arc<RecordingBroker>,
        servers: Arc<FakeServers>,
        installer: Arc<FakeInstaller>,
        proxy_running: bool,
    ) -> QueueInstallation {
        installation_with_channel(broker, servers, installer, proxy_running, true)
    }

    fn installation_with_channel(
        broker: Arc<RecordingBroker>,
        servers: Arc<FakeServers>,
        installer: Arc<FakeInstaller>,
        proxy_running: bool,
        channel_configured: bool,
    ) -> QueueInstallation {
        QueueInstallation::new(
            Arc::new(Setup::new(broker as Arc<dyn ElevationBroker>)),
            servers as Arc<dyn TrustedServerPrinters>,
            installer as Arc<dyn QueueInstaller>,
            Arc::new(FakeProxy {
                running: proxy_running,
            }),
            Arc::new(FakeChannel {
                configured: channel_configured,
            }),
        )
    }

    #[tokio::test]
    async fn a_shared_printer_installs_a_queue_that_routes_through_the_proxy() {
        let broker = Arc::new(RecordingBroker::default());
        let servers = FakeServers::sharing("10.0.0.5:8631", &["Office Printer", "Zebra"]);
        let installer = FakeInstaller::new();
        let installation = installation(
            Arc::clone(&broker),
            Arc::clone(&servers),
            Arc::clone(&installer),
            true,
        );

        let queue = installation
            .install("10.0.0.5:8631", "Office Printer")
            .await
            .expect("installs the queue");

        assert_eq!(
            queue.name().as_str(),
            "Office Printer (ShaPrint 10.0.0.5-8631)"
        );
        // The destination is whatever the installer port reports; the real builder is exercised in
        // `adapters::queue_installation` and in `tests/queue_installation.rs`.
        assert_eq!(queue.uri(), "stub://10.0.0.5:8631/Office Printer");
        assert_eq!(queue.request().server_address(), "10.0.0.5:8631");
        // The install ran elevated, over the queue-specific operation.
        assert_eq!(broker.installed().len(), 1);
        assert_eq!(
            broker.installed()[0].queue_name().as_str(),
            "Office Printer (ShaPrint 10.0.0.5-8631)"
        );
        // And it never ran in this process.
        assert_eq!(installer.installs(), 0);
    }

    #[tokio::test]
    async fn a_printer_the_server_does_not_share_is_refused_before_the_prompt() {
        let broker = Arc::new(RecordingBroker::default());
        let servers = FakeServers::sharing("10.0.0.5:8631", &["Office Printer"]);
        let installation = installation(
            Arc::clone(&broker),
            Arc::clone(&servers),
            FakeInstaller::new(),
            true,
        );

        let error = installation
            .install("10.0.0.5:8631", "Zebra")
            .await
            .expect_err("refused");

        assert_eq!(error.code(), ErrorCode::InvalidInput);
        assert!(error.message().contains("Zebra"));
        assert!(error.message().contains("10.0.0.5:8631"));
        assert!(error.message().contains("Show its shared printers again"));
        assert!(broker.installed().is_empty());
    }

    #[tokio::test]
    async fn an_untrusted_server_is_refused_before_the_prompt() {
        let broker = Arc::new(RecordingBroker::default());
        let installation = installation(
            Arc::clone(&broker),
            FakeServers::failing("This server is not approved."),
            FakeInstaller::new(),
            true,
        );

        let error = installation
            .install("10.0.0.9:8631", "Office Printer")
            .await
            .expect_err("refused");

        assert_eq!(error.code(), ErrorCode::InvalidState);
        assert!(error.message().contains("not approved"));
        assert!(broker.installed().is_empty());
    }

    #[tokio::test]
    async fn a_stopped_proxy_is_reported_before_any_queue_is_installed() {
        let broker = Arc::new(RecordingBroker::default());
        let servers = FakeServers::sharing("10.0.0.5:8631", &["Office Printer"]);
        let installation = installation(
            Arc::clone(&broker),
            Arc::clone(&servers),
            FakeInstaller::new(),
            false,
        );

        let error = installation
            .install("10.0.0.5:8631", "Office Printer")
            .await
            .expect_err("refused");

        assert_eq!(error.code(), ErrorCode::InvalidState);
        assert!(error.message().contains("local print proxy"));
        // Nothing is asked of the network or the helper while the destination cannot work.
        assert_eq!(servers.queries(), 0);
        assert!(broker.installed().is_empty());
    }

    #[tokio::test]
    async fn an_unusable_printer_name_is_refused_before_the_prompt() {
        let broker = Arc::new(RecordingBroker::default());
        let installation = installation(
            Arc::clone(&broker),
            FakeServers::sharing("10.0.0.5:8631", &["Office Printer"]),
            FakeInstaller::new(),
            true,
        );

        let error = installation
            .install("10.0.0.5:8631", "  ")
            .await
            .expect_err("refused");

        assert_eq!(error.code(), ErrorCode::InvalidInput);
        assert!(broker.installed().is_empty());
    }

    #[tokio::test]
    async fn a_platform_without_a_queue_installer_reports_unsupported() {
        let broker = Arc::new(RecordingBroker::default());
        let installation = installation(
            Arc::clone(&broker),
            FakeServers::sharing("10.0.0.5:8631", &["Office Printer"]),
            FakeInstaller::unavailable(),
            true,
        );

        let error = installation
            .install("10.0.0.5:8631", "Office Printer")
            .await
            .expect_err("unsupported");

        assert_eq!(error.code(), ErrorCode::Unsupported);
        assert!(broker.installed().is_empty());
    }

    #[tokio::test]
    async fn a_rejected_install_reports_the_advice_for_the_failure() {
        let broker = RecordingBroker::refusing(SetupFailureKind::SpoolerUnavailable);
        let installation = installation(
            Arc::clone(&broker),
            FakeServers::sharing("10.0.0.5:8631", &["Office Printer"]),
            FakeInstaller::new(),
            true,
        );

        let error = installation
            .install("10.0.0.5:8631", "Office Printer")
            .await
            .expect_err("refused");

        assert_eq!(error.code(), ErrorCode::InvalidState);
        assert!(error
            .message()
            .contains("Office Printer (ShaPrint 10.0.0.5-8631)"));
        assert!(error.message().contains("Print Spooler"));
        assert!(!error.message().contains("recorded failure"));
    }

    #[tokio::test]
    async fn installation_is_rejected_before_elevation_when_network_channel_is_not_configured() {
        let broker = Arc::new(RecordingBroker::default());
        let servers = FakeServers::sharing("10.0.0.5:8631", &["Office Printer"]);
        let installer = FakeInstaller::new();
        let installation = installation_with_channel(
            Arc::clone(&broker),
            Arc::clone(&servers),
            Arc::clone(&installer),
            true,
            false,
        );

        let error = installation
            .install("10.0.0.5:8631", "Office Printer")
            .await
            .expect_err("rejected");

        assert_eq!(error.code(), ErrorCode::InvalidState);
        assert!(error
            .message()
            .contains("Network Channel is not configured"));
        // Precondition failure must prevent UAC/elevation prompt and server queries
        assert_eq!(broker.installed().len(), 0);
        assert_eq!(installer.installs(), 0);
        assert_eq!(servers.queries(), 0);
    }
}
