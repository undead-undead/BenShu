use super::core::Agent;
use super::runtime_contract::{
    BEFORE_RESPONSE_NOTE_METADATA_PROJECTIONS, ENGRAM_WINDOWS_NATIVE_NOTE_METADATA_PROJECTIONS,
    META_APPROVAL_OWNER, META_MATCHED_SKILL_ASSETS, META_MATCHED_SKILL_MANUALS, META_MEMORY_OWNER,
    META_READ_SKILL_ASSETS, META_READ_SKILL_MANUALS, META_SKILL_ASSET_EXECUTION_SURFACES,
    META_SKILL_ASSET_EXECUTION_SURFACE_HAPPENED, META_SKILL_ASSET_FOLLOWUPS,
    META_SKILL_ASSET_FOLLOWUP_HAPPENED, META_SKILL_ASSET_GATE_ACTIVE,
    META_SKILL_ASSET_READ_HAPPENED, META_SKILL_MANUAL_GATE_ACTIVE, META_SKILL_MANUAL_READ_HAPPENED,
    META_SKILL_SURFACE_CLASSIFICATIONS, META_SKILL_SURFACE_EXECUTIONS, META_SKILL_SURFACE_KINDS,
    META_SKILL_SURFACE_RUNTIMES, META_TRUTH_VERIFICATION_GUIDANCE_ACTIVE, META_VISIBLE_OWNER,
    NOTE_BEFORE_LLM_SKILL_ASSET, NOTE_BEFORE_LLM_SKILL_ASSET_GATE_ACTIVE,
    NOTE_BEFORE_LLM_SKILL_MANUAL, NOTE_BEFORE_LLM_SKILL_MANUAL_GATE_ACTIVE,
    NOTE_BEFORE_LLM_TRUTH_VERIFICATION_GUIDANCE_ACTIVE, NOTE_SKILL_ASSET_EXECUTION_SURFACE,
    NOTE_SKILL_ASSET_FOLLOWUP, NOTE_SKILL_ASSET_READ, NOTE_SKILL_MANUAL_READ,
    NOTE_SKILL_SURFACE_CLASSIFICATION, NOTE_SKILL_SURFACE_EXECUTION, NOTE_SKILL_SURFACE_KIND,
    NOTE_SKILL_SURFACE_RUNTIME,
};
use super::runtime_support::RuntimeExecutionSeed;
use crate::agent::message::Message;
use crate::agent::protocol::ChatOutcome;
use crate::agent::provider::Provider;
use crate::agent::reasoner::runtime_session_title;
use crate::hooks::RuntimeHookCapture;
use benshu_telemetry::{RunTrace, ToolTrace, TraceStatus};
use chrono::{DateTime, Utc};
use std::collections::{BTreeSet, HashMap};
use uuid::Uuid;

struct RunTraceMetadataBuilder {
    metadata: HashMap<String, String>,
}

const SKILL_LOADING_ACTIVITY_KEYS: &[&str] = &[
    "matched_skill_manuals",
    "matched_skill_assets",
    "read_skill_manuals",
    "read_skill_assets",
    "skill_asset_followups",
    "skill_manual_gate_active",
    "skill_asset_gate_active",
];
const MEMORY_SESSION_ACTIVITY_KEYS: &[&str] = &[
    "visible_owner",
    "memory_owner",
    "approval_owner",
    "session_title_source",
    "session_title_present",
];
const DEFERRED_TOOL_ACTIVITY_KEYS: &[&str] = &[
    "deferred_tool_filter_active",
    "deferred_tool_visible_count",
    "deferred_tool_total_count",
    "deferred_tool_deferred_count",
];
const TOOL_ERROR_ACTIVITY_KEYS: &[&str] = &[
    "tool_error_tools",
    "tool_error_surface_tools",
    "tool_error_surface_present",
];
const MEMORY_SESSION_CONTRACT_CORE_KEYS: &[&str] = &[
    "visible_owner",
    "memory_owner",
    "approval_owner",
    "session_title_source",
    "session_title_present",
];
const MEMORY_SESSION_ORCHESTRATION_CORE_KEYS: &[&str] = &[
    "memory_session_contract_complete",
    "subagent_budget_surface_note_complete",
    "title_surface_note_complete",
];
const MEMORY_SESSION_ORCHESTRATION_COMPLETE_KEYS: &[&str] = &[
    "memory_session_surface_complete",
    "memory_session_surface_note_complete",
    "summarization_surface_note_complete",
];
const SKILL_LOADING_COMPLETE_KEYS: &[&str] = &[
    "skill_loading_contract_complete",
    "skill_loading_surface_note_complete",
];
const SUBAGENT_BUDGET_REQUIRED_FIELDS: &[&str] = &["delegation", "handover", "parallel_tools"];
const SUBAGENT_BUDGET_METADATA_MAPPINGS: &[(&str, &str)] = &[
    ("delegation", "delegation_present"),
    ("handover", "handover_present"),
    ("parallel_tools", "max_parallel_tools"),
];
const TITLE_SURFACE_REQUIRED_FIELDS: &[&str] = &["present", "source", "value"];
const MEMORY_SESSION_SURFACE_REQUIRED_FIELDS: &[&str] = &[
    "visible",
    "memory",
    "approval",
    "title_present",
    "title_source",
    "summary_present",
];
const VERIFICATION_SURFACE_REQUIRED_FIELDS: &[&str] =
    &["tools", "count", "latest_tool", "complete"];
const ENGRAM_WINDOWS_NATIVE_SURFACE_REQUIRED_FIELDS: &[&str] = &["embed_present", "rerank_present"];
const TACTICAL_SLM_REQUIRED_KEYS: &[&str] = &[
    "tactical_slm_model_id",
    "tactical_slm_factory_id",
    "tactical_slm_source",
    "tactical_slm_roles",
];
const RUNTIME_BUDGET_METADATA_MAPPINGS: &[(&str, &str)] = &[
    (
        "runtime_context_budget_tokens",
        "runtime_context_budget_tokens",
    ),
    (
        "runtime_response_reserve_tokens",
        "runtime_response_reserve_tokens",
    ),
    ("runtime_token_budget", "runtime_token_budget"),
    ("runtime_jit_token_budget", "runtime_jit_token_budget"),
    (
        "shared_worker_context_budget_tokens",
        "shared_worker_context_budget_tokens",
    ),
    (
        "shared_worker_response_reserve_tokens",
        "shared_worker_response_reserve_tokens",
    ),
    ("shared_worker_token_budget", "shared_worker_token_budget"),
    (
        "shared_worker_jit_token_budget",
        "shared_worker_jit_token_budget",
    ),
];
const BACKGROUND_CONTRACT_REQUIRED_KEYS: &[&str] = &[
    "background_present",
    "background_revision",
    "background_quality_signal",
    "background_persona_present",
    "background_relationship_present",
    "background_session_present",
    "background_recent_window_present",
    "background_session_persistence_status",
];

impl RunTraceMetadataBuilder {
    fn new() -> Self {
        Self {
            metadata: HashMap::new(),
        }
    }

    fn into_inner(self) -> HashMap<String, String> {
        self.metadata
    }

    fn as_map(&self) -> &HashMap<String, String> {
        &self.metadata
    }

    fn as_mut_map(&mut self) -> &mut HashMap<String, String> {
        &mut self.metadata
    }

    fn insert(&mut self, key: &str, value: impl Into<String>) {
        self.metadata.insert(key.to_string(), value.into());
    }

    fn insert_nonempty(&mut self, key: &str, value: impl AsRef<str>) -> bool {
        let trimmed = value.as_ref().trim();
        if trimmed.is_empty() {
            return false;
        }
        self.insert(key, trimmed);
        true
    }

    fn insert_if_some(&mut self, key: &str, value: Option<String>) {
        if let Some(value) = value {
            self.insert(key, value);
        }
    }

    fn insert_true(&mut self, key: &str, condition: bool) {
        if condition {
            self.insert(key, "true");
        }
    }

    fn insert_scalar_fields<T: ToString>(&mut self, fields: &[(&str, T)]) {
        for (key, value) in fields {
            self.insert(key, value.to_string());
        }
    }

    fn insert_true_fields(&mut self, fields: &[(&str, bool)]) {
        for (key, value) in fields {
            self.insert_true(key, *value);
        }
    }

    fn insert_joined_set(&mut self, key: &str, values: BTreeSet<String>) {
        if !values.is_empty() {
            self.insert(key, values.into_iter().collect::<Vec<_>>().join(","));
        }
    }

    fn extend(&mut self, other: HashMap<String, String>) {
        self.metadata.extend(other);
    }

    fn contains_key(&self, key: &str) -> bool {
        self.metadata.contains_key(key)
    }

    fn contains_any(&self, keys: &[&str]) -> bool {
        keys.iter().any(|key| self.contains_key(key))
    }

    fn contains_all(&self, keys: &[&str]) -> bool {
        keys.iter().all(|key| self.contains_key(key))
    }

    fn get(&self, key: &str) -> Option<&str> {
        self.metadata.get(key).map(String::as_str)
    }

    fn is_true(&self, key: &str) -> bool {
        self.get(key) == Some("true")
    }

    fn all_true(&self, keys: &[&str]) -> bool {
        keys.iter().all(|key| self.is_true(key))
    }

    fn insert_values_from_map(
        &mut self,
        source: &HashMap<String, String>,
        mappings: &[(&str, &str)],
    ) {
        for (source_key, metadata_key) in mappings {
            if let Some(value) = source.get(*source_key) {
                self.insert(metadata_key, value.clone());
            }
        }
    }

    fn project_trimmed_note_value(
        &mut self,
        note: &str,
        projections: &[super::runtime_contract::NoteMetadataProjection],
    ) -> bool {
        for (prefix, metadata_key) in projections {
            if let Some(value) = note.strip_prefix(prefix) {
                return self.insert_nonempty(metadata_key, value);
            }
        }
        false
    }
}

fn extra_param_string(extra_params: Option<&serde_json::Value>, key: &str) -> Option<String> {
    extra_params
        .and_then(|value| value.get(key))
        .and_then(|value| match value {
            serde_json::Value::String(text) => Some(text.clone()),
            serde_json::Value::Bool(flag) => Some(flag.to_string()),
            serde_json::Value::Number(number) => Some(number.to_string()),
            _ => None,
        })
}

fn extra_param_bool(extra_params: Option<&serde_json::Value>, key: &str) -> bool {
    extra_params
        .and_then(|value| value.get(key))
        .and_then(|value| match value {
            serde_json::Value::Bool(flag) => Some(*flag),
            serde_json::Value::String(text) => Some(text == "true"),
            _ => None,
        })
        .unwrap_or(false)
}

fn insert_extra_param_metadata(
    metadata: &mut RunTraceMetadataBuilder,
    extra_params: Option<&serde_json::Value>,
    mappings: &[(&str, &str)],
) {
    for (extra_key, metadata_key) in mappings {
        metadata.insert_if_some(*metadata_key, extra_param_string(extra_params, extra_key));
    }
}

fn parse_colon_fields(surface: &str) -> HashMap<String, String> {
    let mut fields = HashMap::new();
    for part in surface.split(':') {
        if let Some((key, value)) = part.split_once('=') {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                fields.insert(key.to_string(), trimmed.to_string());
            }
        }
    }
    fields
}

fn surface_has_nonempty_fields(fields: &HashMap<String, String>, required: &[&str]) -> bool {
    required.iter().all(|key| fields.contains_key(*key))
}

fn surface_has_true_fields(fields: &HashMap<String, String>, required: &[&str]) -> bool {
    required
        .iter()
        .all(|key| fields.get(*key).is_some_and(|value| value == "true"))
}

impl<P: Provider> Agent<P> {
    fn finalize_tool_error_trace_metadata(&self, metadata: &mut RunTraceMetadataBuilder) {
        if metadata.contains_key("tool_error_surface_tools") {
            metadata.insert("tool_error_surface_present", "true");
        }
        if metadata.contains_key("tool_error_tools")
            && metadata.is_true("tool_error_surface_present")
        {
            metadata.insert("tool_error_contract_complete", "true");
        }
    }

    fn finalize_memory_session_trace_metadata(
        &self,
        metadata: &mut RunTraceMetadataBuilder,
        hook_capture: &RuntimeHookCapture,
        memory_session_surface_note_present: bool,
        memory_session_surface_note_complete: bool,
    ) {
        let memory_session_contract_core_complete =
            metadata.contains_all(MEMORY_SESSION_CONTRACT_CORE_KEYS);
        if memory_session_contract_core_complete {
            metadata.insert("memory_session_contract_core_complete", "true");
        }
        let title_complete = metadata
            .get("session_title_present")
            .map(|value| value == "false" || metadata.contains_key("session_title"))
            .unwrap_or(false);
        if memory_session_contract_core_complete && title_complete {
            metadata.insert("memory_session_contract_complete", "true");
        }
        let memory_session_surface_core_complete = hook_capture.memory_surface_count > 0
            && hook_capture.subagent_surface_count > 0
            && hook_capture.title_surface_count > 0;
        if memory_session_surface_core_complete {
            metadata.insert("memory_session_surface_core_complete", "true");
        }
        if memory_session_surface_core_complete
            && hook_capture.summarization_surface_count > 0
            && metadata.contains_key("post_run_summary")
        {
            metadata.insert("memory_session_surface_complete", "true");
        }
        metadata.insert_true(
            "memory_session_surface_note_present",
            memory_session_surface_note_present,
        );
        metadata.insert_true(
            "memory_session_surface_note_complete",
            memory_session_surface_note_complete,
        );
        let memory_session_orchestration_contract_core_complete =
            metadata.all_true(MEMORY_SESSION_ORCHESTRATION_CORE_KEYS);
        if memory_session_orchestration_contract_core_complete {
            metadata.insert(
                "memory_session_orchestration_contract_core_complete",
                "true",
            );
        }
        if memory_session_orchestration_contract_core_complete
            && metadata.all_true(MEMORY_SESSION_ORCHESTRATION_COMPLETE_KEYS)
        {
            metadata.insert("memory_session_orchestration_contract_complete", "true");
        }
    }

    fn finalize_runtime_evidence_trace_metadata(&self, metadata: &mut RunTraceMetadataBuilder) {
        let skill_loading_activity_present = metadata.contains_any(SKILL_LOADING_ACTIVITY_KEYS);
        let memory_session_activity_present = metadata.contains_any(MEMORY_SESSION_ACTIVITY_KEYS);
        let deferred_tool_activity_present = metadata.contains_any(DEFERRED_TOOL_ACTIVITY_KEYS);
        let tool_error_activity_present = metadata.contains_any(TOOL_ERROR_ACTIVITY_KEYS);
        let runtime_evidence_contract_core_complete = self
            .provider_runtime_evidence_core_complete(metadata.as_map())
            && (!skill_loading_activity_present
                || metadata.is_true("skill_loading_contract_core_complete"))
            && self.clarification_runtime_evidence_core_complete(metadata.as_map())
            && (!memory_session_activity_present
                || metadata.is_true("memory_session_orchestration_contract_core_complete"))
            && self.media_runtime_evidence_core_complete(metadata.as_map())
            && self.forge_runtime_evidence_core_complete(metadata.as_map());
        if runtime_evidence_contract_core_complete {
            metadata.insert("runtime_evidence_contract_core_complete", "true");
        }

        let runtime_evidence_contract_complete = runtime_evidence_contract_core_complete
            && self.provider_runtime_evidence_complete(metadata.as_map())
            && (!skill_loading_activity_present || metadata.all_true(SKILL_LOADING_COMPLETE_KEYS))
            && self.clarification_runtime_evidence_complete(metadata.as_map())
            && (!memory_session_activity_present
                || metadata.is_true("memory_session_orchestration_contract_complete"))
            && (!deferred_tool_activity_present
                || metadata.is_true("deferred_tool_surface_note_complete"))
            && (!tool_error_activity_present || metadata.is_true("tool_error_contract_complete"))
            && self.media_runtime_evidence_complete(metadata.as_map())
            && self.forge_runtime_evidence_complete(metadata.as_map());
        if runtime_evidence_contract_complete {
            metadata.insert("runtime_evidence_contract_complete", "true");
        }
    }

    fn apply_skill_loading_trace_metadata(
        &self,
        metadata: &mut RunTraceMetadataBuilder,
        hook_capture: &RuntimeHookCapture,
    ) {
        let mut matched_skill_manuals = std::collections::BTreeSet::new();
        let mut matched_skill_assets = std::collections::BTreeSet::new();
        let mut read_skill_manuals = std::collections::BTreeSet::new();
        let mut read_skill_assets = std::collections::BTreeSet::new();
        let mut skill_asset_followups = std::collections::BTreeSet::new();
        let mut skill_asset_execution_surfaces = std::collections::BTreeSet::new();
        let mut skill_surface_classifications = std::collections::BTreeSet::new();
        let mut skill_surface_executions = std::collections::BTreeSet::new();
        let mut skill_surface_runtimes = std::collections::BTreeSet::new();
        let mut skill_surface_kinds = std::collections::BTreeSet::new();
        let mut skill_manual_gate_active = false;
        let mut skill_asset_gate_active = false;
        let mut truth_verification_guidance_active = false;

        for note in &hook_capture.notes {
            if let Some(skill_name) = note.strip_prefix(NOTE_BEFORE_LLM_SKILL_MANUAL) {
                if !skill_name.is_empty() {
                    matched_skill_manuals.insert(skill_name.to_string());
                }
            } else if let Some(asset_path) = note.strip_prefix(NOTE_BEFORE_LLM_SKILL_ASSET) {
                if !asset_path.is_empty() {
                    matched_skill_assets.insert(asset_path.to_string());
                }
            } else if let Some(skill_name) = note.strip_prefix(NOTE_SKILL_MANUAL_READ) {
                if !skill_name.is_empty() {
                    read_skill_manuals.insert(skill_name.to_string());
                }
            } else if let Some(asset_ref) = note.strip_prefix(NOTE_SKILL_ASSET_READ) {
                if !asset_ref.is_empty() {
                    read_skill_assets.insert(asset_ref.to_string());
                }
            } else if let Some(followup) = note.strip_prefix(NOTE_SKILL_ASSET_FOLLOWUP) {
                if !followup.is_empty() {
                    skill_asset_followups.insert(followup.to_string());
                }
            } else if let Some(surface) = note.strip_prefix(NOTE_SKILL_ASSET_EXECUTION_SURFACE) {
                if !surface.is_empty() {
                    skill_asset_execution_surfaces.insert(surface.to_string());
                }
            } else if let Some(classification) =
                note.strip_prefix(NOTE_SKILL_SURFACE_CLASSIFICATION)
            {
                if !classification.is_empty() {
                    skill_surface_classifications.insert(classification.to_string());
                }
            } else if let Some(surface) = note.strip_prefix(NOTE_SKILL_SURFACE_EXECUTION) {
                if !surface.is_empty() {
                    skill_surface_executions.insert(surface.to_string());
                }
            } else if let Some(runtime) = note.strip_prefix(NOTE_SKILL_SURFACE_RUNTIME) {
                if !runtime.is_empty() {
                    skill_surface_runtimes.insert(runtime.to_string());
                }
            } else if let Some(kind) = note.strip_prefix(NOTE_SKILL_SURFACE_KIND) {
                if !kind.is_empty() {
                    skill_surface_kinds.insert(kind.to_string());
                }
            } else if note == NOTE_BEFORE_LLM_SKILL_MANUAL_GATE_ACTIVE {
                skill_manual_gate_active = true;
            } else if note == NOTE_BEFORE_LLM_SKILL_ASSET_GATE_ACTIVE {
                skill_asset_gate_active = true;
            } else if note == NOTE_BEFORE_LLM_TRUTH_VERIFICATION_GUIDANCE_ACTIVE {
                truth_verification_guidance_active = true;
            }
        }

        metadata.insert_joined_set(META_MATCHED_SKILL_MANUALS, matched_skill_manuals);
        metadata.insert_joined_set(META_MATCHED_SKILL_ASSETS, matched_skill_assets);
        metadata.insert_joined_set(META_READ_SKILL_MANUALS, read_skill_manuals);
        metadata.insert_joined_set(META_READ_SKILL_ASSETS, read_skill_assets);
        metadata.insert_joined_set(META_SKILL_ASSET_FOLLOWUPS, skill_asset_followups);
        metadata.insert_joined_set(
            META_SKILL_ASSET_EXECUTION_SURFACES,
            skill_asset_execution_surfaces,
        );
        metadata.insert_joined_set(
            META_SKILL_SURFACE_CLASSIFICATIONS,
            skill_surface_classifications,
        );
        metadata.insert_joined_set(META_SKILL_SURFACE_EXECUTIONS, skill_surface_executions);
        metadata.insert_joined_set(META_SKILL_SURFACE_RUNTIMES, skill_surface_runtimes);
        metadata.insert_joined_set(META_SKILL_SURFACE_KINDS, skill_surface_kinds);
        metadata.insert_true(META_SKILL_MANUAL_GATE_ACTIVE, skill_manual_gate_active);
        metadata.insert_true(META_SKILL_ASSET_GATE_ACTIVE, skill_asset_gate_active);
        metadata.insert_true(
            META_TRUTH_VERIFICATION_GUIDANCE_ACTIVE,
            truth_verification_guidance_active,
        );
        self.apply_media_followup_before_llm_metadata(metadata.as_mut_map(), hook_capture);
        metadata.insert_true(
            META_SKILL_MANUAL_READ_HAPPENED,
            hook_capture.skill_manual_read_count > 0,
        );
        metadata.insert_true(
            META_SKILL_ASSET_READ_HAPPENED,
            hook_capture.skill_asset_read_count > 0,
        );
        metadata.insert_true(
            META_SKILL_ASSET_FOLLOWUP_HAPPENED,
            metadata.contains_key(META_SKILL_ASSET_FOLLOWUPS),
        );
        metadata.insert_true(
            META_SKILL_ASSET_EXECUTION_SURFACE_HAPPENED,
            metadata.contains_key(META_SKILL_ASSET_EXECUTION_SURFACES),
        );
        if metadata.contains_key(META_SKILL_SURFACE_EXECUTIONS) {
            metadata.insert("skill_surface_contract_happened", "true");
        }

        let has_skill_loading_activity = metadata.contains_any(&[
            META_MATCHED_SKILL_MANUALS,
            META_MATCHED_SKILL_ASSETS,
            META_READ_SKILL_MANUALS,
            META_READ_SKILL_ASSETS,
            META_SKILL_MANUAL_GATE_ACTIVE,
            META_SKILL_ASSET_GATE_ACTIVE,
        ]);
        let manual_chain_complete = !metadata.contains_key(META_MATCHED_SKILL_MANUALS)
            || metadata.contains_key(META_READ_SKILL_MANUALS);
        let asset_chain_complete = !metadata.contains_key(META_MATCHED_SKILL_ASSETS)
            || metadata.contains_key(META_READ_SKILL_ASSETS);
        let followup_chain_complete = !metadata.contains_key(META_SKILL_ASSET_FOLLOWUPS)
            || metadata.contains_key(META_SKILL_ASSET_READ_HAPPENED);
        let execution_surface_chain_complete = !metadata.contains_key(META_SKILL_ASSET_FOLLOWUPS)
            || metadata.contains_key(META_SKILL_ASSET_EXECUTION_SURFACES);
        let surface_contract_core_complete = !metadata.contains_key(META_MATCHED_SKILL_MANUALS)
            || (metadata.contains_key(META_SKILL_SURFACE_CLASSIFICATIONS)
                && metadata.contains_key(META_SKILL_SURFACE_EXECUTIONS)
                && metadata.contains_key(META_SKILL_SURFACE_KINDS));
        let surface_contract_complete = !metadata.contains_key(META_MATCHED_SKILL_MANUALS)
            || (surface_contract_core_complete
                && metadata.contains_key(META_SKILL_SURFACE_RUNTIMES));

        if has_skill_loading_activity && manual_chain_complete && asset_chain_complete {
            metadata.insert("skill_loading_contract_core_complete", "true");
        }
        if has_skill_loading_activity
            && manual_chain_complete
            && asset_chain_complete
            && followup_chain_complete
            && execution_surface_chain_complete
        {
            metadata.insert("skill_loading_contract_complete", "true");
        }

        let skill_loading_surface_note_core_complete = metadata.contains_all(&[
            META_MATCHED_SKILL_MANUALS,
            META_READ_SKILL_MANUALS,
            META_SKILL_MANUAL_GATE_ACTIVE,
        ]);
        if skill_loading_surface_note_core_complete {
            metadata.insert("skill_loading_surface_note_core_complete", "true");
        }
        if skill_loading_surface_note_core_complete
            && metadata.contains_key(META_MATCHED_SKILL_ASSETS)
            && metadata.contains_key(META_READ_SKILL_ASSETS)
            && metadata.contains_key(META_SKILL_ASSET_GATE_ACTIVE)
            && metadata.contains_key(META_SKILL_ASSET_FOLLOWUPS)
            && metadata.contains_key(META_SKILL_ASSET_EXECUTION_SURFACES)
        {
            metadata.insert("skill_loading_surface_note_complete", "true");
        }
        if surface_contract_core_complete {
            metadata.insert("skill_surface_contract_core_complete", "true");
        }
        if surface_contract_complete {
            metadata.insert("skill_surface_contract_complete", "true");
        }
    }

    fn apply_runtime_middleware_trace_metadata(
        &self,
        metadata: &mut RunTraceMetadataBuilder,
        hook_capture: &RuntimeHookCapture,
    ) {
        let media_runtime_metadata = self.collect_runtime_media_trace_metadata(hook_capture);
        let clarification_runtime_metadata =
            self.collect_runtime_clarification_trace_metadata(hook_capture);
        let provider_runtime_metadata = self.collect_runtime_provider_trace_metadata(hook_capture);
        let forge_runtime_metadata = self.collect_runtime_forge_trace_metadata(hook_capture);
        let mut deferred_visible_count = None;
        let mut deferred_total_count = None;
        let mut deferred_deferred_count = None;
        let mut tool_error_tools = BTreeSet::new();
        let mut tool_error_surface_tools = BTreeSet::new();
        let mut degraded_tool_names = BTreeSet::new();
        let mut loop_guard_tools = BTreeSet::new();
        let mut post_run_summary = None;
        let mut runtime_finish_reason = None;
        let mut subagent_budget_fields = std::collections::HashMap::new();
        let mut deferred_tool_surface_note_present = false;
        let mut deferred_tool_surface_note_complete = false;
        let mut subagent_budget_surface_note_present = false;
        let mut subagent_budget_surface_note_complete = false;
        let mut title_surface_note_present = false;
        let mut title_surface_note_complete = false;
        let mut summarization_surface_note_present = false;
        let mut summarization_surface_note_complete = false;
        let mut memory_session_surface_note_present = false;
        let mut memory_session_surface_note_complete = false;
        let mut verification_surface_note_present = false;
        let mut verification_surface_note_complete = false;
        let mut engram_windows_native_surface_note_present = false;
        let mut engram_windows_native_surface_note_complete = false;

        for note in &hook_capture.notes {
            if let Some(chat_route) = note.strip_prefix("before_llm:chat_route:") {
                metadata.insert("chat_route", chat_route.trim().to_string());
            } else if let Some(tool_surface_mode) =
                note.strip_prefix("before_llm:tool_surface_mode:")
            {
                metadata.insert("tool_surface_mode", tool_surface_mode.trim().to_string());
            } else if let Some(ownership) = note.strip_prefix("before_llm:ownership:") {
                for part in ownership.split(':') {
                    if let Some((key, value)) = part.split_once('=') {
                        let metadata_key = match key {
                            "visible" => Some(META_VISIBLE_OWNER),
                            "memory" => Some(META_MEMORY_OWNER),
                            "approval" => Some(META_APPROVAL_OWNER),
                            _ => None,
                        };
                        if let Some(metadata_key) = metadata_key {
                            metadata
                                .as_mut_map()
                                .entry(metadata_key.to_string())
                                .or_insert_with(|| value.to_string());
                        }
                    }
                }
            } else if let Some(filter) = note.strip_prefix("before_llm:deferred_tool_filter:") {
                deferred_tool_surface_note_present = true;
                let mut parts = filter.split(":deferred=");
                if let Some(visible_total) = parts.next() {
                    if let Some((visible, total)) = visible_total.split_once('/') {
                        deferred_visible_count = Some(visible.to_string());
                        deferred_total_count = Some(total.to_string());
                    }
                }
                if let Some(deferred) = parts.next() {
                    deferred_deferred_count = Some(deferred.to_string());
                }
                deferred_tool_surface_note_complete = deferred_visible_count.is_some()
                    && deferred_total_count.is_some()
                    && deferred_deferred_count.is_some();
            } else if let Some(tool) = note
                .strip_prefix("tool_error:")
                .and_then(|value| value.split_once(':').map(|(tool, _)| tool))
            {
                if !tool.is_empty() {
                    tool_error_tools.insert(tool.to_string());
                }
            } else if let Some(tool) = note.strip_prefix("tool_error_surface:") {
                if !tool.trim().is_empty() {
                    tool_error_surface_tools.insert(tool.trim().to_string());
                }
            } else if let Some(tool) = note
                .strip_prefix("tool_degradation:")
                .and_then(|value| value.split_once(':').map(|(tool, _)| tool))
            {
                if !tool.is_empty() {
                    degraded_tool_names.insert(tool.to_string());
                }
            } else if let Some(tool) = note
                .strip_prefix("loop_guard:")
                .and_then(|value| value.split_once(':').map(|(tool, _)| tool))
            {
                if !tool.is_empty() {
                    loop_guard_tools.insert(tool.to_string());
                }
            } else if let Some(summary) = note.strip_prefix("post_run_eval:") {
                if !summary.trim().is_empty() {
                    post_run_summary = Some(summary.trim().to_string());
                    summarization_surface_note_present = true;
                    summarization_surface_note_complete = true;
                }
            } else if let Some(reason) = note.strip_prefix("after_llm:finish:") {
                if !reason.trim().is_empty() {
                    runtime_finish_reason = Some(reason.trim().to_string());
                }
            } else if let Some(budget) = note.strip_prefix("before_response:subagent_budget:") {
                subagent_budget_surface_note_present = true;
                subagent_budget_fields = parse_colon_fields(budget);
                subagent_budget_surface_note_complete = surface_has_nonempty_fields(
                    &subagent_budget_fields,
                    SUBAGENT_BUDGET_REQUIRED_FIELDS,
                );
            } else if let Some(surface) = note.strip_prefix("before_response:title:") {
                title_surface_note_present = true;
                let fields = parse_colon_fields(surface);
                title_surface_note_complete =
                    surface_has_nonempty_fields(&fields, TITLE_SURFACE_REQUIRED_FIELDS);
            } else if let Some(surface) =
                note.strip_prefix("before_response:memory_session_surface:")
            {
                memory_session_surface_note_present = true;
                let fields = parse_colon_fields(surface);
                memory_session_surface_note_complete =
                    surface_has_nonempty_fields(&fields, MEMORY_SESSION_SURFACE_REQUIRED_FIELDS);
            } else if metadata
                .project_trimmed_note_value(note, BEFORE_RESPONSE_NOTE_METADATA_PROJECTIONS)
            {
            } else if let Some(surface) = note.strip_prefix("before_response:verification_surface:")
            {
                verification_surface_note_present = true;
                let fields = parse_colon_fields(surface);
                verification_surface_note_complete =
                    surface_has_nonempty_fields(&fields, VERIFICATION_SURFACE_REQUIRED_FIELDS)
                        && fields.get("complete").is_some_and(|value| value == "true");
            } else if metadata
                .project_trimmed_note_value(note, ENGRAM_WINDOWS_NATIVE_NOTE_METADATA_PROJECTIONS)
            {
            } else if let Some(surface) =
                note.strip_prefix("before_response:engram_windows_native_surface:")
            {
                engram_windows_native_surface_note_present = true;
                let fields = parse_colon_fields(surface);
                engram_windows_native_surface_note_complete =
                    surface_has_true_fields(&fields, ENGRAM_WINDOWS_NATIVE_SURFACE_REQUIRED_FIELDS);
            }
        }
        metadata.extend(media_runtime_metadata);
        metadata.extend(clarification_runtime_metadata);
        metadata.extend(provider_runtime_metadata);
        metadata.extend(forge_runtime_metadata);

        metadata.insert_if_some("deferred_tool_visible_count", deferred_visible_count);
        metadata.insert_if_some("deferred_tool_total_count", deferred_total_count);
        if let Some(value) = deferred_deferred_count {
            metadata.insert("deferred_tool_deferred_count", value);
            metadata.insert("deferred_tool_filter_active", "true");
        }
        metadata.insert_true_fields(&[
            (
                "deferred_tool_surface_note_present",
                deferred_tool_surface_note_present,
            ),
            (
                "deferred_tool_surface_note_complete",
                deferred_tool_surface_note_complete,
            ),
            (
                "subagent_budget_surface_note_present",
                subagent_budget_surface_note_present,
            ),
            (
                "subagent_budget_surface_note_complete",
                subagent_budget_surface_note_complete,
            ),
            ("title_surface_note_present", title_surface_note_present),
            ("title_surface_note_complete", title_surface_note_complete),
            (
                "summarization_surface_note_present",
                summarization_surface_note_present,
            ),
            (
                "summarization_surface_note_complete",
                summarization_surface_note_complete,
            ),
            (
                "verification_surface_note_present",
                verification_surface_note_present,
            ),
            (
                "verification_surface_note_complete",
                verification_surface_note_complete,
            ),
            (
                "engram_windows_native_surface_note_present",
                engram_windows_native_surface_note_present,
            ),
            (
                "engram_windows_native_surface_note_complete",
                engram_windows_native_surface_note_complete,
            ),
        ]);
        metadata.insert_joined_set("tool_error_tools", tool_error_tools);
        metadata.insert_joined_set("tool_error_surface_tools", tool_error_surface_tools);
        self.finalize_tool_error_trace_metadata(metadata);
        metadata.insert_joined_set("degraded_tool_names", degraded_tool_names);
        metadata.insert_joined_set("loop_guard_tools", loop_guard_tools);
        if let Some(summary) = post_run_summary {
            metadata.insert("post_run_summary", summary);
        }
        if let Some(reason) = runtime_finish_reason {
            metadata.insert("runtime_finish_reason", reason);
        }
        metadata.insert_values_from_map(&subagent_budget_fields, SUBAGENT_BUDGET_METADATA_MAPPINGS);
        self.finalize_memory_session_trace_metadata(
            metadata,
            hook_capture,
            memory_session_surface_note_present,
            memory_session_surface_note_complete,
        );
    }

    fn apply_tactical_slm_trace_metadata(&self, metadata: &mut RunTraceMetadataBuilder) {
        let extra_params = self.config.extra_params.as_ref();
        insert_extra_param_metadata(metadata, extra_params, RUNTIME_BUDGET_METADATA_MAPPINGS);
        if !extra_param_bool(extra_params, "tactical_slm_present") {
            return;
        }

        metadata.insert("tactical_slm_present", "true");
        metadata.insert_if_some(
            "tactical_slm_model_id",
            extra_param_string(extra_params, "tactical_slm_model_id"),
        );
        metadata.insert_if_some(
            "tactical_slm_factory_id",
            extra_param_string(extra_params, "tactical_slm_factory_id"),
        );
        metadata.insert_if_some(
            "tactical_slm_source",
            extra_param_string(extra_params, "tactical_slm_source"),
        );
        metadata.insert_if_some(
            "tactical_slm_roles",
            extra_param_string(extra_params, "tactical_slm_roles"),
        );

        if metadata.contains_all(TACTICAL_SLM_REQUIRED_KEYS) {
            metadata.insert("tactical_slm_contract_complete", "true");
        }
    }

    fn apply_background_trace_metadata(&self, metadata: &mut RunTraceMetadataBuilder) {
        let background_stats = self.background_runtime_stats.read().clone();
        if background_stats.total_attempts > 0 {
            metadata.insert(
                "background_total_attempts",
                background_stats.total_attempts.to_string(),
            );
            metadata.insert(
                "background_skip_count",
                background_stats.skip_count.to_string(),
            );
            metadata.insert(
                "background_reject_count",
                background_stats.reject_count.to_string(),
            );
            metadata.insert(
                "background_refresh_session_count",
                background_stats.refresh_session_count.to_string(),
            );
            metadata.insert(
                "background_promote_relationship_count",
                background_stats.promote_relationship_count.to_string(),
            );
            metadata.insert(
                "background_rewrite_count",
                background_stats.rewrite_count.to_string(),
            );
        }

        let background_guard = self.background_envelope.read();
        let Some(background) = background_guard.as_ref() else {
            metadata.insert("background_present", "false");
            return;
        };
        if background.is_empty() {
            metadata.insert("background_present", "false");
            return;
        }

        metadata.insert("background_present", "true");
        metadata.insert(
            "background_revision",
            background.revision.revision.to_string(),
        );
        metadata.insert_if_some(
            "background_previous_revision",
            background
                .revision
                .previous_revision
                .map(|value| value.to_string()),
        );
        metadata.insert_if_some(
            "background_update_reason",
            background.revision.update_reason.clone(),
        );
        metadata.insert(
            "background_quality_signal",
            format!("{:?}", background.quality_signal).to_lowercase(),
        );
        metadata.insert(
            "background_persona_present",
            background
                .persona_layer
                .as_ref()
                .is_some_and(|layer| !layer.is_empty())
                .to_string(),
        );
        metadata.insert(
            "background_relationship_present",
            background
                .relationship_layer
                .as_ref()
                .is_some_and(|layer| !layer.is_empty())
                .to_string(),
        );
        metadata.insert(
            "background_session_present",
            background
                .session_layer
                .as_ref()
                .is_some_and(|layer| !layer.is_empty())
                .to_string(),
        );
        metadata.insert(
            "background_recent_window_present",
            background
                .recent_window_summary
                .as_ref()
                .is_some_and(|summary| !summary.is_empty())
                .to_string(),
        );
        metadata.insert(
            "background_source_ref_count",
            background.source_refs.len().to_string(),
        );
        metadata.insert_if_some(
            "background_compression_reason",
            background.compression_reason.clone(),
        );
        if let Some(decision) = background.metadata.get("background_decision") {
            metadata.insert("background_decision", decision.clone());
        }
        if let Some(used_slm) = background.metadata.get("background_used_slm") {
            metadata.insert("background_used_slm", used_slm.clone());
        }
        if let Some(status) = background
            .metadata
            .get("background_session_persistence_status")
        {
            metadata.insert("background_session_persistence_status", status.clone());
        }
        if let Some(error) = background
            .metadata
            .get("background_session_persistence_error")
        {
            metadata.insert("background_session_persistence_error", error.clone());
        }
        if let Some(pending) = background.metadata.get("durable_promotion_pending") {
            metadata.insert("background_durable_promotion_pending", pending.clone());
        }
        if let Some(status) = background.metadata.get("durable_promotion_status") {
            metadata.insert("background_durable_promotion_status", status.clone());
        }
        if let Some(error) = background.metadata.get("durable_promotion_error") {
            metadata.insert("background_durable_promotion_error", error.clone());
        }
        if let Some(reason) = background.metadata.get("background_review_reason") {
            metadata.insert("background_review_reason", reason.clone());
        }
        if let Some(source) = background.metadata.get("background_review_source") {
            metadata.insert("background_review_source", source.clone());
        }
        for key in [
            "background_total_attempts",
            "background_skip_count",
            "background_reject_count",
            "background_refresh_session_count",
            "background_promote_relationship_count",
            "background_rewrite_count",
        ] {
            if let Some(value) = background.metadata.get(key) {
                metadata.insert(key, value.clone());
            }
        }

        if metadata.contains_all(BACKGROUND_CONTRACT_REQUIRED_KEYS) {
            metadata.insert("background_contract_complete", "true");
        }
    }

    fn apply_context_occupancy_trace_metadata(&self, metadata: &mut RunTraceMetadataBuilder) {
        let Some(metrics) = self.context_manager.latest_context_metrics() else {
            return;
        };

        metadata.insert(
            "context_max_window_tokens",
            metrics.max_window_tokens.to_string(),
        );
        metadata.insert(
            "context_reserved_response_tokens",
            metrics.reserved_response_tokens.to_string(),
        );
        metadata.insert(
            "context_safety_margin_tokens",
            metrics.safety_margin_tokens.to_string(),
        );
        metadata.insert(
            "context_history_budget_tokens",
            metrics.history_budget_tokens.to_string(),
        );
        metadata.insert(
            "context_static_prefix_tokens",
            metrics.static_prefix_tokens.to_string(),
        );
        metadata.insert(
            "context_provisional_background_tokens",
            metrics.provisional_background_tokens.to_string(),
        );
        metadata.insert(
            "context_effective_background_tokens",
            metrics.effective_background_tokens.to_string(),
        );
        metadata.insert(
            "context_dynamic_injection_tokens",
            metrics.dynamic_injection_tokens.to_string(),
        );
        metadata.insert(
            "context_selected_history_tokens",
            metrics.selected_history_tokens.to_string(),
        );
        metadata.insert(
            "context_pruned_history_tokens",
            metrics.pruned_history_tokens.to_string(),
        );
        metadata.insert(
            "context_estimated_prefix_tokens",
            metrics.estimated_prefix_tokens.to_string(),
        );
        metadata.insert(
            "context_estimated_final_prompt_tokens",
            metrics.estimated_final_prompt_tokens.to_string(),
        );
        metadata.insert(
            "context_effective_max_history_messages",
            metrics.effective_max_history_messages.to_string(),
        );
        metadata.insert(
            "context_selected_history_messages",
            metrics.selected_history_messages.to_string(),
        );
        metadata.insert(
            "context_pruned_history_messages",
            metrics.pruned_history_messages.to_string(),
        );
        metadata.insert(
            "context_dynamic_injection_messages",
            metrics.dynamic_injection_messages.to_string(),
        );
        metadata.insert(
            "context_background_message_count",
            metrics.background_message_count.to_string(),
        );
        metadata.insert(
            "context_background_occupancy_ratio",
            format!("{:.4}", metrics.background_occupancy_ratio),
        );
        metadata.insert(
            "context_prompt_occupancy_ratio",
            format!("{:.4}", metrics.prompt_occupancy_ratio),
        );
        metadata.insert(
            "context_pressure_band",
            metrics.pressure_band.as_str().to_string(),
        );
        metadata.insert(
            "context_local_provider_mode",
            metrics.local_provider_mode.to_string(),
        );
    }

    pub(crate) fn build_run_trace(
        &self,
        seed: &RuntimeExecutionSeed,
        outcome: &ChatOutcome,
        messages: &[Message],
    ) -> RunTrace {
        let finished_at = Utc::now();
        let session_uuid = seed
            .session_id
            .as_deref()
            .and_then(|value| Uuid::parse_str(value).ok())
            .unwrap_or_else(Uuid::nil);

        let mut metadata = RunTraceMetadataBuilder::new();
        if let Some(session_id) = &seed.session_id {
            metadata.insert("session_id", session_id.clone());
        }
        metadata.insert("thread_id", seed.thread_id.clone());
        metadata.insert("trace_id", seed.run_id.to_string());
        metadata.insert("run_id", seed.run_id.to_string());
        metadata.insert("task_id", seed.task_id.to_string());
        metadata.insert("agent_name", self.config.name.clone());
        let hook_capture = self.runtime_hook_capture.read().clone();
        metadata.insert_scalar_fields(&[
            (
                "hook_trace_injection_count",
                hook_capture.trace_injection_count,
            ),
            ("hook_pre_llm_tap_count", hook_capture.pre_llm_tap_count),
            (
                "hook_memory_surface_count",
                hook_capture.memory_surface_count,
            ),
            ("hook_loop_abort_count", hook_capture.loop_abort_count),
            ("hook_post_llm_tap_count", hook_capture.post_llm_tap_count),
            (
                "hook_clarification_surface_count",
                hook_capture.clarification_surface_count,
            ),
            (
                "hook_skill_manual_read_count",
                hook_capture.skill_manual_read_count,
            ),
            (
                "hook_skill_asset_read_count",
                hook_capture.skill_asset_read_count,
            ),
            ("hook_media_surface_count", hook_capture.media_surface_count),
            ("hook_forge_surface_count", hook_capture.forge_surface_count),
            (
                "hook_degraded_tool_call_count",
                hook_capture.degraded_tool_call_count,
            ),
            ("hook_tool_error_count", hook_capture.tool_error_count),
            (
                "hook_subagent_surface_count",
                hook_capture.subagent_surface_count,
            ),
            ("hook_title_surface_count", hook_capture.title_surface_count),
            (
                "hook_dangling_tool_call_count",
                hook_capture.dangling_tool_call_count,
            ),
            (
                "hook_summarization_surface_count",
                hook_capture.summarization_surface_count,
            ),
            ("hook_post_run_tap_count", hook_capture.post_run_tap_count),
        ]);
        metadata.insert_true(
            "hook_runtime_refs_injected",
            hook_capture.trace_injection_count > 0,
        );
        metadata.insert_true("handover", outcome.handover.is_some());
        if let Some((session_title, title_source)) =
            runtime_session_title(self.config.extra_params.as_ref())
        {
            metadata.insert("session_title", session_title);
            metadata.insert("session_title_source", title_source.to_string());
            metadata.insert("session_title_present", "true");
        } else {
            metadata.insert("session_title_source", "missing");
            metadata.insert("session_title_present", "false");
        }
        self.apply_windows_native_trace_metadata(metadata.as_mut_map());
        self.apply_tactical_slm_trace_metadata(&mut metadata);
        self.apply_background_trace_metadata(&mut metadata);
        self.apply_context_occupancy_trace_metadata(&mut metadata);
        self.apply_skill_loading_trace_metadata(&mut metadata, &hook_capture);
        self.apply_runtime_middleware_trace_metadata(&mut metadata, &hook_capture);
        self.apply_forge_closed_loop_trace_metadata(metadata.as_mut_map(), outcome);
        self.apply_clarification_trace_metadata(metadata.as_mut_map(), messages);

        self.finalize_runtime_evidence_trace_metadata(&mut metadata);

        let tools = outcome
            .tool_calls
            .iter()
            .map(|call| ToolTrace {
                call_id: format!("{}-{}", call.name, call.timestamp),
                tool_name: call.name.clone(),
                status: if call.result.is_some() {
                    TraceStatus::Succeeded
                } else {
                    TraceStatus::Failed
                },
                started_at: DateTime::<Utc>::from_timestamp_millis(call.timestamp as i64)
                    .unwrap_or(finished_at),
                finished_at: Some(
                    DateTime::<Utc>::from_timestamp_millis(
                        call.timestamp.saturating_add(call.duration_ms) as i64,
                    )
                    .unwrap_or(finished_at),
                ),
                duration_ms: Some(call.duration_ms),
                input: Some(serde_json::json!({ "args": call.args })),
                output: call
                    .result
                    .as_ref()
                    .map(|result| serde_json::json!({ "result": result })),
                error: None,
                degraded: false,
            })
            .collect();

        RunTrace {
            run_id: seed.run_id,
            session_id: session_uuid,
            agent_id: self.config.name.clone(),
            status: TraceStatus::Succeeded,
            started_at: seed.started_at,
            finished_at: Some(finished_at),
            task_id: Some(seed.task_id),
            thread_id: Some(seed.thread_id.clone()),
            provider: None,
            model: Some(self.config.model.clone()),
            prompt_tokens: None,
            completion_tokens: None,
            stages: self.build_runtime_stage_traces(seed, outcome, finished_at, metadata.as_map()),
            tools,
            artifacts: Vec::new(),
            degradation_notes: hook_capture.notes,
            witness: None,
            metadata: metadata.into_inner(),
        }
    }
}
