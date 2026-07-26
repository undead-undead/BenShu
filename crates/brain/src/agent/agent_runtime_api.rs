use super::core::Agent;
use crate::agent::executor::ActionExecutor;
use crate::agent::governance::GovernanceContext;
use crate::agent::memory::Memory;
use crate::agent::protocol::*;
use crate::agent::provider::Provider;
use crate::error::Result;
use crate::notification::NotifyChannel;
use crate::security::SecurityHandler;
use crate::skills::tool::ToolSet;
use benshu_infra::traits::resource::ResourceSensor;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use tracing::instrument;

impl<P: Provider> Agent<P> {
    /// Create a tool executor for this agent
    pub(crate) fn build_executor(&self) -> ActionExecutor {
        self.executor_with_cancel_token(self.current_task_token())
    }

    pub(crate) fn executor_with_cancel_token(
        &self,
        cancel_token: CancellationToken,
    ) -> ActionExecutor {
        let exec_config = crate::agent::protocol::ExecutorConfig {
            tool_policy: self.governance.tool_policy().clone(),
            inherited_risk_score: self.governance.risk_score(),
            max_parallel_tools: self.config.max_parallel_tools,
            loop_similarity_threshold: self.config.loop_similarity_threshold,
            max_tool_output_chars: self.config.max_tool_output_chars,
            enable_reflexion: self.config.enable_reflexion,
            default_throttle: self.config.default_throttle,
            trusted_workspaces: self.governance.trusted_workspaces().to_vec(),
            tool_execution_timeout: self.config.tool_execution_timeout,
        };

        ActionExecutor::new(
            exec_config,
            self.tools.clone(),
            self.events.clone(),
            self.governance.clone(),
            self.evolution_manager.clone(),
            self.session_id.clone(),
            Arc::new(parking_lot::RwLock::new(cancel_token)),
            self.seen_tools.clone(),
            self.memory.clone(),
            self.background_envelope.clone(),
            self.sensor.clone(),
            self.build_intervention_manager(),
            self.metrics.clone(),
            self.hook_engine.clone(),
            self.runtime_hook_refs.clone(),
            self.runtime_hook_capture.clone(),
        )
        .expect("ActionExecutor configuration is invalid")
    }

    /// Create an intervention manager for this agent
    pub(crate) fn build_intervention_manager(
        &self,
    ) -> crate::agent::intervention::InterventionManager {
        let config = crate::agent::protocol::InterventionConfig {
            status_recap_threshold_steps: self.config.status_recap_threshold_steps,
            status_recap_threshold_chars: self.config.status_recap_threshold_chars,
            enable_reflexion: self.config.enable_reflexion,
            max_reflexion_retries: self.config.max_reflexion_retries,
            name: self.config.name.clone(),
            status_recap_prompt: self.config.status_recap_prompt.clone(),
            reflexion_prompt: self.config.reflexion_prompt.clone(),
        };
        crate::agent::intervention::InterventionManager::new(
            config,
            self.events.clone(),
            self.session_id.clone(),
        )
    }

    pub fn provider(&self) -> Arc<P> {
        self.provider.clone()
    }

    pub fn sensory_hub(&self) -> Option<Arc<dyn crate::agent::protocol::SensoryLiaison>> {
        self.sensory_hub.clone()
    }

    pub fn tools(&self) -> &ToolSet {
        &self.tools
    }

    pub fn memory(&self) -> &Option<Arc<dyn Memory>> {
        &self.memory
    }

    pub fn security(&self) -> &Arc<dyn SecurityHandler> {
        &self.security
    }

    pub fn approval_handler(&self) -> Arc<dyn ApprovalHandler> {
        self.governance.approval_handler()
    }

    pub fn evolution_manager(
        &self,
    ) -> &Option<Arc<crate::agent::evolution::evolution_manager::EvolutionManager>> {
        &self.evolution_manager
    }

    pub fn lifecycle_token(&self) -> CancellationToken {
        self.lifecycle_token.read().clone()
    }

    pub fn current_task_token(&self) -> CancellationToken {
        self.current_task_token.read().clone()
    }

    pub fn current_governance_risk_score(&self) -> f32 {
        self.governance.risk_score()
    }

    pub(crate) fn current_metabolic_pressure(&self) -> MetabolicStats {
        if let Some(sensor_lock) = &self.sensor {
            let resources = sensor_lock.write().check_resources(false);
            MetabolicStats {
                cpu_usage: resources.cpu_usage,
                vram_pressure: resources.vram_pressure_pct(),
                mem_pressure: 100.0 - resources.free_memory_pct,
                is_throttled: matches!(
                    self.suggest_resource_throttle(),
                    benshu_infra::resource::ThrottleLevel::Low
                        | benshu_infra::resource::ThrottleLevel::Medium
                ),
                token_usage: Some(self.cumulative_usage().clone()),
            }
        } else {
            MetabolicStats::default()
        }
    }

    pub fn register_token_usage(
        &self,
        usage: &TokenUsage,
    ) -> crate::agent::governance::GovernanceBudgetSnapshot {
        self.governance.register_token_usage(usage)
    }

    pub fn register_tool_invocation(&self) -> crate::agent::governance::GovernanceBudgetSnapshot {
        self.governance.register_tool_call()
    }

    pub fn governance_budget_snapshot(&self) -> crate::agent::governance::GovernanceBudgetSnapshot {
        self.governance.budget_snapshot()
    }

    pub fn governance_context(&self) -> Arc<GovernanceContext> {
        self.governance.clone()
    }

    pub fn add_tool<T: crate::skills::tool::Tool + 'static>(&self, tool: T) {
        self.tools.add(tool);
    }

    pub fn add_shared_tool(&self, tool: Arc<dyn crate::skills::tool::Tool>) {
        self.tools.add_shared(tool);
    }

    /// Subscribe to agent events
    pub fn subscribe(&self) -> broadcast::Receiver<AgentEvent> {
        self.events.subscribe()
    }

    /// Helper to emit events
    pub fn emit(&self, data: AgentEventData) {
        let event = AgentEvent {
            session_id: self.session_id.clone(),
            data,
        };

        if let Some(registry) = &self.metrics {
            match &event.data {
                AgentEventData::StepStart { .. } => {
                    registry.counter_inc(&format!("{}:steps_total", self.config.name), 1);
                }
                AgentEventData::Error { .. } => {
                    registry.counter_inc(&format!("{}:errors_total", self.config.name), 1);
                }
                AgentEventData::TokenUsage { usage } => {
                    registry.counter_inc(
                        &format!("{}:tokens_prompt_total", self.config.name),
                        usage.prompt_tokens as u64,
                    );
                    registry.counter_inc(
                        &format!("{}:tokens_completion_total", self.config.name),
                        usage.total_tokens as u64,
                    );

                    let mut cumulative = self.cumulative_usage.write();
                    cumulative.prompt_tokens += usage.prompt_tokens;
                    cumulative.completion_tokens += usage.completion_tokens;
                    cumulative.total_tokens += usage.total_tokens;
                }
                AgentEventData::Thinking { .. } => {
                    registry.counter_inc(&format!("{}:thinking_starts_total", self.config.name), 1);
                }
                AgentEventData::Thought { .. } => {
                    registry.counter_inc(&format!("{}:thoughts_total", self.config.name), 1);
                }
                AgentEventData::ToolExecutionEnd {
                    duration_ms,
                    success,
                    ..
                } => {
                    registry.counter_inc(&format!("{}:tool_calls_total", self.config.name), 1);
                    if !*success {
                        registry.counter_inc(&format!("{}:tool_errors_total", self.config.name), 1);
                    }
                    registry.histogram_observe(
                        &format!("{}:tool_duration_ms", self.config.name),
                        *duration_ms as f64,
                    );
                }
                _ => {}
            }
        }

        if let Err(e) = self.events.send(event) {
            tracing::debug!("Failed to emit event (no receivers): {}", e);
        }
    }

    pub async fn notify(&self, channel: NotifyChannel, message: &str) -> Result<()> {
        if let Some(notifier) = &self.notifier {
            notifier.notify(channel, message).await?;
            Ok(())
        } else {
            tracing::warn!(
                "Agent tried to notify but no notifier is configured: {}",
                message
            );
            Ok(())
        }
    }

    pub async fn current_vram_usage(&self) -> u64 {
        if let Some(hub) = &self.sensory_hub {
            if let Ok(usage) = hub.get_hardware_utilization().await {
                return usage.vram_used_mb;
            }
        }

        if let Some(sensor_lock) = &self.sensor {
            let mut sensor = sensor_lock.write();
            let resources = sensor.check_resources(false);
            (resources.vram_pressure_pct() * 40.96) as u64
        } else {
            0
        }
    }

    #[instrument(skip(self, arguments), fields(tool_name = %name))]
    pub async fn call_tool(&self, name: &str, arguments: &str) -> Result<String> {
        self.build_executor().execute_single(name, arguments).await
    }

    pub fn has_tool(&self, name: &str) -> bool {
        self.tools.contains(name)
    }

    pub async fn tool_definitions(&self) -> Vec<crate::skills::tool::ToolDefinition> {
        self.tools.definitions().await
    }

    pub fn config(&self) -> &AgentConfig {
        &self.config
    }

    pub fn config_mut(&mut self) -> &mut AgentConfig {
        &mut self.config
    }

    pub fn model(&self) -> &str {
        &self.config.model
    }
}
