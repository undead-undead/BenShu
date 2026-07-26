use super::core::Agent;
use super::trace_metadata_helpers::{
    capture_trimmed_value_from_note, collect_csv_set_values_from_note,
    collect_trimmed_set_values_from_note, copy_present_keys, insert_joined_set, metadata_has_all,
    metadata_has_any, metadata_matches,
};
use crate::agent::protocol::ChatOutcome;
use crate::agent::provider::Provider;
use crate::hooks::RuntimeHookCapture;
use benshu_telemetry::RuntimeStage;
use std::collections::{BTreeSet, HashMap};

const TOOL_PLANNING_FORGE_KEYS: &[&str] =
    &["forge_followup_candidates", "forge_followup_gate_active"];
const EXECUTION_FORGE_KEYS: &[&str] = &[
    "forge_followup_candidates",
    "forge_followup_gate_active",
    "forge_registered_tools",
    "forge_source",
    "forge_scope",
    "forge_execution_surfaces",
    "forge_capability_domains",
    "forge_smoke_statuses",
    "forge_smoke_latency_ms",
    "forge_cleanup_recorded",
    "forge_surface_present",
    "forge_contract_complete",
    "forge_followup_tools",
    "forge_followup_execution_happened",
    "forge_closed_loop_complete",
];
const FORGE_ACTIVITY_KEYS: &[&str] = &[
    "forge_registered_tools",
    "forge_source",
    "forge_scope",
    "forge_smoke_statuses",
    "forge_execution_surfaces",
];
const FORGE_CONTRACT_KEYS: &[&str] = &["forge_registered_tools", "forge_smoke_statuses"];

impl<P: Provider + 'static> Agent<P> {
    pub(crate) fn apply_forge_followup_before_llm_metadata(
        &self,
        metadata: &mut HashMap<String, String>,
        pending_forge_followup_tools: &[String],
    ) {
        if pending_forge_followup_tools.is_empty() {
            return;
        }

        metadata.insert(
            "forge_followup_tool_names".to_string(),
            pending_forge_followup_tools.join(","),
        );
        metadata.insert("forge_followup_gate_active".to_string(), "true".to_string());
    }

    pub(crate) fn forge_activity_present(&self, metadata: &HashMap<String, String>) -> bool {
        metadata_has_any(metadata, FORGE_ACTIVITY_KEYS)
    }

    pub(crate) fn forge_runtime_evidence_core_complete(
        &self,
        metadata: &HashMap<String, String>,
    ) -> bool {
        !self.forge_activity_present(metadata)
            || metadata_matches(metadata, "forge_closed_loop_complete", "true")
    }

    pub(crate) fn forge_runtime_evidence_complete(
        &self,
        metadata: &HashMap<String, String>,
    ) -> bool {
        self.forge_runtime_evidence_core_complete(metadata)
    }

    pub(crate) fn apply_forge_stage_runtime_metadata(
        &self,
        stage: RuntimeStage,
        stage_metadata: &mut HashMap<String, String>,
        runtime_metadata: &HashMap<String, String>,
    ) {
        match stage {
            RuntimeStage::ToolPlanningFiltering => {
                copy_present_keys(stage_metadata, runtime_metadata, TOOL_PLANNING_FORGE_KEYS)
            }
            RuntimeStage::Execution => {
                copy_present_keys(stage_metadata, runtime_metadata, EXECUTION_FORGE_KEYS)
            }
            _ => {}
        }
    }

    pub(crate) fn collect_runtime_forge_trace_metadata(
        &self,
        hook_capture: &RuntimeHookCapture,
    ) -> HashMap<String, String> {
        let mut metadata = HashMap::new();
        let mut forge_registered_tools = BTreeSet::new();
        let mut forge_execution_surfaces = BTreeSet::new();
        let mut forge_capability_domains = BTreeSet::new();
        let mut forge_smoke_statuses = BTreeSet::new();
        let mut forge_smoke_latency_ms = BTreeSet::new();
        let mut forge_cleanup_recorded = BTreeSet::new();
        let mut forge_source = None;
        let mut forge_scope = None;
        let mut forge_followup_candidates = BTreeSet::new();
        let mut forge_followup_gate_active = false;

        for note in &hook_capture.notes {
            if collect_trimmed_set_values_from_note(
                note,
                &mut [
                    ("forge_registered:", &mut forge_registered_tools),
                    ("forge_execution_surface:", &mut forge_execution_surfaces),
                    ("forge_capability_domain:", &mut forge_capability_domains),
                    ("forge_smoke_status:", &mut forge_smoke_statuses),
                    ("forge_smoke_latency_ms:", &mut forge_smoke_latency_ms),
                    ("forge_cleanup_recorded:", &mut forge_cleanup_recorded),
                ],
            ) || capture_trimmed_value_from_note(note, "forge_source:", &mut forge_source)
                || capture_trimmed_value_from_note(note, "forge_scope:", &mut forge_scope)
                || collect_csv_set_values_from_note(
                    note,
                    "before_llm:forge_followup_tools:",
                    &mut forge_followup_candidates,
                )
            {
                continue;
            } else if note == "before_llm:forge_followup_gate_active" {
                forge_followup_gate_active = true;
            }
        }

        insert_joined_set(
            &mut metadata,
            "forge_registered_tools",
            &forge_registered_tools,
        );
        if let Some(source) = forge_source {
            metadata.insert("forge_source".to_string(), source);
        }
        if let Some(scope) = forge_scope {
            metadata.insert("forge_scope".to_string(), scope);
        }
        insert_joined_set(
            &mut metadata,
            "forge_followup_candidates",
            &forge_followup_candidates,
        );
        if forge_followup_gate_active {
            metadata.insert("forge_followup_gate_active".to_string(), "true".to_string());
        }
        for (key, values) in [
            ("forge_execution_surfaces", &forge_execution_surfaces),
            ("forge_capability_domains", &forge_capability_domains),
            ("forge_smoke_statuses", &forge_smoke_statuses),
            ("forge_smoke_latency_ms", &forge_smoke_latency_ms),
            ("forge_cleanup_recorded", &forge_cleanup_recorded),
        ] {
            insert_joined_set(&mut metadata, key, values);
        }
        if metadata.contains_key("forge_registered_tools") {
            metadata.insert("forge_surface_present".to_string(), "true".to_string());
        }
        if metadata_has_all(&metadata, FORGE_CONTRACT_KEYS)
            && metadata_matches(&metadata, "forge_source", "forge")
            && metadata_matches(&metadata, "forge_scope", "session")
        {
            metadata.insert("forge_contract_complete".to_string(), "true".to_string());
        }

        metadata
    }

    pub(crate) fn apply_forge_closed_loop_trace_metadata(
        &self,
        metadata: &mut HashMap<String, String>,
        outcome: &ChatOutcome,
    ) {
        let registered_tools: BTreeSet<String> = metadata
            .get("forge_registered_tools")
            .map(|value| {
                value
                    .split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();

        if registered_tools.is_empty() {
            return;
        }

        let followup_tools: BTreeSet<String> = outcome
            .tool_calls
            .iter()
            .map(|call| call.name.trim())
            .filter(|tool_name| !tool_name.is_empty() && registered_tools.contains(*tool_name))
            .map(str::to_string)
            .collect();

        if !followup_tools.is_empty() {
            let followup_tools_joined =
                followup_tools.iter().cloned().collect::<Vec<_>>().join(",");
            metadata.insert("forge_followup_tools".to_string(), followup_tools_joined);
            metadata.insert(
                "forge_followup_execution_happened".to_string(),
                "true".to_string(),
            );
        }

        if metadata_matches(metadata, "forge_contract_complete", "true")
            && metadata_matches(metadata, "forge_followup_execution_happened", "true")
        {
            metadata.insert("forge_closed_loop_complete".to_string(), "true".to_string());
        }
    }
}
