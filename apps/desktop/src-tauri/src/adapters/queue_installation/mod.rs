//! Installing the native Windows queue a shared printer prints through (issue #35).
//!
//! A client queue is a spooler queue whose port is an IPP URI aimed at the local client proxy. The
//! queue outlives the app, so once it exists Windows keeps offering the printer in ordinary print
//! dialogs; the proxy supplies the Network Channel while ShaPrint runs.
//!
//! Windows creates the queue through the PrintManagement module. Other platforms report that they
//! cannot (ADR 0001 keeps Linux a later phase), and the URI builder stays shared so the tests can
//! check that an installed queue points exactly where the proxy listens.

#[cfg(not(windows))]
mod unsupported;
#[cfg(any(windows, test))]
mod windows;

#[cfg(not(windows))]
pub use unsupported::UnsupportedQueueInstaller;
#[cfg(any(windows, test))]
pub use windows::WindowsQueueInstaller;

use async_trait::async_trait;

use std::sync::Arc;

use crate::adapters::client_connections::ClientConnections;
use crate::adapters::CLIENT_PROXY_DEFAULT_PORT;
use crate::application::{QueueInstaller, TrustedPrinters, TrustedServerPrinters};
use crate::domain::{AppError, ClientQueueRequest, PrinterName};

/// The queue installer for the platform this build targets.
///
/// The elevated helper uses it to perform the install; the app uses it to report where an installed
/// queue sends its jobs.
#[cfg(windows)]
pub fn platform_installer() -> Arc<dyn QueueInstaller> {
    Arc::new(WindowsQueueInstaller::new())
}

#[cfg(not(windows))]
pub fn platform_installer() -> Arc<dyn QueueInstaller> {
    Arc::new(UnsupportedQueueInstaller::new())
}

/// The authority installed queues send IPP requests to: the local client proxy on loopback.
pub fn proxy_authority() -> String {
    format!("127.0.0.1:{CLIENT_PROXY_DEFAULT_PORT}")
}

/// The IPP URI a queue installed for `request` routes through `proxy_authority`.
///
/// One builder serves the app, the elevated helper, and the tests, so an installed queue points
/// exactly where the proxy listens; the proxy resolves the URI back to the approved server and
/// printer (`adapters::client_proxy`).
pub fn queue_uri(proxy_authority: &str, request: &ClientQueueRequest) -> Result<String, AppError> {
    crate::adapters::client_queue_uri(
        proxy_authority,
        request.server_address(),
        request.printer().as_str(),
    )
}

/// Serves the install use case from the client trust store.
///
/// A queue is installed only for a server the user approved, so the trust checks that already guard
/// printer queries and print jobs guard queue installation too.
#[async_trait]
impl TrustedServerPrinters for ClientConnections {
    async fn shared_printers(&self, address: &str) -> Result<TrustedPrinters, AppError> {
        let listed = self.printers(address).await?;
        let printers = listed
            .printers
            .iter()
            .map(|name| PrinterName::parse(name))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(TrustedPrinters {
            address: listed.address,
            printers,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> ClientQueueRequest {
        ClientQueueRequest::new(
            "10.0.0.5:8631",
            PrinterName::parse("Office Printer").expect("valid printer name"),
        )
        .expect("valid request")
    }

    #[test]
    fn an_installed_queue_points_at_the_local_proxy() {
        assert_eq!(
            queue_uri(&proxy_authority(), &request()).expect("builds a queue URI"),
            format!(
                "ipp://127.0.0.1:{CLIENT_PROXY_DEFAULT_PORT}/ipp/print/10.0.0.5%3A8631/Office%20Printer"
            )
        );
    }

    #[test]
    fn the_installer_and_the_app_build_the_same_destination() {
        // The app shows the URI the elevated helper installs, so both must come from this builder.
        // The platform installer is used, not a named one, because only one exists per platform.
        assert_eq!(
            platform_installer()
                .queue_uri(&request())
                .expect("builds a queue URI"),
            queue_uri(&proxy_authority(), &request()).expect("builds a queue URI")
        );
    }

    #[test]
    fn an_address_the_proxy_cannot_route_is_rejected() {
        // The trust store normalizes addresses, so anything else never reaches an install; the URI
        // builder keeps that honest by validating again.
        let error = crate::adapters::client_queue_uri(&proxy_authority(), "not a host", "Office")
            .expect_err("rejected");
        assert_eq!(error.code(), crate::domain::ErrorCode::InvalidInput);
    }
}
