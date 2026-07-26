use crate::failure::{should_trigger_error_reflexion, FailureClass};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InitialReasoningStrategy {
    ReAct,
    Reflexion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitialReasoningStrategyInput {
    pub force_react_due_to_resource_pressure: bool,
    pub throttled_by_metabolic_guard: bool,
    pub reflexion_enabled: bool,
    pub explicit_image_generation_turn: bool,
    pub light_frontstage_turn: bool,
    pub has_media_input: bool,
}

pub fn is_explicit_image_generation_first_attempt(
    has_media_input: bool,
    retry_count: usize,
    requests_image_generation: bool,
) -> bool {
    !has_media_input && retry_count == 0 && requests_image_generation
}

pub fn should_append_reflexion_recovery_prompt(
    reflexion_enabled: bool,
    requires_execution_tool_reply: bool,
    failure_classification: FailureClass,
) -> bool {
    reflexion_enabled
        && !requires_execution_tool_reply
        && should_trigger_error_reflexion(failure_classification)
}

pub fn decide_initial_reasoning_strategy(
    input: InitialReasoningStrategyInput,
) -> InitialReasoningStrategy {
    if input.force_react_due_to_resource_pressure
        || input.explicit_image_generation_turn
        || input.light_frontstage_turn
        || input.has_media_input
    {
        return InitialReasoningStrategy::ReAct;
    }

    if input.throttled_by_metabolic_guard {
        return if input.reflexion_enabled {
            InitialReasoningStrategy::Reflexion
        } else {
            InitialReasoningStrategy::ReAct
        };
    }

    if input.reflexion_enabled {
        return InitialReasoningStrategy::Reflexion;
    }

    InitialReasoningStrategy::ReAct
}
