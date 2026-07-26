mod complexity;
mod execution;
mod failure;
mod finalization;
mod intervention;
mod media;
mod model;
mod preflight;
mod recovery;
mod reflexion;
mod strategy;
mod task_complexity;

pub use complexity::{ComplexityEstimator, SemanticComplexityAnalyzer};
pub use execution::{decide_execution_tool_reply_requirement, ExecutionToolReplyRequirementInput};
pub use failure::{
    classify_failure, retry_allows_reflexion_upgrade, should_enqueue_failure_analysis,
    should_trigger_error_reflexion, FailureAnalysisInput, FailureClass,
};
pub use finalization::{
    decide_finalization_fallback, FinalizationFallbackInput, FinalizationFallbackKind,
};
pub use intervention::{
    decide_interventions, InterventionDecision, InterventionGateInput, StatusRecapReason,
};
pub use media::{
    is_frontstage_single_image_turn, is_simple_media_understanding_turn,
    strip_frontstage_media_injection,
};
pub use model::{ComplexityScore, MediaKind, MessageSnapshot};
pub use preflight::{
    classify_extended_pre_flight_level, extended_pre_flight_allows_auto_stepdown,
    extended_pre_flight_runs_complexity_estimator, extended_pre_flight_runs_jit_distillation,
    is_lightweight_repo_inspection_request, should_run_extended_pre_flight_for_turn,
    ExtendedPreFlightLevel, PreFlightRouteClass,
};
pub use recovery::{
    decide_completion_gate, decide_lookup_evidence_recovery, decide_tool_first_recovery,
    CompletionGateDecision, CompletionGateSignal, EvidenceQuality, LookupEvidenceRecoveryInput,
    RecoveryAction, ToolFirstRecoveryInput,
};
pub use reflexion::{
    decide_reflexion_strategy_upgrade, extract_reflexion_critique_reason,
    should_run_reflexion_review, ReflexionReviewInput, ReflexionUpgradeDecision,
    ReflexionUpgradeInput, ReflexionUpgradeReason,
};
pub use strategy::{
    decide_initial_reasoning_strategy, is_explicit_image_generation_first_attempt,
    should_append_reflexion_recovery_prompt, InitialReasoningStrategy,
    InitialReasoningStrategyInput,
};
pub use task_complexity::{sanitize_task_complexity, TaskComplexity};

#[cfg(test)]
mod tests;
