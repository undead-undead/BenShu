use super::core::Agent;
use super::trace_metadata_helpers::{
    copy_present_keys, metadata_has_all, metadata_has_any, metadata_is_true, parse_colon_fields,
    surface_has_nonempty_fields, surface_has_true_fields,
};
use crate::agent::message::Message;
use crate::agent::provider::Provider;
use crate::hooks::RuntimeHookCapture;
use benshu_telemetry::RuntimeStage;
use std::collections::HashMap;

const CLARIFICATION_ACTIVITY_KEYS: &[&str] = &[
    "session_status",
    "clarification_prompt",
    "clarification_original_request",
    "clarification_event",
];
const EGRESS_CLARIFICATION_KEYS: &[&str] = &[
    "clarification_event",
    "clarification_contract_core_complete",
    "clarification_contract_complete",
    "clarification_surface_note_present",
    "clarification_surface_note_complete",
    "clarification_awaiting_seen",
    "clarification_terminal_event_seen",
    "clarification_roundtrip_complete",
    "clarification_status_kind",
    "clarification_failure_reason",
    "clarification_session_status_json_present",
    "clarification_session_status_json_valid",
];
const CLARIFICATION_MESSAGE_KEYS: &[&str] = &[
    "session_status",
    "session_status_json",
    "clarification_prompt",
    "clarification_original_request",
    "clarification_status_kind",
    "clarification_failure_reason",
    "clarification_status_surface",
    "clarification_resolved",
    "clarification_cancelled",
];
const CLARIFICATION_CORE_KEYS: &[&str] = &[
    "session_status",
    "clarification_prompt",
    "clarification_original_request",
    "clarification_status_kind",
];

impl<P: Provider + 'static> Agent<P> {
    pub(crate) fn apply_clarification_before_response_metadata(
        &self,
        metadata: &mut HashMap<String, String>,
        messages: &[Message],
    ) {
        self.apply_clarification_trace_metadata(metadata, messages);
    }

    pub(crate) fn clarification_activity_present(
        &self,
        metadata: &HashMap<String, String>,
    ) -> bool {
        metadata_has_any(metadata, CLARIFICATION_ACTIVITY_KEYS)
    }

    pub(crate) fn clarification_runtime_evidence_core_complete(
        &self,
        metadata: &HashMap<String, String>,
    ) -> bool {
        !self.clarification_activity_present(metadata)
            || metadata_is_true(metadata, "clarification_contract_core_complete")
    }

    pub(crate) fn clarification_runtime_evidence_complete(
        &self,
        metadata: &HashMap<String, String>,
    ) -> bool {
        !self.clarification_activity_present(metadata)
            || (metadata_is_true(metadata, "clarification_contract_complete")
                && metadata_is_true(metadata, "clarification_surface_note_complete"))
    }

    pub(crate) fn apply_clarification_stage_runtime_metadata(
        &self,
        stage: RuntimeStage,
        stage_metadata: &mut HashMap<String, String>,
        runtime_metadata: &HashMap<String, String>,
    ) {
        if stage == RuntimeStage::Egress {
            copy_present_keys(stage_metadata, runtime_metadata, EGRESS_CLARIFICATION_KEYS);
        }
    }

    pub(crate) fn collect_runtime_clarification_trace_metadata(
        &self,
        hook_capture: &RuntimeHookCapture,
    ) -> HashMap<String, String> {
        let mut metadata = HashMap::new();
        let mut clarification_surface_note_present = false;
        let mut clarification_surface_note_complete = false;

        for note in &hook_capture.notes {
            if let Some(surface) = note.strip_prefix("before_response:clarification_surface:") {
                clarification_surface_note_present = true;
                let fields = parse_colon_fields(surface);
                clarification_surface_note_complete =
                    surface_has_nonempty_fields(&fields, &["status", "event"])
                        && surface_has_true_fields(
                            &fields,
                            &["prompt_present", "original_present", "json_valid"],
                        );
            }
        }

        if clarification_surface_note_present {
            metadata.insert(
                "clarification_surface_note_present".to_string(),
                "true".to_string(),
            );
        }
        if clarification_surface_note_complete {
            metadata.insert(
                "clarification_surface_note_complete".to_string(),
                "true".to_string(),
            );
        }

        metadata
    }

    pub(crate) fn apply_clarification_trace_metadata(
        &self,
        metadata: &mut HashMap<String, String>,
        messages: &[Message],
    ) {
        let mut clarification_awaiting_seen = false;
        let mut clarification_terminal_event_seen = false;
        for message in messages {
            if message.metadata.get("session_status").map(String::as_str)
                == Some("awaiting_clarification")
                || message
                    .metadata
                    .get("clarification_status_kind")
                    .map(String::as_str)
                    == Some("awaiting_clarification")
            {
                clarification_awaiting_seen = true;
            }
            if message.metadata.contains_key("clarification_resolved")
                || message.metadata.contains_key("clarification_cancelled")
            {
                clarification_terminal_event_seen = true;
            }
        }

        for message in messages.iter().rev() {
            let has_clarification_fields = message.metadata.contains_key("clarification_prompt")
                || message
                    .metadata
                    .contains_key("clarification_original_request")
                || message.metadata.contains_key("clarification_status_kind")
                || message
                    .metadata
                    .contains_key("clarification_failure_reason")
                || message
                    .metadata
                    .contains_key("clarification_status_surface")
                || message.metadata.contains_key("clarification_resolved")
                || message.metadata.contains_key("clarification_cancelled")
                || message.metadata.contains_key("session_status_json")
                || message.metadata.contains_key("session_status");

            if !has_clarification_fields {
                continue;
            }

            metadata.insert(
                "clarification_surface_note_present".to_string(),
                "true".to_string(),
            );

            copy_present_keys(metadata, &message.metadata, CLARIFICATION_MESSAGE_KEYS);
            if let Some(encoded) = message.metadata.get("session_status_json") {
                metadata.insert(
                    "clarification_session_status_json_present".to_string(),
                    "true".to_string(),
                );
                metadata.insert(
                    "clarification_session_status_json_valid".to_string(),
                    serde_json::from_str::<serde_json::Value>(encoded)
                        .map(|_| "true".to_string())
                        .unwrap_or_else(|_| "false".to_string()),
                );
            }
            let clarification_event = if message.metadata.contains_key("clarification_cancelled") {
                Some("cancelled")
            } else if message.metadata.contains_key("clarification_resolved") {
                Some("resolved")
            } else if message
                .metadata
                .contains_key("clarification_status_surface")
            {
                Some("status_surface")
            } else if message.metadata.get("session_status").map(String::as_str)
                == Some("awaiting_clarification")
            {
                Some("awaiting")
            } else {
                None
            };
            if let Some(event) = clarification_event {
                metadata.insert("clarification_event".to_string(), event.to_string());
            }
            let clarification_surface_note_complete =
                message.metadata.contains_key("session_status")
                    && message.metadata.contains_key("clarification_prompt")
                    && message
                        .metadata
                        .contains_key("clarification_original_request")
                    && message.metadata.contains_key("clarification_status_kind")
                    && (message.metadata.contains_key("clarification_cancelled")
                        || message.metadata.contains_key("clarification_resolved")
                        || message
                            .metadata
                            .contains_key("clarification_status_surface")
                        || message.metadata.get("session_status").map(String::as_str)
                            == Some("awaiting_clarification"));
            if clarification_surface_note_complete {
                metadata.insert(
                    "clarification_surface_note_complete".to_string(),
                    "true".to_string(),
                );
            }
            if clarification_awaiting_seen {
                metadata.insert(
                    "clarification_awaiting_seen".to_string(),
                    "true".to_string(),
                );
            }
            if clarification_terminal_event_seen {
                metadata.insert(
                    "clarification_terminal_event_seen".to_string(),
                    "true".to_string(),
                );
            }
            if clarification_awaiting_seen && clarification_terminal_event_seen {
                metadata.insert(
                    "clarification_roundtrip_complete".to_string(),
                    "true".to_string(),
                );
            }
            let clarification_contract_core_complete =
                metadata_has_all(&metadata, CLARIFICATION_CORE_KEYS);
            if clarification_contract_core_complete {
                metadata.insert(
                    "clarification_contract_core_complete".to_string(),
                    "true".to_string(),
                );
            }
            if clarification_contract_core_complete
                && metadata_is_true(&metadata, "clarification_session_status_json_present")
                && metadata_is_true(&metadata, "clarification_session_status_json_valid")
                && metadata.contains_key("clarification_event")
            {
                metadata.insert(
                    "clarification_contract_complete".to_string(),
                    "true".to_string(),
                );
            }
            break;
        }
    }
}
