//! Passive inspection of one installed Client queue. No print or mutation port is available.
use std::{sync::Arc, time::Duration};

use crate::domain::{ErrorCode, RecognisedClientQueue, ServiceId, ServiceState};

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
        }
    }
}

pub struct LocalTroubleshootingSnapshot {
    pub target: RecognisedClientQueue,
    pub checks: Vec<LocalFinding>,
}

pub struct LocalTroubleshooting {
    catalog: Arc<dyn LocalPrinterCatalog>,
    runtime: Arc<RuntimeCoordinator>,
}

impl LocalTroubleshooting {
    pub fn new(catalog: Arc<dyn LocalPrinterCatalog>, runtime: Arc<RuntimeCoordinator>) -> Self {
        Self { catalog, runtime }
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
        LocalTroubleshootingSnapshot { target, checks }
    }
}
