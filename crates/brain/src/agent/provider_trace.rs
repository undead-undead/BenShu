use super::core::Agent;
use super::trace_metadata_helpers::{
    capture_trimmed_pair_from_note, capture_trimmed_value_from_note, copy_present_keys,
    metadata_has_all, metadata_has_any, metadata_is_true,
};
use crate::agent::provider::Provider;
use crate::hooks::RuntimeHookCapture;
use benshu_telemetry::RuntimeStage;
use std::collections::HashMap;

const PROVIDER_ACTIVITY_KEYS: &[&str] = &[
    "provider_name",
    "provider_model",
    "provider_finish_reason",
    "provider_tool_call_count",
];
const PROVIDER_CORE_KEYS: &[&str] = &[
    "provider_name",
    "provider_model",
    "provider_finish_reason",
    "provider_tool_call_count",
    "provider_tool_contract_mode",
    "provider_mainline_stability",
];
const PROVIDER_USAGE_KEYS: &[&str] = &[
    "provider_prompt_tokens",
    "provider_completion_tokens",
    "provider_total_tokens",
];
const REASONING_PROVIDER_KEYS: &[&str] = &[
    "provider_name",
    "provider_model",
    "provider_latency_ms",
    "provider_prompt_tokens",
    "provider_completion_tokens",
    "provider_total_tokens",
    "provider_finish_reason",
    "provider_tool_call_count",
    "provider_tool_contract_mode",
    "provider_mainline_stability",
    "provider_contract_core_complete",
    "provider_usage_complete",
    "provider_contract_complete",
    "provider_surface_note_core_complete",
    "provider_surface_note_complete",
    "provider_continuation_mode",
    "provider_continuation_cache_source",
    "provider_continuation_prompt_tokens",
    "provider_continuation_prefill_ms",
    "provider_continuation_decode_ms",
    "provider_continuation_miss_reason",
    "provider_continuation_tool_exact_replay_used",
    "provider_continuation_protocol_live_used",
    "runtime_continuation_user_session_id",
    "runtime_continuation_turn_id",
    "runtime_continuation_worker_run_id",
    "runtime_continuation_frontier_id",
    "runtime_continuation_visible_prompt_fingerprint",
];

const PROVIDER_CONTINUATION_NOTE_KEYS: &[(&str, &str)] = &[
    (
        "before_llm:runtime_continuation_user_session_id:",
        "runtime_continuation_user_session_id",
    ),
    (
        "before_llm:runtime_continuation_turn_id:",
        "runtime_continuation_turn_id",
    ),
    (
        "before_llm:runtime_continuation_worker_run_id:",
        "runtime_continuation_worker_run_id",
    ),
    (
        "before_llm:runtime_continuation_frontier_id:",
        "runtime_continuation_frontier_id",
    ),
    (
        "before_llm:runtime_continuation_visible_prompt_fingerprint:",
        "runtime_continuation_visible_prompt_fingerprint",
    ),
    (
        "after_llm:runtime_continuation_user_session_id:",
        "runtime_continuation_user_session_id",
    ),
    (
        "after_llm:runtime_continuation_turn_id:",
        "runtime_continuation_turn_id",
    ),
    (
        "after_llm:runtime_continuation_worker_run_id:",
        "runtime_continuation_worker_run_id",
    ),
    (
        "after_llm:runtime_continuation_frontier_id:",
        "runtime_continuation_frontier_id",
    ),
    (
        "after_llm:runtime_continuation_visible_prompt_fingerprint:",
        "runtime_continuation_visible_prompt_fingerprint",
    ),
    (
        "after_llm:provider_continuation_mode:",
        "provider_continuation_mode",
    ),
    (
        "after_llm:provider_continuation_cache_source:",
        "provider_continuation_cache_source",
    ),
    (
        "after_llm:provider_continuation_prompt_tokens:",
        "provider_continuation_prompt_tokens",
    ),
    (
        "after_llm:provider_continuation_prefill_ms:",
        "provider_continuation_prefill_ms",
    ),
    (
        "after_llm:provider_continuation_decode_ms:",
        "provider_continuation_decode_ms",
    ),
    (
        "after_llm:provider_continuation_miss_reason:",
        "provider_continuation_miss_reason",
    ),
    (
        "after_llm:provider_continuation_tool_exact_replay_used:",
        "provider_continuation_tool_exact_replay_used",
    ),
    (
        "after_llm:provider_continuation_protocol_live_used:",
        "provider_continuation_protocol_live_used",
    ),
];

fn insert_if_some(metadata: &mut HashMap<String, String>, key: &str, value: Option<String>) {
    if let Some(value) = value {
        metadata.insert(key.to_string(), value);
    }
}

impl<P: Provider + 'static> Agent<P> {
    pub(crate) fn apply_provider_before_llm_metadata(
        &self,
        metadata: &mut HashMap<String, String>,
        provider_name: &str,
        provider_model: &str,
        requested_tool_count: usize,
        total_tool_count: usize,
        deferred_tool_count: usize,
    ) {
        metadata.insert("provider_name".to_string(), provider_name.to_string());
        metadata.insert("provider_model".to_string(), provider_model.to_string());
        metadata.insert(
            "requested_tool_count".to_string(),
            requested_tool_count.to_string(),
        );
        metadata.insert("total_tool_count".to_string(), total_tool_count.to_string());
        if deferred_tool_count > 0 {
            metadata.insert(
                "deferred_tool_count".to_string(),
                deferred_tool_count.to_string(),
            );
        }
    }

    pub(crate) fn provider_activity_present(&self, metadata: &HashMap<String, String>) -> bool {
        metadata_has_any(metadata, PROVIDER_ACTIVITY_KEYS)
    }

    pub(crate) fn provider_runtime_evidence_core_complete(
        &self,
        metadata: &HashMap<String, String>,
    ) -> bool {
        !self.provider_activity_present(metadata)
            || metadata_is_true(metadata, "provider_contract_core_complete")
    }

    pub(crate) fn provider_runtime_evidence_complete(
        &self,
        metadata: &HashMap<String, String>,
    ) -> bool {
        !self.provider_activity_present(metadata)
            || (metadata_is_true(metadata, "provider_contract_complete")
                && metadata_is_true(metadata, "provider_surface_note_complete"))
    }

    pub(crate) fn apply_provider_stage_runtime_metadata(
        &self,
        stage: RuntimeStage,
        stage_metadata: &mut HashMap<String, String>,
        runtime_metadata: &HashMap<String, String>,
    ) {
        if stage == RuntimeStage::Reasoning {
            copy_present_keys(stage_metadata, runtime_metadata, REASONING_PROVIDER_KEYS);

            let provider_contract_core_complete =
                metadata_has_all(stage_metadata, PROVIDER_CORE_KEYS);
            let provider_usage_complete = metadata_has_all(stage_metadata, PROVIDER_USAGE_KEYS);

            if provider_contract_core_complete {
                stage_metadata.insert(
                    "provider_contract_core_complete".to_string(),
                    "true".to_string(),
                );
            }
            if provider_usage_complete {
                stage_metadata.insert("provider_usage_complete".to_string(), "true".to_string());
            }
            if provider_contract_core_complete
                && provider_usage_complete
                && stage_metadata.contains_key("provider_latency_ms")
            {
                stage_metadata.insert("provider_contract_complete".to_string(), "true".to_string());
            }
        }
    }

    pub(crate) fn collect_runtime_provider_trace_metadata(
        &self,
        hook_capture: &RuntimeHookCapture,
    ) -> HashMap<String, String> {
        let mut metadata = HashMap::new();
        let mut provider_name = None;
        let mut provider_model = None;
        let mut provider_latency_ms = None;
        let mut provider_prompt_tokens = None;
        let mut provider_completion_tokens = None;
        let mut provider_total_tokens = None;
        let mut provider_finish_reason = None;
        let mut provider_tool_call_count = None;
        let mut provider_tool_contract_mode = None;
        let mut provider_mainline_stability = None;
        let mut provider_note_name_model = false;
        let mut provider_note_latency = false;
        let mut provider_note_prompt_tokens = false;
        let mut provider_note_completion_tokens = false;
        let mut provider_note_total_tokens = false;
        let mut provider_note_finish_reason = false;
        let mut provider_note_tool_call_count = false;
        let mut provider_note_tool_contract_mode = false;
        let mut provider_note_mainline_stability = false;
        let mut provider_continuation = HashMap::new();

        for note in &hook_capture.notes {
            if capture_trimmed_pair_from_note(
                note,
                "after_llm:provider:",
                &mut provider_name,
                &mut provider_model,
            ) {
                provider_note_name_model = provider_name.is_some() && provider_model.is_some();
            } else if capture_trimmed_value_from_note(
                note,
                "after_llm:provider_latency_ms:",
                &mut provider_latency_ms,
            ) {
                provider_note_latency = true;
            } else if capture_trimmed_value_from_note(
                note,
                "after_llm:provider_prompt_tokens:",
                &mut provider_prompt_tokens,
            ) {
                provider_note_prompt_tokens = true;
            } else if capture_trimmed_value_from_note(
                note,
                "after_llm:provider_completion_tokens:",
                &mut provider_completion_tokens,
            ) {
                provider_note_completion_tokens = true;
            } else if capture_trimmed_value_from_note(
                note,
                "after_llm:provider_total_tokens:",
                &mut provider_total_tokens,
            ) {
                provider_note_total_tokens = true;
            } else if capture_trimmed_value_from_note(
                note,
                "after_llm:provider_finish_reason:",
                &mut provider_finish_reason,
            ) {
                provider_note_finish_reason = true;
            } else if capture_trimmed_value_from_note(
                note,
                "after_llm:provider_tool_call_count:",
                &mut provider_tool_call_count,
            ) {
                provider_note_tool_call_count = true;
            } else if capture_trimmed_value_from_note(
                note,
                "after_llm:provider_tool_contract_mode:",
                &mut provider_tool_contract_mode,
            ) {
                provider_note_tool_contract_mode = true;
            } else if capture_trimmed_value_from_note(
                note,
                "after_llm:provider_mainline_stability:",
                &mut provider_mainline_stability,
            ) {
                provider_note_mainline_stability = true;
            } else {
                for (note_prefix, metadata_key) in PROVIDER_CONTINUATION_NOTE_KEYS {
                    if let Some(value) = note.strip_prefix(note_prefix) {
                        let trimmed = value.trim();
                        if !trimmed.is_empty() {
                            provider_continuation
                                .insert((*metadata_key).to_string(), trimmed.to_string());
                        }
                    }
                }
            }
        }

        insert_if_some(&mut metadata, "provider_name", provider_name);
        insert_if_some(&mut metadata, "provider_model", provider_model);
        insert_if_some(&mut metadata, "provider_latency_ms", provider_latency_ms);
        insert_if_some(
            &mut metadata,
            "provider_prompt_tokens",
            provider_prompt_tokens,
        );
        insert_if_some(
            &mut metadata,
            "provider_completion_tokens",
            provider_completion_tokens,
        );
        insert_if_some(
            &mut metadata,
            "provider_total_tokens",
            provider_total_tokens,
        );
        insert_if_some(
            &mut metadata,
            "provider_finish_reason",
            provider_finish_reason,
        );
        insert_if_some(
            &mut metadata,
            "provider_tool_call_count",
            provider_tool_call_count,
        );
        insert_if_some(
            &mut metadata,
            "provider_tool_contract_mode",
            provider_tool_contract_mode,
        );
        insert_if_some(
            &mut metadata,
            "provider_mainline_stability",
            provider_mainline_stability,
        );
        metadata.extend(provider_continuation);

        let provider_contract_core_complete = metadata_has_all(&metadata, PROVIDER_CORE_KEYS);
        let provider_usage_complete = metadata_has_all(&metadata, PROVIDER_USAGE_KEYS);
        if provider_contract_core_complete {
            metadata.insert(
                "provider_contract_core_complete".to_string(),
                "true".to_string(),
            );
        }
        if provider_usage_complete {
            metadata.insert("provider_usage_complete".to_string(), "true".to_string());
        }
        if provider_contract_core_complete
            && provider_usage_complete
            && metadata.contains_key("provider_latency_ms")
        {
            metadata.insert("provider_contract_complete".to_string(), "true".to_string());
        }

        let provider_surface_note_core_complete = provider_note_name_model
            && provider_note_finish_reason
            && provider_note_tool_call_count
            && provider_note_tool_contract_mode
            && provider_note_mainline_stability;
        if provider_surface_note_core_complete {
            metadata.insert(
                "provider_surface_note_core_complete".to_string(),
                "true".to_string(),
            );
        }
        if provider_surface_note_core_complete
            && provider_note_latency
            && provider_note_prompt_tokens
            && provider_note_completion_tokens
            && provider_note_total_tokens
        {
            metadata.insert(
                "provider_surface_note_complete".to_string(),
                "true".to_string(),
            );
        }

        metadata
    }
}
