pub(crate) type NoteMetadataProjection = (&'static str, &'static str);

pub(crate) const NOTE_BEFORE_LLM_SKILL_MANUAL: &str = "before_llm:skill_manual:";
pub(crate) const NOTE_BEFORE_LLM_SKILL_ASSET: &str = "before_llm:skill_asset:";
pub(crate) const NOTE_SKILL_MANUAL_READ: &str = "skill_manual_read:";
pub(crate) const NOTE_SKILL_ASSET_READ: &str = "skill_asset_read:";
pub(crate) const NOTE_SKILL_ASSET_FOLLOWUP: &str = "skill_asset_followup:";
pub(crate) const NOTE_SKILL_ASSET_EXECUTION_SURFACE: &str = "skill_asset_execution_surface:";
pub(crate) const NOTE_SKILL_SURFACE_CLASSIFICATION: &str = "skill_surface_classification:";
pub(crate) const NOTE_SKILL_SURFACE_EXECUTION: &str = "skill_surface_execution:";
pub(crate) const NOTE_SKILL_SURFACE_RUNTIME: &str = "skill_surface_runtime:";
pub(crate) const NOTE_SKILL_SURFACE_KIND: &str = "skill_surface_kind:";
pub(crate) const NOTE_BEFORE_LLM_SKILL_MANUAL_GATE_ACTIVE: &str =
    "before_llm:skill_manual_gate_active";
pub(crate) const NOTE_BEFORE_LLM_SKILL_ASSET_GATE_ACTIVE: &str =
    "before_llm:skill_asset_gate_active";
pub(crate) const NOTE_BEFORE_LLM_TRUTH_VERIFICATION_GUIDANCE_ACTIVE: &str =
    "before_llm:truth_verification_guidance_active";

pub(crate) const META_MATCHED_SKILL_MANUALS: &str = "matched_skill_manuals";
pub(crate) const META_MATCHED_SKILL_ASSETS: &str = "matched_skill_assets";
pub(crate) const META_READ_SKILL_MANUALS: &str = "read_skill_manuals";
pub(crate) const META_READ_SKILL_ASSETS: &str = "read_skill_assets";
pub(crate) const META_SKILL_ASSET_FOLLOWUPS: &str = "skill_asset_followups";
pub(crate) const META_SKILL_ASSET_EXECUTION_SURFACES: &str = "skill_asset_execution_surfaces";
pub(crate) const META_SKILL_SURFACE_CLASSIFICATIONS: &str = "skill_surface_classifications";
pub(crate) const META_SKILL_SURFACE_EXECUTIONS: &str = "skill_surface_executions";
pub(crate) const META_SKILL_SURFACE_RUNTIMES: &str = "skill_surface_runtimes";
pub(crate) const META_SKILL_SURFACE_KINDS: &str = "skill_surface_kinds";
pub(crate) const META_SKILL_MANUAL_GATE_ACTIVE: &str = "skill_manual_gate_active";
pub(crate) const META_SKILL_ASSET_GATE_ACTIVE: &str = "skill_asset_gate_active";
pub(crate) const META_TRUTH_VERIFICATION_GUIDANCE_ACTIVE: &str =
    "truth_verification_guidance_active";
pub(crate) const META_SKILL_MANUAL_READ_HAPPENED: &str = "skill_manual_read_happened";
pub(crate) const META_SKILL_ASSET_READ_HAPPENED: &str = "skill_asset_read_happened";
pub(crate) const META_SKILL_ASSET_FOLLOWUP_HAPPENED: &str = "skill_asset_followup_happened";
pub(crate) const META_SKILL_ASSET_EXECUTION_SURFACE_HAPPENED: &str =
    "skill_asset_execution_surface_happened";

pub(crate) const META_VISIBLE_OWNER: &str = "visible_owner";
pub(crate) const META_MEMORY_OWNER: &str = "memory_owner";
pub(crate) const META_APPROVAL_OWNER: &str = "approval_owner";
pub(crate) const META_TRUTH_STATUS: &str = "truth_status";
pub(crate) const META_VERIFICATION_DOMAIN: &str = "verification_domain";
pub(crate) const META_VERIFICATION_REQUIREMENT: &str = "verification_requirement";
pub(crate) const META_VERIFICATION_MODE: &str = "verification_mode";
pub(crate) const META_VERIFICATION_OUTCOME: &str = "verification_outcome";
pub(crate) const META_VERIFICATION_ANSWER_READINESS: &str = "verification_answer_readiness";
pub(crate) const META_VERIFICATION_ROUTE_REASON: &str = "verification_route_reason";
pub(crate) const META_VERIFICATION_CONTINUATION: &str = "verification_continuation";
pub(crate) const META_VERIFICATION_TERMINATION: &str = "verification_termination";
pub(crate) const META_VERIFICATION_REQUIRES_FOLLOWUP: &str = "verification_requires_followup";
pub(crate) const META_VERIFICATION_CAN_FINALIZE_ANSWER: &str = "verification_can_finalize_answer";
pub(crate) const META_VERIFICATION_NEXT_TOOLS: &str = "verification_next_tools";
pub(crate) const META_VERIFICATION_CITE_REQUIRED: &str = "verification_cite_required";
pub(crate) const META_VERIFICATION_FOLLOWUP_NOTE: &str = "verification_followup_note";
pub(crate) const META_VERIFICATION_SOURCES_JSON: &str = "verification_sources_json";
pub(crate) const META_VERIFICATION_EXECUTION_EVIDENCE_JSON: &str =
    "verification_execution_evidence_json";
pub(crate) const META_VERIFICATION_STATE_EVIDENCE_JSON: &str = "verification_state_evidence_json";
pub(crate) const META_SOURCE_POSTURE: &str = "source_posture";
pub(crate) const META_VERIFICATION_LAST_TOOL: &str = "verification_last_tool";
pub(crate) const META_VERIFICATION_TOOLS: &str = "verification_tools";
pub(crate) const META_VERIFICATION_SOURCE_COUNT: &str = "verification_source_count";
pub(crate) const META_VERIFICATION_EXECUTION_EVIDENCE_COUNT: &str =
    "verification_execution_evidence_count";
pub(crate) const META_VERIFICATION_STATE_EVIDENCE_COUNT: &str = "verification_state_evidence_count";
pub(crate) const META_VERIFICATION_NOTE_COUNT: &str = "verification_note_count";

pub(crate) const META_ENGRAM_WINDOWS_NATIVE_EMBED_OUTCOME: &str =
    "engram_windows_native_embed_outcome";
pub(crate) const META_ENGRAM_WINDOWS_NATIVE_EMBED_CLASS: &str = "engram_windows_native_embed_class";
pub(crate) const META_ENGRAM_WINDOWS_NATIVE_EMBED_PROVIDER: &str =
    "engram_windows_native_embed_provider";
pub(crate) const META_ENGRAM_WINDOWS_NATIVE_EMBED_DEVICE_TARGET: &str =
    "engram_windows_native_embed_device_target";
pub(crate) const META_ENGRAM_WINDOWS_NATIVE_EMBED_FALLBACK_MODE: &str =
    "engram_windows_native_embed_fallback_mode";
pub(crate) const META_ENGRAM_WINDOWS_NATIVE_EMBED_STRATEGY: &str =
    "engram_windows_native_embed_strategy";
pub(crate) const META_ENGRAM_WINDOWS_NATIVE_EMBED_NOTE: &str = "engram_windows_native_embed_note";
pub(crate) const META_ENGRAM_WINDOWS_NATIVE_RERANK_OUTCOME: &str =
    "engram_windows_native_rerank_outcome";
pub(crate) const META_ENGRAM_WINDOWS_NATIVE_RERANK_CLASS: &str =
    "engram_windows_native_rerank_class";
pub(crate) const META_ENGRAM_WINDOWS_NATIVE_RERANK_PROVIDER: &str =
    "engram_windows_native_rerank_provider";
pub(crate) const META_ENGRAM_WINDOWS_NATIVE_RERANK_DEVICE_TARGET: &str =
    "engram_windows_native_rerank_device_target";
pub(crate) const META_ENGRAM_WINDOWS_NATIVE_RERANK_FALLBACK_MODE: &str =
    "engram_windows_native_rerank_fallback_mode";
pub(crate) const META_ENGRAM_WINDOWS_NATIVE_RERANK_STRATEGY: &str =
    "engram_windows_native_rerank_strategy";
pub(crate) const META_ENGRAM_WINDOWS_NATIVE_RERANK_NOTE: &str = "engram_windows_native_rerank_note";

pub(crate) const NOTE_BEFORE_RESPONSE_TRUTH_STATUS: &str = "before_response:truth_status:";
pub(crate) const NOTE_BEFORE_RESPONSE_VERIFICATION_DOMAIN: &str =
    "before_response:verification_domain:";
pub(crate) const NOTE_BEFORE_RESPONSE_VERIFICATION_REQUIREMENT: &str =
    "before_response:verification_requirement:";
pub(crate) const NOTE_BEFORE_RESPONSE_VERIFICATION_MODE: &str =
    "before_response:verification_mode:";
pub(crate) const NOTE_BEFORE_RESPONSE_VERIFICATION_OUTCOME: &str =
    "before_response:verification_outcome:";
pub(crate) const NOTE_BEFORE_RESPONSE_VERIFICATION_ANSWER_READINESS: &str =
    "before_response:verification_answer_readiness:";
pub(crate) const NOTE_BEFORE_RESPONSE_VERIFICATION_ROUTE_REASON: &str =
    "before_response:verification_route_reason:";
pub(crate) const NOTE_BEFORE_RESPONSE_VERIFICATION_CONTINUATION: &str =
    "before_response:verification_continuation:";
pub(crate) const NOTE_BEFORE_RESPONSE_VERIFICATION_TERMINATION: &str =
    "before_response:verification_termination:";
pub(crate) const NOTE_BEFORE_RESPONSE_VERIFICATION_REQUIRES_FOLLOWUP: &str =
    "before_response:verification_requires_followup:";
pub(crate) const NOTE_BEFORE_RESPONSE_VERIFICATION_CAN_FINALIZE_ANSWER: &str =
    "before_response:verification_can_finalize_answer:";
pub(crate) const NOTE_BEFORE_RESPONSE_VERIFICATION_NEXT_TOOLS: &str =
    "before_response:verification_next_tools:";
pub(crate) const NOTE_BEFORE_RESPONSE_VERIFICATION_CITE_REQUIRED: &str =
    "before_response:verification_cite_required:";
pub(crate) const NOTE_BEFORE_RESPONSE_VERIFICATION_FOLLOWUP_NOTE: &str =
    "before_response:verification_followup_note:";
pub(crate) const NOTE_BEFORE_RESPONSE_VERIFICATION_SOURCES_JSON: &str =
    "before_response:verification_sources_json:";
pub(crate) const NOTE_BEFORE_RESPONSE_VERIFICATION_EXECUTION_EVIDENCE_JSON: &str =
    "before_response:verification_execution_evidence_json:";
pub(crate) const NOTE_BEFORE_RESPONSE_VERIFICATION_STATE_EVIDENCE_JSON: &str =
    "before_response:verification_state_evidence_json:";
pub(crate) const NOTE_BEFORE_RESPONSE_SOURCE_POSTURE: &str = "before_response:source_posture:";
pub(crate) const NOTE_BEFORE_RESPONSE_VERIFICATION_LAST_TOOL: &str =
    "before_response:verification_last_tool:";
pub(crate) const NOTE_BEFORE_RESPONSE_VERIFICATION_TOOLS: &str =
    "before_response:verification_tools:";
pub(crate) const NOTE_BEFORE_RESPONSE_VERIFICATION_SOURCE_COUNT: &str =
    "before_response:verification_source_count:";
pub(crate) const NOTE_BEFORE_RESPONSE_VERIFICATION_EXECUTION_EVIDENCE_COUNT: &str =
    "before_response:verification_execution_evidence_count:";
pub(crate) const NOTE_BEFORE_RESPONSE_VERIFICATION_STATE_EVIDENCE_COUNT: &str =
    "before_response:verification_state_evidence_count:";
pub(crate) const NOTE_BEFORE_RESPONSE_VERIFICATION_NOTE_COUNT: &str =
    "before_response:verification_note_count:";

pub(crate) const NOTE_BEFORE_RESPONSE_ENGRAM_WINDOWS_NATIVE_EMBED_OUTCOME: &str =
    "before_response:engram_windows_native_embed_outcome:";
pub(crate) const NOTE_BEFORE_RESPONSE_ENGRAM_WINDOWS_NATIVE_EMBED_CLASS: &str =
    "before_response:engram_windows_native_embed_class:";
pub(crate) const NOTE_BEFORE_RESPONSE_ENGRAM_WINDOWS_NATIVE_EMBED_PROVIDER: &str =
    "before_response:engram_windows_native_embed_provider:";
pub(crate) const NOTE_BEFORE_RESPONSE_ENGRAM_WINDOWS_NATIVE_EMBED_DEVICE_TARGET: &str =
    "before_response:engram_windows_native_embed_device_target:";
pub(crate) const NOTE_BEFORE_RESPONSE_ENGRAM_WINDOWS_NATIVE_EMBED_FALLBACK_MODE: &str =
    "before_response:engram_windows_native_embed_fallback_mode:";
pub(crate) const NOTE_BEFORE_RESPONSE_ENGRAM_WINDOWS_NATIVE_EMBED_STRATEGY: &str =
    "before_response:engram_windows_native_embed_strategy:";
pub(crate) const NOTE_BEFORE_RESPONSE_ENGRAM_WINDOWS_NATIVE_EMBED_NOTE: &str =
    "before_response:engram_windows_native_embed_note:";
pub(crate) const NOTE_BEFORE_RESPONSE_ENGRAM_WINDOWS_NATIVE_RERANK_OUTCOME: &str =
    "before_response:engram_windows_native_rerank_outcome:";
pub(crate) const NOTE_BEFORE_RESPONSE_ENGRAM_WINDOWS_NATIVE_RERANK_CLASS: &str =
    "before_response:engram_windows_native_rerank_class:";
pub(crate) const NOTE_BEFORE_RESPONSE_ENGRAM_WINDOWS_NATIVE_RERANK_PROVIDER: &str =
    "before_response:engram_windows_native_rerank_provider:";
pub(crate) const NOTE_BEFORE_RESPONSE_ENGRAM_WINDOWS_NATIVE_RERANK_DEVICE_TARGET: &str =
    "before_response:engram_windows_native_rerank_device_target:";
pub(crate) const NOTE_BEFORE_RESPONSE_ENGRAM_WINDOWS_NATIVE_RERANK_FALLBACK_MODE: &str =
    "before_response:engram_windows_native_rerank_fallback_mode:";
pub(crate) const NOTE_BEFORE_RESPONSE_ENGRAM_WINDOWS_NATIVE_RERANK_STRATEGY: &str =
    "before_response:engram_windows_native_rerank_strategy:";
pub(crate) const NOTE_BEFORE_RESPONSE_ENGRAM_WINDOWS_NATIVE_RERANK_NOTE: &str =
    "before_response:engram_windows_native_rerank_note:";

pub(crate) const BEFORE_RESPONSE_NOTE_METADATA_PROJECTIONS: &[NoteMetadataProjection] = &[
    (NOTE_BEFORE_RESPONSE_TRUTH_STATUS, META_TRUTH_STATUS),
    (
        NOTE_BEFORE_RESPONSE_VERIFICATION_DOMAIN,
        META_VERIFICATION_DOMAIN,
    ),
    (
        NOTE_BEFORE_RESPONSE_VERIFICATION_REQUIREMENT,
        META_VERIFICATION_REQUIREMENT,
    ),
    (
        NOTE_BEFORE_RESPONSE_VERIFICATION_MODE,
        META_VERIFICATION_MODE,
    ),
    (
        NOTE_BEFORE_RESPONSE_VERIFICATION_OUTCOME,
        META_VERIFICATION_OUTCOME,
    ),
    (
        NOTE_BEFORE_RESPONSE_VERIFICATION_ANSWER_READINESS,
        META_VERIFICATION_ANSWER_READINESS,
    ),
    (
        NOTE_BEFORE_RESPONSE_VERIFICATION_ROUTE_REASON,
        META_VERIFICATION_ROUTE_REASON,
    ),
    (
        NOTE_BEFORE_RESPONSE_VERIFICATION_CONTINUATION,
        META_VERIFICATION_CONTINUATION,
    ),
    (
        NOTE_BEFORE_RESPONSE_VERIFICATION_TERMINATION,
        META_VERIFICATION_TERMINATION,
    ),
    (
        NOTE_BEFORE_RESPONSE_VERIFICATION_REQUIRES_FOLLOWUP,
        META_VERIFICATION_REQUIRES_FOLLOWUP,
    ),
    (
        NOTE_BEFORE_RESPONSE_VERIFICATION_CAN_FINALIZE_ANSWER,
        META_VERIFICATION_CAN_FINALIZE_ANSWER,
    ),
    (
        NOTE_BEFORE_RESPONSE_VERIFICATION_NEXT_TOOLS,
        META_VERIFICATION_NEXT_TOOLS,
    ),
    (
        NOTE_BEFORE_RESPONSE_VERIFICATION_CITE_REQUIRED,
        META_VERIFICATION_CITE_REQUIRED,
    ),
    (
        NOTE_BEFORE_RESPONSE_VERIFICATION_FOLLOWUP_NOTE,
        META_VERIFICATION_FOLLOWUP_NOTE,
    ),
    (
        NOTE_BEFORE_RESPONSE_VERIFICATION_SOURCES_JSON,
        META_VERIFICATION_SOURCES_JSON,
    ),
    (
        NOTE_BEFORE_RESPONSE_VERIFICATION_EXECUTION_EVIDENCE_JSON,
        META_VERIFICATION_EXECUTION_EVIDENCE_JSON,
    ),
    (
        NOTE_BEFORE_RESPONSE_VERIFICATION_STATE_EVIDENCE_JSON,
        META_VERIFICATION_STATE_EVIDENCE_JSON,
    ),
    (NOTE_BEFORE_RESPONSE_SOURCE_POSTURE, META_SOURCE_POSTURE),
    (
        NOTE_BEFORE_RESPONSE_VERIFICATION_LAST_TOOL,
        META_VERIFICATION_LAST_TOOL,
    ),
    (
        NOTE_BEFORE_RESPONSE_VERIFICATION_TOOLS,
        META_VERIFICATION_TOOLS,
    ),
    (
        NOTE_BEFORE_RESPONSE_VERIFICATION_SOURCE_COUNT,
        META_VERIFICATION_SOURCE_COUNT,
    ),
    (
        NOTE_BEFORE_RESPONSE_VERIFICATION_EXECUTION_EVIDENCE_COUNT,
        META_VERIFICATION_EXECUTION_EVIDENCE_COUNT,
    ),
    (
        NOTE_BEFORE_RESPONSE_VERIFICATION_STATE_EVIDENCE_COUNT,
        META_VERIFICATION_STATE_EVIDENCE_COUNT,
    ),
    (
        NOTE_BEFORE_RESPONSE_VERIFICATION_NOTE_COUNT,
        META_VERIFICATION_NOTE_COUNT,
    ),
];

pub(crate) const ENGRAM_WINDOWS_NATIVE_NOTE_METADATA_PROJECTIONS: &[NoteMetadataProjection] = &[
    (
        NOTE_BEFORE_RESPONSE_ENGRAM_WINDOWS_NATIVE_EMBED_OUTCOME,
        META_ENGRAM_WINDOWS_NATIVE_EMBED_OUTCOME,
    ),
    (
        NOTE_BEFORE_RESPONSE_ENGRAM_WINDOWS_NATIVE_EMBED_CLASS,
        META_ENGRAM_WINDOWS_NATIVE_EMBED_CLASS,
    ),
    (
        NOTE_BEFORE_RESPONSE_ENGRAM_WINDOWS_NATIVE_EMBED_PROVIDER,
        META_ENGRAM_WINDOWS_NATIVE_EMBED_PROVIDER,
    ),
    (
        NOTE_BEFORE_RESPONSE_ENGRAM_WINDOWS_NATIVE_EMBED_DEVICE_TARGET,
        META_ENGRAM_WINDOWS_NATIVE_EMBED_DEVICE_TARGET,
    ),
    (
        NOTE_BEFORE_RESPONSE_ENGRAM_WINDOWS_NATIVE_EMBED_FALLBACK_MODE,
        META_ENGRAM_WINDOWS_NATIVE_EMBED_FALLBACK_MODE,
    ),
    (
        NOTE_BEFORE_RESPONSE_ENGRAM_WINDOWS_NATIVE_EMBED_STRATEGY,
        META_ENGRAM_WINDOWS_NATIVE_EMBED_STRATEGY,
    ),
    (
        NOTE_BEFORE_RESPONSE_ENGRAM_WINDOWS_NATIVE_EMBED_NOTE,
        META_ENGRAM_WINDOWS_NATIVE_EMBED_NOTE,
    ),
    (
        NOTE_BEFORE_RESPONSE_ENGRAM_WINDOWS_NATIVE_RERANK_OUTCOME,
        META_ENGRAM_WINDOWS_NATIVE_RERANK_OUTCOME,
    ),
    (
        NOTE_BEFORE_RESPONSE_ENGRAM_WINDOWS_NATIVE_RERANK_CLASS,
        META_ENGRAM_WINDOWS_NATIVE_RERANK_CLASS,
    ),
    (
        NOTE_BEFORE_RESPONSE_ENGRAM_WINDOWS_NATIVE_RERANK_PROVIDER,
        META_ENGRAM_WINDOWS_NATIVE_RERANK_PROVIDER,
    ),
    (
        NOTE_BEFORE_RESPONSE_ENGRAM_WINDOWS_NATIVE_RERANK_DEVICE_TARGET,
        META_ENGRAM_WINDOWS_NATIVE_RERANK_DEVICE_TARGET,
    ),
    (
        NOTE_BEFORE_RESPONSE_ENGRAM_WINDOWS_NATIVE_RERANK_FALLBACK_MODE,
        META_ENGRAM_WINDOWS_NATIVE_RERANK_FALLBACK_MODE,
    ),
    (
        NOTE_BEFORE_RESPONSE_ENGRAM_WINDOWS_NATIVE_RERANK_STRATEGY,
        META_ENGRAM_WINDOWS_NATIVE_RERANK_STRATEGY,
    ),
    (
        NOTE_BEFORE_RESPONSE_ENGRAM_WINDOWS_NATIVE_RERANK_NOTE,
        META_ENGRAM_WINDOWS_NATIVE_RERANK_NOTE,
    ),
];
