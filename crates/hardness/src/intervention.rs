use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StatusRecapReason {
    StepThreshold,
    ContextDensity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct InterventionDecision {
    pub budget_breaker: bool,
    pub metabolic_warning: bool,
    pub error_reflexion: bool,
    pub status_recap: bool,
    pub status_recap_reason: Option<StatusRecapReason>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct InterventionGateInput {
    pub token_usage_total: Option<u32>,
    pub token_budget: Option<u32>,
    pub cpu_usage: f32,
    pub mem_pressure: f32,
    pub enable_reflexion: bool,
    pub quality_error_detected: bool,
    pub complexity_score: f32,
    pub predicted_output_tokens: usize,
    pub is_parallelizable: bool,
    pub current_step: usize,
    pub estimated_steps: usize,
    pub total_chars: usize,
    pub is_local_provider: bool,
    pub is_sub_agent: bool,
    pub is_specialist_worker: bool,
    pub simple_media_understanding: bool,
    pub lightweight_repo_inspection: bool,
    pub compound_realtime_followup_execution: bool,
    pub status_recap_threshold_steps: usize,
    pub status_recap_threshold_chars: usize,
}

pub fn decide_interventions(input: InterventionGateInput) -> InterventionDecision {
    let mut decision = InterventionDecision::default();
    let lightweight_frontstage_turn = input.simple_media_understanding
        || input.lightweight_repo_inspection
        || input.compound_realtime_followup_execution;

    if let (Some(usage), Some(limit)) = (input.token_usage_total, input.token_budget) {
        if usage >= limit {
            decision.budget_breaker = true;
        }
    }

    if input.cpu_usage > 80.0 || input.mem_pressure > 90.0 {
        decision.metabolic_warning = true;
    }

    if input.enable_reflexion && input.quality_error_detected && !lightweight_frontstage_turn {
        decision.error_reflexion = true;
    }

    let recap_by_steps = input.status_recap_threshold_steps > 0
        && input.current_step > 1
        && input.current_step % input.status_recap_threshold_steps == 0;
    let recap_by_chars =
        input.current_step > 1 && input.total_chars > input.status_recap_threshold_chars;

    if !lightweight_frontstage_turn
        && !input.is_specialist_worker
        && (recap_by_steps || recap_by_chars)
    {
        decision.status_recap = true;
        decision.status_recap_reason = Some(if recap_by_steps {
            StatusRecapReason::StepThreshold
        } else {
            StatusRecapReason::ContextDensity
        });
    }

    decision
}
