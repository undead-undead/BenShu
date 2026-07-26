use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use tempfile::tempdir;

use benshu_brain::agent::agent_identity::AgentIdentity;
use benshu_brain::agent::context::ContextInjector;
use benshu_brain::agent::core::Agent;
use benshu_brain::agent::evolution::auditor::Auditor;
use benshu_brain::agent::evolution::evolution_manager::EvolutionManager;
use benshu_brain::agent::memory::InMemoryMemory;
use benshu_brain::agent::memory::{
    Fact, FactProtection, FactReviewResolution, FactReviewResolutionOutcome, FactStatus,
    LearnedMemoryInjector, Memory, MemoryCapabilities, MemoryManager, MultimodalDerivedFact,
    MultimodalMemoryKind, MultimodalMemoryRecord, Relation, ShortTermMemory,
};
use benshu_brain::agent::message::Message;
use benshu_brain::agent::multi_agent::{Coordinator, MultiAgent};
use benshu_brain::agent::protocol::{
    ChatOutcome, RejectAllApprovalHandler, RiskyToolPolicy, TaskOwnership, ToolPolicy,
};
use benshu_brain::agent::provider::{
    ChatRequest, CircuitBreakerConfig, Provider, ProviderMetadata, ResilientProvider,
};
use benshu_brain::agent::session::{AgentSession, SessionStatus};
use benshu_brain::agent::streaming::{
    MockStreamBuilder, StreamingChoice, StreamingResponse, Usage,
};
use benshu_brain::skills::tool::{SafetyLevel, Tool, ToolDefinition};
use benshu_brain::testing::{CommTestEnv, MockSecurityHandler, SequenceMockProvider};
use benshu_comm::protocol::a2a::{A2AMessage, DelegationEnvelope, DelegationReturnMode};
use benshu_comm::protocol::Address;
use benshu_infra::agent::{AgentMessage, AgentRole, MessageType};
use benshu_infra::traits::resource::{HostResources, ResourceSensor, ThrottleLevel};
use benshu_telemetry::{
    AgentTracer, RealHarness, RealHarnessCase, RuntimeStage, RuntimeStageTrace, TelemetryLevel,
    TelemetryManager, TraceStatus,
};
use parking_lot::RwLock;
use std::collections::HashMap;

const RUNTIME_FIRST_BATCH_SCENARIOS: &[&str] = &[
    "single_agent_foreground_chat",
    "loop_guard_tool_execution",
    "tool_output_degradation",
    "provider_failover_foreground_chat",
    "foreground_preemptive_chat_merge",
];

const RUNTIME_CONTEXT_BATCH_SCENARIOS: &[&str] = &[
    "clean_tool_execution",
    "retrieval_signal_injection",
    "retrieval_low_signal_skip",
    "session_thread_refs_are_stable",
    "failing_context_injector_is_non_fatal",
];

const RUNTIME_GOVERNANCE_MEMORY_BATCH_SCENARIOS: &[&str] = &[
    "approval_guard_blocks_risky_tool",
    "prime_delegation_keeps_ownership",
    "comm_inbox_owner_rollup_persists",
    "memory_pending_review_persists",
    "memory_archive_and_prune_completes",
];

const RUNTIME_HARDENING_BATCH_SCENARIOS: &[&str] = &[
    "token_budget_exhaustion_blocks_run",
    "relation_depth_is_hard_capped",
    "multimodal_writeback_persists_contract",
    "cancel_marker_persists_to_stm",
    "pending_review_resolution_persists",
];

struct EchoTool;
struct LongOutputTool;
struct SyntheticKnowledgeSearchTool;
struct AlwaysFailProvider;
#[derive(Clone)]
struct RecordingProvider {
    requests: Arc<tokio::sync::Mutex<Vec<ChatRequest>>>,
    response_text: Arc<String>,
}

impl RecordingProvider {
    fn new(response_text: impl Into<String>) -> Self {
        Self {
            requests: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            response_text: Arc::new(response_text.into()),
        }
    }

    async fn recorded_requests(&self) -> Vec<ChatRequest> {
        self.requests.lock().await.clone()
    }
}

struct FailingContextInjector;

#[async_trait]
impl Provider for AlwaysFailProvider {
    async fn stream_completion(
        &self,
        _request: ChatRequest,
    ) -> benshu_infra::error::Result<StreamingResponse> {
        Err(benshu_infra::error::Error::Agent(
            "primary provider failed".to_string(),
        ))
    }

    fn name(&self) -> &str {
        "always-fail"
    }

    fn metadata() -> ProviderMetadata
    where
        Self: Sized,
    {
        ProviderMetadata {
            id: "always-fail".to_string(),
            name: "Always Fail".to_string(),
            description: "Test-only failing provider".to_string(),
            icon: "x".to_string(),
            fields: vec![],
            capabilities: vec!["fallback".to_string()],
            preferred_models: vec![],
        }
    }
}

#[async_trait]
impl Provider for RecordingProvider {
    async fn stream_completion(
        &self,
        request: ChatRequest,
    ) -> benshu_infra::error::Result<StreamingResponse> {
        self.requests.lock().await.push(request);
        Ok(MockStreamBuilder::new()
            .message(self.response_text.as_ref().clone())
            .done()
            .build())
    }

    fn name(&self) -> &str {
        "recording-provider"
    }

    fn metadata() -> ProviderMetadata
    where
        Self: Sized,
    {
        ProviderMetadata {
            id: "recording-provider".to_string(),
            name: "Recording Provider".to_string(),
            description: "Records foreground chat requests for runtime suite assertions."
                .to_string(),
            icon: "".to_string(),
            fields: vec![],
            capabilities: vec!["streaming".to_string()],
            preferred_models: vec![],
        }
    }
}

#[async_trait]
impl ContextInjector for FailingContextInjector {
    async fn inject(&self, _history: &[Message]) -> benshu_brain::error::Result<Vec<Message>> {
        Err(benshu_brain::error::Error::Internal(
            "intentional injector failure".to_string(),
        ))
    }
}

struct FailingMemory {
    inner: InMemoryMemory,
    fail_session_store: AtomicBool,
    fail_session_delete: AtomicBool,
    fail_fact_store: AtomicBool,
    fail_fact_delete: AtomicBool,
    fail_fact_importance_update: AtomicBool,
}

fn traced_operational_case(
    agent_name: &str,
    detail: impl Into<String>,
    status: TraceStatus,
) -> benshu_telemetry::RunTrace {
    let tracer = AgentTracer::new(uuid::Uuid::new_v4(), agent_name);
    let mut trace = tracer.start_run_trace();
    trace.status = status.clone();
    trace.finished_at = Some(chrono::Utc::now());
    trace.stages.push(RuntimeStageTrace {
        stage: RuntimeStage::Execution,
        status,
        started_at: chrono::Utc::now(),
        finished_at: Some(chrono::Utc::now()),
        detail: Some(detail.into()),
        metadata: HashMap::new(),
    });
    trace
}

impl FailingMemory {
    fn new() -> Self {
        Self {
            inner: InMemoryMemory::new(),
            fail_session_store: AtomicBool::new(false),
            fail_session_delete: AtomicBool::new(false),
            fail_fact_store: AtomicBool::new(false),
            fail_fact_delete: AtomicBool::new(false),
            fail_fact_importance_update: AtomicBool::new(false),
        }
    }

    fn fail_session_store(&self) {
        self.fail_session_store.store(true, Ordering::Relaxed);
    }

    fn fail_fact_store(&self) {
        self.fail_fact_store.store(true, Ordering::Relaxed);
    }

    fn fail_fact_importance_update(&self) {
        self.fail_fact_importance_update
            .store(true, Ordering::Relaxed);
    }
}

#[async_trait]
impl Memory for FailingMemory {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn capabilities(&self) -> MemoryCapabilities {
        self.inner.capabilities()
    }

    async fn store(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        message: Message,
    ) -> benshu_brain::error::Result<()> {
        self.inner.store(user_id, agent_id, message).await
    }

    async fn store_batch(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        messages: Vec<Message>,
    ) -> benshu_brain::error::Result<()> {
        self.inner.store_batch(user_id, agent_id, messages).await
    }

    async fn retrieve(&self, user_id: &str, agent_id: Option<&str>, limit: usize) -> Vec<Message> {
        self.inner.retrieve(user_id, agent_id, limit).await
    }

    async fn retrieve_full_history(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
    ) -> benshu_brain::error::Result<Vec<Message>> {
        self.inner.retrieve_full_history(user_id, agent_id).await
    }

    async fn clear(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
    ) -> benshu_brain::error::Result<()> {
        self.inner.clear(user_id, agent_id).await
    }

    async fn undo(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
    ) -> benshu_brain::error::Result<Option<Message>> {
        self.inner.undo(user_id, agent_id).await
    }

    async fn store_session(&self, session: AgentSession) -> benshu_brain::error::Result<()> {
        if self.fail_session_store.load(Ordering::Relaxed) {
            return Err(benshu_brain::error::Error::MemoryStorage(
                "synthetic session store failure".to_string(),
            ));
        }
        self.inner.store_session(session).await
    }

    async fn retrieve_session(
        &self,
        session_id: &str,
    ) -> benshu_brain::error::Result<Option<AgentSession>> {
        self.inner.retrieve_session(session_id).await
    }

    async fn delete_session(&self, session_id: &str) -> benshu_brain::error::Result<()> {
        if self.fail_session_delete.load(Ordering::Relaxed) {
            return Err(benshu_brain::error::Error::MemoryStorage(
                "synthetic session delete failure".to_string(),
            ));
        }
        self.inner.delete_session(session_id).await
    }

    async fn store_fact(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        fact: Fact,
    ) -> benshu_brain::error::Result<()> {
        if self.fail_fact_store.load(Ordering::Relaxed) {
            return Err(benshu_brain::error::Error::MemoryStorage(
                "synthetic fact store failure".to_string(),
            ));
        }
        self.inner.store_fact(user_id, agent_id, fact).await
    }

    async fn retrieve_facts(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
    ) -> benshu_brain::error::Result<Vec<Fact>> {
        self.inner.retrieve_facts(user_id, agent_id).await
    }

    async fn delete_fact(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        fact_id: &str,
    ) -> benshu_brain::error::Result<()> {
        if self.fail_fact_delete.load(Ordering::Relaxed) {
            return Err(benshu_brain::error::Error::MemoryStorage(
                "synthetic fact delete failure".to_string(),
            ));
        }
        self.inner.delete_fact(user_id, agent_id, fact_id).await
    }

    async fn find_related_facts(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        fact_id: &str,
        depth: usize,
    ) -> benshu_brain::error::Result<Vec<Fact>> {
        self.inner
            .find_related_facts(user_id, agent_id, fact_id, depth)
            .await
    }

    async fn maintenance(&self) -> benshu_brain::error::Result<()> {
        self.inner.maintenance().await
    }

    async fn update_utility(
        &self,
        collection: &str,
        fact_id: &str,
        increment: f32,
    ) -> benshu_brain::error::Result<()> {
        self.inner
            .update_utility(collection, fact_id, increment)
            .await
    }

    async fn age_vectors(
        &self,
        collection: &str,
        older_than_days: usize,
    ) -> benshu_brain::error::Result<()> {
        self.inner.age_vectors(collection, older_than_days).await
    }

    async fn update_fact_importance(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        fact_id: &str,
        importance: f32,
    ) -> benshu_brain::error::Result<()> {
        if self.fail_fact_importance_update.load(Ordering::Relaxed) {
            return Err(benshu_brain::error::Error::MemoryStorage(
                "synthetic fact importance failure".to_string(),
            ));
        }
        self.inner
            .update_fact_importance(user_id, agent_id, fact_id, importance)
            .await
    }

    fn set_emitter(&self, emitter: Arc<dyn benshu_infra::traits::memory::MemoryEmitter>) {
        self.inner.set_emitter(emitter);
    }
}

struct StubDelegatingAgent {
    role: AgentRole,
    response: String,
    handover: Option<AgentRole>,
}

#[async_trait]
impl MultiAgent for StubDelegatingAgent {
    fn role(&self) -> AgentRole {
        self.role.clone()
    }

    async fn handle_message(
        &self,
        message: AgentMessage,
    ) -> benshu_brain::error::Result<Option<AgentMessage>> {
        Ok(Some(AgentMessage {
            from: self.role.clone(),
            to: Some(message.from),
            content: self.response.clone(),
            msg_type: MessageType::Response,
        }))
    }

    async fn process(&self, _input: &str) -> benshu_brain::error::Result<String> {
        Ok(self.response.clone())
    }

    async fn chat(
        &self,
        _messages: Vec<Message>,
        _session_id: Option<String>,
    ) -> benshu_brain::error::Result<ChatOutcome> {
        Ok(ChatOutcome {
            response: self.response.clone(),
            thoughts: vec![],
            tool_calls: vec![],
            metabolic_stats: None,
            ownership: TaskOwnership::direct(self.role.clone(), None),
            delegation: None,
            handover: self.handover.clone(),
            runtime_task: None,
            run_trace: None,
        })
    }

    fn agent_identity(&self) -> Option<Arc<parking_lot::RwLock<Option<AgentIdentity>>>> {
        None
    }

    fn events(&self) -> tokio::sync::broadcast::Receiver<benshu_brain::agent::AgentEvent> {
        let (_, rx) = tokio::sync::broadcast::channel(1);
        rx
    }

    fn security(&self) -> Option<Arc<dyn benshu_brain::security::SecurityHandler>> {
        None
    }

    fn cancel(&self) {}

    fn ensure_active_token(&self) {}
}

#[derive(Clone)]
struct RecordingPreemptiveProvider {
    requests: Arc<tokio::sync::Mutex<Vec<ChatRequest>>>,
    call_count: Arc<AtomicUsize>,
}

impl RecordingPreemptiveProvider {
    fn new() -> Self {
        Self {
            requests: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            call_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    async fn recorded_requests(&self) -> Vec<ChatRequest> {
        self.requests.lock().await.clone()
    }
}

struct SequencedSensor {
    levels: std::collections::VecDeque<ThrottleLevel>,
}

impl SequencedSensor {
    fn new(levels: impl IntoIterator<Item = ThrottleLevel>) -> Self {
        Self {
            levels: levels.into_iter().collect(),
        }
    }
}

impl ResourceSensor for SequencedSensor {
    fn check_resources(&mut self, _detailed: bool) -> HostResources {
        HostResources::default()
    }

    fn suggest_throttle_level(&mut self, _config_threshold: Option<f32>) -> ThrottleLevel {
        self.levels.pop_front().unwrap_or(ThrottleLevel::High)
    }
}

#[derive(Default)]
struct MetadataMemory {
    inner: InMemoryMemory,
    metadata: RwLock<HashMap<String, String>>,
}

#[async_trait]
impl Memory for MetadataMemory {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn capabilities(&self) -> MemoryCapabilities {
        let mut capabilities = self.inner.capabilities();
        capabilities.metadata = true;
        capabilities
    }

    async fn store(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        message: Message,
    ) -> benshu_brain::error::Result<()> {
        self.inner.store(user_id, agent_id, message).await
    }

    async fn store_batch(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        messages: Vec<Message>,
    ) -> benshu_brain::error::Result<()> {
        self.inner.store_batch(user_id, agent_id, messages).await
    }

    async fn retrieve(&self, user_id: &str, agent_id: Option<&str>, limit: usize) -> Vec<Message> {
        self.inner.retrieve(user_id, agent_id, limit).await
    }

    async fn retrieve_full_history(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
    ) -> benshu_brain::error::Result<Vec<Message>> {
        self.inner.retrieve_full_history(user_id, agent_id).await
    }

    async fn clear(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
    ) -> benshu_brain::error::Result<()> {
        self.inner.clear(user_id, agent_id).await
    }

    async fn undo(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
    ) -> benshu_brain::error::Result<Option<Message>> {
        self.inner.undo(user_id, agent_id).await
    }

    async fn store_session(&self, session: AgentSession) -> benshu_brain::error::Result<()> {
        self.inner.store_session(session).await
    }

    async fn retrieve_session(
        &self,
        session_id: &str,
    ) -> benshu_brain::error::Result<Option<AgentSession>> {
        self.inner.retrieve_session(session_id).await
    }

    async fn delete_session(&self, session_id: &str) -> benshu_brain::error::Result<()> {
        self.inner.delete_session(session_id).await
    }

    async fn store_fact(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        fact: Fact,
    ) -> benshu_brain::error::Result<()> {
        self.inner.store_fact(user_id, agent_id, fact).await
    }

    async fn retrieve_facts(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
    ) -> benshu_brain::error::Result<Vec<Fact>> {
        self.inner.retrieve_facts(user_id, agent_id).await
    }

    async fn delete_fact(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        fact_id: &str,
    ) -> benshu_brain::error::Result<()> {
        self.inner.delete_fact(user_id, agent_id, fact_id).await
    }

    async fn find_related_facts(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        fact_id: &str,
        depth: usize,
    ) -> benshu_brain::error::Result<Vec<Fact>> {
        self.inner
            .find_related_facts(user_id, agent_id, fact_id, depth)
            .await
    }

    async fn maintenance(&self) -> benshu_brain::error::Result<()> {
        self.inner.maintenance().await
    }

    async fn update_utility(
        &self,
        collection: &str,
        fact_id: &str,
        increment: f32,
    ) -> benshu_brain::error::Result<()> {
        self.inner
            .update_utility(collection, fact_id, increment)
            .await
    }

    async fn age_vectors(
        &self,
        collection: &str,
        older_than_days: usize,
    ) -> benshu_brain::error::Result<()> {
        self.inner.age_vectors(collection, older_than_days).await
    }

    async fn promote_vectors(
        &self,
        collection: &str,
        level: benshu_inference::QuantLevel,
    ) -> benshu_brain::error::Result<()> {
        self.inner.promote_vectors(collection, level).await
    }

    async fn update_fact_importance(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        fact_id: &str,
        importance: f32,
    ) -> benshu_brain::error::Result<()> {
        self.inner
            .update_fact_importance(user_id, agent_id, fact_id, importance)
            .await
    }

    async fn get_metadata(&self, key: &str) -> benshu_brain::error::Result<Option<String>> {
        Ok(self.metadata.read().get(key).cloned())
    }

    async fn set_metadata(&self, key: &str, value: &str) -> benshu_brain::error::Result<()> {
        self.metadata
            .write()
            .insert(key.to_string(), value.to_string());
        Ok(())
    }

    fn set_emitter(&self, _emitter: Arc<dyn benshu_infra::traits::memory::MemoryEmitter>) {}
}

#[async_trait]
impl Tool for EchoTool {
    fn name(&self) -> String {
        "echo_tool".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "echo_tool".to_string(),
            description: "Echo test tool".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "value": { "type": "string" }
                }
            }),
            parameters_ts: None,
            is_binary: false,
            is_verified: true,
            usage_guidelines: None,
            safety_level: SafetyLevel::Green,
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        Ok(arguments.to_string())
    }
}

#[async_trait]
impl Tool for LongOutputTool {
    fn name(&self) -> String {
        "long_output_tool".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "long_output_tool".to_string(),
            description: "Returns a very long payload for truncation tests".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "value": { "type": "string" }
                }
            }),
            parameters_ts: None,
            is_binary: false,
            is_verified: true,
            usage_guidelines: None,
            safety_level: SafetyLevel::Green,
        }
    }

    async fn call(&self, _arguments: &str) -> anyhow::Result<String> {
        Ok("x".repeat(256))
    }
}

#[async_trait]
impl Tool for SyntheticKnowledgeSearchTool {
    fn name(&self) -> String {
        "knowledge_search".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "knowledge_search".to_string(),
            description: "Synthetic retrieval tool for degradation trace tests".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "limit": { "type": "integer" }
                }
            }),
            parameters_ts: None,
            is_binary: false,
            is_verified: true,
            usage_guidelines: None,
            safety_level: SafetyLevel::Green,
        }
    }

    async fn call(&self, _arguments: &str) -> anyhow::Result<String> {
        Ok(
            "### Retrieval Route\n\nRequested Limit: 5\nInitial Candidates: 2/5\nSafety Net: applied\nRetrieval Degradation: candidate_pool_below_limit, returned_below_limit\nLatency Ms: 1\n\n### Knowledge Search Results\n\n1. [docs/sparse.md] (Score: 0.91)\n"
                .to_string(),
        )
    }
}

#[async_trait]
impl Provider for RecordingPreemptiveProvider {
    async fn stream_completion(
        &self,
        request: ChatRequest,
    ) -> benshu_infra::error::Result<StreamingResponse> {
        self.requests.lock().await.push(request);
        let call_index = self.call_count.fetch_add(1, Ordering::SeqCst);

        let stream = async_stream::stream! {
            if call_index == 0 {
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                yield Ok(StreamingChoice::Message("first".to_string()));
            } else {
                yield Ok(StreamingChoice::Message("second".to_string()));
            }
            yield Ok(StreamingChoice::Done);
        };

        Ok(StreamingResponse::from_stream(stream))
    }

    fn name(&self) -> &str {
        "recording-preemptive"
    }

    fn metadata() -> ProviderMetadata
    where
        Self: Sized,
    {
        ProviderMetadata {
            id: "recording-preemptive".to_string(),
            name: "Recording Preemptive".to_string(),
            description: "Records requests and delays the first foreground stream.".to_string(),
            icon: "".to_string(),
            fields: vec![],
            capabilities: vec!["streaming".to_string()],
            preferred_models: vec![],
        }
    }
}

#[tokio::test]
async fn cancel_only_stops_current_task_not_background_runtime() {
    let env = CommTestEnv::new();
    let responses = vec![MockStreamBuilder::new().message("ok").done().build()];
    let provider: Arc<dyn Provider> = Arc::new(SequenceMockProvider::new(responses));

    let agent = Agent::builder(provider)
        .name("runtime-guard")
        .with_comm_client(env.create_client("runtime-guard"))
        .with_security(Arc::new(MockSecurityHandler))
        .build()
        .expect("agent should build");

    agent.start_background_tasks();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(
        agent.active_background_tasks() > 0,
        "background tasks should be running after startup"
    );

    agent.cancel();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(
        agent.active_background_tasks() > 0,
        "cancelling the current task must not kill lifecycle-managed background tasks"
    );

    agent.shutdown().await;
    assert_eq!(agent.active_background_tasks(), 0);
}

#[tokio::test]
async fn memory_manager_prefers_hot_for_inflight_sessions_even_when_engram_is_newer() {
    let hot: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
    let engram: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
    let manager = MemoryManager::new(hot.clone(), engram.clone());

    let mut hot_session = AgentSession::new("ownership-hot".to_string());
    hot_session.status = SessionStatus::Executing;
    hot_session.updated_at = chrono::Utc::now() - chrono::Duration::minutes(5);

    let mut engram_session = hot_session.clone();
    engram_session.status = SessionStatus::Completed;
    engram_session.updated_at = chrono::Utc::now();

    hot.store_session(hot_session.clone())
        .await
        .expect("store hot");
    engram
        .store_session(engram_session)
        .await
        .expect("store engram");

    let resolved = manager
        .retrieve_session("ownership-hot")
        .await
        .expect("retrieve")
        .expect("session");

    assert!(matches!(resolved.status, SessionStatus::Executing));
    assert!(resolved.lifecycle.recovered_from.is_none());
}

#[tokio::test]
async fn memory_manager_routes_metadata_to_contract_and_engram_namespaces() {
    let hot = Arc::new(MetadataMemory::default());
    let engram = Arc::new(MetadataMemory::default());
    let manager = MemoryManager::new(hot.clone(), engram.clone());

    hot.set_metadata("custom.hot.key", "hot-value")
        .await
        .expect("set hot");
    engram
        .set_metadata("engram.vector.execution_profile", "ann_rescore")
        .await
        .expect("set engram");

    assert_eq!(
        manager
            .get_metadata("brain.memory.authority.sessions")
            .await
            .expect("metadata"),
        Some("hot_for_inflight__engram_for_archived_recovery".to_string())
    );
    assert_eq!(
        manager
            .get_metadata("engram.vector.execution_profile")
            .await
            .expect("metadata"),
        Some("ann_rescore".to_string())
    );
    assert_eq!(
        manager
            .get_metadata("custom.hot.key")
            .await
            .expect("metadata"),
        Some("hot-value".to_string())
    );
}

#[tokio::test]
async fn shutdown_stops_all_background_loops() {
    let env = CommTestEnv::new();
    let responses = vec![MockStreamBuilder::new().message("ok").done().build()];
    let provider: Arc<dyn Provider> = Arc::new(SequenceMockProvider::new(responses));

    let agent = Agent::builder(provider)
        .name("runtime-shutdown")
        .with_comm_client(env.create_client("runtime-shutdown"))
        .with_security(Arc::new(MockSecurityHandler))
        .build()
        .expect("agent should build");

    agent.start_background_tasks();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(agent.active_background_tasks() > 0);

    agent.shutdown().await;
    assert_eq!(agent.active_background_tasks(), 0);
}

#[tokio::test]
async fn background_runtime_start_is_idempotent() {
    let env = CommTestEnv::new();
    let responses = vec![MockStreamBuilder::new().message("ok").done().build()];
    let provider: Arc<dyn Provider> = Arc::new(SequenceMockProvider::new(responses));

    let agent = Agent::builder(provider)
        .name("runtime-idempotent")
        .with_comm_client(env.create_client("runtime-idempotent"))
        .with_security(Arc::new(MockSecurityHandler))
        .build()
        .expect("agent should build");

    agent.start_background_tasks();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let first_count = agent.active_background_tasks();
    assert!(
        first_count > 0,
        "background tasks should start on first call"
    );

    agent.start_background_tasks();
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    assert_eq!(
        agent.active_background_tasks(),
        first_count,
        "restarting background runtime should be a no-op"
    );

    agent.shutdown().await;
}

#[tokio::test]
async fn foreground_chat_emits_runtime_stage_trace_and_replay_contract() {
    let responses = vec![MockStreamBuilder::new()
        .message("runtime ok")
        .done()
        .build()];
    let provider: Arc<dyn Provider> = Arc::new(SequenceMockProvider::new(responses));
    let agent = Agent::builder(provider)
        .name("runtime-trace")
        .with_security(Arc::new(MockSecurityHandler))
        .build()
        .expect("agent should build");

    let session_id = uuid::Uuid::new_v4().to_string();
    let outcome = agent
        .chat(
            vec![Message::user("hello runtime")],
            Some(session_id.clone()),
        )
        .await
        .expect("chat should succeed");

    let task = outcome.runtime_task.expect("runtime task");
    let trace = outcome.run_trace.expect("run trace");
    let replay = trace.to_replay();

    assert_eq!(task.session_id.as_deref(), Some(session_id.as_str()));
    assert_eq!(task.thread_id.as_deref(), Some(session_id.as_str()));
    assert_eq!(task.run_id, Some(trace.run_id));
    assert_eq!(task.trace_id, Some(trace.run_id));
    assert_eq!(trace.task_id, Some(task.id));
    assert_eq!(trace.thread_id.as_deref(), Some(session_id.as_str()));
    assert!(trace
        .stages
        .iter()
        .any(|stage| stage.stage == RuntimeStage::Ingress));
    assert!(trace
        .stages
        .iter()
        .any(|stage| stage.stage == RuntimeStage::Egress));
    assert!(replay.replayable);
    assert!(replay.steps.len() >= trace.stages.len());
}

#[test]
fn gate5_eval_suite_catalog_covers_twenty_standard_runtime_tasks() {
    let all = [
        RUNTIME_FIRST_BATCH_SCENARIOS,
        RUNTIME_CONTEXT_BATCH_SCENARIOS,
        RUNTIME_GOVERNANCE_MEMORY_BATCH_SCENARIOS,
        RUNTIME_HARDENING_BATCH_SCENARIOS,
    ];
    let total_cases = all.iter().map(|batch| batch.len()).sum::<usize>();
    let unique_cases = all
        .iter()
        .flat_map(|batch| batch.iter().copied())
        .collect::<std::collections::BTreeSet<_>>();

    assert_eq!(total_cases, 20);
    assert_eq!(unique_cases.len(), 20);
    assert!(total_cases >= 20);
}

#[tokio::test]
async fn gate5_replay_contract_covers_three_main_paths() {
    let responses = vec![MockStreamBuilder::new()
        .message("replay coverage ok")
        .done()
        .build()];
    let provider: Arc<dyn Provider> = Arc::new(SequenceMockProvider::new(responses));
    let agent = Agent::builder(provider)
        .name("replay-coverage")
        .with_security(Arc::new(MockSecurityHandler))
        .build()
        .expect("agent should build");

    let foreground = agent
        .chat(
            vec![Message::user("cover replay paths")],
            Some(uuid::Uuid::new_v4().to_string()),
        )
        .await
        .expect("foreground chat should succeed");
    let trace = foreground.run_trace.expect("foreground trace");
    let direct_replay = trace.to_replay();
    assert!(
        direct_replay.replayable,
        "trace -> replay should stay available"
    );

    let telemetry_root = tempdir().expect("telemetry tempdir");
    let telemetry = TelemetryManager::with_storage_root(
        TelemetryLevel::Production,
        Some(telemetry_root.path().to_path_buf()),
    );
    telemetry.save_run_trace(trace.clone());
    let persisted_replay = telemetry
        .get_run_replay(&trace.run_id)
        .expect("telemetry replay should load");
    assert!(
        persisted_replay.replayable,
        "telemetry run_id -> replay should stay available"
    );
    assert_eq!(persisted_replay.trace_id, trace.run_id);

    let harness_provider: Arc<dyn Provider> =
        Arc::new(SequenceMockProvider::new(vec![MockStreamBuilder::new()
            .message("real harness replay ok")
            .done()
            .build()]));
    let harness_agent = Agent::builder(harness_provider)
        .name("replay-harness")
        .with_security(Arc::new(MockSecurityHandler))
        .build()
        .expect("harness agent should build");
    let result = RealHarness::run_case(
        RealHarnessCase {
            suite_id: "runtime_replay_gate5".to_string(),
            scenario: "foreground_chat_replay".to_string(),
            session_id: Some(uuid::Uuid::new_v4().to_string()),
            thread_id: None,
        },
        |case| {
            let session_id = case.session_id.clone();
            async move {
                let outcome = harness_agent
                    .chat(
                        vec![Message::user("run replayable harness path")],
                        session_id,
                    )
                    .await
                    .expect("harness chat should succeed");
                Ok(outcome.run_trace.expect("harness trace"))
            }
        },
        None,
    )
    .await
    .expect("real harness case should succeed");
    assert!(
        result.witness.replay.replayable,
        "real harness witness replay should stay available"
    );
}

#[tokio::test]
async fn real_harness_can_execute_foreground_runtime_case() {
    let responses = vec![MockStreamBuilder::new()
        .message("real harness ok")
        .done()
        .build()];
    let provider: Arc<dyn Provider> = Arc::new(SequenceMockProvider::new(responses));
    let agent = Agent::builder(provider)
        .name("real-harness")
        .with_security(Arc::new(MockSecurityHandler))
        .build()
        .expect("agent should build");

    let case = RealHarnessCase {
        suite_id: "runtime_real_suite".to_string(),
        scenario: "single_agent_foreground_chat".to_string(),
        session_id: Some(uuid::Uuid::new_v4().to_string()),
        thread_id: None,
    };

    let result = RealHarness::run_case(
        case,
        |case| {
            let session_id = case.session_id.clone();
            async move {
                let outcome = agent
                    .chat(vec![Message::user("hello real harness")], session_id)
                    .await
                    .expect("chat should succeed");
                Ok(outcome.run_trace.expect("run trace should exist"))
            }
        },
        None,
    )
    .await
    .expect("real harness should run");

    assert_eq!(result.scorecard.total_trials, 1);
    assert!(
        result.witness.replay.replayable,
        "real harness witness should stay replayable"
    );
    assert_eq!(result.witness.task.suite_id, "runtime_real_suite");
    assert_eq!(result.witness.task.scenario, "single_agent_foreground_chat");
    assert!(
        result
            .witness
            .notes
            .iter()
            .any(|note| note == "real_harness"),
        "real harness witness should record harness provenance"
    );
}

#[tokio::test]
async fn real_harness_suite_executes_first_runtime_batch() {
    let suite_cases = vec![
        RealHarnessCase {
            suite_id: "runtime_first_batch".to_string(),
            scenario: "single_agent_foreground_chat".to_string(),
            session_id: Some(uuid::Uuid::new_v4().to_string()),
            thread_id: None,
        },
        RealHarnessCase {
            suite_id: "runtime_first_batch".to_string(),
            scenario: "loop_guard_tool_execution".to_string(),
            session_id: Some(uuid::Uuid::new_v4().to_string()),
            thread_id: None,
        },
        RealHarnessCase {
            suite_id: "runtime_first_batch".to_string(),
            scenario: "tool_output_degradation".to_string(),
            session_id: Some(uuid::Uuid::new_v4().to_string()),
            thread_id: None,
        },
        RealHarnessCase {
            suite_id: "runtime_first_batch".to_string(),
            scenario: "provider_failover_foreground_chat".to_string(),
            session_id: Some(uuid::Uuid::new_v4().to_string()),
            thread_id: None,
        },
        RealHarnessCase {
            suite_id: "runtime_first_batch".to_string(),
            scenario: "foreground_preemptive_chat_merge".to_string(),
            session_id: Some("suite-preemptive".to_string()),
            thread_id: None,
        },
    ];

    let suite = RealHarness::run_suite(suite_cases, |case| async move {
        match case.scenario.as_str() {
            "single_agent_foreground_chat" => {
                let responses = vec![MockStreamBuilder::new()
                    .message("suite runtime ok")
                    .done()
                    .build()];
                let provider: Arc<dyn Provider> = Arc::new(SequenceMockProvider::new(responses));
                let agent = Agent::builder(provider)
                    .name("suite-runtime-trace")
                    .with_security(Arc::new(MockSecurityHandler))
                    .build()
                    .expect("agent should build");

                let outcome = agent
                    .chat(
                        vec![Message::user("hello suite runtime")],
                        case.session_id.clone(),
                    )
                    .await
                    .expect("chat should succeed");
                Ok(outcome.run_trace.expect("run trace should exist"))
            }
            "loop_guard_tool_execution" => {
                let responses = vec![
                    MockStreamBuilder::new()
                        .tool_call("call-1", "echo_tool", serde_json::json!({"value": "same"}))
                        .tool_call("call-2", "echo_tool", serde_json::json!({"value": "same"}))
                        .done()
                        .build(),
                    MockStreamBuilder::new()
                        .message("suite loop handled")
                        .done()
                        .build(),
                ];
                let provider: Arc<dyn Provider> = Arc::new(SequenceMockProvider::new(responses));
                let agent = Agent::builder(provider)
                    .name("suite-loop-guard")
                    .tool(EchoTool)
                    .with_security(Arc::new(MockSecurityHandler))
                    .build()
                    .expect("agent should build");

                let outcome = agent
                    .chat(
                        vec![Message::user("use the tool twice")],
                        case.session_id.clone(),
                    )
                    .await
                    .expect("chat should succeed");
                Ok(outcome.run_trace.expect("run trace should exist"))
            }
            "tool_output_degradation" => {
                let responses = vec![
                    MockStreamBuilder::new()
                        .tool_call(
                            "call-1",
                            "long_output_tool",
                            serde_json::json!({"value": "truncate me"}),
                        )
                        .done()
                        .build(),
                    MockStreamBuilder::new().message("done").done().build(),
                ];
                let provider: Arc<dyn Provider> = Arc::new(SequenceMockProvider::new(responses));
                let mut agent = Agent::builder(provider)
                    .name("suite-tool-degradation")
                    .tool(LongOutputTool)
                    .with_security(Arc::new(MockSecurityHandler))
                    .build()
                    .expect("agent should build");
                agent.config_mut().max_tool_output_chars = 32;

                let outcome = agent
                    .chat(
                        vec![Message::user("truncate the tool output")],
                        case.session_id.clone(),
                    )
                    .await
                    .expect("chat should succeed");
                Ok(outcome.run_trace.expect("run trace should exist"))
            }
            "provider_failover_foreground_chat" => {
                let fallback = SequenceMockProvider::new(vec![MockStreamBuilder::new()
                    .message("fallback ok")
                    .done()
                    .build()]);
                let provider: Arc<dyn Provider> = Arc::new(ResilientProvider::new(
                    AlwaysFailProvider,
                    fallback,
                    CircuitBreakerConfig::default(),
                ));
                let agent = Agent::builder(provider)
                    .name("suite-provider-failover")
                    .with_security(Arc::new(MockSecurityHandler))
                    .build()
                    .expect("agent should build");

                let outcome = agent
                    .chat(
                        vec![Message::user("trigger provider failover")],
                        case.session_id.clone(),
                    )
                    .await
                    .expect("fallback chat should succeed");
                Ok(outcome.run_trace.expect("run trace should exist"))
            }
            "foreground_preemptive_chat_merge" => {
                let provider = RecordingPreemptiveProvider::new();
                let provider: Arc<dyn Provider> = Arc::new(provider);
                let memory = Arc::new(InMemoryMemory::new());
                let agent = Agent::builder(provider)
                    .name("suite-preemptive")
                    .with_security(Arc::new(MockSecurityHandler))
                    .with_memory(memory)
                    .with_session_id(case.session_id.clone().expect("session id"))
                    .build()
                    .expect("agent should build");

                let first_agent = agent.clone();
                let first_session = case.session_id.clone();
                let first_task = tokio::spawn(async move {
                    first_agent
                        .chat(vec![Message::user("first question")], first_session)
                        .await
                });

                tokio::time::sleep(std::time::Duration::from_millis(50)).await;

                let second_outcome = agent
                    .chat(
                        vec![Message::user("second question")],
                        case.session_id.clone(),
                    )
                    .await
                    .expect("second request should complete after preemption");

                let first_result = first_task.await.expect("first task should join");
                let first_error = first_result.expect_err("first task should be cancelled");
                assert!(
                    first_error.to_string().to_lowercase().contains("preempt")
                        || first_error.to_string().to_lowercase().contains("cancel"),
                    "expected preemption error, got {}",
                    first_error
                );

                Ok(second_outcome.run_trace.expect("run trace should exist"))
            }
            other => panic!("unexpected suite scenario {other}"),
        }
    })
    .await
    .expect("real harness suite should run");

    assert_eq!(suite.suite_id, "runtime_first_batch");
    assert_eq!(suite.total_cases, 5);
    assert_eq!(suite.results.len(), 5);
    assert_eq!(suite.scorecard.total_trials, 5);
    assert!(
        suite.scorecard.passed_trials
            + suite.scorecard.failed_trials
            + suite.scorecard.warned_trials
            == 5,
        "scorecard counts should cover the whole suite"
    );
    assert!(
        suite.scorecard.failed_trials >= 1,
        "expected at least one guarded/degraded runtime case to surface as failure"
    );
    assert!(suite
        .results
        .iter()
        .all(|result| result.witness.replay.replayable));
    assert!(suite
        .results
        .iter()
        .any(|result| result.case.scenario == "provider_failover_foreground_chat"));
}

#[tokio::test]
async fn real_harness_suite_executes_context_and_memory_batch() {
    let suite_cases = vec![
        RealHarnessCase {
            suite_id: "runtime_context_batch".to_string(),
            scenario: "clean_tool_execution".to_string(),
            session_id: Some(uuid::Uuid::new_v4().to_string()),
            thread_id: None,
        },
        RealHarnessCase {
            suite_id: "runtime_context_batch".to_string(),
            scenario: "retrieval_signal_injection".to_string(),
            session_id: Some(uuid::Uuid::new_v4().to_string()),
            thread_id: None,
        },
        RealHarnessCase {
            suite_id: "runtime_context_batch".to_string(),
            scenario: "retrieval_low_signal_skip".to_string(),
            session_id: Some(uuid::Uuid::new_v4().to_string()),
            thread_id: None,
        },
        RealHarnessCase {
            suite_id: "runtime_context_batch".to_string(),
            scenario: "session_thread_refs_are_stable".to_string(),
            session_id: Some("suite-session-refs".to_string()),
            thread_id: None,
        },
        RealHarnessCase {
            suite_id: "runtime_context_batch".to_string(),
            scenario: "failing_context_injector_is_non_fatal".to_string(),
            session_id: Some(uuid::Uuid::new_v4().to_string()),
            thread_id: None,
        },
    ];

    let suite = RealHarness::run_suite(suite_cases, |case| async move {
        match case.scenario.as_str() {
            "clean_tool_execution" => {
                let responses = vec![
                    MockStreamBuilder::new()
                        .tool_call(
                            "call-1",
                            "echo_tool",
                            serde_json::json!({"value": "hello clean tool"}),
                        )
                        .done()
                        .build(),
                    MockStreamBuilder::new()
                        .message("clean tool ok")
                        .done()
                        .build(),
                ];
                let provider: Arc<dyn Provider> = Arc::new(SequenceMockProvider::new(responses));
                let agent = Agent::builder(provider)
                    .name("suite-clean-tool")
                    .tool(EchoTool)
                    .with_security(Arc::new(MockSecurityHandler))
                    .build()
                    .expect("agent should build");

                let outcome = agent
                    .chat(
                        vec![Message::user("use the tool once cleanly")],
                        case.session_id.clone(),
                    )
                    .await
                    .expect("chat should succeed");
                let trace = outcome.run_trace.expect("run trace should exist");
                assert!(
                    trace.tools.iter().any(|tool| tool.tool_name == "echo_tool"),
                    "clean tool case should record tool usage"
                );
                Ok(trace)
            }
            "retrieval_signal_injection" => {
                let provider = RecordingProvider::new("retrieval ok");
                let provider_for_asserts = provider.clone();
                let provider: Arc<dyn Provider> = Arc::new(provider);
                let memory: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
                memory
                    .store_knowledge(
                        "user",
                        None,
                        "Sky Fact",
                        "Earlier context about weather patterns.\nLet's keep thinking about atmospheric optics.\nCan you remind me why the sky is blue during the day?",
                        "science",
                        true,
                    )
                    .await
                    .expect("knowledge should store");
                let agent = Agent::builder(provider)
                    .name("suite-retrieval-signal")
                    .with_security(Arc::new(MockSecurityHandler))
                    .with_memory(memory)
                    .build()
                    .expect("agent should build");

                let outcome = agent
                    .chat(
                        vec![Message::user(
                            "Can you remind me why the sky is blue during the day?",
                        )],
                        case.session_id.clone(),
                    )
                    .await
                    .expect("chat should succeed");
                let recorded = provider_for_asserts.recorded_requests().await;
                assert!(
                    recorded.iter().any(|request| request.messages.iter().any(|message| {
                        message.text().contains("LEARNED KNOWLEDGE (RAG)")
                            && message.text().contains("Sky Fact")
                    })),
                    "high-signal retrieval case should inject learned knowledge"
                );
                Ok(outcome.run_trace.expect("run trace should exist"))
            }
            "retrieval_low_signal_skip" => {
                let provider = RecordingProvider::new("low signal ok");
                let provider_for_asserts = provider.clone();
                let provider: Arc<dyn Provider> = Arc::new(provider);
                let memory: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
                memory
                    .store_knowledge(
                        "user",
                        None,
                        "Sky Fact",
                        "Earlier context about weather patterns.\nLet's keep thinking about atmospheric optics.\nCan you remind me why the sky is blue during the day?",
                        "science",
                        true,
                    )
                    .await
                    .expect("knowledge should store");
                let agent = Agent::builder(provider)
                    .name("suite-retrieval-low-signal")
                    .with_security(Arc::new(MockSecurityHandler))
                    .with_memory(memory)
                    .build()
                    .expect("agent should build");

                let outcome = agent
                    .chat(vec![Message::user("hi")], case.session_id.clone())
                    .await
                    .expect("chat should succeed");
                let recorded = provider_for_asserts.recorded_requests().await;
                assert!(
                    recorded.iter().all(|request| request.messages.iter().all(|message| {
                        !message.text().contains("LEARNED KNOWLEDGE (RAG)")
                            && !message.text().contains("Sky Fact")
                    })),
                    "low-signal retrieval case should skip learned knowledge injection"
                );
                Ok(outcome.run_trace.expect("run trace should exist"))
            }
            "session_thread_refs_are_stable" => {
                let provider: Arc<dyn Provider> =
                    Arc::new(RecordingProvider::new("session refs ok"));
                let session_id = case.session_id.clone().expect("session id");
                let agent = Agent::builder(provider)
                    .name("suite-session-refs")
                    .with_security(Arc::new(MockSecurityHandler))
                    .with_session_id(session_id.clone())
                    .build()
                    .expect("agent should build");

                let outcome = agent
                    .chat(
                        vec![Message::user("verify runtime refs")],
                        Some(session_id.clone()),
                    )
                    .await
                    .expect("chat should succeed");
                let trace = outcome.run_trace.clone().expect("run trace should exist");
                assert!(
                    trace.thread_id.as_deref() == Some(session_id.as_str())
                        && trace.task_id.is_some()
                        && outcome
                            .runtime_task
                            .as_ref()
                            .and_then(|task| task.session_id.as_deref())
                            == Some(session_id.as_str()),
                    "runtime refs should stay stable for explicit session runs"
                );
                Ok(trace)
            }
            "failing_context_injector_is_non_fatal" => {
                let provider: Arc<dyn Provider> =
                    Arc::new(RecordingProvider::new("injector failure tolerated"));
                let agent = Agent::builder(provider)
                    .name("suite-failing-injector")
                    .with_security(Arc::new(MockSecurityHandler))
                    .add_injector(FailingContextInjector)
                    .build()
                    .expect("agent should build");

                let outcome = agent
                    .chat(
                        vec![Message::user("context injector should not crash the run")],
                        case.session_id.clone(),
                    )
                    .await
                    .expect("chat should survive injector failure");
                Ok(outcome.run_trace.expect("run trace should exist"))
            }
            other => panic!("unexpected context suite scenario {other}"),
        }
    })
    .await
    .expect("context and memory suite should run");

    assert_eq!(suite.suite_id, "runtime_context_batch");
    assert_eq!(suite.total_cases, 5);
    assert_eq!(suite.results.len(), 5);
    assert_eq!(suite.scorecard.total_trials, 5);
    assert_eq!(
        suite.scorecard.passed_trials
            + suite.scorecard.failed_trials
            + suite.scorecard.warned_trials,
        5,
        "scorecard counts should cover the whole context suite"
    );
    assert!(suite
        .results
        .iter()
        .all(|result| result.witness.replay.replayable));
    assert!(suite
        .results
        .iter()
        .any(|result| result.case.scenario == "retrieval_signal_injection"));
}

#[tokio::test]
async fn real_harness_suite_executes_governance_and_memory_batch() {
    let suite_cases = vec![
        RealHarnessCase {
            suite_id: "runtime_governance_memory_batch".to_string(),
            scenario: "approval_guard_blocks_risky_tool".to_string(),
            session_id: Some(uuid::Uuid::new_v4().to_string()),
            thread_id: None,
        },
        RealHarnessCase {
            suite_id: "runtime_governance_memory_batch".to_string(),
            scenario: "prime_delegation_keeps_ownership".to_string(),
            session_id: Some("prime-ownership-session-suite".to_string()),
            thread_id: None,
        },
        RealHarnessCase {
            suite_id: "runtime_governance_memory_batch".to_string(),
            scenario: "comm_inbox_owner_rollup_persists".to_string(),
            session_id: Some("session-a2a-suite".to_string()),
            thread_id: None,
        },
        RealHarnessCase {
            suite_id: "runtime_governance_memory_batch".to_string(),
            scenario: "memory_pending_review_persists".to_string(),
            session_id: Some(uuid::Uuid::new_v4().to_string()),
            thread_id: None,
        },
        RealHarnessCase {
            suite_id: "runtime_governance_memory_batch".to_string(),
            scenario: "memory_archive_and_prune_completes".to_string(),
            session_id: Some("session-archive-suite".to_string()),
            thread_id: None,
        },
    ];

    let suite = RealHarness::run_suite(suite_cases, |case| async move {
        match case.scenario.as_str() {
            "approval_guard_blocks_risky_tool" => {
                let provider: Arc<dyn Provider> = Arc::new(SequenceMockProvider::new(vec![]));
                let agent = Agent::builder(provider)
                    .name("suite-approval-guard")
                    .with_security(Arc::new(MockSecurityHandler))
                    .with_tool(EchoTool)
                    .with_tool_policy(benshu_brain::agent::protocol::RiskyToolPolicy {
                        default_policy: ToolPolicy::Auto,
                        overrides: std::collections::HashMap::new(),
                    })
                    .with_approval_handler(Arc::new(RejectAllApprovalHandler))
                    .with_initial_risk_score(0.95)
                    .build()
                    .expect("agent should build");

                let err = agent
                    .call_tool("echo_tool", r#"{"value":"hello"}"#)
                    .await
                    .expect_err("high-risk context should force approval");
                assert!(
                    err.to_string().contains("Approval required")
                        || err.to_string().contains("approval"),
                    "unexpected approval error: {}",
                    err
                );

                let mut trace = traced_operational_case(
                    "suite-approval-guard",
                    "approval handler blocked risky tool execution",
                    TraceStatus::Failed,
                );
                trace
                    .degradation_notes
                    .push("governance:approval_required".to_string());
                Ok(trace)
            }
            "prime_delegation_keeps_ownership" => {
                let coordinator = Coordinator::new();
                let prime_role = AgentRole::Custom("benshu".to_string());
                let specialist_role = AgentRole::Researcher;

                coordinator.register(Arc::new(StubDelegatingAgent {
                    role: prime_role.clone(),
                    response: "I should delegate this internally.".to_string(),
                    handover: Some(specialist_role.clone()),
                }));
                coordinator.register(Arc::new(StubDelegatingAgent {
                    role: specialist_role.clone(),
                    response: "specialist result".to_string(),
                    handover: None,
                }));

                let outcome = coordinator
                    .chat_session(
                        case.session_id.as_deref().expect("session id"),
                        vec![Message::user("research this")],
                    )
                    .await
                    .expect("chat session should succeed");
                assert_eq!(outcome.ownership.visible_owner.name(), "benshu");
                assert_eq!(outcome.ownership.memory_owner.name(), "benshu");
                assert_eq!(outcome.ownership.approval_owner.name(), "benshu");
                let delegation = outcome
                    .delegation
                    .expect("prime delegation should be recorded");
                assert_eq!(delegation.delegated_by.name(), "benshu");
                assert_eq!(delegation.delegated_to.name(), "researcher");

                let mut trace = traced_operational_case(
                    "suite-prime-delegation",
                    "prime-owned delegation preserved visible/memory/approval ownership",
                    TraceStatus::Succeeded,
                );
                trace.metadata.insert(
                    "delegated_to".to_string(),
                    delegation.delegated_to.name().to_string(),
                );
                Ok(trace)
            }
            "comm_inbox_owner_rollup_persists" => {
                let env = CommTestEnv::new();
                let comm_client = env.create_client("benshu");
                let sender = env.create_client("researcher");
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;

                let memory = Arc::new(MetadataMemory::default());
                let provider: Arc<dyn Provider> = Arc::new(SequenceMockProvider::new(vec![]));
                let agent = Agent::builder(provider)
                    .name("benshu")
                    .with_security(Arc::new(MockSecurityHandler))
                    .with_memory(memory.clone())
                    .with_comm_client(comm_client)
                    .build()
                    .expect("agent should build");

                let delegation = DelegationEnvelope {
                    session_id: case.session_id.clone(),
                    visible_owner_id: "benshu".to_string(),
                    memory_owner_id: "benshu".to_string(),
                    approval_owner_id: "benshu".to_string(),
                    final_response_owner_id: "benshu".to_string(),
                    delegated_by_id: "benshu".to_string(),
                    delegated_to_id: "researcher".to_string(),
                    return_mode: DelegationReturnMode::ReturnToOwner,
                    trace_id: Some("trace-a2a-suite".to_string()),
                    task_id: Some("task-a2a-suite".to_string()),
                    parent_task_id: Some("parent-a2a-suite".to_string()),
                    root_task_id: Some("root-a2a-suite".to_string()),
                    state: benshu_comm::protocol::a2a::DelegationState::Returned,
                };
                let message = A2AMessage::Result {
                    request_id: "req-a2a-suite".to_string(),
                    performer_id: "researcher".to_string(),
                    output: "specialist finished".to_string(),
                    success: true,
                    delegation: Some(delegation),
                };
                sender
                    .send_msg(
                        Address::Agent("benshu".to_string()),
                        serde_json::to_vec(&message).expect("payload should serialize"),
                    )
                    .await
                    .expect("message should send");

                agent.poll_comm_once().await.expect("poll should succeed");

                let owner_rollup = memory
                    .get_metadata("brain.comm.benshu.owner_rollup.last_json")
                    .await
                    .expect("metadata should load")
                    .expect("owner rollup should exist");
                let inbox = memory
                    .get_metadata("brain.comm.benshu.inbox.recent_json")
                    .await
                    .expect("metadata should load")
                    .expect("inbox should exist");
                assert!(owner_rollup.contains("\"req-a2a-suite\""));
                assert!(inbox.contains("\"req-a2a-suite\""));

                let mut trace = traced_operational_case(
                    "suite-comm-owner-rollup",
                    "comm receive loop persisted inbox and owner rollup metadata",
                    TraceStatus::Succeeded,
                );
                trace
                    .metadata
                    .insert("request_id".to_string(), "req-a2a-suite".to_string());
                Ok(trace)
            }
            "memory_pending_review_persists" => {
                let hot: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
                let engram: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
                let memory = MemoryManager::new(hot.clone(), engram.clone());

                let fact = Fact {
                    id: "fact-review-suite".to_string(),
                    category: "facts".to_string(),
                    content: "This summary may conflict with newer evidence.".to_string(),
                    importance: 0.5,
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                    verified: false,
                    source: Some("test".to_string()),
                    confidence: 0.4,
                    relations: Vec::new(),
                    semantic_hash: Some("fact-review-suite".to_string()),
                    status: FactStatus::Pending,
                    protection: FactProtection::Normal,
                };

                hot.store_fact("user", None, fact.clone())
                    .await
                    .expect("hot fact stored");
                engram
                    .store_fact("user", None, fact)
                    .await
                    .expect("engram fact stored");
                memory
                    .mark_pending_review("fact-review-suite", Some("conflicts with prior summary"))
                    .await
                    .expect("pending review should persist");

                let hot_fact = hot
                    .retrieve_facts("user", None)
                    .await
                    .expect("hot retrieve succeeds")
                    .into_iter()
                    .find(|fact| fact.id == "fact-review-suite")
                    .expect("hot fact exists");
                let engram_fact = engram
                    .retrieve_facts("user", None)
                    .await
                    .expect("engram retrieve succeeds")
                    .into_iter()
                    .find(|fact| fact.id == "fact-review-suite")
                    .expect("engram fact exists");
                assert!(matches!(hot_fact.status, FactStatus::PendingReview));
                assert!(matches!(engram_fact.status, FactStatus::PendingReview));

                Ok(traced_operational_case(
                    "suite-memory-pending-review",
                    "pending review state propagated across hot and engram",
                    TraceStatus::Succeeded,
                ))
            }
            "memory_archive_and_prune_completes" => {
                let hot: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
                let engram: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
                let memory = MemoryManager::new(hot.clone(), engram.clone());
                let session_id = case.session_id.clone().expect("session id");

                memory
                    .store_session(AgentSession::new(session_id.clone()))
                    .await
                    .expect("session should store");
                let retention_until = chrono::Utc::now() - chrono::Duration::minutes(1);
                let archived = memory
                    .archive_session(
                        &session_id,
                        Some("session window closed"),
                        Some(retention_until),
                    )
                    .await
                    .expect("archive succeeds")
                    .expect("session should exist");
                assert!(archived.is_archived());

                let pruned = memory
                    .prune_expired_sessions(chrono::Utc::now())
                    .await
                    .expect("prune succeeds");
                assert_eq!(pruned, 1);
                assert!(hot
                    .retrieve_session(&session_id)
                    .await
                    .expect("hot retrieve succeeds")
                    .is_none());
                assert!(engram
                    .retrieve_session(&session_id)
                    .await
                    .expect("engram retrieve succeeds")
                    .is_none());

                Ok(traced_operational_case(
                    "suite-memory-archive-prune",
                    "archived session pruned across hot and engram",
                    TraceStatus::Succeeded,
                ))
            }
            other => panic!("unexpected governance/memory suite scenario {other}"),
        }
    })
    .await
    .expect("governance and memory suite should run");

    assert_eq!(suite.suite_id, "runtime_governance_memory_batch");
    assert_eq!(suite.total_cases, 5);
    assert_eq!(suite.results.len(), 5);
    assert_eq!(suite.scorecard.total_trials, 5);
    assert_eq!(
        suite.scorecard.passed_trials
            + suite.scorecard.failed_trials
            + suite.scorecard.warned_trials,
        5,
        "scorecard counts should cover the whole governance/memory suite"
    );
    assert!(
        suite.scorecard.failed_trials >= 1,
        "approval-guarded case should surface at least one failed witness"
    );
    assert!(suite
        .results
        .iter()
        .all(|result| result.witness.replay.replayable));
    assert!(suite
        .results
        .iter()
        .any(|result| result.case.scenario == "comm_inbox_owner_rollup_persists"));
}

#[tokio::test]
async fn real_harness_suite_executes_hardening_batch() {
    let suite_cases = vec![
        RealHarnessCase {
            suite_id: "runtime_hardening_batch".to_string(),
            scenario: "token_budget_exhaustion_blocks_run".to_string(),
            session_id: Some(uuid::Uuid::new_v4().to_string()),
            thread_id: None,
        },
        RealHarnessCase {
            suite_id: "runtime_hardening_batch".to_string(),
            scenario: "relation_depth_is_hard_capped".to_string(),
            session_id: Some(uuid::Uuid::new_v4().to_string()),
            thread_id: None,
        },
        RealHarnessCase {
            suite_id: "runtime_hardening_batch".to_string(),
            scenario: "multimodal_writeback_persists_contract".to_string(),
            session_id: Some(uuid::Uuid::new_v4().to_string()),
            thread_id: None,
        },
        RealHarnessCase {
            suite_id: "runtime_hardening_batch".to_string(),
            scenario: "cancel_marker_persists_to_stm".to_string(),
            session_id: Some(uuid::Uuid::new_v4().to_string()),
            thread_id: None,
        },
        RealHarnessCase {
            suite_id: "runtime_hardening_batch".to_string(),
            scenario: "pending_review_resolution_persists".to_string(),
            session_id: Some(uuid::Uuid::new_v4().to_string()),
            thread_id: None,
        },
    ];

    let suite = RealHarness::run_suite(suite_cases, |case| async move {
        match case.scenario.as_str() {
            "token_budget_exhaustion_blocks_run" => {
                let responses = vec![MockStreamBuilder::new()
                    .message("budget breach")
                    .usage(Usage {
                        prompt_tokens: 8,
                        completion_tokens: 7,
                        total_tokens: 15,
                    })
                    .done()
                    .build()];
                let provider: Arc<dyn Provider> = Arc::new(SequenceMockProvider::new(responses));
                let agent = Agent::builder(provider)
                    .name("suite-budget-guard")
                    .with_security(Arc::new(MockSecurityHandler))
                    .with_token_budget(10)
                    .build()
                    .expect("agent should build");

                let err = agent
                    .chat(vec![Message::user("trigger budget")], case.session_id.clone())
                    .await
                    .expect_err("budget should hard-stop the run");
                assert!(err
                    .to_string()
                    .contains("Governance token budget exhausted"));

                let mut trace = traced_operational_case(
                    "suite-budget-guard",
                    "governance token budget exhausted before run completion",
                    TraceStatus::Failed,
                );
                trace
                    .degradation_notes
                    .push("governance:token_budget_exhausted".to_string());
                Ok(trace)
            }
            "relation_depth_is_hard_capped" => {
                let hot: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
                let engram: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
                let memory = MemoryManager::new(hot.clone(), engram.clone());
                let now = chrono::Utc::now();
                let facts = vec![
                    Fact {
                        id: "root".to_string(),
                        category: "facts".to_string(),
                        content: "root".to_string(),
                        importance: 0.8,
                        created_at: now,
                        updated_at: now,
                        verified: true,
                        source: Some("test".to_string()),
                        confidence: 1.0,
                        relations: vec![Relation {
                            predicate: "linked_to".to_string(),
                            target_id: "a".to_string(),
                            strength: 1.0,
                        }],
                        semantic_hash: Some("root".to_string()),
                        status: FactStatus::Verified,
                        protection: FactProtection::Normal,
                    },
                    Fact {
                        id: "a".to_string(),
                        category: "facts".to_string(),
                        content: "a".to_string(),
                        importance: 0.8,
                        created_at: now,
                        updated_at: now,
                        verified: true,
                        source: Some("test".to_string()),
                        confidence: 1.0,
                        relations: vec![Relation {
                            predicate: "linked_to".to_string(),
                            target_id: "b".to_string(),
                            strength: 1.0,
                        }],
                        semantic_hash: Some("a".to_string()),
                        status: FactStatus::Verified,
                        protection: FactProtection::Normal,
                    },
                    Fact {
                        id: "b".to_string(),
                        category: "facts".to_string(),
                        content: "b".to_string(),
                        importance: 0.8,
                        created_at: now,
                        updated_at: now,
                        verified: true,
                        source: Some("test".to_string()),
                        confidence: 1.0,
                        relations: vec![Relation {
                            predicate: "linked_to".to_string(),
                            target_id: "c".to_string(),
                            strength: 1.0,
                        }],
                        semantic_hash: Some("b".to_string()),
                        status: FactStatus::Verified,
                        protection: FactProtection::Normal,
                    },
                    Fact {
                        id: "c".to_string(),
                        category: "facts".to_string(),
                        content: "c".to_string(),
                        importance: 0.8,
                        created_at: now,
                        updated_at: now,
                        verified: true,
                        source: Some("test".to_string()),
                        confidence: 1.0,
                        relations: vec![Relation {
                            predicate: "linked_to".to_string(),
                            target_id: "d".to_string(),
                            strength: 1.0,
                        }],
                        semantic_hash: Some("c".to_string()),
                        status: FactStatus::Verified,
                        protection: FactProtection::Normal,
                    },
                    Fact {
                        id: "d".to_string(),
                        category: "facts".to_string(),
                        content: "d".to_string(),
                        importance: 0.8,
                        created_at: now,
                        updated_at: now,
                        verified: true,
                        source: Some("test".to_string()),
                        confidence: 1.0,
                        relations: Vec::new(),
                        semantic_hash: Some("d".to_string()),
                        status: FactStatus::Verified,
                        protection: FactProtection::Normal,
                    },
                ];

                for fact in facts {
                    hot.store_fact("user", None, fact.clone())
                        .await
                        .expect("hot fact stored");
                    engram
                        .store_fact("user", None, fact)
                        .await
                        .expect("engram fact stored");
                }

                let related = memory
                    .find_related_facts("user", None, "root", 10)
                    .await
                    .expect("related facts should resolve");
                let related_ids = related.iter().map(|fact| fact.id.as_str()).collect::<Vec<_>>();
                assert_eq!(related_ids, vec!["a", "b", "c"]);
                assert!(!related_ids.contains(&"d"));

                Ok(traced_operational_case(
                    "suite-relation-cap",
                    "relation traversal stopped at hard cap depth",
                    TraceStatus::Succeeded,
                ))
            }
            "multimodal_writeback_persists_contract" => {
                let hot: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
                let engram: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
                let memory = MemoryManager::new(hot.clone(), engram.clone());

                let document = memory
                    .store_multimodal_memory(
                        "user",
                        None,
                        MultimodalMemoryRecord {
                            kind: MultimodalMemoryKind::Understanding,
                            modality: "image".to_string(),
                            title: "Screenshot Analysis".to_string(),
                            summary: "The screenshot shows the BenShu dashboard health panel."
                                .to_string(),
                            content: "The screenshot shows the BenShu dashboard health panel with stable memory status and no active degradation warnings.".to_string(),
                            collection: "multimodal".to_string(),
                            source_path: Some("/tmp/dashboard.png".to_string()),
                            source_url: None,
                            route: Some("provider_vision".to_string()),
                            model: Some("configured-vision-model".to_string()),
                            prompt: Some("Summarize this screenshot.".to_string()),
                            artifact_locator: None,
                            transient: false,
                            derived_fact: Some(MultimodalDerivedFact {
                                content:
                                    "The BenShu dashboard health panel is currently stable."
                                        .to_string(),
                                category: "multimodal_observation".to_string(),
                                importance: 0.7,
                                verified: false,
                            }),
                            metadata: HashMap::new(),
                        },
                    )
                    .await
                    .expect("multimodal writeback should succeed");

                assert_eq!(document.collection.as_deref(), Some("multimodal"));
                Ok(traced_operational_case(
                    "suite-multimodal-writeback",
                    "multimodal document and derived fact persisted",
                    TraceStatus::Succeeded,
                ))
            }
            "cancel_marker_persists_to_stm" => {
                let dir = tempdir().expect("tempdir");
                let memory = ShortTermMemory::new(8, 8, dir.path().join("stm.redb")).await;
                memory
                    .mark_cancelled("user", None, "user requested stop")
                    .await
                    .expect("cancel marker should store");
                let history = memory.retrieve("user", None, 10).await;
                let last = history.last().expect("cancel marker should exist");
                assert!(last.text().contains("user requested stop"));

                Ok(traced_operational_case(
                    "suite-cancel-marker",
                    "short-term memory appended cancellation marker",
                    TraceStatus::Succeeded,
                ))
            }
            "pending_review_resolution_persists" => {
                let hot: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
                let engram: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
                let memory = MemoryManager::new(hot.clone(), engram.clone());
                let fact = Fact {
                    id: "fact-review-resolve-suite".to_string(),
                    category: "facts".to_string(),
                    content: "This fact should survive challenger review.".to_string(),
                    importance: 0.7,
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                    verified: false,
                    source: Some("test".to_string()),
                    confidence: 0.8,
                    relations: Vec::new(),
                    semantic_hash: Some("fact-review-resolve-suite".to_string()),
                    status: FactStatus::Pending,
                    protection: FactProtection::Normal,
                };
                hot.store_fact("user", None, fact.clone())
                    .await
                    .expect("hot fact stored");
                engram
                    .store_fact("user", None, fact)
                    .await
                    .expect("engram fact stored");
                memory
                    .mark_pending_review(
                        "fact-review-resolve-suite",
                        Some("challenger requested re-check"),
                    )
                    .await
                    .expect("pending review should be stored");
                let resolution = FactReviewResolution {
                    outcome: FactReviewResolutionOutcome::Verified,
                    resolution_reason: Some(
                        "challenger accepted the revised summary".to_string(),
                    ),
                    resolution_basis: Some("challenger_re_summary".to_string()),
                    resolved_by: Some("sleep_consolidator_challenger".to_string()),
                    resolved_at: chrono::Utc::now(),
                };
                memory
                    .resolve_pending_review("fact-review-resolve-suite", resolution)
                    .await
                    .expect("resolution should sync");
                let hot_fact = hot
                    .retrieve_facts("user", None)
                    .await
                    .expect("hot retrieve succeeds")
                    .into_iter()
                    .find(|item| item.id == "fact-review-resolve-suite")
                    .expect("hot fact exists");
                assert!(matches!(hot_fact.status, FactStatus::Verified));

                Ok(traced_operational_case(
                    "suite-pending-review-resolve",
                    "pending review resolution synchronized across hot and engram",
                    TraceStatus::Succeeded,
                ))
            }
            other => panic!("unexpected hardening suite scenario {other}"),
        }
    })
    .await
    .expect("hardening suite should run");

    assert_eq!(suite.suite_id, "runtime_hardening_batch");
    assert_eq!(suite.total_cases, 5);
    assert_eq!(suite.results.len(), 5);
    assert_eq!(suite.scorecard.total_trials, 5);
    assert_eq!(
        suite.scorecard.passed_trials
            + suite.scorecard.failed_trials
            + suite.scorecard.warned_trials,
        5,
        "scorecard counts should cover the whole hardening suite"
    );
    assert!(
        suite.scorecard.failed_trials >= 1,
        "budget guard should surface at least one failed witness"
    );
    assert!(suite
        .results
        .iter()
        .all(|result| result.witness.replay.replayable));
    assert!(suite
        .results
        .iter()
        .any(|result| result.case.scenario == "multimodal_writeback_persists_contract"));
}

#[tokio::test]
async fn governance_token_budget_blocks_over_limit_session_usage() {
    let responses = vec![MockStreamBuilder::new()
        .message("budget breach")
        .usage(Usage {
            prompt_tokens: 8,
            completion_tokens: 7,
            total_tokens: 15,
        })
        .done()
        .build()];
    let provider: Arc<dyn Provider> = Arc::new(SequenceMockProvider::new(responses));
    let agent = Agent::builder(provider)
        .name("budget-guard")
        .with_security(Arc::new(MockSecurityHandler))
        .with_token_budget(10)
        .build()
        .expect("agent should build");

    let err = agent
        .chat(vec![Message::user("trigger budget")], None)
        .await
        .expect_err("budget should hard-stop the run");

    assert!(err
        .to_string()
        .contains("Governance token budget exhausted"));
}

#[tokio::test]
async fn runtime_hook_loop_guard_is_projected_into_run_trace() {
    let responses = vec![
        MockStreamBuilder::new()
            .tool_call("call-1", "echo_tool", serde_json::json!({"value": "same"}))
            .tool_call("call-2", "echo_tool", serde_json::json!({"value": "same"}))
            .done()
            .build(),
        MockStreamBuilder::new()
            .message("loop handled")
            .done()
            .build(),
    ];
    let provider: Arc<dyn Provider> = Arc::new(SequenceMockProvider::new(responses));
    let agent = Agent::builder(provider)
        .name("runtime-hook-loop")
        .tool(EchoTool)
        .with_security(Arc::new(MockSecurityHandler))
        .build()
        .expect("agent should build");

    let outcome = agent
        .chat(vec![Message::user("use the tool twice")], None)
        .await
        .expect("chat should succeed");
    let trace = outcome.run_trace.expect("run trace");

    assert_eq!(
        trace
            .metadata
            .get("hook_loop_abort_count")
            .map(String::as_str),
        Some("1")
    );
    assert_eq!(
        trace
            .metadata
            .get("hook_runtime_refs_injected")
            .map(String::as_str),
        Some("true")
    );
    assert!(
        trace
            .degradation_notes
            .iter()
            .any(|note| note.contains("loop_guard:echo_tool")),
        "expected loop guard note in run trace"
    );
}

#[tokio::test]
async fn runtime_hook_degradation_and_post_run_tap_are_projected_into_run_trace() {
    let responses = vec![
        MockStreamBuilder::new()
            .tool_call(
                "call-1",
                "long_output_tool",
                serde_json::json!({"value": "truncate me"}),
            )
            .done()
            .build(),
        MockStreamBuilder::new().message("done").done().build(),
    ];
    let provider: Arc<dyn Provider> = Arc::new(SequenceMockProvider::new(responses));
    let mut agent = Agent::builder(provider)
        .name("runtime-hook-degradation")
        .tool(LongOutputTool)
        .with_security(Arc::new(MockSecurityHandler))
        .build()
        .expect("agent should build");
    agent.config_mut().max_tool_output_chars = 32;

    let outcome = agent
        .chat(vec![Message::user("truncate the tool output")], None)
        .await
        .expect("chat should succeed");
    let trace = outcome.run_trace.expect("run trace");

    assert_eq!(
        trace
            .metadata
            .get("hook_degraded_tool_call_count")
            .map(String::as_str),
        Some("1")
    );
    assert_eq!(
        trace
            .metadata
            .get("hook_post_run_tap_count")
            .map(String::as_str),
        Some("1")
    );
    assert!(trace
        .degradation_notes
        .iter()
        .any(|note| note.contains("tool_degradation:long_output_tool:tool_output_truncated")));
    assert!(trace
        .degradation_notes
        .iter()
        .any(|note| note.contains("post_run_eval:thoughts=")));
}

#[tokio::test]
async fn retrieval_degradation_surface_is_projected_into_run_trace() {
    let responses = vec![
        MockStreamBuilder::new()
            .tool_call(
                "call-1",
                "knowledge_search",
                serde_json::json!({"query": "sparse retrieval", "limit": 5}),
            )
            .done()
            .build(),
        MockStreamBuilder::new().message("done").done().build(),
    ];
    let provider: Arc<dyn Provider> = Arc::new(SequenceMockProvider::new(responses));
    let agent = Agent::builder(provider)
        .name("runtime-hook-retrieval-degradation")
        .tool(SyntheticKnowledgeSearchTool)
        .with_security(Arc::new(MockSecurityHandler))
        .build()
        .expect("agent should build");

    let outcome = agent
        .chat(vec![Message::user("search the sparse docs")], None)
        .await
        .expect("chat should succeed");
    let trace = outcome.run_trace.expect("run trace");

    assert!(trace.degradation_notes.iter().any(|note| {
        note.contains(
            "tool_degradation:knowledge_search:retrieval:candidate_pool_below_limit|returned_below_limit",
        )
    }));
}

#[tokio::test]
async fn prime_session_keeps_visible_ownership_when_specialist_is_recommended() {
    let coordinator = Coordinator::new();
    let prime_role = AgentRole::Custom("benshu".to_string());
    let specialist_role = AgentRole::Researcher;

    coordinator.register(Arc::new(StubDelegatingAgent {
        role: prime_role.clone(),
        response: "I should delegate this internally.".to_string(),
        handover: Some(specialist_role.clone()),
    }));
    coordinator.register(Arc::new(StubDelegatingAgent {
        role: specialist_role.clone(),
        response: "specialist result".to_string(),
        handover: None,
    }));

    let outcome = coordinator
        .chat_session(
            "prime-ownership-session",
            vec![Message::user("research this")],
        )
        .await
        .expect("chat session should succeed");

    assert_eq!(outcome.ownership.visible_owner.name(), "benshu");
    assert_eq!(outcome.ownership.memory_owner.name(), "benshu");
    assert_eq!(outcome.ownership.approval_owner.name(), "benshu");

    let delegation = outcome
        .delegation
        .expect("prime delegation should be recorded");
    assert_eq!(delegation.delegated_by.name(), "benshu");
    assert_eq!(delegation.delegated_to.name(), "researcher");

    let active = coordinator.active_agents();
    let (_, role) = active
        .into_iter()
        .find(|(session_id, _)| session_id == "prime-ownership-session")
        .expect("session should remain tracked");
    assert_eq!(role.name(), "benshu");
}

#[tokio::test]
async fn memory_manager_prefers_newer_engram_session_and_backfills_hot_cache() {
    let hot: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
    let engram: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
    let memory = MemoryManager::new(hot.clone(), engram.clone());

    let mut hot_session = AgentSession::new("session-merge".to_string());
    hot_session.status = SessionStatus::Completed;
    hot_session.updated_at = chrono::Utc::now() - chrono::Duration::minutes(5);

    let mut engram_session = hot_session.clone();
    engram_session.status = SessionStatus::Completed;
    engram_session.updated_at = chrono::Utc::now();

    hot.store_session(hot_session)
        .await
        .expect("hot session stored");
    engram
        .store_session(engram_session.clone())
        .await
        .expect("engram session stored");

    let resolved = memory
        .retrieve_session("session-merge")
        .await
        .expect("retrieve session succeeds")
        .expect("session should exist");
    assert!(matches!(resolved.status, SessionStatus::Completed));
    assert_eq!(resolved.lifecycle.recovered_from.as_deref(), Some("engram"));
    assert!(resolved.lifecycle.last_recovered_at.is_some());

    let backfilled = hot
        .retrieve_session("session-merge")
        .await
        .expect("hot read succeeds")
        .expect("hot cache should be backfilled");
    assert!(matches!(backfilled.status, SessionStatus::Completed));
    assert_eq!(
        backfilled.lifecycle.recovered_from.as_deref(),
        Some("engram")
    );
}

#[tokio::test]
async fn memory_manager_archives_and_prunes_expired_sessions_across_hot_and_engram() {
    let hot: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
    let engram: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
    let memory = MemoryManager::new(hot.clone(), engram.clone());

    let session = AgentSession::new("session-archive".to_string());
    memory
        .store_session(session)
        .await
        .expect("session stored in both layers");

    let retention_until = chrono::Utc::now() - chrono::Duration::minutes(1);
    let archived = memory
        .archive_session(
            "session-archive",
            Some("session window closed"),
            Some(retention_until),
        )
        .await
        .expect("archive succeeds")
        .expect("session should exist");

    assert!(archived.is_archived());
    assert_eq!(
        archived.lifecycle.archive_reason.as_deref(),
        Some("session window closed")
    );
    assert_eq!(archived.lifecycle.retention_until, Some(retention_until));

    let pruned = memory
        .prune_expired_sessions(chrono::Utc::now())
        .await
        .expect("prune succeeds");
    assert_eq!(pruned, 1);

    assert!(
        hot.retrieve_session("session-archive")
            .await
            .expect("hot retrieve succeeds")
            .is_none(),
        "expired archived session should be removed from hot memory"
    );
    assert!(
        engram
            .retrieve_session("session-archive")
            .await
            .expect("engram retrieve succeeds")
            .is_none(),
        "expired archived session should be removed from engram"
    );
}

#[tokio::test]
async fn memory_manager_prefers_newer_engram_fact_state() {
    let hot: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
    let engram: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
    let memory = MemoryManager::new(hot.clone(), engram.clone());

    let fact_id = "fact-merge".to_string();
    let created_at = chrono::Utc::now() - chrono::Duration::minutes(10);

    let hot_fact = Fact {
        id: fact_id.clone(),
        category: "facts".to_string(),
        content: "BenShu prefers prime ownership.".to_string(),
        importance: 0.2,
        created_at,
        updated_at: created_at,
        verified: false,
        source: Some("hot".to_string()),
        confidence: 0.5,
        relations: Vec::new(),
        semantic_hash: Some("hash-hot".to_string()),
        status: FactStatus::Pending,
        protection: FactProtection::Normal,
    };

    let mut engram_fact = hot_fact.clone();
    engram_fact.importance = 0.9;
    engram_fact.verified = true;
    engram_fact.status = FactStatus::Verified;
    engram_fact.updated_at = chrono::Utc::now();
    engram_fact.source = Some("engram".to_string());

    hot.store_fact("user", None, hot_fact)
        .await
        .expect("hot fact stored");
    engram
        .store_fact("user", None, engram_fact)
        .await
        .expect("engram fact stored");

    let merged = memory
        .retrieve_facts("user", None)
        .await
        .expect("retrieve facts succeeds");
    let resolved = merged
        .into_iter()
        .find(|fact| fact.id == fact_id)
        .expect("fact should survive merge");

    assert!(resolved.verified);
    assert!(matches!(resolved.status, FactStatus::Verified));
    assert!((resolved.importance - 0.9).abs() < f32::EPSILON);
    assert_eq!(resolved.source.as_deref(), Some("engram"));
}

#[tokio::test]
async fn memory_manager_marks_pending_review_across_hot_and_engram() {
    let hot: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
    let engram: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
    let memory = MemoryManager::new(hot.clone(), engram.clone());

    let fact = Fact {
        id: "fact-review".to_string(),
        category: "facts".to_string(),
        content: "This summary may conflict with newer evidence.".to_string(),
        importance: 0.5,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        verified: false,
        source: Some("test".to_string()),
        confidence: 0.4,
        relations: Vec::new(),
        semantic_hash: Some("fact-review".to_string()),
        status: FactStatus::Pending,
        protection: FactProtection::Normal,
    };

    hot.store_fact("user", None, fact.clone())
        .await
        .expect("hot fact stored");
    engram
        .store_fact("user", None, fact)
        .await
        .expect("engram fact stored");

    memory
        .mark_pending_review("fact-review", Some("conflicts with prior summary"))
        .await
        .expect("pending review should be stored");

    let hot_fact = hot
        .retrieve_facts("user", None)
        .await
        .expect("hot retrieve succeeds")
        .into_iter()
        .find(|fact| fact.id == "fact-review")
        .expect("hot fact exists");
    assert!(matches!(hot_fact.status, FactStatus::PendingReview));
    assert!(!hot_fact.verified);

    let engram_fact = engram
        .retrieve_facts("user", None)
        .await
        .expect("engram retrieve succeeds")
        .into_iter()
        .find(|fact| fact.id == "fact-review")
        .expect("engram fact exists");
    assert!(matches!(engram_fact.status, FactStatus::PendingReview));
    assert!(!engram_fact.verified);
}

#[tokio::test]
async fn short_term_memory_marks_fact_pending_review() {
    let dir = tempdir().expect("tempdir");
    let memory = ShortTermMemory::new(8, 8, dir.path().join("stm-facts.redb")).await;

    let fact = Fact {
        id: "stm-review".to_string(),
        category: "prefs".to_string(),
        content: "This local fact needs human review.".to_string(),
        importance: 0.5,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        verified: false,
        source: Some("stm".to_string()),
        confidence: 0.6,
        relations: Vec::new(),
        semantic_hash: Some("stm-review".to_string()),
        status: FactStatus::Pending,
        protection: FactProtection::Normal,
    };

    memory
        .store_fact("user", None, fact)
        .await
        .expect("fact stored");
    memory
        .mark_pending_review("stm-review", Some("needs challenger summary"))
        .await
        .expect("pending review stored");

    let stored = memory
        .retrieve_facts("user", None)
        .await
        .expect("facts retrieved")
        .into_iter()
        .find(|fact| fact.id == "stm-review")
        .expect("fact retained");
    assert!(matches!(stored.status, FactStatus::PendingReview));
    assert!(!stored.verified);

    let payload = memory
        .get_fact_review_payload("stm-review")
        .await
        .expect("review payload retrieval should succeed")
        .expect("review payload should exist");
    assert_eq!(
        payload.challenger_summary.as_deref(),
        Some("needs challenger summary")
    );
    assert_eq!(payload.challenger_source.as_deref(), Some("memory_auditor"));
}

#[tokio::test]
async fn memory_manager_resolves_pending_review_across_hot_and_engram() {
    let hot: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
    let engram: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
    let memory = MemoryManager::new(hot.clone(), engram.clone());

    let fact = Fact {
        id: "fact-review-resolve".to_string(),
        category: "facts".to_string(),
        content: "This fact should survive challenger review.".to_string(),
        importance: 0.7,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        verified: false,
        source: Some("test".to_string()),
        confidence: 0.8,
        relations: Vec::new(),
        semantic_hash: Some("fact-review-resolve".to_string()),
        status: FactStatus::Pending,
        protection: FactProtection::Normal,
    };

    hot.store_fact("user", None, fact.clone())
        .await
        .expect("hot fact stored");
    engram
        .store_fact("user", None, fact)
        .await
        .expect("engram fact stored");
    memory
        .mark_pending_review("fact-review-resolve", Some("challenger requested re-check"))
        .await
        .expect("pending review should be stored");

    let resolution = FactReviewResolution {
        outcome: FactReviewResolutionOutcome::Verified,
        resolution_reason: Some("challenger accepted the revised summary".to_string()),
        resolution_basis: Some("challenger_re_summary".to_string()),
        resolved_by: Some("sleep_consolidator_challenger".to_string()),
        resolved_at: chrono::Utc::now(),
    };
    memory
        .resolve_pending_review("fact-review-resolve", resolution)
        .await
        .expect("resolution should sync");

    let hot_fact = hot
        .retrieve_facts("user", None)
        .await
        .expect("hot retrieve succeeds")
        .into_iter()
        .find(|item| item.id == "fact-review-resolve")
        .expect("hot fact exists");
    assert!(matches!(hot_fact.status, FactStatus::Verified));
    assert!(hot_fact.verified);

    let engram_fact = engram
        .retrieve_facts("user", None)
        .await
        .expect("engram retrieve succeeds")
        .into_iter()
        .find(|item| item.id == "fact-review-resolve")
        .expect("engram fact exists");
    assert!(matches!(engram_fact.status, FactStatus::Verified));
    assert!(engram_fact.verified);

    let payload = memory
        .get_fact_review_payload("fact-review-resolve")
        .await
        .expect("review payload retrieval should succeed")
        .expect("review payload should exist");
    let resolved = payload
        .resolution
        .expect("resolution metadata should exist");
    assert!(matches!(
        resolved.outcome,
        FactReviewResolutionOutcome::Verified
    ));
    assert_eq!(
        resolved.resolution_basis.as_deref(),
        Some("challenger_re_summary")
    );
}

#[tokio::test]
async fn memory_manager_sets_fact_protection_across_hot_and_engram() {
    let hot: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
    let engram: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
    let memory = MemoryManager::new(hot.clone(), engram.clone());

    let fact = Fact {
        id: "fact-protection-sync".to_string(),
        category: "identity".to_string(),
        content: "The user's legal name is stable identity memory.".to_string(),
        importance: 0.8,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        verified: true,
        source: Some("test".to_string()),
        confidence: 0.9,
        relations: Vec::new(),
        semantic_hash: Some("fact-protection-sync".to_string()),
        status: FactStatus::Verified,
        protection: FactProtection::Normal,
    };

    hot.store_fact("user", None, fact.clone())
        .await
        .expect("hot fact stored");
    engram
        .store_fact("user", None, fact)
        .await
        .expect("engram fact stored");

    memory
        .set_fact_protection(
            "user",
            None,
            "fact-protection-sync",
            FactProtection::CoreIdentity,
        )
        .await
        .expect("protection should sync");

    let hot_fact = hot
        .retrieve_facts("user", None)
        .await
        .expect("hot retrieve succeeds")
        .into_iter()
        .find(|item| item.id == "fact-protection-sync")
        .expect("hot fact exists");
    assert!(matches!(hot_fact.protection, FactProtection::CoreIdentity));

    let engram_fact = engram
        .retrieve_facts("user", None)
        .await
        .expect("engram retrieve succeeds")
        .into_iter()
        .find(|item| item.id == "fact-protection-sync")
        .expect("engram fact exists");
    assert!(matches!(
        engram_fact.protection,
        FactProtection::CoreIdentity
    ));
}

#[tokio::test]
async fn memory_manager_relation_depth_is_hard_capped() {
    let hot: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
    let engram: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
    let memory = MemoryManager::new(hot.clone(), engram.clone());

    let now = chrono::Utc::now();
    let facts = vec![
        Fact {
            id: "root".to_string(),
            category: "facts".to_string(),
            content: "root".to_string(),
            importance: 0.8,
            created_at: now,
            updated_at: now,
            verified: true,
            source: Some("test".to_string()),
            confidence: 1.0,
            relations: vec![Relation {
                predicate: "linked_to".to_string(),
                target_id: "a".to_string(),
                strength: 1.0,
            }],
            semantic_hash: Some("root".to_string()),
            status: FactStatus::Verified,
            protection: FactProtection::Normal,
        },
        Fact {
            id: "a".to_string(),
            category: "facts".to_string(),
            content: "a".to_string(),
            importance: 0.8,
            created_at: now,
            updated_at: now,
            verified: true,
            source: Some("test".to_string()),
            confidence: 1.0,
            relations: vec![Relation {
                predicate: "linked_to".to_string(),
                target_id: "b".to_string(),
                strength: 1.0,
            }],
            semantic_hash: Some("a".to_string()),
            status: FactStatus::Verified,
            protection: FactProtection::Normal,
        },
        Fact {
            id: "b".to_string(),
            category: "facts".to_string(),
            content: "b".to_string(),
            importance: 0.8,
            created_at: now,
            updated_at: now,
            verified: true,
            source: Some("test".to_string()),
            confidence: 1.0,
            relations: vec![Relation {
                predicate: "linked_to".to_string(),
                target_id: "c".to_string(),
                strength: 1.0,
            }],
            semantic_hash: Some("b".to_string()),
            status: FactStatus::Verified,
            protection: FactProtection::Normal,
        },
        Fact {
            id: "c".to_string(),
            category: "facts".to_string(),
            content: "c".to_string(),
            importance: 0.8,
            created_at: now,
            updated_at: now,
            verified: true,
            source: Some("test".to_string()),
            confidence: 1.0,
            relations: vec![Relation {
                predicate: "linked_to".to_string(),
                target_id: "d".to_string(),
                strength: 1.0,
            }],
            semantic_hash: Some("c".to_string()),
            status: FactStatus::Verified,
            protection: FactProtection::Normal,
        },
        Fact {
            id: "d".to_string(),
            category: "facts".to_string(),
            content: "d".to_string(),
            importance: 0.8,
            created_at: now,
            updated_at: now,
            verified: true,
            source: Some("test".to_string()),
            confidence: 1.0,
            relations: Vec::new(),
            semantic_hash: Some("d".to_string()),
            status: FactStatus::Verified,
            protection: FactProtection::Normal,
        },
    ];

    for fact in facts {
        hot.store_fact("user", None, fact.clone())
            .await
            .expect("hot fact stored");
        engram
            .store_fact("user", None, fact)
            .await
            .expect("engram fact stored");
    }

    let related = memory
        .find_related_facts("user", None, "root", 10)
        .await
        .expect("related facts should resolve");
    let related_ids = related
        .iter()
        .map(|fact| fact.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(related_ids, vec!["a", "b", "c"]);
    assert!(
        !related_ids.contains(&"d"),
        "hard cap depth should prevent traversing beyond depth 3"
    );

    assert_eq!(
        memory
            .get_metadata("brain.memory.relation.default_max_depth")
            .await
            .expect("metadata lookup succeeds")
            .as_deref(),
        Some("2")
    );
    assert_eq!(
        memory
            .get_metadata("brain.memory.relation.hard_cap_depth")
            .await
            .expect("metadata lookup succeeds")
            .as_deref(),
        Some("3")
    );
    assert_eq!(
        memory
            .get_metadata("brain.memory.relation.last_root_fact_id")
            .await
            .expect("metadata lookup succeeds")
            .as_deref(),
        Some("root")
    );
    assert_eq!(
        memory
            .get_metadata("brain.memory.relation.last_truncated")
            .await
            .expect("metadata lookup succeeds")
            .as_deref(),
        Some("true")
    );
    assert_eq!(
        memory
            .get_metadata("brain.memory.relation.last_budget_exceeded")
            .await
            .expect("metadata lookup succeeds")
            .as_deref(),
        Some("true")
    );
    assert_eq!(
        memory
            .get_metadata("brain.memory.relation.last_truncation_reason")
            .await
            .expect("metadata lookup succeeds")
            .as_deref(),
        Some("depth_hard_cap")
    );
    assert_eq!(
        memory
            .get_metadata("brain.memory.relation.last_cycle_safe")
            .await
            .expect("metadata lookup succeeds")
            .as_deref(),
        Some("true")
    );
}

#[tokio::test]
async fn memory_manager_stores_multimodal_document_and_derived_fact() {
    let hot: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
    let engram: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
    let memory = MemoryManager::new(hot.clone(), engram.clone());

    let document = memory
        .store_multimodal_memory(
            "user",
            None,
            MultimodalMemoryRecord {
                kind: MultimodalMemoryKind::Understanding,
                modality: "image".to_string(),
                title: "Screenshot Analysis".to_string(),
                summary: "The screenshot shows the BenShu dashboard health panel.".to_string(),
                content: "The screenshot shows the BenShu dashboard health panel with stable memory status and no active degradation warnings.".to_string(),
                collection: "multimodal".to_string(),
                source_path: Some("/tmp/dashboard.png".to_string()),
                source_url: None,
                route: Some("provider_vision".to_string()),
                model: Some("configured-vision-model".to_string()),
                prompt: Some("Summarize this screenshot.".to_string()),
                artifact_locator: None,
                transient: false,
                derived_fact: Some(MultimodalDerivedFact {
                    content: "The BenShu dashboard health panel is currently stable."
                        .to_string(),
                    category: "multimodal_observation".to_string(),
                    importance: 0.7,
                    verified: false,
                }),
                metadata: HashMap::new(),
            },
        )
        .await
        .expect("multimodal writeback should succeed");

    assert_eq!(document.collection.as_deref(), Some("multimodal"));
    assert_eq!(
        document
            .metadata
            .get("multimodal_modality")
            .map(String::as_str),
        Some("image")
    );
    assert_eq!(
        document
            .metadata
            .get("document_ingest_source")
            .map(String::as_str),
        Some("brain_multimodal_writeback")
    );

    let fetched = memory
        .fetch_document(
            document.collection.as_deref().unwrap_or("multimodal"),
            document.path.as_deref().unwrap_or("unknown"),
        )
        .await
        .expect("fetch should succeed")
        .expect("document should exist");
    assert_eq!(
        fetched.summary.as_deref(),
        Some(document.summary.as_deref().unwrap_or(""))
    );

    let facts = memory
        .retrieve_facts("user", None)
        .await
        .expect("facts should be retrievable");
    let derived = facts
        .into_iter()
        .find(|fact| fact.category == "multimodal_observation")
        .expect("derived fact should exist");
    assert!(matches!(derived.status, FactStatus::Pending));
    assert_eq!(
        memory
            .get_metadata("brain.memory.multimodal.last_collection")
            .await
            .expect("metadata succeeds")
            .as_deref(),
        Some("multimodal")
    );
}

#[tokio::test]
async fn memory_manager_rolls_back_hot_session_when_engram_store_fails() {
    let hot: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
    let failing_engram = Arc::new(FailingMemory::new());
    failing_engram.fail_session_store();
    let manager = MemoryManager::new(hot.clone(), failing_engram.clone());

    let mut session = AgentSession::new("rollback-session".to_string());
    session.status = SessionStatus::Executing;

    let err = manager
        .store_session(session.clone())
        .await
        .expect_err("engram failure should bubble");
    assert!(
        err.to_string().contains("rolled back"),
        "error should make rollback semantics explicit"
    );
    assert!(
        hot.retrieve_session("rollback-session")
            .await
            .expect("hot retrieve should succeed")
            .is_none(),
        "hot layer should be restored when durable write fails"
    );
}

#[tokio::test]
async fn memory_manager_rolls_back_hot_fact_when_engram_store_fails() {
    let hot: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
    let failing_engram = Arc::new(FailingMemory::new());
    failing_engram.fail_fact_store();
    let manager = MemoryManager::new(hot.clone(), failing_engram.clone());

    let fact = Fact {
        id: "rollback-fact".to_string(),
        category: "facts".to_string(),
        content: "fact should not remain in hot layer after rollback".to_string(),
        importance: 0.7,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        verified: false,
        source: Some("test".to_string()),
        confidence: 0.5,
        relations: Vec::new(),
        semantic_hash: Some("rollback-fact".to_string()),
        status: FactStatus::Pending,
        protection: FactProtection::Normal,
    };

    let err = manager
        .store_fact("user", None, fact)
        .await
        .expect_err("engram failure should bubble");
    assert!(
        err.to_string().contains("rolled back"),
        "error should make rollback semantics explicit"
    );
    assert!(
        hot.retrieve_facts("user", None)
            .await
            .expect("hot retrieve should succeed")
            .into_iter()
            .all(|fact| fact.id != "rollback-fact"),
        "hot layer should be restored when durable fact write fails"
    );
}

#[tokio::test]
async fn memory_manager_rolls_back_hot_fact_importance_when_engram_update_fails() {
    let hot: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
    let failing_engram = Arc::new(FailingMemory::new());
    let manager = MemoryManager::new(hot.clone(), failing_engram.clone());

    let fact = Fact {
        id: "rollback-fact-importance".to_string(),
        category: "facts".to_string(),
        content: "importance should stay at the previous value after rollback".to_string(),
        importance: 0.2,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        verified: false,
        source: Some("test".to_string()),
        confidence: 0.5,
        relations: Vec::new(),
        semantic_hash: Some("rollback-fact-importance".to_string()),
        status: FactStatus::Pending,
        protection: FactProtection::Normal,
    };

    hot.store_fact("user", None, fact.clone())
        .await
        .expect("hot fact stored");
    failing_engram
        .store_fact("user", None, fact)
        .await
        .expect("engram fact stored");
    failing_engram.fail_fact_importance_update();

    let err = manager
        .update_fact_importance("user", None, "rollback-fact-importance", 0.95)
        .await
        .expect_err("engram failure should bubble");
    assert!(
        err.to_string().contains("rolled back"),
        "error should make rollback semantics explicit"
    );

    let fact = hot
        .retrieve_facts("user", None)
        .await
        .expect("hot retrieve should succeed")
        .into_iter()
        .find(|fact| fact.id == "rollback-fact-importance")
        .expect("hot fact should still exist");
    assert!((fact.importance - 0.2).abs() < f32::EPSILON);
}

#[tokio::test]
async fn short_term_memory_mark_cancelled_appends_cancel_marker() {
    let dir = tempdir().expect("tempdir");
    let memory = ShortTermMemory::new(8, 8, dir.path().join("stm.redb")).await;

    memory
        .mark_cancelled("user", None, "user requested stop")
        .await
        .expect("cancel marker should store");

    let history = memory.retrieve("user", None, 10).await;
    let last = history.last().expect("cancel marker should exist");
    assert!(matches!(
        last.role,
        benshu_brain::agent::message::Role::Assistant
    ));
    assert!(
        last.text().contains("user requested stop"),
        "cancel reason should be visible in stored marker"
    );
}

#[tokio::test]
async fn learned_memory_injector_uses_recent_context_window_and_skips_low_signal_queries() {
    let memory: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
    memory
        .store_knowledge(
            "user",
            None,
            "Sky Fact",
            "Earlier context about weather patterns.\nLet's keep thinking about atmospheric optics.\nCan you remind me why the sky is blue during the day?",
            "science",
            true,
        )
        .await
        .expect("knowledge should store");

    let injector = LearnedMemoryInjector::with_limits(memory.clone(), 3, 3);
    let history = vec![
        Message::system("always obey system rules"),
        Message::user("Earlier context about weather patterns."),
        Message::assistant("Let's keep thinking about atmospheric optics."),
        Message::user("Can you remind me why the sky is blue during the day?"),
    ];

    let injected = injector
        .inject(&history)
        .await
        .expect("inject should succeed");
    assert_eq!(
        injected.len(),
        1,
        "high-signal query should inject one summary"
    );
    assert!(
        injected[0].text().contains("Sky Fact"),
        "the injected summary should reference retrieved knowledge"
    );

    let low_signal = vec![Message::user("hi")];
    let skipped = injector
        .inject(&low_signal)
        .await
        .expect("inject should handle low-signal query");
    assert!(
        skipped.is_empty(),
        "short low-signal prompts should not cause blind retrieval injection"
    );
}

#[tokio::test]
async fn evolution_manager_fork_isolates_runtime_bindings() {
    let base_dir = tempdir().expect("tempdir").path().to_path_buf();
    let provider: Arc<dyn Provider> = Arc::new(SequenceMockProvider::new(vec![]));
    let auditor = Arc::new(Auditor::new(provider, "test-model".to_string()));

    let manager = Arc::new(EvolutionManager::new(auditor, base_dir));
    let forked = manager.fork();

    let memory_a = Arc::new(InMemoryMemory::new());
    let memory_b = Arc::new(InMemoryMemory::new());

    manager
        .try_set_memory(memory_a.clone())
        .expect("first binding should succeed");
    forked
        .try_set_memory(memory_b.clone())
        .expect("fork should own isolated binding state");
    assert!(
        manager.try_set_memory(memory_b).is_err(),
        "rebinding the original manager to a different memory must fail"
    );
}

#[tokio::test]
async fn evolution_worker_exits_on_shutdown() {
    let base_dir = tempdir().expect("tempdir").path().to_path_buf();
    let provider: Arc<dyn Provider> = Arc::new(SequenceMockProvider::new(vec![]));
    let auditor = Arc::new(Auditor::new(provider, "test-model".to_string()));

    let manager = Arc::new(EvolutionManager::new(auditor, base_dir));
    manager.start_worker();
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    assert!(manager.is_worker_running());

    manager.shutdown_worker().await;
    assert!(!manager.is_worker_running());
}

#[tokio::test(start_paused = true)]
async fn evolution_task_is_requeued_under_low_throttle() {
    let dir = tempdir().expect("tempdir");
    let base_dir = dir.path().to_path_buf();
    let exp_json = r#"{
        "problem_description": "slow parser",
        "successful_path": ["step_A", "step_B"],
        "key_parameters": [],
        "lessons_learned": [],
        "anti_patterns": [],
        "timestamp": "2026-03-20T10:00:00Z"
    }"#;

    let provider: Arc<dyn Provider> =
        Arc::new(SequenceMockProvider::new(vec![MockStreamBuilder::new()
            .message(exp_json)
            .done()
            .build()]));
    let auditor = Arc::new(Auditor::new(provider, "test-model".to_string()));
    let manager = Arc::new(EvolutionManager::new(auditor, base_dir.clone()));
    let memory = Arc::new(InMemoryMemory::new());
    memory
        .store_experience(serde_json::json!({
            "id": "exp_parser",
            "problem_description": "slow parser",
            "utility_score": 1.0,
            "last_updated_at": 0
        }))
        .await
        .expect("seed experience");
    manager
        .try_set_memory(memory.clone())
        .expect("memory binding should succeed");
    manager
        .try_set_sensor(Arc::new(tokio::sync::Mutex::new(SequencedSensor::new([
            ThrottleLevel::Low,
            ThrottleLevel::High,
        ]))))
        .expect("sensor binding should succeed");

    manager.start_worker();
    let mut message = benshu_brain::Message::user("optimize parser");
    message.used_experience_ids = vec!["exp_parser".to_string()];
    manager.enqueue_learning(
        vec![message],
        benshu_brain::agent::protocol::ChatOutcome {
            response: "done".to_string(),
            thoughts: Vec::new(),
            tool_calls: Vec::new(),
            metabolic_stats: None,
            ownership: benshu_brain::agent::protocol::TaskOwnership::direct(
                benshu_infra::agent::AgentRole::Custom("benshu".to_string()),
                None,
            ),
            delegation: None,
            handover: None,
            runtime_task: None,
            run_trace: None,
        },
        "agent-test".to_string(),
    );

    tokio::task::yield_now().await;
    tokio::time::advance(std::time::Duration::from_secs(6)).await;
    tokio::task::yield_now().await;

    for _ in 0..5 {
        if memory
            .get_experience("exp_parser")
            .await
            .expect("experience lookup should succeed")
            .and_then(|value| value["last_updated_at"].as_u64())
            .unwrap_or_default()
            > 0
        {
            break;
        }
        tokio::time::advance(std::time::Duration::from_millis(10)).await;
        tokio::task::yield_now().await;
    }

    manager.shutdown_worker().await;

    let persisted = memory
        .get_experience("exp_parser")
        .await
        .expect("experience lookup should succeed")
        .expect("requeued task should eventually persist experience");
    assert!(
        persisted["last_updated_at"].as_u64().unwrap_or_default() > 0,
        "expected delayed evolution task to be processed after throttle lifted"
    );
}

#[tokio::test]
async fn explicit_risk_context_upgrades_auto_tools_to_approval() {
    let provider: Arc<dyn Provider> = Arc::new(SequenceMockProvider::new(vec![]));

    let agent = Agent::builder(provider)
        .name("risk-guard")
        .with_security(Arc::new(MockSecurityHandler))
        .with_tool(EchoTool)
        .with_tool_policy(benshu_brain::agent::protocol::RiskyToolPolicy {
            default_policy: ToolPolicy::Auto,
            overrides: std::collections::HashMap::new(),
        })
        .with_approval_handler(Arc::new(RejectAllApprovalHandler))
        .with_initial_risk_score(0.95)
        .build()
        .expect("agent should build");

    let err = agent
        .call_tool("echo_tool", r#"{"value":"hello"}"#)
        .await
        .expect_err("high-risk context should force approval and reject execution");

    assert!(
        err.to_string().contains("Approval required") || err.to_string().contains("approval"),
        "unexpected error: {}",
        err
    );
}

#[tokio::test]
async fn inherited_governance_context_preserves_boundary_state() {
    let workspace = std::path::PathBuf::from("/tmp/benshu-governance");
    let policy = RiskyToolPolicy {
        default_policy: ToolPolicy::RequiresApproval,
        overrides: std::collections::HashMap::from([("echo_tool".to_string(), ToolPolicy::Auto)]),
    };
    let parent_provider: Arc<dyn Provider> = Arc::new(SequenceMockProvider::new(vec![]));
    let parent = Agent::builder(parent_provider)
        .name("governance-parent")
        .with_security(Arc::new(MockSecurityHandler))
        .with_tool_policy(policy.clone())
        .with_trusted_workspaces(vec![workspace.clone()])
        .with_token_budget(321)
        .with_initial_risk_score(0.88)
        .build()
        .expect("parent should build");

    let child = Agent::builder(parent.provider().as_ref().clone())
        .name("governance-child")
        .with_governance_context(parent.governance_context().inherit_full())
        .build()
        .expect("child should build");

    let child_governance = child.governance_context();
    assert_eq!(
        child_governance.tool_policy().default_policy,
        ToolPolicy::RequiresApproval
    );
    assert_eq!(
        child_governance
            .tool_policy()
            .overrides
            .get("echo_tool")
            .cloned(),
        Some(ToolPolicy::Auto)
    );
    assert_eq!(child_governance.trusted_workspaces(), &[workspace]);
    assert_eq!(child_governance.token_budget(), Some(321));
    assert!((child_governance.risk_score() - 0.88).abs() < f32::EPSILON);
}

#[tokio::test]
async fn preemptive_chat_cancels_prior_foreground_task_and_merges_context() {
    let provider = RecordingPreemptiveProvider::new();
    let provider_for_asserts = provider.clone();
    let provider: Arc<dyn Provider> = Arc::new(provider);
    let memory = Arc::new(InMemoryMemory::new());

    let agent = Agent::builder(provider)
        .name("preemptive-guard")
        .with_security(Arc::new(MockSecurityHandler))
        .with_memory(memory)
        .with_session_id("session-preemptive")
        .build()
        .expect("agent should build");

    let first_agent = agent.clone();
    let first_task = tokio::spawn(async move {
        first_agent
            .chat_simple("first question", Some("session-preemptive".to_string()))
            .await
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let second_response = agent
        .chat_simple("second question", Some("session-preemptive".to_string()))
        .await
        .expect("second request should complete after preemption");
    assert_eq!(second_response, "second");

    let first_result = first_task.await.expect("first task should join");
    let first_error = first_result.expect_err("first task should be cancelled by hot interjection");
    let first_error_text = first_error.to_string().to_lowercase();
    assert!(
        first_error_text.contains("preempt") || first_error_text.contains("cancel"),
        "first request should be cancelled by the second foreground request"
    );

    let recorded = provider_for_asserts.recorded_requests().await;
    assert!(
        recorded.len() >= 2,
        "expected at least two foreground requests, got {}",
        recorded.len()
    );

    let merged_request = recorded.iter().find(|request| {
        let messages = &request.messages;
        messages.iter().any(|message| {
            message
                .content
                .as_text()
                .contains(benshu_brain::agent::core::MARKER_INTERJECTION)
        }) && messages
            .iter()
            .any(|message| message.text().contains("first question"))
            && messages
                .iter()
                .any(|message| message.text().contains("second question"))
    });

    assert!(
        merged_request.is_some(),
        "expected at least one merged preemptive request carrying the interjection marker and both user turns"
    );
}

#[tokio::test]
async fn foreground_preemption_is_scoped_to_session_id() {
    let provider = RecordingPreemptiveProvider::new();
    let provider_for_asserts = provider.clone();
    let provider: Arc<dyn Provider> = Arc::new(provider);
    let memory = Arc::new(InMemoryMemory::new());

    let agent = Agent::builder(provider)
        .name("preemptive-scope")
        .with_security(Arc::new(MockSecurityHandler))
        .with_memory(memory)
        .build()
        .expect("agent should build");

    let first_agent = agent.clone();
    let first_task = tokio::spawn(async move {
        first_agent
            .chat_simple("first question", Some("session-a".to_string()))
            .await
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let second_response = agent
        .chat_simple("second question", Some("session-b".to_string()))
        .await
        .expect("second request on a different session should complete");
    assert_eq!(second_response, "second");

    let first_response = first_task
        .await
        .expect("first task should join")
        .expect("different-session request should not be preempted");
    assert_eq!(first_response, "first");

    let recorded = provider_for_asserts.recorded_requests().await;
    assert_eq!(
        recorded.len(),
        2,
        "expected exactly two foreground requests"
    );
    assert!(
        recorded.iter().all(|request| {
            !request.messages.iter().any(|message| {
                message
                    .content
                    .as_text()
                    .contains(benshu_brain::agent::core::MARKER_INTERJECTION)
            })
        }),
        "different-session requests should not be merged as a hot interjection"
    );
}

#[tokio::test]
async fn comm_receive_loop_records_owner_rollup_and_inbox_metadata() {
    let env = CommTestEnv::new();
    let comm_client = env.create_client("benshu");
    let sender = env.create_client("researcher");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let memory = Arc::new(MetadataMemory::default());
    let provider: Arc<dyn Provider> = Arc::new(SequenceMockProvider::new(vec![]));

    let agent = Agent::builder(provider)
        .name("benshu")
        .with_security(Arc::new(MockSecurityHandler))
        .with_memory(memory.clone())
        .with_comm_client(comm_client)
        .build()
        .expect("agent should build");

    let delegation = DelegationEnvelope {
        session_id: Some("session-a2a".to_string()),
        visible_owner_id: "benshu".to_string(),
        memory_owner_id: "benshu".to_string(),
        approval_owner_id: "benshu".to_string(),
        final_response_owner_id: "benshu".to_string(),
        delegated_by_id: "benshu".to_string(),
        delegated_to_id: "researcher".to_string(),
        return_mode: DelegationReturnMode::ReturnToOwner,
        trace_id: Some("trace-a2a-1".to_string()),
        task_id: Some("task-a2a-1".to_string()),
        parent_task_id: Some("parent-a2a-1".to_string()),
        root_task_id: Some("root-a2a-1".to_string()),
        state: benshu_comm::protocol::a2a::DelegationState::Returned,
    };

    let message = A2AMessage::Result {
        request_id: "req-a2a-1".to_string(),
        performer_id: "researcher".to_string(),
        output: "specialist finished".to_string(),
        success: true,
        delegation: Some(delegation),
    };
    let payload = serde_json::to_vec(&message).expect("payload should serialize");
    sender
        .send_msg(Address::Agent("benshu".to_string()), payload)
        .await
        .expect("message should send");

    agent.poll_comm_once().await.expect("poll should succeed");

    let owner_rollup = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if let Some(value) = memory
                .get_metadata("brain.comm.benshu.owner_rollup.last_json")
                .await
                .expect("metadata should load")
            {
                break value;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("owner rollup metadata should be written");

    assert!(
        owner_rollup.contains("\"req-a2a-1\"") && owner_rollup.contains("\"researcher\""),
        "unexpected owner rollup payload: {}",
        owner_rollup
    );

    let inbox = memory
        .get_metadata("brain.comm.benshu.inbox.recent_json")
        .await
        .expect("inbox metadata should load")
        .expect("inbox metadata should exist");
    assert!(
        inbox.contains("\"result\"") && inbox.contains("\"req-a2a-1\""),
        "unexpected inbox payload: {}",
        inbox
    );
}

#[tokio::test]
async fn comm_receive_loop_emits_a2a_processed_events() {
    let env = CommTestEnv::new();
    let comm_client = env.create_client("benshu");
    let sender = env.create_client("researcher");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let provider: Arc<dyn Provider> = Arc::new(SequenceMockProvider::new(vec![]));
    let agent = Agent::builder(provider)
        .name("benshu")
        .with_security(Arc::new(MockSecurityHandler))
        .with_memory(Arc::new(MetadataMemory::default()))
        .with_comm_client(comm_client)
        .build()
        .expect("agent should build");

    let mut events = agent.events();

    let message = A2AMessage::TaskRequest {
        request_id: "req-a2a-2".to_string(),
        requester_id: "researcher".to_string(),
        task_content: "inspect logs".to_string(),
        required_capabilities: vec!["analysis".to_string()],
        delegation: Some(DelegationEnvelope {
            session_id: Some("session-a2a".to_string()),
            visible_owner_id: "benshu".to_string(),
            memory_owner_id: "benshu".to_string(),
            approval_owner_id: "benshu".to_string(),
            final_response_owner_id: "benshu".to_string(),
            delegated_by_id: "benshu".to_string(),
            delegated_to_id: "researcher".to_string(),
            return_mode: DelegationReturnMode::ReturnToOwner,
            trace_id: Some("trace-a2a-2".to_string()),
            task_id: Some("task-a2a-2".to_string()),
            parent_task_id: Some("parent-a2a-2".to_string()),
            root_task_id: Some("root-a2a-2".to_string()),
            state: benshu_comm::protocol::a2a::DelegationState::Created,
        }),
    };
    sender
        .send_msg(
            Address::Agent("benshu".to_string()),
            serde_json::to_vec(&message).expect("payload should serialize"),
        )
        .await
        .expect("message should send");

    agent.poll_comm_once().await.expect("poll should succeed");

    let event = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let event = events.recv().await.expect("event should arrive");
            if let benshu_brain::agent::AgentEventData::A2aEnvelopeProcessed {
                kind,
                request_id,
                runtime_profile,
                root_task_id,
                visible_owner,
                memory_owner,
                approval_owner,
                delegated_to,
                ..
            } = event.data
            {
                break (
                    kind,
                    request_id,
                    runtime_profile,
                    root_task_id,
                    visible_owner,
                    memory_owner,
                    approval_owner,
                    delegated_to,
                );
            }
        }
    })
    .await
    .expect("a2a processed event should be emitted");

    assert_eq!(event.0, "task_request");
    assert_eq!(event.1.as_deref(), Some("req-a2a-2"));
    assert_eq!(event.2, "embedded");
    assert_eq!(event.3.as_deref(), Some("root-a2a-2"));
    assert_eq!(event.4.as_deref(), Some("benshu"));
    assert_eq!(event.5.as_deref(), Some("benshu"));
    assert_eq!(event.6.as_deref(), Some("benshu"));
    assert_eq!(event.7.as_deref(), Some("researcher"));
}
