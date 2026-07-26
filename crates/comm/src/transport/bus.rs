use crate::error::TransportError;
use crate::protocol::CommEnvelope;
use crate::transport::Transport;
use async_trait::async_trait;
use benshu_infra::bus::MessageBus;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};

/// Transport implementation using benshu-infra::bus::MessageBus
pub struct BusTransport {
    bus: MessageBus,
    address: String,
    receiver: Arc<Mutex<broadcast::Receiver<(String, Vec<u8>)>>>,
}

impl BusTransport {
    pub fn new(bus: MessageBus, address: String) -> Self {
        let receiver = bus.subscribe_comm();
        Self {
            bus,
            address,
            receiver: Arc::new(Mutex::new(receiver)),
        }
    }
}

#[async_trait]
impl Transport for BusTransport {
    async fn send(&self, envelope: CommEnvelope) -> Result<(), TransportError> {
        let target = envelope.target.to_string();
        let payload = serde_json::to_vec(&envelope)
            .map_err(|e| TransportError::Protocol(format!("Serialization failed: {}", e)))?;

        self.bus
            .publish_comm(target, payload)
            .await
            .map_err(|e| TransportError::Network(format!("Bus publish failed: {}", e)))?;

        Ok(())
    }

    async fn status(&self) -> crate::transport::TransportStatus {
        crate::transport::TransportStatus {
            name: "BusTransport".to_string(),
            is_connected: true, // MessageBus is always ready in memory
            metrics: [("address".to_string(), self.address.clone())]
                .into_iter()
                .collect(),
        }
    }

    async fn receive(&self) -> Result<CommEnvelope, TransportError> {
        let mut rx = self.receiver.lock().await;

        loop {
            match rx.recv().await {
                Ok((target, payload)) => {
                    // Check if it's for us or a broadcast
                    // "all" is a special system broadcast address usually
                    if target == self.address || target == "all" || target == "system://all" {
                        let envelope: CommEnvelope =
                            serde_json::from_slice(&payload).map_err(|e| {
                                TransportError::Protocol(format!("Deserialization failed: {}", e))
                            })?;
                        return Ok(envelope);
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    return Err(TransportError::Network(format!(
                        "Receiver lagged by {} messages",
                        n
                    )));
                }
                Err(broadcast::error::RecvError::Closed) => {
                    return Err(TransportError::Internal("Bus channel closed".into()));
                }
            }
        }
    }

    async fn close(&self) -> Result<(), TransportError> {
        // Broadcast receivers don't really "close" the bus
        Ok(())
    }
}
