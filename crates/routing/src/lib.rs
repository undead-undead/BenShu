//! Shared routing contracts for BenShu.
//!
//! This crate owns stable data types used by the coordinator routing,
//! capability routing, and verification policy surfaces. Strategy functions
//! can move here incrementally, but runtime orchestration stays in `brain`.

pub mod capability;
pub mod coordinator;
pub mod query;
pub mod truth;
pub mod verification;
pub mod web_verification;

pub use capability::{
    capability_route_debug_label, capability_route_hint_label,
    capability_route_preferred_tool_names, capability_route_prefers_direct_tool_surface,
    capability_route_requires_real_tool_call, capability_route_requires_source_fetch,
    capability_route_should_inject_system_message, CapabilityClarificationHint,
    CapabilityRouteHint, CapabilityRouteRequest, RealtimeLookupKind,
};
pub use coordinator::{
    coordinator_routing_judgment_only_message, coordinator_task_mode_label,
    coordinator_task_mode_should_include_media_followup_prompt,
    coordinator_task_mode_should_include_route_prompt,
    coordinator_task_mode_should_include_tool_index,
    coordinator_task_mode_should_include_truth_guidance, coordinator_task_mode_system_message,
    select_coordinator_task_mode, CoordinatorTaskMode,
};
pub use query::{
    classify_query_capability_domain, classify_query_capability_route,
    classify_query_verification_plan, classify_query_verification_plan_with_request,
    preferred_capability_domain_for_route, query_requests_image_generation,
    query_requests_routing_judgment_only, resolve_capability_route, CapabilityRouter,
};
pub use truth::TruthVerificationPolicyEngine;
pub use verification::{
    build_observed_verification_result_envelope, build_pending_verification_followup_plan,
    build_pending_verification_result_envelope, build_search_result_followup_plan,
    build_source_observed_followup_plan, build_verification_followup_plan,
    build_verified_verification_result_envelope, QueryVerificationPlan, SourcePosture, TruthStatus,
    VerificationDomain, VerificationFollowupPlan, VerificationMode, VerificationOutcome,
    VerificationRequirement, VerificationResultEnvelope, VerificationSource,
};
pub use web_verification::{
    route_reason_for_plan, WebVerificationAnswerReadiness, WebVerificationContinuation,
    WebVerificationDecision, WebVerificationOrchestrator, WebVerificationRouteReason,
    WebVerificationTermination,
};
