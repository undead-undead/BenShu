use crate::agent::agent_identity::AgentIdentity;
use crate::agent::governance::GovernanceBudgetSnapshot;
pub use crate::agent::message::Message;
use crate::error::{Error, Result};
use crate::hooks::{HookEvent, HookResult};
use async_trait::async_trait;
use benshu_state::TaskState;
use benshu_telemetry::RunTrace;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub use benshu_comm::protocol::Address;
pub use benshu_infra::agent::{
    AgentEvent, AgentEventData, AgentMessage, AgentRole, MessageType, MetabolicStats, SafetyLevel,
    TokenUsage,
};
pub use benshu_protocol_core::{DelegationMode, DelegationRecord, TaskOwnership};
pub use benshu_runtime_policy_core::{
    constants, ExecutorConfig, InterventionConfig, ReasonerConfig, ReasoningStrategy,
    RiskyToolPolicy, ToolPolicy,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallData {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    pub name: String,
    pub args: String,
    pub result: Option<String>,
    pub backup: Option<crate::skills::BackupInfo>,
    pub duration_ms: u64,
    pub timestamp: u64,
    pub caller_id: Option<String>,
    pub safety_level: SafetyLevel,
    #[serde(default)]
    pub cpu_pressure: Option<f32>,
    #[serde(default)]
    pub vram_pressure: Option<f32>,
    #[serde(default)]
    pub result_truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_original_chars: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_omitted_chars: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<ToolOutcomeMeta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay: Option<ToolCallReplayReceipt>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutcomeMeta {
    pub status: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_chars: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_artifact_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_count: Option<usize>,
    #[serde(default)]
    pub progress_signal: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallReplayReceipt {
    pub tool_call_id: String,
    pub replay_mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sampled_call_block: Option<String>,
    pub sampled_call_fingerprint: String,
    pub sampled_call_ref: String,
    pub normalized_call_fingerprint: String,
}
pub use benshu_infra::traits::agent::{ApprovalHandler, InteractionHandler};

/// Configuration for an Agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub name: String,
    pub model: String,
    pub preamble: String,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u64>,
    pub extra_params: Option<serde_json::Value>,
    pub tool_policy: RiskyToolPolicy,
    pub max_history_messages: usize,
    pub max_tool_output_chars: usize,
    pub json_mode: bool,
    pub agent_identity: Option<AgentIdentity>,
    pub role: AgentRole,
    pub max_parallel_tools: usize,
    pub loop_similarity_threshold: f64,
    pub sop: Option<String>,
    pub enable_cache_control: bool,
    pub smart_pruning: bool,
    pub agent_path: Option<std::path::PathBuf>,
    pub efficiency_trigger_secs: u64,
    pub status_recap_threshold_steps: usize,
    pub status_recap_threshold_chars: usize,
    pub default_throttle: benshu_infra::resource::ThrottleLevel,
    pub enable_reflexion: bool,
    pub max_reflexion_retries: usize,
    pub enable_meta_cognition: bool,
    pub default_max_steps: usize,
    pub trusted_workspaces: Vec<std::path::PathBuf>,
    pub response_reserve: usize,
    pub tool_execution_timeout: std::time::Duration,
    pub status_recap_prompt: Option<String>,
    pub reflexion_prompt: Option<String>,
    pub llm_timeout: std::time::Duration,
    pub token_budget: Option<u32>,
    pub jit_distillation_model: Option<String>,
    /// Phase 15.3: JIT Token Budget (Cost & Resource control)
    pub jit_token_budget: Option<u32>,
    /// Phase 15.3: Metabolic Throttling Threshold (0.0 to 100.0)
    pub metabolic_threshold: f32,
    /// Phase 15.3: Enable JIT Distillation
    pub enable_jit_distillation: bool,
    /// Configurable low-complexity model overrides keyed by a substring match on the primary model.
    #[serde(default)]
    pub auto_stepdown_targets: std::collections::BTreeMap<String, String>,
    /// Structured retry budget for background envelope persistence.
    pub background_persistence_retry_count: usize,
    /// Delay between background persistence retries.
    pub background_persistence_retry_backoff_ms: u64,
    /// Limit the number of recent JIT facts scanned for dedupe/cooldown.
    pub jit_fact_dedupe_limit: usize,
    /// Cooldown before another JIT fact with the same source is allowed.
    pub jit_fact_cooldown_secs: u64,
    /// Runtime threshold that forces the cheaper reasoning path under heavy VRAM pressure.
    pub vram_react_stepdown_threshold: f32,
    /// Tunable background relationship fact importance.
    pub background_relationship_fact_importance: f32,
    /// Tunable background relationship fact confidence.
    pub background_relationship_fact_confidence: f32,
    /// Phase 15.3: Memory Observability Filter Level
    pub memory_event_level: benshu_infra::traits::memory::EventLevel,
    /// Phase 18: Maximum concurrent background tasks
    pub max_background_tasks: usize,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            name: "agent".to_string(),
            model: "benshu-unconfigured-model".to_string(),
            preamble: "You are a helpful AI assistant.".to_string(),
            temperature: Some(0.7),
            max_tokens: Some(128000),
            extra_params: None,
            tool_policy: RiskyToolPolicy::default(),
            max_history_messages: constants::DEFAULT_MAX_HISTORY_MESSAGES,
            max_tool_output_chars: constants::DEFAULT_MAX_TOOL_OUTPUT_CHARS,
            json_mode: false,
            agent_identity: None,
            role: AgentRole::Custom("benshu".to_string()),
            max_parallel_tools: constants::DEFAULT_MAX_PARALLEL_TOOLS,
            loop_similarity_threshold: constants::DEFAULT_LOOP_SIMILARITY_THRESHOLD,
            sop: None,
            enable_cache_control: false,
            smart_pruning: false,
            agent_path: None,
            status_recap_threshold_steps: constants::DEFAULT_STATUS_RECAP_THRESHOLD_STEPS,
            status_recap_threshold_chars: constants::DEFAULT_STATUS_RECAP_THRESHOLD_CHARS,
            default_throttle: benshu_infra::resource::ThrottleLevel::Medium,
            enable_reflexion: true,
            max_reflexion_retries: constants::DEFAULT_MAX_REFLEXION_RETRIES,
            enable_meta_cognition: false,
            default_max_steps: constants::DEFAULT_MAX_STEPS,
            trusted_workspaces: Vec::new(),
            response_reserve: constants::DEFAULT_RESPONSE_RESERVE,
            tool_execution_timeout: std::time::Duration::from_secs(
                constants::DEFAULT_TOOL_EXECUTION_TIMEOUT_SECS,
            ),
            status_recap_prompt: None,
            reflexion_prompt: None,
            efficiency_trigger_secs: 0,
            llm_timeout: std::time::Duration::from_secs(60),
            token_budget: None,
            enable_jit_distillation: true,
            jit_distillation_model: None,
            auto_stepdown_targets: std::collections::BTreeMap::from([
                (
                    "benshu-unconfigured-model".to_string(),
                    "benshu-unconfigured-model".to_string(),
                ),
                (
                    "claude-3-5-sonnet".to_string(),
                    "claude-3-haiku-20240307".to_string(),
                ),
            ]),
            background_persistence_retry_count: 2,
            background_persistence_retry_backoff_ms: 150,
            jit_fact_dedupe_limit: 12,
            jit_fact_cooldown_secs: 1800,
            vram_react_stepdown_threshold: 90.0,
            background_relationship_fact_importance: 0.86,
            background_relationship_fact_confidence: 0.72,
            jit_token_budget: Some(100000), // Default budget (e.g. 100k tokens per session)
            metabolic_threshold: 85.0,      // 85% pressure triggers Low Throttle
            memory_event_level: benshu_infra::traits::memory::EventLevel::Info,
            max_background_tasks: 32,
        }
    }
}

impl AgentConfig {
    pub fn validate(&self) -> Result<()> {
        if self.model.is_empty() {
            return Err(Error::AgentConfig("model name cannot be empty".to_string()));
        }
        if self.max_history_messages == 0 {
            return Err(Error::AgentConfig(
                "max_history_messages must be at least 1".to_string(),
            ));
        }
        if self.max_parallel_tools == 0 {
            return Err(Error::AgentConfig(
                "max_parallel_tools must be at least 1".to_string(),
            ));
        }
        Ok(())
    }

    pub fn from_file(path: &std::path::Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| Error::AgentConfig(format!("Failed to read config file: {}", e)))?;
        let config: Self = serde_json::from_str(&content)
            .map_err(|e| Error::AgentConfig(format!("Failed to parse config JSON: {}", e)))?;
        config.validate()?;
        Ok(config)
    }

    pub fn update(&mut self, new: Self) -> Result<()> {
        self.temperature = new.temperature;
        self.max_tokens = new.max_tokens;
        self.tool_policy = new.tool_policy;
        self.max_parallel_tools = new.max_parallel_tools;
        self.loop_similarity_threshold = new.loop_similarity_threshold;
        self.max_tool_output_chars = new.max_tool_output_chars;
        self.trusted_workspaces = new.trusted_workspaces;
        self.validate()?;
        Ok(())
    }

    pub fn trader_template() -> Self {
        let mut config = Self::default();
        config.name = "trader_agent".to_string();
        config.role = AgentRole::Trader;
        config.temperature = Some(0.1);
        config.tool_policy = RiskyToolPolicy {
            default_policy: ToolPolicy::RequiresApproval,
            overrides: std::collections::HashMap::from_iter([
                ("query_market".to_string(), ToolPolicy::Auto),
                ("execute_trade".to_string(), ToolPolicy::RequiresApproval),
            ]),
        };
        config.tool_execution_timeout = std::time::Duration::from_secs(30);
        config
    }

    pub fn researcher_template() -> Self {
        let mut config = Self::default();
        config.name = "research_agent".to_string();
        config.role = AgentRole::Researcher;
        config.temperature = Some(0.4);
        config.max_tool_output_chars = 16384;
        config.tool_policy.default_policy = ToolPolicy::Auto;
        config.tool_execution_timeout = std::time::Duration::from_secs(180);
        config
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ToolExecutionError {
    #[error("Tool not found: {0}")]
    NotFound(String),
    #[error("Policy disabled: {0}")]
    PolicyDisabled(String),
    #[error("Approval required: {0}")]
    ApprovalRequired(String),
    #[error("Security violation: {0}")]
    SecurityViolation(String),
    #[error("Execution failed: {0}: {1}")]
    ExecutionFailed(String, String),
    #[error("Loop detected: {0}")]
    LoopDetected(String),
}

#[derive(Debug, thiserror::Error)]
pub enum ReasoningError {
    #[error("LLM stream cancelled by user")]
    Cancelled,
    #[error("Provider error: {0}")]
    ProviderError(String),
    #[error("Invalid tool call format: {0}")]
    InvalidToolCall(String),
    #[error("No response from LLM")]
    NoResponse,
    #[error("Tool '{0}' not found")]
    ToolNotFound(String),
    #[error("Tool call validation failed: {0}")]
    ToolValidationFailed(String),
    #[error("Token limit exceeded for single step")]
    TokenLimitExceeded,
}

#[derive(Debug, thiserror::Error)]
pub enum InterventionError {
    #[error("Failed to calculate message character count: {0}")]
    CharCountError(String),
    #[error("Intervention injection failed: {0}")]
    InjectionError(String),
}

pub struct RejectAllApprovalHandler;

#[async_trait]
impl ApprovalHandler for RejectAllApprovalHandler {
    async fn approve_with_timeout(
        &self,
        _tool: &str,
        _args: &str,
        _safety: SafetyLevel,
        _timeout: std::time::Duration,
    ) -> anyhow::Result<bool> {
        Ok(false)
    }
}

pub struct DefaultApprovalHandler {
    pub policy: RiskyToolPolicy,
}

impl DefaultApprovalHandler {
    pub fn new(policy: RiskyToolPolicy) -> Self {
        Self { policy }
    }
}

#[async_trait]
impl ApprovalHandler for DefaultApprovalHandler {
    async fn approve_with_timeout(
        &self,
        _tool: &str,
        _args: &str,
        safety: SafetyLevel,
        _timeout: std::time::Duration,
    ) -> anyhow::Result<bool> {
        if safety == SafetyLevel::Green {
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

#[derive(Debug)]
pub struct ApprovalRequest {
    pub id: String,
    pub tool_name: String,
    pub arguments: String,
    pub responder: tokio::sync::oneshot::Sender<bool>,
}

pub struct ChannelApprovalHandler {
    pub sender: tokio::sync::mpsc::Sender<ApprovalRequest>,
}

#[async_trait]
impl ApprovalHandler for ChannelApprovalHandler {
    async fn approve_with_timeout(
        &self,
        tool_name: &str,
        arguments: &str,
        _safety: SafetyLevel,
        timeout: std::time::Duration,
    ) -> anyhow::Result<bool> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let request = ApprovalRequest {
            id: uuid::Uuid::new_v4().to_string(),
            tool_name: tool_name.to_string(),
            arguments: arguments.to_string(),
            responder: tx,
        };
        self.sender
            .send(request)
            .await
            .map_err(|_| anyhow::anyhow!("Channel closed"))?;
        let approved = tokio::time::timeout(timeout, rx).await??;
        Ok(approved)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatOutcome {
    pub response: String,
    pub thoughts: Vec<String>,
    pub tool_calls: Vec<ToolCallData>,
    pub metabolic_stats: Option<MetabolicStats>,
    pub ownership: TaskOwnership,
    pub delegation: Option<DelegationRecord>,
    pub handover: Option<AgentRole>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_task: Option<TaskState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_trace: Option<RunTrace>,
}

#[async_trait::async_trait]
pub trait AgentLiaison: Send + Sync {
    async fn prepare_for_step(
        &self,
        messages: &mut Vec<Message>,
        steps: usize,
    ) -> Result<Option<ChatOutcome>>;
    async fn finalize_outcome(
        &self,
        messages: &[Message],
        full_text: String,
        usage: Option<TokenUsage>,
        thoughts: Vec<String>,
        tool_trace: Vec<ToolCallData>,
        steps: usize,
    ) -> Result<ChatOutcome>;
    async fn run_runtime_hook(&self, event: HookEvent) -> Result<HookResult>;
    fn suggest_resource_throttle(&self) -> benshu_infra::resource::ThrottleLevel;
    fn current_metabolic_pressure(&self) -> MetabolicStats;
    fn evolution_manager(
        &self,
    ) -> Option<Arc<crate::agent::evolution::evolution_manager::EvolutionManager>>;
    fn agent_id(&self) -> Option<String>;
    fn emit(&self, data: AgentEventData);
    fn cancel_token(&self) -> tokio_util::sync::CancellationToken;
    async fn wait_if_paused(&self) -> Result<Vec<String>> {
        Ok(Vec::new())
    }
    fn context_manager(&self) -> &crate::agent::context::ContextManager;
    fn current_background_envelope(&self) -> Option<crate::agent::memory::BackgroundEnvelope>;
    fn executor(&self) -> crate::agent::executor::ActionExecutor;
    fn intervention(&self) -> crate::agent::intervention::InterventionManager;
    fn sensory_hub(&self) -> Option<Arc<dyn SensoryLiaison>>;
    fn token_budget(&self) -> Option<u32>;
    fn register_token_usage(&self, usage: &TokenUsage) -> GovernanceBudgetSnapshot;
    fn register_tool_invocation(&self) -> GovernanceBudgetSnapshot;
    fn governance_budget_snapshot(&self) -> GovernanceBudgetSnapshot;
    fn current_risk_score(&self) -> f32;
    fn current_model_override(&self) -> Option<String>;
    fn cumulative_usage(&self) -> TokenUsage;
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use serde_json::json;
    use uuid::Uuid;

    #[test]
    fn chat_outcome_round_trip_preserves_runtime_refs() {
        let mut task = TaskState::new(
            "foreground_chat",
            "runtime task",
            json!({"entry": "chat"}),
            "agent",
        );
        let run_id = Uuid::new_v4();
        task.run_id = Some(run_id);
        task.trace_id = Some(run_id);
        task.session_id = Some("session-1".to_string());

        let trace = RunTrace {
            run_id,
            session_id: Uuid::nil(),
            agent_id: "agent".to_string(),
            status: benshu_telemetry::TraceStatus::Succeeded,
            started_at: Utc::now(),
            finished_at: Some(Utc::now()),
            task_id: Some(task.id),
            thread_id: None,
            provider: None,
            model: Some("demo-model".to_string()),
            prompt_tokens: None,
            completion_tokens: None,
            stages: Vec::new(),
            tools: Vec::new(),
            artifacts: Vec::new(),
            degradation_notes: Vec::new(),
            witness: None,
            metadata: std::collections::HashMap::new(),
        };

        let outcome = ChatOutcome {
            response: "ok".to_string(),
            thoughts: vec!["done".to_string()],
            tool_calls: Vec::new(),
            metabolic_stats: None,
            ownership: TaskOwnership::direct(AgentRole::Custom("benshu".to_string()), None),
            delegation: None,
            handover: None,
            runtime_task: Some(task),
            run_trace: Some(trace),
        };

        let encoded = serde_json::to_string(&outcome).expect("serialize outcome");
        let decoded: ChatOutcome = serde_json::from_str(&encoded).expect("deserialize outcome");

        assert!(decoded.runtime_task.is_some());
        assert!(decoded.run_trace.is_some());
        assert_eq!(
            decoded
                .runtime_task
                .as_ref()
                .and_then(|task| task.session_id.as_deref()),
            Some("session-1")
        );
    }
}

pub use benshu_infra::traits::SensoryLiaison;
