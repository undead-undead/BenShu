use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolFirstRecoveryInput {
    pub current_step: usize,
    pub max_steps: usize,
    pub available_tool_count: usize,
    pub has_recent_tool_execution_required_prompt: bool,
    pub simple_media_understanding: bool,
}

pub fn decide_tool_first_recovery(input: ToolFirstRecoveryInput) -> bool {
    input.current_step < input.max_steps
        && input.available_tool_count > 0
        && !input.has_recent_tool_execution_required_prompt
        && !input.simple_media_understanding
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceQuality {
    Empty,
    Irrelevant,
    UntrustedSource,
    MissingConcreteSource,
    BlockedByAccess,
    LowInformation,
    Partial,
    Sufficient,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryAction {
    RetrySameSurfaceSmallBudget,
    SwitchObservationSurface,
    DelegateSpecialist,
    EmitBlocker,
    FinalizeFromEvidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LookupEvidenceRecoveryInput {
    pub current_step: usize,
    pub max_steps: usize,
    pub evidence_quality: EvidenceQuality,
    pub has_search_tool: bool,
    pub search_attempts: usize,
    pub has_observation_tool: bool,
    pub observation_already_attempted: bool,
    pub has_delegate_tool: bool,
    pub specialist_already_attempted: bool,
    pub required_persistence: bool,
}

pub fn decide_lookup_evidence_recovery(input: LookupEvidenceRecoveryInput) -> RecoveryAction {
    if matches!(input.evidence_quality, EvidenceQuality::Sufficient) {
        return RecoveryAction::FinalizeFromEvidence;
    }

    let has_step_budget = input.current_step < input.max_steps;
    if !has_step_budget {
        return RecoveryAction::EmitBlocker;
    }

    let needs_grounded_recovery = matches!(
        input.evidence_quality,
        EvidenceQuality::Empty
            | EvidenceQuality::Irrelevant
            | EvidenceQuality::UntrustedSource
            | EvidenceQuality::MissingConcreteSource
            | EvidenceQuality::BlockedByAccess
            | EvidenceQuality::LowInformation
            | EvidenceQuality::Partial
    );

    if needs_grounded_recovery && input.has_observation_tool && !input.observation_already_attempted
    {
        return RecoveryAction::SwitchObservationSurface;
    }

    if input.has_search_tool
        && input.search_attempts < 2
        && matches!(
            input.evidence_quality,
            EvidenceQuality::Empty | EvidenceQuality::Irrelevant
        )
    {
        return RecoveryAction::RetrySameSurfaceSmallBudget;
    }

    if input.has_delegate_tool
        && !input.specialist_already_attempted
        && (input.required_persistence
            || matches!(
                input.evidence_quality,
                EvidenceQuality::MissingConcreteSource
                    | EvidenceQuality::BlockedByAccess
                    | EvidenceQuality::LowInformation
                    | EvidenceQuality::Partial
            ))
    {
        return RecoveryAction::DelegateSpecialist;
    }

    RecoveryAction::EmitBlocker
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionGateSignal {
    Complete,
    MissingRequiredEffect,
    QualityFailed,
    RuntimeBlocker,
    Uncertain,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionGateDecision {
    Pass,
    Fail,
    Blocked,
    Uncertain,
    Skip,
}

pub fn decide_completion_gate(signal: CompletionGateSignal) -> CompletionGateDecision {
    match signal {
        CompletionGateSignal::Complete => CompletionGateDecision::Pass,
        CompletionGateSignal::MissingRequiredEffect | CompletionGateSignal::QualityFailed => {
            CompletionGateDecision::Fail
        }
        CompletionGateSignal::RuntimeBlocker => CompletionGateDecision::Blocked,
        CompletionGateSignal::Uncertain => CompletionGateDecision::Uncertain,
        CompletionGateSignal::Skipped => CompletionGateDecision::Skip,
    }
}
