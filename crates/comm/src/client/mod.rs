use crate::error::Result;
use crate::protocol::a2a::A2AMessage;
use crate::protocol::{Address, CausalityMetadata, CommEnvelope, Metadata};
use crate::scheduler::A2AScheduler;
use crate::transport::Transport;
use std::sync::Arc;

use benshu_infra::agent::{AgentEvent, AgentEventData};
use benshu_infra::observable::EventDispatcher;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommRuntimeProfile {
    Standalone,
    Embedded,
    Swarm,
}

impl CommRuntimeProfile {
    pub fn receive_loop_enabled(self) -> bool {
        !matches!(self, Self::Standalone)
    }

    pub fn heartbeat_enabled(self) -> bool {
        matches!(self, Self::Embedded | Self::Swarm)
    }

    pub fn signing_required(self) -> bool {
        matches!(self, Self::Swarm)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Standalone => "standalone",
            Self::Embedded => "embedded",
            Self::Swarm => "swarm",
        }
    }
}

/// High-level communication client for Agents
#[derive(Clone)]
pub struct CommClient {
    scheduler: Arc<A2AScheduler>,
    transport: Arc<dyn Transport>,
    self_addr: Address,
    event_bus: Option<Arc<EventDispatcher>>,
    secret_key: Option<Vec<u8>>,
    runtime_profile: CommRuntimeProfile,
}

impl CommClient {
    pub fn new(
        scheduler: Arc<A2AScheduler>,
        transport: Arc<dyn Transport>,
        self_addr: Address,
        event_bus: Option<Arc<EventDispatcher>>,
    ) -> Self {
        Self {
            scheduler,
            transport,
            self_addr,
            event_bus,
            secret_key: None,
            runtime_profile: CommRuntimeProfile::Embedded,
        }
    }

    pub fn with_security(mut self, secret_key: Vec<u8>) -> Self {
        self.secret_key = Some(secret_key);
        self
    }

    pub fn with_runtime_profile(mut self, runtime_profile: CommRuntimeProfile) -> Self {
        self.runtime_profile = runtime_profile;
        self
    }

    pub fn runtime_profile(&self) -> CommRuntimeProfile {
        self.runtime_profile
    }

    pub fn security_enabled(&self) -> bool {
        self.secret_key.is_some()
    }

    pub async fn send_msg(&self, target: Address, payload: Vec<u8>) -> Result<()> {
        self.send_with_tenant(target, payload, None).await
    }

    pub async fn send_a2a(&self, target: Address, message: &A2AMessage) -> Result<()> {
        self.send_a2a_with_context(target, message, None, None)
            .await
    }

    pub async fn send_a2a_with_context(
        &self,
        target: Address,
        message: &A2AMessage,
        tenant_id: Option<String>,
        causality: Option<CausalityMetadata>,
    ) -> Result<()> {
        let payload = serde_json::to_vec(message).map_err(|err| {
            crate::error::CommError::Protocol(crate::error::ProtocolError::Validation(format!(
                "failed to serialize A2A message: {}",
                err
            )))
        })?;
        self.send_with_metadata(target, payload, tenant_id, causality)
            .await
    }

    /// Send a message with explicit tenant context
    pub async fn send_with_tenant(
        &self,
        target: Address,
        payload: Vec<u8>,
        tenant_id: Option<String>,
    ) -> Result<()> {
        let mut meta = Metadata::new(self.self_addr.clone());
        meta.tenant_id = tenant_id;
        self.send_envelope(target, payload, meta).await
    }

    pub async fn send_with_metadata(
        &self,
        target: Address,
        payload: Vec<u8>,
        tenant_id: Option<String>,
        causality: Option<CausalityMetadata>,
    ) -> Result<()> {
        let mut meta = Metadata::new(self.self_addr.clone());
        meta.tenant_id = tenant_id;
        meta.causality = causality;
        self.send_envelope(target, payload, meta).await
    }

    async fn send_envelope(
        &self,
        target: Address,
        payload: Vec<u8>,
        mut meta: Metadata,
    ) -> Result<()> {
        // 0. Sign the message if key available
        if let Some(key) = &self.secret_key {
            let _ = meta.sign(key, self.self_addr.clone());
        }

        let envelope = CommEnvelope::new(target.clone(), payload.clone(), meta);

        // 1. Through scheduler for validation/rate-limiting/state sync
        self.scheduler
            .handle_message(&envelope)
            .await
            .map_err(|e| crate::error::CommError::Scheduler(e))?;

        // 2. Dispatch to transport
        let res = self.transport.send(envelope).await;

        // 3. Emit audit event
        if let Some(bus) = &self.event_bus {
            bus.dispatch(&AgentEvent {
                session_id: None,
                data: AgentEventData::CommSent {
                    target: target.to_string(),
                    size: payload.len(),
                    success: res.is_ok(),
                },
            })
            .await;
        }

        res.map_err(|e| crate::error::CommError::Transport(e))
    }

    /// Set a throttle limit for a tenant
    pub async fn set_tenant_limit(&self, tenant_id: &str, limit: u32) {
        self.scheduler.set_tenant_throttle(tenant_id, limit).await;
    }

    /// Set a throttle limit for an agent
    pub async fn set_agent_limit(&self, agent_id: &str, limit: u32) {
        self.scheduler.set_throttle(agent_id, limit).await;
    }

    /// Take the next envelope from transport
    pub async fn receive_next(&self) -> Result<Option<CommEnvelope>> {
        match self.transport.receive().await {
            Ok(envelope) => {
                // 0. Verify security signature if key available
                if let Some(key) = &self.secret_key {
                    if !envelope.meta.verify(key) {
                        return Err(crate::error::CommError::Protocol(
                            crate::error::ProtocolError::Validation(
                                "A2A signature verification failed".into(),
                            ),
                        ));
                    }
                }

                // 1. Let scheduler see incoming message for discovery/state sync
                let _ = self.scheduler.handle_message(&envelope).await;

                // 2. Emit audit event
                if let Some(bus) = &self.event_bus {
                    bus.dispatch(&AgentEvent {
                        session_id: None,
                        data: AgentEventData::CommReceived {
                            source: envelope.meta.source.to_string(),
                            size: envelope.payload.len(),
                        },
                    })
                    .await;
                }

                Ok(Some(envelope))
            }
            Err(e) => Err(crate::error::CommError::Transport(e)),
        }
    }

    /// Get client address
    pub fn address(&self) -> &Address {
        &self.self_addr
    }

    /// Get current network/transport status
    pub async fn status(&self) -> crate::transport::TransportStatus {
        self.transport.status().await
    }

    /// Access the discovery manager
    pub fn discovery(&self) -> Arc<dyn crate::scheduler::discovery::Discovery> {
        self.scheduler.discovery.clone()
    }
}
