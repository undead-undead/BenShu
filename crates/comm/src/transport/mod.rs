use crate::error::TransportError;
use crate::protocol::CommEnvelope;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Transport Introspection Information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportStatus {
    pub name: String,
    pub is_connected: bool,
    pub metrics: std::collections::HashMap<String, String>,
}

/// Base trait for all communication transports
#[async_trait]
pub trait Transport: Send + Sync {
    /// Send an envelope over the transport
    async fn send(&self, envelope: CommEnvelope) -> Result<(), TransportError>;

    /// Receive an envelope from the transport
    async fn receive(&self) -> Result<CommEnvelope, TransportError>;

    /// Get transport status
    async fn status(&self) -> TransportStatus;

    /// Close the transport
    async fn close(&self) -> Result<(), TransportError>;
}

pub mod bridge;
pub mod bus;
pub mod dispatcher;
pub mod memory;

pub use bridge::BridgeTransport;
pub use bus::BusTransport;
pub use dispatcher::GatewayDispatcher;
pub use memory::{MemoryHub, MemoryTransport};
