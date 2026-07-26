use crate::error::TransportError;
use crate::protocol::CommEnvelope;
use crate::transport::bridge::BridgeTransport;
use crate::transport::bus::BusTransport;
use crate::transport::memory::MemoryHub;
use crate::transport::Transport;
use async_trait::async_trait;
use std::sync::Arc;

use dashmap::DashMap;

/// Dispatcher that automatically routes between Memory, Bridge, and Bus transports
pub struct GatewayDispatcher {
    local: Arc<dyn Transport>,
    global: Arc<dyn Transport>,
    hub: Arc<MemoryHub>,
    bridges: Arc<DashMap<String, Arc<BridgeTransport>>>,
}

impl GatewayDispatcher {
    pub fn new(local: Arc<dyn Transport>, global: Arc<dyn Transport>, hub: Arc<MemoryHub>) -> Self {
        Self {
            local,
            global,
            hub,
            bridges: Arc::new(DashMap::new()),
        }
    }

    /// Register a remote node bridge
    pub fn register_bridge(&self, node_id: String, bridge: Arc<BridgeTransport>) {
        self.bridges.insert(node_id, bridge);
    }
}

#[async_trait]
impl Transport for GatewayDispatcher {
    async fn send(&self, envelope: CommEnvelope) -> Result<(), TransportError> {
        // 1. Check if hierarchical addressing targets a remote node
        // agent://node2/researcher
        if let Some(parent) = envelope.target.parent() {
            let root = parent.root_id();
            if let Some(bridge) = self.bridges.get(root) {
                // Route to BRIDGE
                return bridge.send(envelope).await;
            }
        }

        // 2. Try local high-speed routing (MemoryHub)
        match self.hub.dispatch(envelope.clone()).await {
            Ok(_) => Ok(()),
            Err(TransportError::Routing(_)) => {
                // 3. Fallback to Global Bus
                self.global.send(envelope).await
            }
            Err(e) => Err(e),
        }
    }

    async fn status(&self) -> crate::transport::TransportStatus {
        let local_status = self.local.status().await;
        let global_status = self.global.status().await;

        let mut metrics = std::collections::HashMap::new();
        metrics.insert("local_type".to_string(), local_status.name);
        metrics.insert("global_type".to_string(), global_status.name);
        metrics.insert("bridge_count".to_string(), self.bridges.len().to_string());

        crate::transport::TransportStatus {
            name: "GatewayDispatcher".to_string(),
            is_connected: local_status.is_connected && global_status.is_connected,
            metrics,
        }
    }

    async fn receive(&self) -> Result<CommEnvelope, TransportError> {
        // Concurrent race between Local and Global channels
        tokio::select! {
            res = self.local.receive() => res,
            res = self.global.receive() => res,
        }
    }

    async fn close(&self) -> Result<(), TransportError> {
        let _ = self.local.close().await;
        let _ = self.global.close().await;
        Ok(())
    }
}
