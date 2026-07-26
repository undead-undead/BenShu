use crate::{
    QueryVerificationPlan, SourcePosture, VerificationFollowupPlan, VerificationMode,
    VerificationResultEnvelope,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WebVerificationAnswerReadiness {
    VerificationPending,
    SearchResultsOnly,
    SourceContentObserved,
    ExecutionOrStateObserved,
    StructuredLookupObserved,
    Unknown,
}

impl WebVerificationAnswerReadiness {
    pub fn from_label(value: &str) -> Self {
        match value.trim() {
            "verification_pending" => Self::VerificationPending,
            "search_results_only" => Self::SearchResultsOnly,
            "source_content_observed" => Self::SourceContentObserved,
            "execution_or_state_observed" => Self::ExecutionOrStateObserved,
            "structured_lookup_observed" => Self::StructuredLookupObserved,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WebVerificationContinuation {
    None,
    ContinueWithSuggestedTools,
    ContinueFetchOrBrowse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WebVerificationTermination {
    NotReady,
    TentativeOnly,
    FinalizeWithSources,
    FinalizeWithExecutionEvidence,
    FinalizeWithStateEvidence,
    FinalizeStructuredLookup,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebVerificationDecision {
    pub readiness: WebVerificationAnswerReadiness,
    pub continuation: WebVerificationContinuation,
    pub termination: WebVerificationTermination,
    pub next_tools: Vec<String>,
    pub cite_required: bool,
    pub requires_followup: bool,
    pub can_finalize_answer: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WebVerificationRouteReason {
    ExternalFactRequiresSearchThenRead,
    StructuredLookupCanAnswerDirectly,
    RuntimeStateMustBeObservedDirectly,
    ExecutionResultMustBeObservedDirectly,
    ToolAvailabilityMustBeCheckedDirectly,
    BrowserObservationMustReadSourceContent,
    LocalContextMayAnswerTentatively,
    ToolLookupRequiredBeforeAnswering,
    GeneralCapabilityRouting,
}

impl WebVerificationRouteReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExternalFactRequiresSearchThenRead => {
                "external_fact_requires_search_then_source_read"
            }
            Self::StructuredLookupCanAnswerDirectly => "structured_lookup_can_answer_directly",
            Self::RuntimeStateMustBeObservedDirectly => "runtime_state_must_be_observed_directly",
            Self::ExecutionResultMustBeObservedDirectly => {
                "execution_result_must_be_observed_directly"
            }
            Self::ToolAvailabilityMustBeCheckedDirectly => {
                "tool_availability_must_be_checked_directly"
            }
            Self::BrowserObservationMustReadSourceContent => {
                "browser_observation_must_read_source_content"
            }
            Self::LocalContextMayAnswerTentatively => {
                "local_context_may_answer_without_live_verification"
            }
            Self::ToolLookupRequiredBeforeAnswering => "tool_lookup_required_before_answering",
            Self::GeneralCapabilityRouting => "general_capability_routing",
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct WebVerificationOrchestrator;

impl WebVerificationOrchestrator {
    pub fn new() -> Self {
        Self
    }

    pub fn decide(
        &self,
        plan: Option<&QueryVerificationPlan>,
        result: Option<&VerificationResultEnvelope>,
        followup: Option<&VerificationFollowupPlan>,
    ) -> WebVerificationDecision {
        let readiness = followup
            .map(|value| WebVerificationAnswerReadiness::from_label(&value.answer_readiness))
            .unwrap_or(WebVerificationAnswerReadiness::VerificationPending);
        let next_tools = followup
            .map(|value| value.next_tools.clone())
            .unwrap_or_default();
        let cite_required = followup.is_some_and(|value| value.cite_required);

        let continuation = match readiness {
            WebVerificationAnswerReadiness::SearchResultsOnly => {
                WebVerificationContinuation::ContinueFetchOrBrowse
            }
            WebVerificationAnswerReadiness::VerificationPending
                if !next_tools.is_empty() || plan.is_some() =>
            {
                WebVerificationContinuation::ContinueWithSuggestedTools
            }
            WebVerificationAnswerReadiness::Unknown if !next_tools.is_empty() => {
                WebVerificationContinuation::ContinueWithSuggestedTools
            }
            _ => WebVerificationContinuation::None,
        };

        let termination = match (
            readiness,
            result.map(|value| value.source_posture),
            cite_required,
        ) {
            (
                WebVerificationAnswerReadiness::SourceContentObserved,
                Some(SourcePosture::SourcesAttached),
                _,
            ) => WebVerificationTermination::FinalizeWithSources,
            (
                WebVerificationAnswerReadiness::ExecutionOrStateObserved,
                Some(SourcePosture::ExecutionEvidenceAttached),
                _,
            ) => WebVerificationTermination::FinalizeWithExecutionEvidence,
            (
                WebVerificationAnswerReadiness::ExecutionOrStateObserved,
                Some(SourcePosture::StateEvidenceAttached),
                _,
            ) => WebVerificationTermination::FinalizeWithStateEvidence,
            (WebVerificationAnswerReadiness::StructuredLookupObserved, _, false) => {
                WebVerificationTermination::FinalizeStructuredLookup
            }
            (WebVerificationAnswerReadiness::SearchResultsOnly, _, _) => {
                WebVerificationTermination::TentativeOnly
            }
            (WebVerificationAnswerReadiness::VerificationPending, _, _) => {
                WebVerificationTermination::NotReady
            }
            (WebVerificationAnswerReadiness::Unknown, _, _) if !next_tools.is_empty() => {
                WebVerificationTermination::NotReady
            }
            _ => WebVerificationTermination::TentativeOnly,
        };

        let can_finalize_answer = matches!(
            termination,
            WebVerificationTermination::FinalizeWithSources
                | WebVerificationTermination::FinalizeWithExecutionEvidence
                | WebVerificationTermination::FinalizeWithStateEvidence
                | WebVerificationTermination::FinalizeStructuredLookup
        );
        let requires_followup = matches!(
            continuation,
            WebVerificationContinuation::ContinueWithSuggestedTools
                | WebVerificationContinuation::ContinueFetchOrBrowse
        );

        WebVerificationDecision {
            readiness,
            continuation,
            termination,
            next_tools,
            cite_required,
            requires_followup,
            can_finalize_answer,
        }
    }
}

pub fn route_reason_for_plan(plan: Option<&QueryVerificationPlan>) -> WebVerificationRouteReason {
    match plan.map(|value| value.mode) {
        Some(VerificationMode::WebSearchFetch) => {
            WebVerificationRouteReason::ExternalFactRequiresSearchThenRead
        }
        Some(VerificationMode::RealtimeLookup) => {
            WebVerificationRouteReason::StructuredLookupCanAnswerDirectly
        }
        Some(VerificationMode::RuntimeStateCheck) => {
            WebVerificationRouteReason::RuntimeStateMustBeObservedDirectly
        }
        Some(VerificationMode::ExecutionResultCheck) => {
            WebVerificationRouteReason::ExecutionResultMustBeObservedDirectly
        }
        Some(VerificationMode::ToolInventoryCheck) => {
            WebVerificationRouteReason::ToolAvailabilityMustBeCheckedDirectly
        }
        Some(VerificationMode::BrowserValidation) => {
            WebVerificationRouteReason::BrowserObservationMustReadSourceContent
        }
        Some(VerificationMode::LocalContextOnly) => {
            WebVerificationRouteReason::LocalContextMayAnswerTentatively
        }
        Some(VerificationMode::ToolLookup) => {
            WebVerificationRouteReason::ToolLookupRequiredBeforeAnswering
        }
        _ => WebVerificationRouteReason::GeneralCapabilityRouting,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        TruthStatus, VerificationDomain, VerificationOutcome, VerificationRequirement,
        VerificationSource,
    };

    fn source_result() -> VerificationResultEnvelope {
        VerificationResultEnvelope {
            domain: VerificationDomain::KnowledgeFact,
            requirement: VerificationRequirement::Required,
            mode: VerificationMode::WebSearchFetch,
            outcome: VerificationOutcome::VerificationSucceeded,
            truth_status: TruthStatus::Verified,
            source_posture: SourcePosture::SourcesAttached,
            sources: vec![VerificationSource {
                kind: "web_page".to_string(),
                title: "Example".to_string(),
                uri: "https://example.com".to_string(),
                observed_at: None,
            }],
            execution_evidence: Vec::new(),
            state_evidence: Vec::new(),
            notes: vec!["observed".to_string()],
        }
    }

    #[test]
    fn orchestrator_requires_followup_for_search_results_only() {
        let followup = VerificationFollowupPlan {
            answer_readiness: "search_results_only".to_string(),
            next_tools: vec!["web_fetch".to_string()],
            cite_required: true,
            note: "fetch a source".to_string(),
        };

        let decision = WebVerificationOrchestrator::new().decide(None, None, Some(&followup));
        assert!(decision.requires_followup);
        assert!(!decision.can_finalize_answer);
        assert_eq!(
            decision.continuation,
            WebVerificationContinuation::ContinueFetchOrBrowse
        );
    }

    #[test]
    fn orchestrator_finalizes_when_source_content_is_observed() {
        let followup = VerificationFollowupPlan {
            answer_readiness: "source_content_observed".to_string(),
            next_tools: Vec::new(),
            cite_required: true,
            note: "source observed".to_string(),
        };

        let decision = WebVerificationOrchestrator::new().decide(
            None,
            Some(&source_result()),
            Some(&followup),
        );
        assert!(decision.can_finalize_answer);
        assert_eq!(
            decision.termination,
            WebVerificationTermination::FinalizeWithSources
        );
    }
}
