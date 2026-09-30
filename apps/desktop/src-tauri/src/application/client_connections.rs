//! Client use cases for reviewing an IPPS server identity and listing its shared printers.

use crate::{
    adapters::client_connections::{ClientConnections, ConnectionPrinters, ConnectionReview},
    domain::AppError,
};

pub struct ServerConnections {
    adapter: ClientConnections,
}
impl ServerConnections {
    pub fn new(data_dir: impl AsRef<std::path::Path>) -> Result<Self, AppError> {
        Ok(Self {
            adapter: ClientConnections::new(data_dir)?,
        })
    }
    pub async fn inspect(&self, address: &str) -> Result<ConnectionReview, AppError> {
        self.adapter.inspect(address).await
    }
    pub async fn approve(
        &self,
        address: &str,
        fingerprint: &str,
    ) -> Result<ConnectionReview, AppError> {
        self.adapter.approve(address, fingerprint).await
    }
    pub async fn printers(&self, address: &str) -> Result<ConnectionPrinters, AppError> {
        self.adapter.printers(address).await
    }
}
