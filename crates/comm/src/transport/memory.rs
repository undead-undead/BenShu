use crate::error::TransportError;
use crate::protocol::CommEnvelope;
use crate::transport::Transport;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

/// In-memory transport using direct MPSC channels
/// Suitable for ultra-high performance 1-to-1 communication in same process
pub struct MemoryTransport {
    address: String,
    sender: mpsc::Sender<CommEnvelope>,
    receiver: Arc<Mutex<mpsc::Receiver<CommEnvelope>>>,
}

impl MemoryTransport {
    pub fn new(address: String, buffer_size: usize) -> (Self, mpsc::Sender<CommEnvelope>) {
        let (tx, rx) = mpsc::channel(buffer_size);
        (
            Self {
                address,
                sender: tx.clone(),
                receiver: Arc::new(Mutex::new(rx)),
            },
            tx,
        )
    }

    /// Access the internal sender to allow others to send to this transport
    pub fn get_sender(&self) -> mpsc::Sender<CommEnvelope> {
        self.sender.clone()
    }
}

#[async_trait]
impl Transport for MemoryTransport {
    async fn send(&self, _envelope: CommEnvelope) -> Result<(), TransportError> {
        // Direct memory transport 'send' usually means routing to another MemoryTransport's sender.
        // This implementation is a passive 'endpoint'.
        // For active routing, use a Hub or GatewayDispatcher.
        Err(TransportError::Internal(
            "MemoryTransport send() must be routed via a Hub".into(),
        ))
    }

    async fn status(&self) -> crate::transport::TransportStatus {
        crate::transport::TransportStatus {
            name: "MemoryTransport".to_string(),
            is_connected: true,
            metrics: [("address".to_string(), self.address.clone())]
                .into_iter()
                .collect(),
        }
    }

    async fn receive(&self) -> Result<CommEnvelope, TransportError> {
        let mut rx = self.receiver.lock().await;
        rx.recv()
            .await
            .ok_or_else(|| TransportError::Internal("Memory channel closed".into()))
    }

    async fn close(&self) -> Result<(), TransportError> {
        Ok(())
    }
}

/// A central Hub for routing MemoryTransports
pub struct MemoryHub {
    endpoints: Arc<Mutex<HashMap<String, mpsc::Sender<CommEnvelope>>>>,
}

impl MemoryHub {
    pub fn new() -> Self {
        Self {
            endpoints: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn register(&self, address: String, sender: mpsc::Sender<CommEnvelope>) {
        self.endpoints.lock().await.insert(address, sender);
    }

    pub async fn dispatch(&self, envelope: CommEnvelope) -> Result<(), TransportError> {
        let target = envelope.target.to_string();
        let endpoints = self.endpoints.lock().await;

        if let Some(tx) = endpoints.get(&target) {
            tx.send(envelope)
                .await
                .map_err(|e| TransportError::Network(format!("MPSC send failed: {}", e)))?;
            Ok(())
        } else {
            // If target not found in memory hub, it might be on the Bus
            Err(TransportError::Routing(format!(
                "Target {} not found in MemoryHub",
                target
            )))
        }
    }
}
