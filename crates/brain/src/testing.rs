use crate::agent::provider::{ChatRequest, Provider, ProviderMetadata};
use crate::agent::streaming::StreamingResponse;
use crate::security::{AuditLogRecord, LeakDetection, SanitizedOutput, SecurityHandler};
use async_trait::async_trait;
use futures::executor::block_on;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Mock security handler that permits everything and logs nothing
pub struct MockSecurityHandler;

#[async_trait]
impl SecurityHandler for MockSecurityHandler {
    fn check_input(&self, text: &str) -> SanitizedOutput {
        SanitizedOutput {
            content: text.to_string(),
            warnings: vec![],
            was_modified: false,
        }
    }
    fn check_output(&self, text: &str) -> (String, Vec<LeakDetection>) {
        (text.to_string(), vec![])
    }
    fn log_action(
        &self,
        _s: Option<&str>,
        _t: &str,
        _a: &str,
        _succ: bool,
        _o: &str,
        _b: Option<benshu_infra::skill::BackupInfo>,
    ) {
    }
    async fn retrieve_audit_logs(&self, _l: usize) -> anyhow::Result<Vec<AuditLogRecord>> {
        Ok(vec![])
    }
}

/// Provider that returns pre-configured responses in sequence
#[derive(Clone)]
pub struct SequenceMockProvider {
    responses: Arc<Mutex<Vec<StreamingResponse>>>,
}

impl SequenceMockProvider {
    pub fn new(responses: Vec<StreamingResponse>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(responses)),
        }
    }
}

#[async_trait]
impl Provider for SequenceMockProvider {
    async fn stream_completion(
        &self,
        _request: ChatRequest,
    ) -> benshu_infra::error::Result<StreamingResponse> {
        let mut resps = self.responses.lock().await;
        if resps.is_empty() {
            return Err(benshu_infra::error::Error::Internal(
                "MockProvider: No more responses configured".to_string(),
            ));
        }
        Ok(resps.remove(0))
    }

    fn name(&self) -> &'static str {
        "sequence-mock"
    }

    fn metadata() -> ProviderMetadata
    where
        Self: Sized,
    {
        ProviderMetadata {
            id: "mock".to_string(),
            name: "Mock Provider".to_string(),
            description: "Sequence-based mock provider for testing".to_string(),
            icon: "".to_string(),
            fields: vec![],
            capabilities: vec![],
            preferred_models: vec![],
        }
    }
}

/// Helper for setting up communication test environments for agents
pub struct CommTestEnv {
    pub hub: Arc<benshu_comm::transport::MemoryHub>,
}

impl CommTestEnv {
    pub fn new() -> Self {
        Self {
            hub: Arc::new(benshu_comm::transport::MemoryHub::new()),
        }
    }

    /// Create a CommClient for an agent role
    pub fn create_client(&self, addr: &str) -> benshu_comm::client::CommClient {
        let addr_str = addr.to_string();
        let self_addr = benshu_comm::protocol::Address::Agent(addr_str.clone());

        let (mem_transport, mem_tx) =
            benshu_comm::transport::MemoryTransport::new(addr_str.clone(), 1024);

        // Register to hub synchronously for test determinism.
        let addr_fullname = self_addr.to_string();
        block_on(self.hub.register(addr_fullname, mem_tx));

        // Use a dummy bus/dispatcher for now as we only need MemoryHub for most A2A tests
        // In real system this matches GatewayDispatcher
        let bus = Arc::new(benshu_infra::bus::MessageBus::new(10));
        let bus_transport = Arc::new(benshu_comm::transport::BusTransport::new(
            (*bus).clone(),
            addr_str,
        ));

        let dispatcher = Arc::new(benshu_comm::transport::GatewayDispatcher::new(
            Arc::new(mem_transport),
            bus_transport,
            self.hub.clone(),
        ));

        let scheduler = Arc::new(benshu_comm::scheduler::A2AScheduler::new());
        benshu_comm::client::CommClient::new(
            scheduler, dispatcher, self_addr, None, // No event bus for simple tests
        )
        .with_runtime_profile(benshu_comm::client::CommRuntimeProfile::Embedded)
    }
}
