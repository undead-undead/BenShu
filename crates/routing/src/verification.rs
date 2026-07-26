use serde::{Deserialize, Serialize};

use crate::CapabilityRouteHint;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerificationDomain {
    KnowledgeFact,
    ToolFact,
    ExecutionFact,
    StateFact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerificationRequirement {
    Required,
    Recommended,
    LocalContextAllowed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerificationMode {
    None,
    LocalContextOnly,
    ToolInventoryCheck,
    RuntimeStateCheck,
    ExecutionResultCheck,
    ToolLookup,
    WebSearchFetch,
    BrowserValidation,
    RealtimeLookup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerificationOutcome {
    VerificationSucceeded,
    VerificationNotRequired,
    VerificationToolUnavailable,
    VerificationFetchFailed,
    VerificationSourceInsufficient,
    VerificationExecutionMissing,
    VerificationStateMissing,
    VerificationSkippedByPolicyGap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourcePosture {
    SourcesAttached,
    SourcesReferencedButNotAttached,
    ExecutionEvidenceAttached,
    StateEvidenceAttached,
    NoSourcesRequired,
    SourcesRequiredButMissing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TruthStatus {
    Verified,
    Unverified,
    Inferred,
    Uncertain,
    ClarificationRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryVerificationPlan {
    pub domain: VerificationDomain,
    pub requirement: VerificationRequirement,
    pub mode: VerificationMode,
    pub route_hint: Option<CapabilityRouteHint>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationSource {
    pub kind: String,
    pub title: String,
    pub uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationResultEnvelope {
    pub domain: VerificationDomain,
    pub requirement: VerificationRequirement,
    pub mode: VerificationMode,
    pub outcome: VerificationOutcome,
    pub truth_status: TruthStatus,
    pub source_posture: SourcePosture,
    pub sources: Vec<VerificationSource>,
    pub execution_evidence: Vec<String>,
    pub state_evidence: Vec<String>,
    pub notes: Vec<String>,
}

pub fn build_pending_verification_result_envelope(
    plan: QueryVerificationPlan,
    requires_source_fetch: bool,
    note: impl Into<String>,
) -> VerificationResultEnvelope {
    let source_posture = if requires_source_fetch {
        SourcePosture::SourcesRequiredButMissing
    } else {
        SourcePosture::NoSourcesRequired
    };

    let truth_status = match plan.requirement {
        VerificationRequirement::LocalContextAllowed => TruthStatus::Unverified,
        VerificationRequirement::Required | VerificationRequirement::Recommended => {
            TruthStatus::Uncertain
        }
    };

    VerificationResultEnvelope {
        domain: plan.domain,
        requirement: plan.requirement,
        mode: plan.mode,
        outcome: VerificationOutcome::VerificationSkippedByPolicyGap,
        truth_status,
        source_posture,
        sources: Vec::new(),
        execution_evidence: Vec::new(),
        state_evidence: Vec::new(),
        notes: vec![note.into()],
    }
}

pub fn build_verified_verification_result_envelope(
    domain: VerificationDomain,
    mode: VerificationMode,
    sources: Vec<VerificationSource>,
    note: impl Into<String>,
) -> VerificationResultEnvelope {
    build_observed_verification_result_envelope(domain, mode, sources, Vec::new(), Vec::new(), note)
}

pub fn build_observed_verification_result_envelope(
    domain: VerificationDomain,
    mode: VerificationMode,
    sources: Vec<VerificationSource>,
    execution_evidence: Vec<String>,
    state_evidence: Vec<String>,
    note: impl Into<String>,
) -> VerificationResultEnvelope {
    let has_observed_evidence =
        !sources.is_empty() || !execution_evidence.is_empty() || !state_evidence.is_empty();
    let source_posture = if !sources.is_empty() {
        SourcePosture::SourcesAttached
    } else if !execution_evidence.is_empty() {
        SourcePosture::ExecutionEvidenceAttached
    } else if !state_evidence.is_empty() {
        SourcePosture::StateEvidenceAttached
    } else {
        SourcePosture::SourcesRequiredButMissing
    };
    let truth_status = if has_observed_evidence {
        TruthStatus::Verified
    } else {
        TruthStatus::Uncertain
    };

    VerificationResultEnvelope {
        domain,
        requirement: VerificationRequirement::Required,
        mode,
        outcome: VerificationOutcome::VerificationSucceeded,
        truth_status,
        source_posture,
        sources,
        execution_evidence,
        state_evidence,
        notes: vec![note.into()],
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationFollowupPlan {
    pub answer_readiness: String,
    pub next_tools: Vec<String>,
    pub cite_required: bool,
    pub note: String,
}

pub fn build_pending_verification_followup_plan(
    mode: VerificationMode,
) -> VerificationFollowupPlan {
    VerificationFollowupPlan {
        answer_readiness: "verification_pending".to_string(),
        next_tools: Vec::new(),
        cite_required: matches!(
            mode,
            VerificationMode::WebSearchFetch | VerificationMode::BrowserValidation
        ),
        note:
            "Verification is still pending or incomplete; do not present the answer as confirmed."
                .to_string(),
    }
}

pub fn build_search_result_followup_plan() -> VerificationFollowupPlan {
    VerificationFollowupPlan {
        answer_readiness: "search_results_only".to_string(),
        next_tools: vec!["web_fetch".to_string()],
        cite_required: true,
        note: "Search results were observed, but at least one returned source should be fetched before answering with confirmed facts.".to_string(),
    }
}

pub fn build_source_observed_followup_plan(cite_required: bool) -> VerificationFollowupPlan {
    VerificationFollowupPlan {
        answer_readiness: "source_content_observed".to_string(),
        next_tools: Vec::new(),
        cite_required,
        note: "Source content has been observed directly and can now be summarized with explicit citation posture.".to_string(),
    }
}

pub fn build_verification_followup_plan(
    mode: VerificationMode,
    outcome: VerificationOutcome,
) -> VerificationFollowupPlan {
    match (mode, outcome) {
        (VerificationMode::WebSearchFetch, VerificationOutcome::VerificationSucceeded) => {
            build_search_result_followup_plan()
        }
        (VerificationMode::RealtimeLookup, VerificationOutcome::VerificationSucceeded) => {
            VerificationFollowupPlan {
                answer_readiness: "structured_lookup_observed".to_string(),
                next_tools: Vec::new(),
                cite_required: false,
                note: "Realtime lookup returned a structured observation that can be used directly, while still surfacing the lookup source when available.".to_string(),
            }
        }
        (
            VerificationMode::RuntimeStateCheck | VerificationMode::ExecutionResultCheck,
            VerificationOutcome::VerificationSucceeded,
        ) => VerificationFollowupPlan {
            answer_readiness: "execution_or_state_observed".to_string(),
            next_tools: Vec::new(),
            cite_required: false,
            note: "The requested runtime or execution evidence has been directly observed."
                .to_string(),
        },
        _ => build_pending_verification_followup_plan(mode),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_search_success_requires_source_fetch_followup() {
        let plan = build_verification_followup_plan(
            VerificationMode::WebSearchFetch,
            VerificationOutcome::VerificationSucceeded,
        );
        assert_eq!(plan.answer_readiness, "search_results_only");
        assert_eq!(plan.next_tools, vec!["web_fetch"]);
        assert!(plan.cite_required);
    }

    #[test]
    fn execution_success_is_ready_without_citation_requirement() {
        let plan = build_verification_followup_plan(
            VerificationMode::ExecutionResultCheck,
            VerificationOutcome::VerificationSucceeded,
        );
        assert_eq!(plan.answer_readiness, "execution_or_state_observed");
        assert!(!plan.cite_required);
    }
}
