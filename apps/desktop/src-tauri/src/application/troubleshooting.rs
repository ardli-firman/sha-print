//! Passive inspection of one installed Client queue. No print or mutation port is available.
use std::{sync::Arc, time::Duration};

use crate::domain::{AppError, ErrorCode, RecognisedClientQueue, ServiceId, ServiceState};

use super::{LocalPrinterCatalog, RuntimeCoordinator};

pub const LOCAL_DIAGNOSTIC_TIMEOUT: Duration = Duration::from_secs(15);
// The overall UI deadline includes local IPC delivery. Finalize native evidence first so
// a slow pending-job query cannot race the UI's independent transport watchdog.
const LOCAL_INSPECTION_BUDGET: Duration =
    LOCAL_DIAGNOSTIC_TIMEOUT.saturating_sub(Duration::from_secs(1));

/// Native spooler evidence, excluding job names, documents, and raw driver messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalQueueInspection {
    Matching { pending_jobs: Option<u32> },
    Missing,
    Mismatched,
}

impl LocalQueueInspection {
    pub fn from_record(
        record: crate::domain::SpoolerRecord,
        target: &RecognisedClientQueue,
        proxy_authority: &str,
        pending_jobs: Option<u32>,
    ) -> Self {
        match record.classify(proxy_authority) {
            crate::domain::SpoolerRecordClassification::RecognisedClientQueue(queue)
                if &queue == target =>
            {
                Self::Matching { pending_jobs }
            }
            _ => Self::Mismatched,
        }
    }
}

/// A classified observation with structurally derived public text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalFinding {
    SpoolerReady,
    SpoolerUnavailable,
    SpoolerUnknown,
    InspectionTimeout,
    QueueMatching,
    QueueMissing,
    QueueMismatched,
    QueueUnknown,
    QueueEmpty,
    PendingJobs,
    PendingUnknown,
    PendingNotApplicable,
    ProxyRunning,
    ProxyStopped,
    ProxyUnknown,
    // Server reachability
    ServerReachable,
    ServerUnreachable,
    ServerReachabilityTimeout,
    ServerReachabilityUnknown,
    // Server identity
    ServerIdentityMatching,
    ServerNotTrusted,
    ServerIdentityChanged,
    ServerIdentityUnknown,
    ServerIdentityNotApplicable,
    // Shared printer
    PrinterShared,
    PrinterNotShared,
    PrinterAvailabilityUnknown,
    PrinterAvailabilityNotApplicable,
    // Network Channel authorization
    ChannelAuthorizationRejected,
    ChannelConfiguredUnobserved,
    ChannelUnconfigured,
    ChannelNotApplicable,
    // Optional inspection limits
    FirewallInspectionUnavailable,
}

impl LocalFinding {
    /// Check id, outcome, safe explanation, and optional stable code.
    pub fn description(self) -> (&'static str, &'static str, &'static str, Option<ErrorCode>) {
        use LocalFinding::*;
        match self {
            SpoolerReady => ("spooler", "passed", "The native print spooler responded.", None),
            SpoolerUnavailable => ("spooler", "failed", "The native print spooler is unavailable. Check the Windows Print Spooler service or the Linux CUPS service.", Some(ErrorCode::QueueUnavailable)),
            SpoolerUnknown => ("spooler", "inconclusive", "The native print spooler could not be inspected.", None),
            InspectionTimeout => ("spooler", "inconclusive", "Native queue inspection did not finish within 15 seconds.", Some(ErrorCode::Timeout)),
            QueueMatching => ("client-queue", "passed", "The installed Client queue matches the selected Server and shared printer.", None),
            QueueMissing => ("client-queue", "failed", "The selected Client queue is no longer installed.", Some(ErrorCode::QueueUnavailable)),
            QueueMismatched => ("client-queue", "failed", "The queue name exists but its destination does not match the selected Server and shared printer.", Some(ErrorCode::QueueUnavailable)),
            QueueUnknown => ("client-queue", "inconclusive", "The selected Client queue could not be inspected.", None),
            QueueEmpty => ("pending-jobs", "passed", "No pending print jobs were observed in this Client queue.", None),
            PendingJobs => ("pending-jobs", "failed", "Print jobs are pending. Wait for them to finish or inspect the native queue before testing or repairing.", Some(ErrorCode::InvalidState)),
            PendingUnknown => ("pending-jobs", "inconclusive", "Pending print jobs could not be read. Testing and repair remain blocked until the queue is known to be empty.", None),
            PendingNotApplicable => ("pending-jobs", "not-applicable", "Pending jobs cannot be checked for an absent or mismatched Client queue.", None),
            ProxyRunning => ("client-proxy", "passed", "The client proxy reports running.", None),
            ProxyStopped => ("client-proxy", "failed", "The client proxy is not running. Check Print service status to start it.", Some(ErrorCode::InvalidState)),
            ProxyUnknown => ("client-proxy", "inconclusive", "The client proxy state could not be established.", None),
            ServerReachable => ("server-reachability", "passed", "The ShaPrint server responded over TLS.", None),
            ServerUnreachable => ("server-reachability", "failed", "Could not reach the ShaPrint server. Check network connection and confirm the server is running.", Some(ErrorCode::ServerUnavailable)),
            ServerReachabilityTimeout => ("server-reachability", "inconclusive", "Connecting to the server timed out. Check network connectivity.", Some(ErrorCode::Timeout)),
            ServerReachabilityUnknown => ("server-reachability", "inconclusive", "Server reachability could not be established.", None),
            ServerIdentityMatching => ("server-identity", "passed", "The server certificate fingerprint matches the saved approval.", None),
            ServerNotTrusted => ("server-identity", "failed", "This server has not been approved yet. Inspect its certificate fingerprint in Server connections before using it.", Some(ErrorCode::ServerNotTrusted)),
            ServerIdentityChanged => ("server-identity", "failed", "The server certificate has changed since it was approved. Review the fingerprint in Server connections to reapprove or reject.", Some(ErrorCode::ServerIdentityChanged)),
            ServerIdentityUnknown => ("server-identity", "inconclusive", "Server certificate identity could not be verified.", None),
            ServerIdentityNotApplicable => ("server-identity", "not-applicable", "Server identity cannot be checked while the server is unreachable.", None),
            PrinterShared => ("shared-printer", "passed", "The server shares the selected printer queue.", None),
            PrinterNotShared => ("shared-printer", "failed", "The server is no longer sharing this printer queue. Confirm the printer is shared in ShaPrint on the server.", Some(ErrorCode::PrinterNotShared)),
            PrinterAvailabilityUnknown => ("shared-printer", "inconclusive", "Shared printer availability could not be established.", None),
            PrinterAvailabilityNotApplicable => ("shared-printer", "not-applicable", "Shared printer availability cannot be checked while the server is unreachable or untrusted.", None),
            ChannelAuthorizationRejected => ("network-channel", "failed", "The server rejected the print request because the Network Channel was missing or incorrect.", Some(ErrorCode::NotAuthorized)),
            ChannelConfiguredUnobserved => ("network-channel", "inconclusive", "A Network Channel is configured locally, but authorization has not been confirmed in an accepted server response.", None),
            ChannelUnconfigured => ("network-channel", "inconclusive", "No Network Channel is configured locally. If this network uses one, set it in Network Channel settings.", None),
            ChannelNotApplicable => ("network-channel", "not-applicable", "Network Channel authorization cannot be checked while the server is unreachable or untrusted.", None),
            FirewallInspectionUnavailable => ("firewall", "not-applicable", "Optional firewall inspection is not available without administrator privileges. An unreachable server is not automatically a firewall diagnosis.", None),
        }
    }
}

#[async_trait::async_trait]
pub trait ServerInspector: Send + Sync + 'static {
    async fn inspect_server(
        &self,
        server_address: &str,
        printer_name: &str,
    ) -> Result<ServerInspection, AppError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerInspection {
    Reachable {
        identity: ServerIdentityInspection,
        printer_shared: Option<bool>,
    },
    Unreachable,
    Timeout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerIdentityInspection {
    Matching,
    NotTrusted,
    Changed,
}

pub trait ObservedAuthorization: Send + Sync + 'static {
    fn is_rejected(&self, printer_name: &str) -> bool;
}

pub struct LocalTroubleshootingSnapshot {
    pub target: RecognisedClientQueue,
    pub checks: Vec<LocalFinding>,
}

impl LocalTroubleshootingSnapshot {
    /// Whether the selected Client queue is eligible for repair based on classified findings.
    pub fn is_repair_eligible(&self) -> bool {
        self.checks.contains(&LocalFinding::SpoolerReady)
            && self.checks.contains(&LocalFinding::QueueMatching)
            && self.checks.contains(&LocalFinding::QueueEmpty)
            && self.checks.contains(&LocalFinding::ProxyRunning)
            && self.checks.contains(&LocalFinding::ServerReachable)
            && self.checks.contains(&LocalFinding::ServerIdentityMatching)
            && self.checks.contains(&LocalFinding::PrinterShared)
            && !self
                .checks
                .contains(&LocalFinding::ChannelAuthorizationRejected)
            && !self.checks.contains(&LocalFinding::ChannelUnconfigured)
            && !self.checks.contains(&LocalFinding::ChannelNotApplicable)
    }
}

pub struct LocalTroubleshooting {
    catalog: Arc<dyn LocalPrinterCatalog>,
    runtime: Arc<RuntimeCoordinator>,
    servers: Option<Arc<dyn ServerInspector>>,
    channel: Option<Arc<dyn super::ChannelState>>,
    authorization: Option<Arc<dyn ObservedAuthorization>>,
}

impl LocalTroubleshooting {
    pub fn new(catalog: Arc<dyn LocalPrinterCatalog>, runtime: Arc<RuntimeCoordinator>) -> Self {
        Self {
            catalog,
            runtime,
            servers: None,
            channel: None,
            authorization: None,
        }
    }

    pub fn with_server_inspection(
        mut self,
        servers: Arc<dyn ServerInspector>,
        channel: Arc<dyn super::ChannelState>,
        authorization: Arc<dyn ObservedAuthorization>,
    ) -> Self {
        self.servers = Some(servers);
        self.channel = Some(channel);
        self.authorization = Some(authorization);
        self
    }

    pub async fn inspect(&self, target: RecognisedClientQueue) -> LocalTroubleshootingSnapshot {
        let deadline = tokio::time::Instant::now() + LOCAL_INSPECTION_BUDGET;
        let proxy = match self.runtime.status().service(ServiceId::ClientProxy) {
            Some(service) if service.state() == ServiceState::Running => LocalFinding::ProxyRunning,
            Some(_) => LocalFinding::ProxyStopped,
            None => LocalFinding::ProxyUnknown,
        };
        let result =
            tokio::time::timeout_at(deadline, self.catalog.inspect_client_queue(&target)).await;
        use LocalFinding::*;
        let mut checks = match result {
            Ok(Ok(inspection)) => {
                let (queue, pending) = match inspection {
                    LocalQueueInspection::Matching {
                        pending_jobs: Some(0),
                    } => (QueueMatching, QueueEmpty),
                    LocalQueueInspection::Matching {
                        pending_jobs: Some(_),
                    } => (QueueMatching, PendingJobs),
                    LocalQueueInspection::Matching { pending_jobs: None } => {
                        let pending = match tokio::time::timeout_at(
                            deadline,
                            self.catalog.pending_client_jobs(&target),
                        )
                        .await
                        {
                            Ok(Ok(0)) => QueueEmpty,
                            Ok(Ok(_)) => PendingJobs,
                            _ => PendingUnknown,
                        };
                        (QueueMatching, pending)
                    }
                    LocalQueueInspection::Missing => (QueueMissing, PendingNotApplicable),
                    LocalQueueInspection::Mismatched => (QueueMismatched, PendingNotApplicable),
                };
                vec![SpoolerReady, queue, pending]
            }
            Ok(Err(error)) => vec![
                if error.code() == ErrorCode::QueueUnavailable {
                    SpoolerUnavailable
                } else {
                    SpoolerUnknown
                },
                QueueUnknown,
                PendingUnknown,
            ],
            Err(_) => vec![InspectionTimeout, QueueUnknown, PendingUnknown],
        };
        checks.push(proxy);

        if let Some(servers) = &self.servers {
            let server_res = tokio::time::timeout_at(
                deadline,
                servers.inspect_server(target.server_address(), target.printer_name().as_str()),
            )
            .await;

            let auth_rejected = self
                .authorization
                .as_ref()
                .map(|a| a.is_rejected(target.printer_name().as_str()))
                .unwrap_or(false);
            let channel_configured = self
                .channel
                .as_ref()
                .map(|c| c.is_configured())
                .unwrap_or(false);

            match server_res {
                Ok(Ok(ServerInspection::Reachable {
                    identity,
                    printer_shared,
                })) => {
                    checks.push(LocalFinding::ServerReachable);
                    match identity {
                        ServerIdentityInspection::Matching => {
                            checks.push(LocalFinding::ServerIdentityMatching);
                            match printer_shared {
                                Some(true) => checks.push(LocalFinding::PrinterShared),
                                Some(false) => checks.push(LocalFinding::PrinterNotShared),
                                None => checks.push(LocalFinding::PrinterAvailabilityUnknown),
                            }
                            if auth_rejected {
                                checks.push(LocalFinding::ChannelAuthorizationRejected);
                            } else if channel_configured {
                                checks.push(LocalFinding::ChannelConfiguredUnobserved);
                            } else {
                                checks.push(LocalFinding::ChannelUnconfigured);
                            }
                        }
                        ServerIdentityInspection::NotTrusted => {
                            checks.push(LocalFinding::ServerNotTrusted);
                            checks.push(LocalFinding::PrinterAvailabilityNotApplicable);
                            checks.push(LocalFinding::ChannelNotApplicable);
                        }
                        ServerIdentityInspection::Changed => {
                            checks.push(LocalFinding::ServerIdentityChanged);
                            checks.push(LocalFinding::PrinterAvailabilityNotApplicable);
                            checks.push(LocalFinding::ChannelNotApplicable);
                        }
                    }
                }
                Ok(Ok(ServerInspection::Unreachable)) => {
                    checks.push(LocalFinding::ServerUnreachable);
                    checks.push(LocalFinding::ServerIdentityNotApplicable);
                    checks.push(LocalFinding::PrinterAvailabilityNotApplicable);
                    checks.push(LocalFinding::ChannelNotApplicable);
                }
                Ok(Ok(ServerInspection::Timeout)) | Err(_) => {
                    checks.push(LocalFinding::ServerReachabilityTimeout);
                    checks.push(LocalFinding::ServerIdentityUnknown);
                    checks.push(LocalFinding::PrinterAvailabilityUnknown);
                    checks.push(LocalFinding::ChannelNotApplicable);
                }
                Ok(Err(_)) => {
                    checks.push(LocalFinding::ServerReachabilityUnknown);
                    checks.push(LocalFinding::ServerIdentityUnknown);
                    checks.push(LocalFinding::PrinterAvailabilityUnknown);
                    checks.push(LocalFinding::ChannelNotApplicable);
                }
            }
            checks.push(LocalFinding::FirewallInspectionUnavailable);
        }

        LocalTroubleshootingSnapshot { target, checks }
    }
}
