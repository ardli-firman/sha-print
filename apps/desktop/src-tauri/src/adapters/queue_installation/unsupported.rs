//! Queue installation for platforms that have no spooler to install into.

use crate::application::QueueInstaller;
use crate::domain::{AppError, ClientQueueRequest, SetupFailure, SetupFailureKind};

/// Reports that this platform cannot create a Windows printer queue.
///
/// The Windows MVP has no Linux or macOS spooler adapter yet (ADR 0001); reporting the gap keeps the
/// shell honest instead of failing later, when the queue would not appear in a print dialog.
#[derive(Debug, Default)]
pub struct UnsupportedQueueInstaller;

impl UnsupportedQueueInstaller {
    pub fn new() -> Self {
        Self
    }
}

impl QueueInstaller for UnsupportedQueueInstaller {
    fn is_available(&self) -> bool {
        false
    }

    fn queue_uri(&self, request: &ClientQueueRequest) -> Result<String, AppError> {
        // The destination is platform-independent, so the URI stays checkable here too.
        super::queue_uri(&super::proxy_authority(), request)
    }

    fn install(&self, _request: &ClientQueueRequest) -> Result<(), SetupFailure> {
        Err(SetupFailure::new(
            SetupFailureKind::Unsupported,
            "installing a Windows printer queue is only supported on Windows",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::PrinterName;

    fn request() -> ClientQueueRequest {
        ClientQueueRequest::new(
            "10.0.0.5:8631",
            PrinterName::parse("Office Printer").expect("valid printer name"),
        )
        .expect("valid request")
    }

    #[test]
    fn this_platform_reports_that_it_cannot_install_a_queue() {
        let installer = UnsupportedQueueInstaller::new();
        assert!(!installer.is_available());

        let failure = installer.install(&request()).expect_err("unsupported");
        assert_eq!(failure.kind(), SetupFailureKind::Unsupported);
    }

    #[test]
    fn the_destination_is_still_reported() {
        assert_eq!(
            UnsupportedQueueInstaller::new()
                .queue_uri(&request())
                .expect("builds the queue URI"),
            super::super::queue_uri(&super::super::proxy_authority(), &request())
                .expect("builds the queue URI")
        );
    }
}
