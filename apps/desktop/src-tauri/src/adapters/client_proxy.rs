//! The client proxy runtime.
//!
//! The proxy owns the loopback endpoint that installed Windows printer queues print to, attaches
//! the Network Channel credentials, and forwards jobs to a trusted server over IPPS so the inbox
//! Windows IPP driver never has to answer an authentication challenge (issue #34; ADR 0001).
//!
//! This type is the supervised runtime slot for that work: it reports readiness once its startup
//! prerequisites hold and returns as soon as the shell cancels it. Job handling is added by #34.

use async_trait::async_trait;

use crate::application::{RuntimeService, ServiceContext};
use crate::domain::{AppError, ServiceId};

/// Supervises the local client proxy.
#[derive(Debug, Default)]
pub struct ClientProxyService;

impl ClientProxyService {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl RuntimeService for ClientProxyService {
    fn id(&self) -> ServiceId {
        ServiceId::ClientProxy
    }

    /// The proxy runs whenever the app runs: installed queues must always reach it (ADR 0001).
    fn autostart(&self) -> bool {
        true
    }

    async fn run(&self, context: ServiceContext) -> Result<(), AppError> {
        context.reporter().ready()?;
        context.cancelled().await;
        Ok(())
    }
}
