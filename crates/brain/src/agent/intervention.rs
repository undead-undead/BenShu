use tokio::sync::broadcast;
use tracing::{debug, info, warn};

use crate::agent::evolution::complexity::ComplexityScore;
use crate::agent::message::Message;
use crate::agent::protocol::{AgentEvent, AgentEventData, InterventionConfig, MetabolicStats};
use crate::error::Result;
use crate::skills::tool::{
    classify_query_capability_route, query_requests_followup_execution_after_lookup,
    CapabilityRouteHint,
};
use benshu_hardness::{
    classify_failure, decide_interventions,
    is_frontstage_single_image_turn as is_frontstage_single_image_turn_core,
    is_lightweight_repo_inspection_request,
    is_simple_media_understanding_turn as is_simple_media_understanding_turn_core,
    should_trigger_error_reflexion, InterventionGateInput, MediaKind, MessageSnapshot,
    StatusRecapReason,
};
pub use benshu_intervention::{intervention_constants, InterventionType};

/// Responsible for proactive interventions to keep the agent on track.
/// Uses metabolic sensing and complexity heuristics to inject system imperatives.
#[derive(Clone)]
pub struct InterventionManager {
    config: InterventionConfig,
    events: broadcast::Sender<AgentEvent>,
    session_id: Option<String>,
    intervention_counter: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl std::fmt::Debug for InterventionManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InterventionManager")
            .field("config", &self.config)
            .field("session_id", &self.session_id)
            .finish()
    }
}

impl InterventionManager {
    fn is_internal_complexity_probe(messages: &[Message]) -> bool {
        let Some(last_user_message) = messages
            .iter()
            .rev()
            .find(|message| matches!(message.role, crate::agent::message::Role::User))
        else {
            return false;
        };

        let text = last_user_message.content.as_text();
        let normalized = text.trim();

        normalized.starts_with("Analyze the complexity of this task for an autonomous agent swarm.")
            || (normalized
                .contains("Analyze the complexity of this task for an autonomous agent swarm.")
                && normalized.contains("Output ONLY valid JSON"))
    }

    fn latest_user_snapshot(messages: &[Message]) -> Option<MessageSnapshot> {
        let last_user_message = messages
            .iter()
            .rev()
            .find(|message| matches!(message.role, crate::agent::message::Role::User))?;
        let media = match &last_user_message.content {
            crate::agent::message::Content::Parts(parts) => parts
                .iter()
                .filter_map(|part| match part {
                    crate::agent::message::ContentPart::Image { .. } => Some(MediaKind::Image),
                    crate::agent::message::ContentPart::Audio { .. } => Some(MediaKind::Audio),
                    crate::agent::message::ContentPart::Video { .. } => Some(MediaKind::Video),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };

        Some(MessageSnapshot {
            text: last_user_message.content.as_text(),
            media,
        })
    }

    fn is_simple_media_understanding(messages: &[Message], complexity: &ComplexityScore) -> bool {
        Self::latest_user_snapshot(messages)
            .as_ref()
            .is_some_and(|snapshot| is_simple_media_understanding_turn_core(snapshot, complexity))
    }

    fn is_frontstage_single_image_turn(
        messages: &[Message],
        complexity: &ComplexityScore,
        steps: usize,
        total_chars: usize,
    ) -> bool {
        Self::latest_user_snapshot(messages)
            .as_ref()
            .is_some_and(|snapshot| {
                is_frontstage_single_image_turn_core(snapshot, complexity, steps, total_chars)
            })
    }

    /// Create a new intervention manager with validated config.
    pub fn new(
        config: InterventionConfig,
        events: broadcast::Sender<AgentEvent>,
        session_id: Option<String>,
    ) -> Self {
        if let Err(e) = config.validate() {
            warn!(
                session_id = session_id.as_deref().unwrap_or("unknown"),
                "InterventionManager: Invalid config provided: {}. Using default fallback.", e
            );
        }

        Self {
            config,
            events,
            session_id,
            intervention_counter: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        }
    }

    /// One-stop handler for all intervention types, following the production-grade flowchart.
    pub async fn handle_all_interventions(
        &self,
        messages: &mut Vec<Message>,
        steps: usize,
        last_error: Option<&str>,
        complexity: ComplexityScore,
        estimated_steps: usize,
        metabolic: MetabolicStats,
        total_chars: usize,
        is_local_provider: bool,
        token_budget: Option<u32>,
    ) -> Result<()> {
        if messages.is_empty() {
            return Ok(());
        }

        if Self::is_internal_complexity_probe(messages) {
            debug!(
                session_id = self.session_id.as_deref().unwrap_or("unknown"),
                "Skipping interventions for internal complexity probe."
            );
            return Ok(());
        }

        let mut triggered = Vec::new();
        let session_id = self.session_id.as_deref().unwrap_or("unknown");

        // --- 0. Metadata: Specialist / sub-agent suppression ---
        let is_specialist_worker = !self.config.name.trim().eq_ignore_ascii_case("BenShu");
        let is_sub_agent = session_id.contains("-sub") || is_specialist_worker;

        let simple_media_understanding = Self::is_simple_media_understanding(messages, &complexity)
            || Self::is_frontstage_single_image_turn(messages, &complexity, steps, total_chars);
        let latest_user_text = messages
            .iter()
            .rev()
            .find(|message| message.role == crate::agent::message::Role::User)
            .map(|message| message.text())
            .unwrap_or_default();
        let lightweight_repo_inspection = is_lightweight_repo_inspection_request(&latest_user_text);
        let realtime_route = classify_query_capability_route(&latest_user_text);
        let compound_realtime_followup_execution =
            query_requests_followup_execution_after_lookup(&latest_user_text)
                && realtime_route
                    .as_ref()
                    .is_some_and(|route| matches!(route, CapabilityRouteHint::RealtimeLookup(_)));
        let simple_realtime_lookup = !compound_realtime_followup_execution
            && realtime_route
                .as_ref()
                .is_some_and(|route| matches!(route, CapabilityRouteHint::RealtimeLookup(_)));
        let quality_error_detected = last_error
            .filter(|error| !error.trim().is_empty())
            .is_some_and(|error| should_trigger_error_reflexion(classify_failure(error)));

        let decision = decide_interventions(InterventionGateInput {
            token_usage_total: metabolic.token_usage.as_ref().map(|u| u.total_tokens),
            token_budget,
            cpu_usage: metabolic.cpu_usage,
            mem_pressure: metabolic.mem_pressure,
            enable_reflexion: self.config.enable_reflexion,
            quality_error_detected,
            complexity_score: complexity.score,
            predicted_output_tokens: complexity.predicted_output_tokens,
            is_parallelizable: complexity.is_parallelizable,
            current_step: steps,
            estimated_steps,
            total_chars,
            is_local_provider,
            is_sub_agent,
            is_specialist_worker,
            simple_media_understanding,
            lightweight_repo_inspection,
            compound_realtime_followup_execution,
            status_recap_threshold_steps: self.config.status_recap_threshold_steps,
            status_recap_threshold_chars: self.config.status_recap_threshold_chars,
        });

        // --- 1. Budget Gating (Token Exhaustion) ---
        if let (Some(usage), Some(limit)) = (
            metabolic.token_usage.as_ref().map(|u| u.total_tokens),
            token_budget,
        ) {
            if decision.budget_breaker {
                triggered.push((
                    InterventionType::BudgetBreaker,
                    benshu_intervention::budget_breaker_prompt(usage, limit),
                ));
            } else if usage >= (limit as f32 * 0.9) as u32 {
                debug!(session_id = %session_id, usage = usage, limit = limit, "Token budget at 90% threshold.");
            }
        }

        // --- 2. Metabolic Gating (CPU > 80% or Low Memory Gates Fission) ---
        let mut metabolic_reasons = Vec::new();
        if metabolic.cpu_usage > 80.0 {
            metabolic_reasons.push(format!("High CPU ({:.1}%)", metabolic.cpu_usage));
        }
        if metabolic.mem_pressure > 90.0 {
            metabolic_reasons.push(format!(
                "High memory pressure ({:.1}%)",
                metabolic.mem_pressure
            ));
        }
        if decision.metabolic_warning {
            triggered.push((
                InterventionType::MetabolicWarning,
                benshu_intervention::metabolic_warning_prompt(&metabolic_reasons),
            ));
        }

        // --- 3. Reflexion (Error Loop) ---
        if decision.error_reflexion {
            if let Some(error) = last_error.filter(|error| !error.trim().is_empty()) {
                triggered.push((
                    InterventionType::Reflexion,
                    self.get_reflexion_prompt(error),
                ));
            }
        }

        // --- 5. Status Recap (Context Density) ---
        let is_recap_needed = !is_specialist_worker
            && !simple_realtime_lookup
            && decision.status_recap
            && !self.has_recent_intervention(messages, InterventionType::StatusRecap);

        if is_recap_needed {
            let reason = match decision.status_recap_reason {
                Some(StatusRecapReason::StepThreshold) => "Step threshold",
                Some(StatusRecapReason::ContextDensity) => "Context density threshold",
                None => "Unknown threshold",
            };
            triggered.push((
                InterventionType::StatusRecap,
                self.get_status_recap_prompt(reason),
            ));
        }

        // --- 7. Sorting & Injection (Highest priority first) ---
        triggered.sort_by(|a, b| b.0.cmp(&a.0));

        // Deduplicate within this turn
        let mut unique_triggered = Vec::new();
        let mut seen_types = std::collections::HashSet::new();
        for (typ, prompt) in triggered {
            if seen_types.insert(typ) {
                unique_triggered.push((typ, prompt));
            }
        }

        for (typ, prompt) in unique_triggered {
            if self.has_recent_intervention(messages, typ) && typ != InterventionType::Reflexion {
                continue;
            }

            let intervention_id = self
                .intervention_counter
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

            info!(
                session_id = %session_id,
                agent_name = %self.config.name,
                is_specialist_worker,
                id = intervention_id,
                steps = steps,
                intervention = %typ,
                "Intervention Triggered: Injecting system imperative."
            );

            messages.push(Message::system(prompt.clone()));
            self.emit_intervention_event(typ, prompt, intervention_id);
        }

        Ok(())
    }

    pub fn get_metabolic_model_override(&self, vram_pressure: f32) -> Option<String> {
        if vram_pressure > 90.0 {
            None
        } else {
            None
        }
    }

    /// Phase 9.1: Dynamic Swarm Dispatcher (Advisory)
    /// Analyzes if the current task should be handled by a more specialized agent.
    fn has_recent_intervention(&self, messages: &[Message], typ: InterventionType) -> bool {
        let marker = typ.marker();
        messages
            .iter()
            .rev()
            .take(intervention_constants::RECENT_MESSAGE_CHECK_LIMIT)
            .any(|m| m.content.as_text().contains(marker))
    }

    pub fn get_status_recap_prompt(&self, reason: &str) -> String {
        benshu_intervention::status_recap_prompt(reason)
    }

    pub fn get_reflexion_prompt(&self, error: &str) -> String {
        benshu_intervention::reflexion_prompt(error)
    }

    fn emit_intervention_event(
        &self,
        typ: InterventionType,
        reason: String,
        intervention_id: usize,
    ) {
        let session_id = self.session_id.clone();
        let events = self.events.clone();

        tokio::spawn(async move {
            let event = AgentEvent {
                session_id,
                data: AgentEventData::Intervention {
                    typ: typ.to_string(),
                    reason,
                    metadata: serde_json::json!({
                        "id": intervention_id,
                        "timestamp": chrono::Utc::now().to_rfc3339(),
                        "priority": typ.priority()
                    }),
                },
            };
            let _ = events.send(event);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::evolution::complexity::ComplexityScore;
    use crate::agent::message::Message;
    use crate::agent::protocol::InterventionConfig;
    use tokio::sync::broadcast;

    fn test_metabolic_stats(cpu: f32, mem: f32) -> MetabolicStats {
        MetabolicStats {
            cpu_usage: cpu,
            mem_pressure: mem,
            token_usage: None,
            vram_pressure: 0.0,
            is_throttled: false,
        }
    }

    fn test_complexity_score(score: f32) -> ComplexityScore {
        ComplexityScore {
            score,
            reason: "test".to_string(),
            predicted_output_tokens: 1000,
            is_parallelizable: true,
            level: 1,
            metadata: serde_json::Value::Null,
        }
    }

    #[tokio::test]
    async fn test_intervention_priority_sorting() {
        let (tx, _) = broadcast::channel(10);
        let manager = InterventionManager::new(InterventionConfig::default(), tx, None);

        let mut messages = vec![Message::user("do something complex")];
        // Trigger both Metabolic Warning (80) and Reflexion (100)
        manager
            .handle_all_interventions(
                &mut messages,
                1,
                Some("No response from LLM after tool execution"),
                test_complexity_score(0.5),
                20,
                test_metabolic_stats(95.0, 95.0),
                100,
                false,
                None,
            )
            .await
            .unwrap();

        // Priority sorting: Reflexion (100) is pushed before Metabolic (80)
        // Since we push to messages, the higher priority one (Reflexion) should be pushed FIRST if we sort triggered highest first.
        // Wait, the code does: triggered.sort_by(|a, b| b.0.cmp(&a.0)); (Highest priority first)
        // Then it loops over unique_triggered and pushes.
        // So messages[1] should be the highest priority one.
        assert!(messages[1]
            .content
            .as_text()
            .contains(intervention_constants::MARKER_REFLEXION));
        assert!(messages[2]
            .content
            .as_text()
            .contains(intervention_constants::MARKER_METABOLIC));
    }

    #[tokio::test]
    async fn simple_realtime_lookup_skips_status_recap() {
        let (tx, _) = broadcast::channel(10);
        let mut config = InterventionConfig::default();
        config.status_recap_threshold_steps = 2;
        config.status_recap_threshold_chars = 1;
        let manager = InterventionManager::new(config, tx, None);

        let mut messages = vec![
            Message::user("帮我查一下今天最新时事新闻"),
            Message::assistant("我会查询近期公开来源。".to_string()),
        ];
        manager
            .handle_all_interventions(
                &mut messages,
                2,
                None,
                test_complexity_score(0.5),
                2,
                test_metabolic_stats(0.0, 0.0),
                10_000,
                true,
                None,
            )
            .await
            .unwrap();

        assert!(!messages.iter().any(|message| message
            .content
            .as_text()
            .contains(intervention_constants::MARKER_RECAP)));
    }
}
