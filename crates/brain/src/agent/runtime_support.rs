use super::core::Agent;
use crate::agent::message::Message;
use crate::agent::protocol::*;
use crate::agent::provider::Provider;
use crate::error::Result;
use crate::hooks::{HookEvent, HookResult};
use benshu_telemetry::{RuntimeStage, TraceStatus};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::sync::{Mutex, Notify};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Clone, Default)]
pub struct PauseController {
    paused: Arc<AtomicBool>,
    notify: Arc<Notify>,
    queued_inputs: Arc<Mutex<Vec<String>>>,
}

impl PauseController {
    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    pub async fn queue_input(&self, input: impl Into<String>) {
        let input = input.into();
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return;
        }
        self.queued_inputs.lock().await.push(trimmed.to_string());
    }

    pub async fn wait_if_paused(&self, cancel_token: &CancellationToken) -> Result<Vec<String>> {
        while self.is_paused() {
            tokio::select! {
                _ = self.notify.notified() => {},
                _ = cancel_token.cancelled() => {
                    return Err(crate::error::Error::agent_config("Task cancelled by user"));
                }
            }
        }
        let mut queued = self.queued_inputs.lock().await;
        Ok(std::mem::take(&mut *queued))
    }
}

/// Represents a handle to an active reasoning task that can be preempted.
pub struct TaskHandle {
    pub task_id: String,
    pub cancel_token: CancellationToken,
    pub pause_controller: PauseController,
    pub join_handle: tokio::task::JoinHandle<()>,
}

pub(crate) type ActiveForegroundTasks = HashMap<String, TaskHandle>;

#[derive(Debug, Clone)]
pub(crate) struct RuntimeExecutionSeed {
    pub(crate) task_id: Uuid,
    pub(crate) run_id: Uuid,
    pub(crate) started_at: DateTime<Utc>,
    pub(crate) session_id: Option<String>,
    pub(crate) thread_id: String,
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimeStageSignal {
    pub(crate) stage: RuntimeStage,
    pub(crate) status: TraceStatus,
    pub(crate) at: DateTime<Utc>,
    pub(crate) detail: Option<String>,
}

/// A wrapper bridge that overrides the cancellation token for a specific task.
pub struct PreemptiveBridge<'a, P: Provider + 'static> {
    pub inner: &'a Agent<P>,
    pub task_cancel: CancellationToken,
    pub task_pause: PauseController,
}

#[async_trait::async_trait]
impl<'a, P: Provider + 'static> AgentLiaison for PreemptiveBridge<'a, P> {
    async fn prepare_for_step(
        &self,
        messages: &mut Vec<Message>,
        steps: usize,
    ) -> Result<Option<ChatOutcome>> {
        self.inner.prepare_for_step(messages, steps).await
    }

    async fn finalize_outcome(
        &self,
        messages: &[Message],
        full_text: String,
        usage: Option<TokenUsage>,
        thoughts: Vec<String>,
        tool_trace: Vec<ToolCallData>,
        steps: usize,
    ) -> Result<ChatOutcome> {
        self.inner
            .finalize_outcome(messages, full_text, usage, thoughts, tool_trace, steps)
            .await
    }

    async fn run_runtime_hook(&self, event: HookEvent) -> Result<HookResult> {
        self.inner.run_runtime_hook(event).await
    }

    fn suggest_resource_throttle(&self) -> crate::skills::ThrottleLevel {
        self.inner.suggest_resource_throttle()
    }

    fn current_metabolic_pressure(&self) -> MetabolicStats {
        self.inner.current_metabolic_pressure()
    }

    fn evolution_manager(
        &self,
    ) -> Option<Arc<crate::agent::evolution::evolution_manager::EvolutionManager>> {
        self.inner.evolution_manager().clone()
    }

    fn agent_id(&self) -> Option<String> {
        self.inner.agent_id()
    }

    fn emit(&self, data: AgentEventData) {
        self.inner.emit(data);
    }

    fn cancel_token(&self) -> CancellationToken {
        self.task_cancel.clone()
    }

    async fn wait_if_paused(&self) -> Result<Vec<String>> {
        self.task_pause.wait_if_paused(&self.task_cancel).await
    }

    fn context_manager(&self) -> &crate::agent::context::ContextManager {
        self.inner.context_manager()
    }

    fn current_background_envelope(&self) -> Option<crate::agent::memory::BackgroundEnvelope> {
        self.inner.current_background_envelope()
    }

    fn executor(&self) -> crate::agent::executor::ActionExecutor {
        self.inner
            .executor_with_cancel_token(self.task_cancel.clone())
    }

    fn intervention(&self) -> crate::agent::intervention::InterventionManager {
        self.inner.build_intervention_manager()
    }

    fn sensory_hub(&self) -> Option<Arc<dyn SensoryLiaison>> {
        self.inner.sensory_hub()
    }

    fn token_budget(&self) -> Option<u32> {
        self.inner.token_budget()
    }

    fn register_token_usage(
        &self,
        usage: &TokenUsage,
    ) -> crate::agent::governance::GovernanceBudgetSnapshot {
        self.inner.register_token_usage(usage)
    }

    fn register_tool_invocation(&self) -> crate::agent::governance::GovernanceBudgetSnapshot {
        self.inner.register_tool_invocation()
    }

    fn governance_budget_snapshot(&self) -> crate::agent::governance::GovernanceBudgetSnapshot {
        self.inner.governance_budget_snapshot()
    }

    fn current_risk_score(&self) -> f32 {
        self.inner.current_governance_risk_score()
    }

    fn current_model_override(&self) -> Option<String> {
        None
    }

    fn cumulative_usage(&self) -> TokenUsage {
        self.inner.cumulative_usage()
    }
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct AskUserArgs {
    /// The question to ask the user
    pub(crate) question: String,
}

pub(crate) struct AskUserTool {
    handler: Arc<dyn InteractionHandler>,
}

impl AskUserTool {
    pub fn new(handler: Arc<dyn InteractionHandler>) -> Self {
        Self { handler }
    }
}

#[async_trait::async_trait]
impl crate::skills::tool::Tool for AskUserTool {
    fn name(&self) -> String {
        "ask_user".to_string()
    }

    async fn definition(&self) -> crate::skills::tool::ToolDefinition {
        let gen = schemars::gen::SchemaSettings::openapi3().into_generator();
        let schema = gen.into_root_schema_for::<AskUserArgs>();
        let schema_json = serde_json::to_value(schema).unwrap_or_default();

        crate::skills::tool::ToolDefinition {
            name: "ask_user".to_string(),
            description: "Ask the user for clarification, additional information, or a final decision. Use this when you are stuck or need human input.".to_string(),
            parameters: schema_json,
            parameters_ts: Some("interface AskUserArgs {\n  /** The question to ask the user */\n  question: string;\n}".to_string()),
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some("Use this only when you need critical missing information or explicit permission to proceed with a dangerous action (e.g., executing a trade). Avoid asking for obvious or non-essential details.".to_string()),
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let args: AskUserArgs = serde_json::from_str(arguments)?;
        self.handler.ask(&args.question).await
    }
}
