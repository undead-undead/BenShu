use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

pub const META_TRUTH_STATUS: &str = "truth_status";
pub const META_VERIFICATION_DOMAIN: &str = "verification_domain";
pub const META_VERIFICATION_REQUIREMENT: &str = "verification_requirement";
pub const META_VERIFICATION_MODE: &str = "verification_mode";
pub const META_VERIFICATION_OUTCOME: &str = "verification_outcome";
pub const META_VERIFICATION_ANSWER_READINESS: &str = "verification_answer_readiness";
pub const META_VERIFICATION_ROUTE_REASON: &str = "verification_route_reason";
pub const META_VERIFICATION_CONTINUATION: &str = "verification_continuation";
pub const META_VERIFICATION_TERMINATION: &str = "verification_termination";
pub const META_VERIFICATION_REQUIRES_FOLLOWUP: &str = "verification_requires_followup";
pub const META_VERIFICATION_CAN_FINALIZE_ANSWER: &str = "verification_can_finalize_answer";
pub const META_VERIFICATION_NEXT_TOOLS: &str = "verification_next_tools";
pub const META_VERIFICATION_CITE_REQUIRED: &str = "verification_cite_required";
pub const META_VERIFICATION_FOLLOWUP_NOTE: &str = "verification_followup_note";
pub const META_VERIFICATION_SOURCES_JSON: &str = "verification_sources_json";
pub const META_VERIFICATION_EXECUTION_EVIDENCE_JSON: &str = "verification_execution_evidence_json";
pub const META_VERIFICATION_STATE_EVIDENCE_JSON: &str = "verification_state_evidence_json";
pub const META_SOURCE_POSTURE: &str = "source_posture";
pub const META_TRUTH_VERIFICATION_GUIDANCE_ACTIVE: &str = "truth_verification_guidance_active";
pub const META_VERIFICATION_LAST_TOOL: &str = "verification_last_tool";
pub const META_VERIFICATION_TOOLS: &str = "verification_tools";
pub const META_VERIFICATION_SOURCE_COUNT: &str = "verification_source_count";
pub const META_VERIFICATION_EXECUTION_EVIDENCE_COUNT: &str =
    "verification_execution_evidence_count";
pub const META_VERIFICATION_STATE_EVIDENCE_COUNT: &str = "verification_state_evidence_count";
pub const META_VERIFICATION_NOTE_COUNT: &str = "verification_note_count";
pub const META_VERIFICATION_SURFACE_NOTE_PRESENT: &str = "verification_surface_note_present";
pub const META_VERIFICATION_SURFACE_NOTE_COMPLETE: &str = "verification_surface_note_complete";

pub const META_WINDOWS_NATIVE_EMBED_OUTCOME: &str = "engram_windows_native_embed_outcome";
pub const META_WINDOWS_NATIVE_EMBED_CLASS: &str = "engram_windows_native_embed_class";
pub const META_WINDOWS_NATIVE_EMBED_STRATEGY: &str = "engram_windows_native_embed_strategy";
pub const META_WINDOWS_NATIVE_RERANK_OUTCOME: &str = "engram_windows_native_rerank_outcome";
pub const META_WINDOWS_NATIVE_RERANK_CLASS: &str = "engram_windows_native_rerank_class";
pub const META_WINDOWS_NATIVE_RERANK_STRATEGY: &str = "engram_windows_native_rerank_strategy";

pub const HOOK_RUNTIME_NOTE_PROJECTIONS: &[(&str, &str)] = &[
    ("hook_memory_surface_count", "runtime_memory_surface_count"),
    (
        "hook_subagent_surface_count",
        "runtime_subagent_surface_count",
    ),
    ("hook_title_surface_count", "runtime_title_surface_count"),
    (
        "hook_summarization_surface_count",
        "runtime_summarization_surface_count",
    ),
    (
        "hook_dangling_tool_call_count",
        "runtime_dangling_tool_call_count",
    ),
    ("hook_tool_error_count", "runtime_tool_error_count"),
    ("hook_forge_surface_count", "runtime_forge_surface_count"),
    ("hook_media_surface_count", "runtime_media_surface_count"),
];

pub const SESSION_RUNTIME_NOTE_PROJECTIONS: &[(&str, &str)] = &[
    ("session_title", "session_title"),
    ("session_title_source", "session_title_source"),
];

pub const RUNTIME_NOTE_PROJECTIONS: &[(&str, &str)] = &[
    ("deferred_tool_filter_active", "deferred_tool_filter_active"),
    ("deferred_tool_visible_count", "deferred_tool_visible_count"),
    ("deferred_tool_total_count", "deferred_tool_total_count"),
    (
        "deferred_tool_deferred_count",
        "deferred_tool_deferred_count",
    ),
    (
        "deferred_tool_surface_note_present",
        "runtime_deferred_tool_surface_note_present",
    ),
    (
        "deferred_tool_surface_note_complete",
        "runtime_deferred_tool_surface_note_complete",
    ),
    ("session_status", "runtime_session_status"),
    ("clarification_prompt", "runtime_clarification_prompt"),
    (
        "clarification_original_request",
        "runtime_clarification_original_request",
    ),
    (
        "clarification_status_kind",
        "runtime_clarification_status_kind",
    ),
    (
        "clarification_session_status_json_present",
        "runtime_clarification_session_status_json_present",
    ),
    (
        "clarification_session_status_json_valid",
        "runtime_clarification_session_status_json_valid",
    ),
    (
        "clarification_contract_core_complete",
        "runtime_clarification_contract_core_complete",
    ),
    (
        "clarification_contract_complete",
        "runtime_clarification_contract_complete",
    ),
    (
        "clarification_surface_note_present",
        "runtime_clarification_surface_note_present",
    ),
    (
        "clarification_surface_note_complete",
        "runtime_clarification_surface_note_complete",
    ),
    (
        "clarification_awaiting_seen",
        "runtime_clarification_awaiting_seen",
    ),
    (
        "clarification_terminal_event_seen",
        "runtime_clarification_terminal_event_seen",
    ),
    (
        "clarification_roundtrip_complete",
        "runtime_clarification_roundtrip_complete",
    ),
    (
        "clarification_failure_reason",
        "runtime_clarification_failure_reason",
    ),
    ("clarification_event", "runtime_clarification_event"),
    (
        "clarification_status_surface",
        "runtime_clarification_status_surface",
    ),
    ("clarification_resolved", "runtime_clarification_resolved"),
    ("clarification_cancelled", "runtime_clarification_cancelled"),
    (META_TRUTH_STATUS, "runtime_truth_status"),
    (META_VERIFICATION_DOMAIN, "runtime_verification_domain"),
    (
        META_VERIFICATION_REQUIREMENT,
        "runtime_verification_requirement",
    ),
    (META_VERIFICATION_MODE, "runtime_verification_mode"),
    (META_VERIFICATION_OUTCOME, "runtime_verification_outcome"),
    (
        META_VERIFICATION_ANSWER_READINESS,
        "runtime_verification_answer_readiness",
    ),
    (
        META_VERIFICATION_ROUTE_REASON,
        "runtime_verification_route_reason",
    ),
    (
        META_VERIFICATION_CONTINUATION,
        "runtime_verification_continuation",
    ),
    (
        META_VERIFICATION_TERMINATION,
        "runtime_verification_termination",
    ),
    (
        META_VERIFICATION_REQUIRES_FOLLOWUP,
        "runtime_verification_requires_followup",
    ),
    (
        META_VERIFICATION_CAN_FINALIZE_ANSWER,
        "runtime_verification_can_finalize_answer",
    ),
    (
        META_VERIFICATION_NEXT_TOOLS,
        "runtime_verification_next_tools",
    ),
    (
        META_VERIFICATION_CITE_REQUIRED,
        "runtime_verification_cite_required",
    ),
    (
        META_VERIFICATION_FOLLOWUP_NOTE,
        "runtime_verification_followup_note",
    ),
    (
        META_VERIFICATION_SOURCES_JSON,
        "runtime_verification_sources_json",
    ),
    (
        META_VERIFICATION_EXECUTION_EVIDENCE_JSON,
        "runtime_verification_execution_evidence_json",
    ),
    (
        META_VERIFICATION_STATE_EVIDENCE_JSON,
        "runtime_verification_state_evidence_json",
    ),
    (META_SOURCE_POSTURE, "runtime_source_posture"),
    (
        META_TRUTH_VERIFICATION_GUIDANCE_ACTIVE,
        "runtime_truth_verification_guidance_active",
    ),
    (
        META_VERIFICATION_LAST_TOOL,
        "runtime_verification_last_tool",
    ),
    (META_VERIFICATION_TOOLS, "runtime_verification_tools"),
    (
        META_VERIFICATION_SOURCE_COUNT,
        "runtime_verification_source_count",
    ),
    (
        META_VERIFICATION_EXECUTION_EVIDENCE_COUNT,
        "runtime_verification_execution_evidence_count",
    ),
    (
        META_VERIFICATION_STATE_EVIDENCE_COUNT,
        "runtime_verification_state_evidence_count",
    ),
    (
        META_VERIFICATION_NOTE_COUNT,
        "runtime_verification_note_count",
    ),
    (
        META_VERIFICATION_SURFACE_NOTE_PRESENT,
        "runtime_verification_surface_note_present",
    ),
    (
        META_VERIFICATION_SURFACE_NOTE_COMPLETE,
        "runtime_verification_surface_note_complete",
    ),
    ("tool_error_tools", "runtime_tool_error_tools"),
    (
        "tool_error_surface_tools",
        "runtime_tool_error_surface_tools",
    ),
    (
        "tool_error_surface_present",
        "runtime_tool_error_surface_present",
    ),
    (
        "tool_error_contract_complete",
        "runtime_tool_error_contract_complete",
    ),
    ("media_preprocess_tools", "runtime_media_preprocess_tools"),
    (
        "media_preprocess_statuses",
        "runtime_media_preprocess_statuses",
    ),
    ("media_preprocess_kinds", "runtime_media_preprocess_kinds"),
    ("media_preprocess_inputs", "runtime_media_preprocess_inputs"),
    (
        "media_preprocess_outputs",
        "runtime_media_preprocess_outputs",
    ),
    (
        "media_preprocess_source_kinds",
        "runtime_media_preprocess_source_kinds",
    ),
    (
        "media_preprocess_source_refs",
        "runtime_media_preprocess_source_refs",
    ),
    (
        "media_preprocess_engines",
        "runtime_media_preprocess_engines",
    ),
    (
        "media_preprocess_cleanup",
        "runtime_media_preprocess_cleanup",
    ),
    ("media_preprocess_frames", "runtime_media_preprocess_frames"),
    (
        "media_preprocess_artifact_registered",
        "runtime_media_preprocess_artifact_registered",
    ),
    (
        "media_preprocess_artifact_source_kinds",
        "runtime_media_preprocess_artifact_source_kinds",
    ),
    (
        "media_preprocess_artifact_kinds",
        "runtime_media_preprocess_artifact_kinds",
    ),
    (
        "media_preprocess_artifact_uris",
        "runtime_media_preprocess_artifact_uris",
    ),
    (
        "media_preprocess_consumed_by",
        "runtime_media_preprocess_consumed_by",
    ),
    (
        "media_preprocess_consumption_routes",
        "runtime_media_preprocess_consumption_routes",
    ),
    (
        "media_preprocess_outcomes",
        "runtime_media_preprocess_outcomes",
    ),
    (
        "media_preprocess_preprocess_failed_routes",
        "runtime_media_preprocess_preprocess_failed_routes",
    ),
    (
        "media_preprocess_model_failed_routes",
        "runtime_media_preprocess_model_failed_routes",
    ),
    (
        "media_preprocess_result_insufficient_routes",
        "runtime_media_preprocess_result_insufficient_routes",
    ),
    (
        "media_preprocess_followup_strategies",
        "runtime_media_preprocess_followup_strategies",
    ),
    (
        "media_followup_strategies",
        "runtime_media_followup_strategies",
    ),
    (
        "media_followup_capability_route",
        "runtime_media_followup_capability_route",
    ),
    (
        "media_followup_execution_surface",
        "runtime_media_followup_execution_surface",
    ),
    (
        "media_followup_guidance_active",
        "runtime_media_followup_guidance_active",
    ),
    (
        "media_preprocess_attachment_fallback_routes",
        "runtime_media_preprocess_attachment_fallback_routes",
    ),
    (
        "media_preprocess_alternate_model_fallback_routes",
        "runtime_media_preprocess_alternate_model_fallback_routes",
    ),
    (
        "media_preprocess_clarification_routes",
        "runtime_media_preprocess_clarification_routes",
    ),
    (
        "media_preprocess_surface_note_present",
        "runtime_media_preprocess_surface_note_present",
    ),
    (
        "media_preprocess_surface_note_complete",
        "runtime_media_preprocess_surface_note_complete",
    ),
    (
        "media_preprocess_artifact_surface_note_present",
        "runtime_media_preprocess_artifact_surface_note_present",
    ),
    (
        "media_preprocess_artifact_surface_note_complete",
        "runtime_media_preprocess_artifact_surface_note_complete",
    ),
    (
        "media_preprocess_consumption_surface_note_complete",
        "runtime_media_preprocess_consumption_surface_note_complete",
    ),
    (
        "media_preprocess_outcome_surface_note_complete",
        "runtime_media_preprocess_outcome_surface_note_complete",
    ),
    (
        "media_preprocess_contract_core_complete",
        "runtime_media_preprocess_contract_core_complete",
    ),
    (
        "media_preprocess_contract_complete",
        "runtime_media_preprocess_contract_complete",
    ),
    (
        "media_preprocess_artifact_contract_complete",
        "runtime_media_preprocess_artifact_contract_complete",
    ),
    (
        "media_preprocess_consumption_contract_complete",
        "runtime_media_preprocess_consumption_contract_complete",
    ),
    (
        "media_preprocess_outcome_contract_complete",
        "runtime_media_preprocess_outcome_contract_complete",
    ),
    (
        "media_preprocess_strategy_surface_note_complete",
        "runtime_media_preprocess_strategy_surface_note_complete",
    ),
    (
        "media_preprocess_strategy_contract_complete",
        "runtime_media_preprocess_strategy_contract_complete",
    ),
    ("forge_registered_tools", "runtime_forge_registered_tools"),
    ("forge_source", "runtime_forge_source"),
    ("forge_scope", "runtime_forge_scope"),
    (
        "forge_followup_candidates",
        "runtime_forge_followup_candidates",
    ),
    (
        "forge_followup_gate_active",
        "runtime_forge_followup_gate_active",
    ),
    (
        "forge_execution_surfaces",
        "runtime_forge_execution_surfaces",
    ),
    (
        "forge_capability_domains",
        "runtime_forge_capability_domains",
    ),
    ("forge_smoke_statuses", "runtime_forge_smoke_statuses"),
    ("forge_smoke_latency_ms", "runtime_forge_smoke_latency_ms"),
    ("forge_cleanup_recorded", "runtime_forge_cleanup_recorded"),
    ("forge_surface_present", "runtime_forge_surface_present"),
    ("forge_contract_complete", "runtime_forge_contract_complete"),
    ("forge_followup_tools", "runtime_forge_followup_tools"),
    (
        "forge_followup_execution_happened",
        "runtime_forge_followup_execution_happened",
    ),
    (
        "forge_closed_loop_complete",
        "runtime_forge_closed_loop_complete",
    ),
    ("degraded_tool_names", "runtime_degraded_tool_names"),
    ("loop_guard_tools", "runtime_loop_guard_tools"),
    ("runtime_finish_reason", "runtime_finish_reason"),
    ("tactical_slm_present", "runtime_tactical_slm_present"),
    ("tactical_slm_model_id", "runtime_tactical_slm_model_id"),
    ("tactical_slm_factory_id", "runtime_tactical_slm_factory_id"),
    ("tactical_slm_source", "runtime_tactical_slm_source"),
    ("tactical_slm_roles", "runtime_tactical_slm_roles"),
    (
        "tactical_slm_contract_complete",
        "runtime_tactical_slm_contract_complete",
    ),
    ("background_present", "runtime_background_present"),
    ("background_revision", "runtime_background_revision"),
    (
        "background_previous_revision",
        "runtime_background_previous_revision",
    ),
    (
        "background_update_reason",
        "runtime_background_update_reason",
    ),
    (
        "background_quality_signal",
        "runtime_background_quality_signal",
    ),
    (
        "background_persona_present",
        "runtime_background_persona_present",
    ),
    (
        "background_relationship_present",
        "runtime_background_relationship_present",
    ),
    (
        "background_session_present",
        "runtime_background_session_present",
    ),
    (
        "background_recent_window_present",
        "runtime_background_recent_window_present",
    ),
    (
        "background_source_ref_count",
        "runtime_background_source_ref_count",
    ),
    (
        "background_compression_reason",
        "runtime_background_compression_reason",
    ),
    ("background_decision", "runtime_background_decision"),
    ("background_used_slm", "runtime_background_used_slm"),
    (
        "background_total_attempts",
        "runtime_background_total_attempts",
    ),
    ("background_skip_count", "runtime_background_skip_count"),
    ("background_reject_count", "runtime_background_reject_count"),
    (
        "background_refresh_session_count",
        "runtime_background_refresh_session_count",
    ),
    (
        "background_promote_relationship_count",
        "runtime_background_promote_relationship_count",
    ),
    (
        "background_rewrite_count",
        "runtime_background_rewrite_count",
    ),
    (
        "background_session_persistence_status",
        "runtime_background_session_persistence_status",
    ),
    (
        "background_session_persistence_error",
        "runtime_background_session_persistence_error",
    ),
    (
        "background_durable_promotion_pending",
        "runtime_background_durable_promotion_pending",
    ),
    (
        "background_durable_promotion_status",
        "runtime_background_durable_promotion_status",
    ),
    (
        "background_review_reason",
        "runtime_background_review_reason",
    ),
    (
        "background_review_source",
        "runtime_background_review_source",
    ),
    (
        "background_durable_promotion_error",
        "runtime_background_durable_promotion_error",
    ),
    (
        "background_contract_complete",
        "runtime_background_contract_complete",
    ),
    ("provider_name", "runtime_provider_name"),
    ("provider_model", "runtime_provider_model"),
    ("provider_latency_ms", "runtime_provider_latency_ms"),
    ("provider_prompt_tokens", "runtime_provider_prompt_tokens"),
    (
        "provider_completion_tokens",
        "runtime_provider_completion_tokens",
    ),
    ("provider_total_tokens", "runtime_provider_total_tokens"),
    ("provider_finish_reason", "runtime_provider_finish_reason"),
    (
        "provider_tool_call_count",
        "runtime_provider_tool_call_count",
    ),
    (
        "provider_tool_contract_mode",
        "runtime_provider_tool_contract_mode",
    ),
    (
        "provider_mainline_stability",
        "runtime_provider_mainline_stability",
    ),
    (
        "provider_media_preprocess_consumed_by",
        "runtime_provider_media_preprocess_consumed_by",
    ),
    (
        "provider_media_preprocess_consumption_routes",
        "runtime_provider_media_preprocess_consumption_routes",
    ),
    (
        "provider_media_preprocess_outcomes",
        "runtime_provider_media_preprocess_outcomes",
    ),
    (
        "provider_media_preprocess_preprocess_failed_routes",
        "runtime_provider_media_preprocess_preprocess_failed_routes",
    ),
    (
        "provider_media_preprocess_model_failed_routes",
        "runtime_provider_media_preprocess_model_failed_routes",
    ),
    (
        "provider_media_preprocess_result_insufficient_routes",
        "runtime_provider_media_preprocess_result_insufficient_routes",
    ),
    (
        "provider_media_preprocess_followup_strategies",
        "runtime_provider_media_preprocess_followup_strategies",
    ),
    (
        "provider_media_preprocess_attachment_fallback_routes",
        "runtime_provider_media_preprocess_attachment_fallback_routes",
    ),
    (
        "provider_media_preprocess_alternate_model_fallback_routes",
        "runtime_provider_media_preprocess_alternate_model_fallback_routes",
    ),
    (
        "provider_media_preprocess_clarification_routes",
        "runtime_provider_media_preprocess_clarification_routes",
    ),
    (
        "provider_media_preprocess_outcome_note_complete",
        "runtime_provider_media_preprocess_outcome_note_complete",
    ),
    (
        "provider_media_preprocess_outcome_contract_complete",
        "runtime_provider_media_preprocess_outcome_contract_complete",
    ),
    (
        "provider_media_preprocess_strategy_note_complete",
        "runtime_provider_media_preprocess_strategy_note_complete",
    ),
    (
        "provider_media_preprocess_strategy_contract_complete",
        "runtime_provider_media_preprocess_strategy_contract_complete",
    ),
    (
        "windows_native_host_runtime",
        "runtime_windows_native_host_runtime",
    ),
    (
        "windows_native_deployment_lane",
        "runtime_windows_native_deployment_lane",
    ),
    (
        "windows_native_deployment_strategy",
        "runtime_windows_native_deployment_strategy",
    ),
    (
        "windows_native_deployment_note",
        "runtime_windows_native_deployment_note",
    ),
    (
        "windows_native_product_mainline",
        "runtime_windows_native_product_mainline",
    ),
    (
        "windows_native_validation_tracks",
        "runtime_windows_native_validation_tracks",
    ),
    ("windows_native_priority", "runtime_windows_native_priority"),
    (
        "windows_native_small_model_runtime_target",
        "runtime_windows_native_small_model_runtime_target",
    ),
    (
        "windows_native_small_model_execution_linked",
        "runtime_windows_native_small_model_execution_linked",
    ),
    (
        "windows_native_small_model_execution_provider",
        "runtime_windows_native_small_model_execution_provider",
    ),
    (
        "windows_native_small_model_device_target",
        "runtime_windows_native_small_model_device_target",
    ),
    (
        "windows_native_small_model_fallback_mode",
        "runtime_windows_native_small_model_fallback_mode",
    ),
    (
        "windows_native_small_model_runtime_outcome",
        "runtime_windows_native_small_model_runtime_outcome",
    ),
    (
        "windows_native_small_model_runtime_strategy",
        "runtime_windows_native_small_model_runtime_strategy",
    ),
    (
        "windows_native_small_model_runtime_readiness",
        "runtime_windows_native_small_model_runtime_readiness",
    ),
    (
        "windows_native_small_model_runtime_reason",
        "runtime_windows_native_small_model_runtime_reason",
    ),
    (
        "windows_native_main_brain_runtime_target",
        "runtime_windows_native_main_brain_runtime_target",
    ),
    (
        "windows_native_runtime_contract_complete",
        "runtime_windows_native_runtime_contract_complete",
    ),
    (
        "windows_native_runtime_surface_note_complete",
        "runtime_windows_native_runtime_surface_note_complete",
    ),
    (
        META_WINDOWS_NATIVE_EMBED_OUTCOME,
        "runtime_engram_windows_native_embed_outcome",
    ),
    (
        META_WINDOWS_NATIVE_EMBED_CLASS,
        "runtime_engram_windows_native_embed_class",
    ),
    (
        "engram_windows_native_embed_provider",
        "runtime_engram_windows_native_embed_provider",
    ),
    (
        "engram_windows_native_embed_device_target",
        "runtime_engram_windows_native_embed_device_target",
    ),
    (
        "engram_windows_native_embed_fallback_mode",
        "runtime_engram_windows_native_embed_fallback_mode",
    ),
    (
        META_WINDOWS_NATIVE_EMBED_STRATEGY,
        "runtime_engram_windows_native_embed_strategy",
    ),
    (
        "engram_windows_native_embed_note",
        "runtime_engram_windows_native_embed_note",
    ),
    (
        META_WINDOWS_NATIVE_RERANK_OUTCOME,
        "runtime_engram_windows_native_rerank_outcome",
    ),
    (
        META_WINDOWS_NATIVE_RERANK_CLASS,
        "runtime_engram_windows_native_rerank_class",
    ),
    (
        "engram_windows_native_rerank_provider",
        "runtime_engram_windows_native_rerank_provider",
    ),
    (
        "engram_windows_native_rerank_device_target",
        "runtime_engram_windows_native_rerank_device_target",
    ),
    (
        "engram_windows_native_rerank_fallback_mode",
        "runtime_engram_windows_native_rerank_fallback_mode",
    ),
    (
        META_WINDOWS_NATIVE_RERANK_STRATEGY,
        "runtime_engram_windows_native_rerank_strategy",
    ),
    (
        "engram_windows_native_rerank_note",
        "runtime_engram_windows_native_rerank_note",
    ),
    (
        "engram_windows_native_surface_note_present",
        "runtime_engram_windows_native_surface_note_present",
    ),
    (
        "engram_windows_native_surface_note_complete",
        "runtime_engram_windows_native_surface_note_complete",
    ),
    (
        "provider_contract_core_complete",
        "runtime_provider_contract_core_complete",
    ),
    ("provider_usage_complete", "runtime_provider_usage_complete"),
    (
        "provider_contract_complete",
        "runtime_provider_contract_complete",
    ),
    (
        "provider_surface_note_core_complete",
        "runtime_provider_surface_note_core_complete",
    ),
    (
        "provider_surface_note_complete",
        "runtime_provider_surface_note_complete",
    ),
    ("post_run_summary", "runtime_post_run_summary"),
    ("visible_owner", "runtime_visible_owner"),
    ("memory_owner", "runtime_memory_owner"),
    ("approval_owner", "runtime_approval_owner"),
    (
        "memory_session_contract_core_complete",
        "runtime_memory_session_contract_core_complete",
    ),
    (
        "memory_session_contract_complete",
        "runtime_memory_session_contract_complete",
    ),
    (
        "memory_session_surface_core_complete",
        "runtime_memory_session_surface_core_complete",
    ),
    (
        "memory_session_surface_complete",
        "runtime_memory_session_surface_complete",
    ),
    (
        "memory_session_surface_note_present",
        "runtime_memory_session_surface_note_present",
    ),
    (
        "memory_session_surface_note_complete",
        "runtime_memory_session_surface_note_complete",
    ),
    (
        "subagent_budget_surface_note_present",
        "runtime_subagent_budget_surface_note_present",
    ),
    (
        "subagent_budget_surface_note_complete",
        "runtime_subagent_budget_surface_note_complete",
    ),
    (
        "title_surface_note_present",
        "runtime_title_surface_note_present",
    ),
    (
        "title_surface_note_complete",
        "runtime_title_surface_note_complete",
    ),
    (
        "summarization_surface_note_present",
        "runtime_summarization_surface_note_present",
    ),
    (
        "summarization_surface_note_complete",
        "runtime_summarization_surface_note_complete",
    ),
    (
        "memory_session_orchestration_contract_core_complete",
        "runtime_memory_session_orchestration_contract_core_complete",
    ),
    (
        "memory_session_orchestration_contract_complete",
        "runtime_memory_session_orchestration_contract_complete",
    ),
    (
        "runtime_evidence_contract_core_complete",
        "runtime_evidence_contract_core_complete",
    ),
    (
        "runtime_evidence_contract_complete",
        "runtime_evidence_contract_complete",
    ),
    ("delegation_present", "runtime_delegation_present"),
    ("handover_present", "runtime_handover_present"),
    ("max_parallel_tools", "runtime_max_parallel_tools"),
    ("hook_loop_abort_count", "runtime_loop_abort_count"),
];

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TruthVerificationQueryFields {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truth_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_domain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_requirement: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_outcome: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_answer_readiness: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_route_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_continuation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_termination: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_requires_followup: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_can_finalize_answer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_next_tools: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_cite_required: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_posture: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_last_tool: Option<String>,
}

impl TruthVerificationQueryFields {
    pub fn has_filters(&self) -> bool {
        self.truth_status.is_some()
            || self.verification_domain.is_some()
            || self.verification_requirement.is_some()
            || self.verification_mode.is_some()
            || self.verification_outcome.is_some()
            || self.verification_answer_readiness.is_some()
            || self.verification_route_reason.is_some()
            || self.verification_continuation.is_some()
            || self.verification_termination.is_some()
            || self.verification_requires_followup.is_some()
            || self.verification_can_finalize_answer.is_some()
            || self.verification_next_tools.is_some()
            || self.verification_cite_required.is_some()
            || self.source_posture.is_some()
            || self.verification_last_tool.is_some()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct WindowsNativeQueryFields {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub windows_native_embed_outcome: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub windows_native_embed_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub windows_native_embed_strategy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub windows_native_rerank_outcome: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub windows_native_rerank_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub windows_native_rerank_strategy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub windows_native_failure_reason: Option<String>,
}

impl WindowsNativeQueryFields {
    pub fn has_filters(&self) -> bool {
        self.windows_native_embed_outcome.is_some()
            || self.windows_native_embed_class.is_some()
            || self.windows_native_embed_strategy.is_some()
            || self.windows_native_rerank_outcome.is_some()
            || self.windows_native_rerank_class.is_some()
            || self.windows_native_rerank_strategy.is_some()
            || self.windows_native_failure_reason.is_some()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct WitnessLogQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suite_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub degraded: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_exhausted: Option<bool>,
    #[serde(flatten)]
    pub truth_verification: TruthVerificationQueryFields,
    #[serde(flatten)]
    pub windows_native: WindowsNativeQueryFields,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ScorecardQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suite_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(flatten)]
    pub truth_verification: TruthVerificationQueryFields,
    #[serde(flatten)]
    pub windows_native: WindowsNativeQueryFields,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TruthVerificationMetadata<'a> {
    pub truth_status: Option<&'a str>,
    pub verification_domain: Option<&'a str>,
    pub verification_requirement: Option<&'a str>,
    pub verification_mode: Option<&'a str>,
    pub verification_outcome: Option<&'a str>,
    pub verification_answer_readiness: Option<&'a str>,
    pub verification_route_reason: Option<&'a str>,
    pub verification_continuation: Option<&'a str>,
    pub verification_termination: Option<&'a str>,
    pub verification_requires_followup: Option<&'a str>,
    pub verification_can_finalize_answer: Option<&'a str>,
    pub verification_next_tools: Option<&'a str>,
    pub verification_cite_required: Option<&'a str>,
    pub source_posture: Option<&'a str>,
    pub verification_last_tool: Option<&'a str>,
}

impl<'a> TruthVerificationMetadata<'a> {
    pub fn from_map(metadata: &'a HashMap<String, String>) -> Self {
        Self {
            truth_status: metadata_value(metadata, META_TRUTH_STATUS),
            verification_domain: metadata_value(metadata, META_VERIFICATION_DOMAIN),
            verification_requirement: metadata_value(metadata, META_VERIFICATION_REQUIREMENT),
            verification_mode: metadata_value(metadata, META_VERIFICATION_MODE),
            verification_outcome: metadata_value(metadata, META_VERIFICATION_OUTCOME),
            verification_answer_readiness: metadata_value(
                metadata,
                META_VERIFICATION_ANSWER_READINESS,
            ),
            verification_route_reason: metadata_value(metadata, META_VERIFICATION_ROUTE_REASON),
            verification_continuation: metadata_value(metadata, META_VERIFICATION_CONTINUATION),
            verification_termination: metadata_value(metadata, META_VERIFICATION_TERMINATION),
            verification_requires_followup: metadata_value(
                metadata,
                META_VERIFICATION_REQUIRES_FOLLOWUP,
            ),
            verification_can_finalize_answer: metadata_value(
                metadata,
                META_VERIFICATION_CAN_FINALIZE_ANSWER,
            ),
            verification_next_tools: metadata_value(metadata, META_VERIFICATION_NEXT_TOOLS),
            verification_cite_required: metadata_value(metadata, META_VERIFICATION_CITE_REQUIRED),
            source_posture: metadata_value(metadata, META_SOURCE_POSTURE),
            verification_last_tool: metadata_value(metadata, META_VERIFICATION_LAST_TOOL),
        }
    }
}

pub fn metadata_value<'a>(metadata: &'a HashMap<String, String>, key: &str) -> Option<&'a str> {
    metadata
        .get(key)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
}

pub fn append_metadata_notes(
    notes: &mut Vec<String>,
    metadata: &HashMap<String, String>,
    projections: &[(&str, &str)],
) {
    for (metadata_key, note_prefix) in projections {
        if let Some(value) = metadata_value(metadata, metadata_key) {
            notes.push(format!("{note_prefix}:{value}"));
        }
    }
}

pub fn append_nonzero_metadata_notes(
    notes: &mut Vec<String>,
    metadata: &HashMap<String, String>,
    projections: &[(&str, &str)],
) {
    for (metadata_key, note_prefix) in projections {
        if let Some(value) = metadata_value(metadata, metadata_key) {
            if value != "0" {
                notes.push(format!("{note_prefix}:{value}"));
            }
        }
    }
}
