//! The server sharing runtime.
//!
//! Sharing exposes the local Windows printer queues the user selected over IPPS, authorizes
//! incoming jobs with the Network Channel, and submits accepted jobs to the selected queues
//! (issues #31 and #32; ADR 0001).
//!
//! This type is the supervised runtime slot for that work: it reports readiness once its startup
//! prerequisites hold and returns as soon as the shell cancels it. Sharing never starts on its
//! own — the user controls it through the shell's lifecycle commands (ADR 0001); #31 backs it with
//! the selected queues and their IPPS advertisement.

use async_trait::async_trait;

use crate::application::{RuntimeService, ServiceContext};
use crate::domain::{AppError, ServiceId};

/// Supervises IPPS sharing of the local printer queues.
#[derive(Debug, Default)]
pub struct ServerSharingService;

impl ServerSharingService {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl RuntimeService for ServerSharingService {
    fn id(&self) -> ServiceId {
        ServiceId::ServerSharing
    }

    /// Sharing is opt-in: the user starts and stops it explicitly (ADR 0001).
    fn autostart(&self) -> bool {
        false
    }

    async fn run(&self, context: ServiceContext) -> Result<(), AppError> {
        context.reporter().ready()?;
        context.cancelled().await;
        Ok(())
    }
}
