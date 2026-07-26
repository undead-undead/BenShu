use super::core::Agent;
use crate::agent::message::Message;
use crate::agent::protocol::*;
use crate::agent::provider::Provider;
use crate::error::Result;
use crate::hooks::{HookEvent, HookResult};
use benshu_infra::traits::resource::ResourceSensor;
use std::sync::Arc;

#[async_trait::async_trait]
impl<P: Provider + 'static> AgentLiaison for Agent<P> {
    async fn prepare_for_step(
        &self,
        messages: &mut Vec<Message>,
        steps: usize,
    ) -> Result<Option<ChatOutcome>> {
        Agent::prepare_for_step(self, messages, steps).await
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
        Agent::finalize_outcome(
            self, messages, full_text, usage, thoughts, tool_trace, steps,
        )
        .await
    }

    async fn run_runtime_hook(&self, event: HookEvent) -> Result<HookResult> {
        let mut runtime_event = self.build_runtime_hook_event(event.timing);
        runtime_event.user_input = event.user_input;
        runtime_event.llm_response = event.llm_response;
        runtime_event.tool_name = event.tool_name;
        runtime_event.tool_args = event.tool_args;
        runtime_event.tool_result = event.tool_result;
        runtime_event.error = event.error;
        runtime_event.metadata.extend(event.metadata);

        Ok(self.hook_engine.fire(&runtime_event).await)
    }

    fn emit(&self, data: AgentEventData) {
        Agent::emit(self, data);
    }

    fn cancel_token(&self) -> tokio_util::sync::CancellationToken {
        self.current_task_token.read().clone()
    }

    fn suggest_resource_throttle(&self) -> benshu_infra::resource::ThrottleLevel {
        if let Some(sensor_lock) = &self.sensor {
            let mut sensor = sensor_lock.write();
            sensor.suggest_throttle_level(Some(self.config.metabolic_threshold))
        } else {
            benshu_infra::resource::ThrottleLevel::High
        }
    }

    fn current_metabolic_pressure(&self) -> MetabolicStats {
        Agent::current_metabolic_pressure(self)
    }

    fn context_manager(&self) -> &crate::agent::context::ContextManager {
        &self.context_manager
    }

    fn current_background_envelope(&self) -> Option<crate::agent::memory::BackgroundEnvelope> {
        self.background_envelope.read().clone()
    }

    fn executor(&self) -> crate::agent::executor::ActionExecutor {
        Agent::build_executor(self)
    }

    fn intervention(&self) -> crate::agent::intervention::InterventionManager {
        Agent::build_intervention_manager(self)
    }

    fn sensory_hub(&self) -> Option<Arc<dyn SensoryLiaison>> {
        self.sensory_hub.clone()
    }

    fn evolution_manager(
        &self,
    ) -> Option<Arc<crate::agent::evolution::evolution_manager::EvolutionManager>> {
        self.evolution_manager.clone()
    }

    fn agent_id(&self) -> Option<String> {
        self.session_id.clone()
    }

    fn token_budget(&self) -> Option<u32> {
        self.config.token_budget
    }

    fn register_token_usage(
        &self,
        usage: &TokenUsage,
    ) -> crate::agent::governance::GovernanceBudgetSnapshot {
        self.governance.register_token_usage(usage)
    }

    fn register_tool_invocation(&self) -> crate::agent::governance::GovernanceBudgetSnapshot {
        self.governance.register_tool_call()
    }

    fn governance_budget_snapshot(&self) -> crate::agent::governance::GovernanceBudgetSnapshot {
        self.governance.budget_snapshot()
    }

    fn current_risk_score(&self) -> f32 {
        self.current_governance_risk_score()
    }

    fn current_model_override(&self) -> Option<String> {
        None
    }

    fn cumulative_usage(&self) -> TokenUsage {
        self.cumulative_usage.read().clone()
    }
}
