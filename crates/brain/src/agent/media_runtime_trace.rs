use super::core::Agent;
use super::trace_metadata_helpers::{
    collect_csv_set_values_from_note, collect_csv_set_values_from_note_map,
    collect_flagged_trimmed_set_values_from_note,
    collect_flagged_trimmed_set_values_from_note_with_shared_flag,
    collect_trimmed_set_values_from_note, copy_present_keys, insert_joined_set, metadata_has_any,
    metadata_is_true,
};
use crate::agent::provider::Provider;
use crate::hooks::RuntimeHookCapture;
use benshu_telemetry::RuntimeStage;
use std::collections::{BTreeSet, HashMap};

const REASONING_MEDIA_KEYS: &[&str] = &[
    "provider_media_preprocess_consumed_by",
    "provider_media_preprocess_consumption_routes",
    "provider_media_preprocess_outcomes",
    "provider_media_preprocess_preprocess_failed_routes",
    "provider_media_preprocess_model_failed_routes",
    "provider_media_preprocess_result_insufficient_routes",
    "provider_media_preprocess_followup_strategies",
    "provider_media_preprocess_attachment_fallback_routes",
    "provider_media_preprocess_alternate_model_fallback_routes",
    "provider_media_preprocess_clarification_routes",
    "provider_media_preprocess_outcome_note_complete",
    "provider_media_preprocess_outcome_contract_complete",
    "provider_media_preprocess_strategy_note_complete",
    "provider_media_preprocess_strategy_contract_complete",
];
const TOOL_PLANNING_MEDIA_KEYS: &[&str] = &[
    "media_followup_strategies",
    "media_followup_capability_route",
    "media_followup_execution_surface",
    "media_followup_guidance_active",
];
const EXECUTION_MEDIA_KEYS: &[&str] = &[
    "media_preprocess_tools",
    "media_preprocess_statuses",
    "media_preprocess_kinds",
    "media_preprocess_inputs",
    "media_preprocess_outputs",
    "media_preprocess_source_kinds",
    "media_preprocess_source_refs",
    "media_preprocess_engines",
    "media_preprocess_cleanup",
    "media_preprocess_frames",
    "media_preprocess_artifact_registered",
    "media_preprocess_artifact_source_kinds",
    "media_preprocess_artifact_kinds",
    "media_preprocess_artifact_uris",
    "media_preprocess_consumed_by",
    "media_preprocess_consumption_routes",
    "media_preprocess_outcomes",
    "media_preprocess_preprocess_failed_routes",
    "media_preprocess_model_failed_routes",
    "media_preprocess_result_insufficient_routes",
    "media_preprocess_followup_strategies",
    "media_preprocess_attachment_fallback_routes",
    "media_preprocess_alternate_model_fallback_routes",
    "media_preprocess_clarification_routes",
    "media_preprocess_surface_note_present",
    "media_preprocess_surface_note_complete",
    "media_preprocess_artifact_surface_note_present",
    "media_preprocess_artifact_surface_note_complete",
    "media_preprocess_contract_core_complete",
    "media_preprocess_contract_complete",
    "media_preprocess_artifact_contract_complete",
    "media_preprocess_consumption_surface_note_complete",
    "media_preprocess_consumption_contract_complete",
    "media_preprocess_outcome_surface_note_complete",
    "media_preprocess_outcome_contract_complete",
    "media_preprocess_strategy_surface_note_complete",
    "media_preprocess_strategy_contract_complete",
];
const MEDIA_PREPROCESS_ACTIVITY_KEYS: &[&str] = &[
    "media_preprocess_tools",
    "media_preprocess_statuses",
    "media_preprocess_outputs",
];

impl<P: Provider + 'static> Agent<P> {
    pub(crate) fn apply_media_followup_request_metadata(
        &self,
        metadata: &mut HashMap<String, String>,
        media_followup_strategies: &[String],
        media_followup_capability_route: Option<&str>,
        media_followup_execution_surface: Option<&str>,
    ) {
        if media_followup_strategies.is_empty() {
            return;
        }

        metadata.insert(
            "media_followup_strategies".to_string(),
            media_followup_strategies.join(","),
        );
        metadata.insert(
            "media_followup_guidance_active".to_string(),
            "true".to_string(),
        );
        if let Some(route) = media_followup_capability_route {
            metadata.insert(
                "media_followup_capability_route".to_string(),
                route.to_string(),
            );
        }
        if let Some(surface) = media_followup_execution_surface {
            metadata.insert(
                "media_followup_execution_surface".to_string(),
                surface.to_string(),
            );
        }
    }

    pub(crate) fn media_preprocess_activity_present(
        &self,
        metadata: &HashMap<String, String>,
    ) -> bool {
        metadata_has_any(metadata, MEDIA_PREPROCESS_ACTIVITY_KEYS)
    }

    pub(crate) fn media_runtime_evidence_core_complete(
        &self,
        metadata: &HashMap<String, String>,
    ) -> bool {
        !self.media_preprocess_activity_present(metadata)
            || metadata_is_true(metadata, "media_preprocess_contract_core_complete")
    }

    pub(crate) fn media_runtime_evidence_complete(
        &self,
        metadata: &HashMap<String, String>,
    ) -> bool {
        !self.media_preprocess_activity_present(metadata)
            || (metadata_is_true(metadata, "media_preprocess_contract_complete")
                && metadata_is_true(metadata, "media_preprocess_surface_note_complete"))
    }

    pub(crate) fn apply_media_stage_runtime_metadata(
        &self,
        stage: RuntimeStage,
        stage_metadata: &mut HashMap<String, String>,
        runtime_metadata: &HashMap<String, String>,
    ) {
        match stage {
            RuntimeStage::Reasoning => {
                copy_present_keys(stage_metadata, runtime_metadata, REASONING_MEDIA_KEYS)
            }
            RuntimeStage::ToolPlanningFiltering => {
                copy_present_keys(stage_metadata, runtime_metadata, TOOL_PLANNING_MEDIA_KEYS)
            }
            RuntimeStage::Execution => {
                copy_present_keys(stage_metadata, runtime_metadata, EXECUTION_MEDIA_KEYS);
                if runtime_metadata.contains_key("media_preprocess_tools")
                    && metadata_is_true(runtime_metadata, "media_preprocess_contract_complete")
                {
                    stage_metadata.insert(
                        "media_preprocess_contract_complete".to_string(),
                        "true".to_string(),
                    );
                }
            }
            _ => {}
        }
    }

    pub(crate) fn apply_media_followup_before_llm_metadata(
        &self,
        metadata: &mut HashMap<String, String>,
        hook_capture: &RuntimeHookCapture,
    ) {
        let mut media_followup_strategies = BTreeSet::new();
        let mut media_followup_capability_route = None;
        let mut media_followup_execution_surface = None;
        let mut media_followup_guidance_active = false;

        for note in &hook_capture.notes {
            if collect_csv_set_values_from_note(
                note,
                "before_llm:media_followup_strategies:",
                &mut media_followup_strategies,
            ) {
                continue;
            } else if let Some(route) =
                note.strip_prefix("before_llm:media_followup_capability_route:")
            {
                let trimmed = route.trim();
                if !trimmed.is_empty() {
                    media_followup_capability_route = Some(trimmed.to_string());
                }
            } else if let Some(surface) =
                note.strip_prefix("before_llm:media_followup_execution_surface:")
            {
                let trimmed = surface.trim();
                if !trimmed.is_empty() {
                    media_followup_execution_surface = Some(trimmed.to_string());
                }
            } else if note == "before_llm:media_followup_guidance_active" {
                media_followup_guidance_active = true;
            }
        }

        if !media_followup_strategies.is_empty() {
            metadata.insert(
                "media_followup_strategies".to_string(),
                media_followup_strategies
                    .into_iter()
                    .collect::<Vec<_>>()
                    .join(","),
            );
        }
        if let Some(route) = media_followup_capability_route {
            metadata.insert("media_followup_capability_route".to_string(), route);
        }
        if let Some(surface) = media_followup_execution_surface {
            metadata.insert("media_followup_execution_surface".to_string(), surface);
        }
        if media_followup_guidance_active {
            metadata.insert(
                "media_followup_guidance_active".to_string(),
                "true".to_string(),
            );
        }
    }

    pub(crate) fn collect_runtime_media_trace_metadata(
        &self,
        hook_capture: &RuntimeHookCapture,
    ) -> HashMap<String, String> {
        let mut metadata = HashMap::new();
        let mut provider_media_preprocess_consumed_by = BTreeSet::new();
        let mut provider_media_preprocess_consumption_routes = BTreeSet::new();
        let mut provider_media_preprocess_outcomes = BTreeSet::new();
        let mut provider_media_preprocess_preprocess_failed_routes = BTreeSet::new();
        let mut provider_media_preprocess_model_failed_routes = BTreeSet::new();
        let mut provider_media_preprocess_result_insufficient_routes = BTreeSet::new();
        let mut provider_media_preprocess_followup_strategies = BTreeSet::new();
        let mut provider_media_preprocess_attachment_fallback_routes = BTreeSet::new();
        let mut provider_media_preprocess_alternate_model_fallback_routes = BTreeSet::new();
        let mut provider_media_preprocess_clarification_routes = BTreeSet::new();
        let mut media_preprocess_tools = BTreeSet::new();
        let mut media_preprocess_statuses = BTreeSet::new();
        let mut media_preprocess_kinds = BTreeSet::new();
        let mut media_preprocess_inputs = BTreeSet::new();
        let mut media_preprocess_outputs = BTreeSet::new();
        let mut media_preprocess_source_kinds = BTreeSet::new();
        let mut media_preprocess_source_refs = BTreeSet::new();
        let mut media_preprocess_engines = BTreeSet::new();
        let mut media_preprocess_cleanup = BTreeSet::new();
        let mut media_preprocess_frames = BTreeSet::new();
        let mut media_preprocess_artifact_registered = BTreeSet::new();
        let mut media_preprocess_artifact_source_kinds = BTreeSet::new();
        let mut media_preprocess_artifact_kinds = BTreeSet::new();
        let mut media_preprocess_artifact_uris = BTreeSet::new();
        let mut media_preprocess_consumed_by = BTreeSet::new();
        let mut media_preprocess_consumption_routes = BTreeSet::new();
        let mut media_preprocess_outcomes = BTreeSet::new();
        let mut media_preprocess_preprocess_failed_routes = BTreeSet::new();
        let mut media_preprocess_model_failed_routes = BTreeSet::new();
        let mut media_preprocess_result_insufficient_routes = BTreeSet::new();
        let mut media_preprocess_followup_strategies = BTreeSet::new();
        let mut media_preprocess_attachment_fallback_routes = BTreeSet::new();
        let mut media_preprocess_alternate_model_fallback_routes = BTreeSet::new();
        let mut media_preprocess_clarification_routes = BTreeSet::new();
        let mut media_preprocess_surface_note_present = false;
        let mut media_preprocess_artifact_surface_note_present = false;
        let mut media_preprocess_outcome_surface_note_present = false;
        let mut media_preprocess_strategy_surface_note_present = false;

        for note in &hook_capture.notes {
            if collect_csv_set_values_from_note_map(
                note,
                &mut [
                    (
                        "after_llm:provider_media_preprocess_consumed_by:",
                        &mut provider_media_preprocess_consumed_by,
                    ),
                    (
                        "after_llm:provider_media_preprocess_consumption_routes:",
                        &mut provider_media_preprocess_consumption_routes,
                    ),
                    (
                        "after_llm:provider_media_preprocess_outcomes:",
                        &mut provider_media_preprocess_outcomes,
                    ),
                    (
                        "after_llm:provider_media_preprocess_preprocess_failed_routes:",
                        &mut provider_media_preprocess_preprocess_failed_routes,
                    ),
                    (
                        "after_llm:provider_media_preprocess_model_failed_routes:",
                        &mut provider_media_preprocess_model_failed_routes,
                    ),
                    (
                        "after_llm:provider_media_preprocess_result_insufficient_routes:",
                        &mut provider_media_preprocess_result_insufficient_routes,
                    ),
                    (
                        "after_llm:provider_media_preprocess_followup_strategies:",
                        &mut provider_media_preprocess_followup_strategies,
                    ),
                    (
                        "after_llm:provider_media_preprocess_attachment_fallback_routes:",
                        &mut provider_media_preprocess_attachment_fallback_routes,
                    ),
                    (
                        "after_llm:provider_media_preprocess_alternate_model_fallback_routes:",
                        &mut provider_media_preprocess_alternate_model_fallback_routes,
                    ),
                    (
                        "after_llm:provider_media_preprocess_clarification_routes:",
                        &mut provider_media_preprocess_clarification_routes,
                    ),
                ],
            ) {
                continue;
            }

            if collect_flagged_trimmed_set_values_from_note(
                note,
                &mut [(
                    "media_preprocess_tool:",
                    &mut media_preprocess_tools,
                    &mut media_preprocess_surface_note_present,
                )],
            ) || collect_trimmed_set_values_from_note(
                note,
                &mut [
                    ("media_preprocess_status:", &mut media_preprocess_statuses),
                    ("media_preprocess_kind:", &mut media_preprocess_kinds),
                    ("media_preprocess_input:", &mut media_preprocess_inputs),
                    ("media_preprocess_output:", &mut media_preprocess_outputs),
                    (
                        "media_preprocess_source_kind:",
                        &mut media_preprocess_source_kinds,
                    ),
                    (
                        "media_preprocess_source_ref:",
                        &mut media_preprocess_source_refs,
                    ),
                    ("media_preprocess_engine:", &mut media_preprocess_engines),
                    ("media_preprocess_cleanup:", &mut media_preprocess_cleanup),
                    ("media_preprocess_frames:", &mut media_preprocess_frames),
                ],
            ) {
                continue;
            }

            if collect_flagged_trimmed_set_values_from_note_with_shared_flag(
                note,
                &mut media_preprocess_artifact_surface_note_present,
                &mut [
                    (
                        "media_preprocess_artifact_registered:",
                        &mut media_preprocess_artifact_registered,
                    ),
                    (
                        "media_preprocess_artifact_source_kind:",
                        &mut media_preprocess_artifact_source_kinds,
                    ),
                    (
                        "media_preprocess_artifact_kind:",
                        &mut media_preprocess_artifact_kinds,
                    ),
                    (
                        "media_preprocess_artifact_uri:",
                        &mut media_preprocess_artifact_uris,
                    ),
                ],
            ) {
                continue;
            }

            if collect_trimmed_set_values_from_note(
                note,
                &mut [
                    (
                        "media_preprocess_consumed_by:",
                        &mut media_preprocess_consumed_by,
                    ),
                    (
                        "media_preprocess_consumption_route:",
                        &mut media_preprocess_consumption_routes,
                    ),
                ],
            ) {
                continue;
            }

            if collect_flagged_trimmed_set_values_from_note_with_shared_flag(
                note,
                &mut media_preprocess_outcome_surface_note_present,
                &mut [
                    ("media_preprocess_outcome:", &mut media_preprocess_outcomes),
                    (
                        "media_preprocess_preprocess_failed:",
                        &mut media_preprocess_preprocess_failed_routes,
                    ),
                    (
                        "media_preprocess_model_failed:",
                        &mut media_preprocess_model_failed_routes,
                    ),
                    (
                        "media_preprocess_result_insufficient:",
                        &mut media_preprocess_result_insufficient_routes,
                    ),
                ],
            ) {
                continue;
            }

            let _ = collect_flagged_trimmed_set_values_from_note_with_shared_flag(
                note,
                &mut media_preprocess_strategy_surface_note_present,
                &mut [
                    (
                        "media_preprocess_followup_strategy:",
                        &mut media_preprocess_followup_strategies,
                    ),
                    (
                        "media_preprocess_strategy_attachment_fallback:",
                        &mut media_preprocess_attachment_fallback_routes,
                    ),
                    (
                        "media_preprocess_strategy_alternate_model_fallback:",
                        &mut media_preprocess_alternate_model_fallback_routes,
                    ),
                    (
                        "media_preprocess_strategy_clarification:",
                        &mut media_preprocess_clarification_routes,
                    ),
                ],
            );
        }

        for (key, values) in [
            (
                "provider_media_preprocess_consumed_by",
                &provider_media_preprocess_consumed_by,
            ),
            (
                "provider_media_preprocess_consumption_routes",
                &provider_media_preprocess_consumption_routes,
            ),
            (
                "provider_media_preprocess_outcomes",
                &provider_media_preprocess_outcomes,
            ),
            (
                "provider_media_preprocess_preprocess_failed_routes",
                &provider_media_preprocess_preprocess_failed_routes,
            ),
            (
                "provider_media_preprocess_model_failed_routes",
                &provider_media_preprocess_model_failed_routes,
            ),
            (
                "provider_media_preprocess_result_insufficient_routes",
                &provider_media_preprocess_result_insufficient_routes,
            ),
            (
                "provider_media_preprocess_followup_strategies",
                &provider_media_preprocess_followup_strategies,
            ),
            (
                "provider_media_preprocess_attachment_fallback_routes",
                &provider_media_preprocess_attachment_fallback_routes,
            ),
            (
                "provider_media_preprocess_alternate_model_fallback_routes",
                &provider_media_preprocess_alternate_model_fallback_routes,
            ),
            (
                "provider_media_preprocess_clarification_routes",
                &provider_media_preprocess_clarification_routes,
            ),
        ] {
            insert_joined_set(&mut metadata, key, values);
        }
        let provider_media_preprocess_outcome_note_complete = !provider_media_preprocess_outcomes
            .is_empty()
            && provider_media_preprocess_outcomes.len()
                == provider_media_preprocess_preprocess_failed_routes.len()
                    + provider_media_preprocess_model_failed_routes.len()
                    + provider_media_preprocess_result_insufficient_routes.len();
        if provider_media_preprocess_outcome_note_complete {
            metadata.insert(
                "provider_media_preprocess_outcome_note_complete".to_string(),
                "true".to_string(),
            );
            metadata.insert(
                "provider_media_preprocess_outcome_contract_complete".to_string(),
                "true".to_string(),
            );
        }
        let provider_media_preprocess_strategy_note_complete =
            !provider_media_preprocess_followup_strategies.is_empty()
                && provider_media_preprocess_followup_strategies.len()
                    == provider_media_preprocess_attachment_fallback_routes.len()
                        + provider_media_preprocess_alternate_model_fallback_routes.len()
                        + provider_media_preprocess_clarification_routes.len();
        if provider_media_preprocess_strategy_note_complete {
            metadata.insert(
                "provider_media_preprocess_strategy_note_complete".to_string(),
                "true".to_string(),
            );
            metadata.insert(
                "provider_media_preprocess_strategy_contract_complete".to_string(),
                "true".to_string(),
            );
        }

        for (key, values) in [
            ("media_preprocess_tools", &media_preprocess_tools),
            ("media_preprocess_statuses", &media_preprocess_statuses),
            ("media_preprocess_kinds", &media_preprocess_kinds),
            ("media_preprocess_inputs", &media_preprocess_inputs),
            ("media_preprocess_outputs", &media_preprocess_outputs),
            (
                "media_preprocess_source_kinds",
                &media_preprocess_source_kinds,
            ),
            (
                "media_preprocess_source_refs",
                &media_preprocess_source_refs,
            ),
            ("media_preprocess_engines", &media_preprocess_engines),
            ("media_preprocess_cleanup", &media_preprocess_cleanup),
            ("media_preprocess_frames", &media_preprocess_frames),
            (
                "media_preprocess_artifact_registered",
                &media_preprocess_artifact_registered,
            ),
            (
                "media_preprocess_artifact_source_kinds",
                &media_preprocess_artifact_source_kinds,
            ),
            (
                "media_preprocess_artifact_kinds",
                &media_preprocess_artifact_kinds,
            ),
            (
                "media_preprocess_artifact_uris",
                &media_preprocess_artifact_uris,
            ),
            (
                "media_preprocess_consumed_by",
                &media_preprocess_consumed_by,
            ),
            (
                "media_preprocess_consumption_routes",
                &media_preprocess_consumption_routes,
            ),
            ("media_preprocess_outcomes", &media_preprocess_outcomes),
            (
                "media_preprocess_preprocess_failed_routes",
                &media_preprocess_preprocess_failed_routes,
            ),
            (
                "media_preprocess_model_failed_routes",
                &media_preprocess_model_failed_routes,
            ),
            (
                "media_preprocess_result_insufficient_routes",
                &media_preprocess_result_insufficient_routes,
            ),
            (
                "media_preprocess_followup_strategies",
                &media_preprocess_followup_strategies,
            ),
            (
                "media_preprocess_attachment_fallback_routes",
                &media_preprocess_attachment_fallback_routes,
            ),
            (
                "media_preprocess_alternate_model_fallback_routes",
                &media_preprocess_alternate_model_fallback_routes,
            ),
            (
                "media_preprocess_clarification_routes",
                &media_preprocess_clarification_routes,
            ),
        ] {
            insert_joined_set(&mut metadata, key, values);
        }
        if media_preprocess_surface_note_present {
            metadata.insert(
                "media_preprocess_surface_note_present".to_string(),
                "true".to_string(),
            );
        }
        let media_preprocess_surface_note_complete = !media_preprocess_tools.is_empty()
            && media_preprocess_tools.len() == media_preprocess_statuses.len()
            && media_preprocess_tools.len() == media_preprocess_kinds.len()
            && media_preprocess_tools.len() == media_preprocess_engines.len()
            && media_preprocess_tools.len() == media_preprocess_inputs.len();
        if media_preprocess_surface_note_complete {
            metadata.insert(
                "media_preprocess_surface_note_complete".to_string(),
                "true".to_string(),
            );
        }
        if media_preprocess_artifact_surface_note_present {
            metadata.insert(
                "media_preprocess_artifact_surface_note_present".to_string(),
                "true".to_string(),
            );
        }
        let media_preprocess_output_tools = media_preprocess_outputs
            .iter()
            .filter_map(|output| output.split_once(':').map(|(tool, _)| tool.to_string()))
            .collect::<BTreeSet<_>>();
        let media_preprocess_artifact_surface_note_complete = !media_preprocess_output_tools
            .is_empty()
            && media_preprocess_output_tools.iter().all(|tool| {
                media_preprocess_artifact_registered
                    .iter()
                    .any(|value| value == &format!("{tool}:true"))
                    && media_preprocess_artifact_source_kinds
                        .iter()
                        .any(|value| value.starts_with(&format!("{tool}:")))
                    && media_preprocess_artifact_kinds
                        .iter()
                        .any(|value| value.starts_with(&format!("{tool}:")))
                    && media_preprocess_artifact_uris
                        .iter()
                        .any(|value| value.starts_with(&format!("{tool}:")))
            });
        if media_preprocess_artifact_surface_note_complete {
            metadata.insert(
                "media_preprocess_artifact_surface_note_complete".to_string(),
                "true".to_string(),
            );
        }
        let media_preprocess_outputs_complete = media_preprocess_tools.iter().all(|tool| {
            media_preprocess_statuses
                .iter()
                .find(|value| value.starts_with(&format!("{tool}:")))
                .is_some_and(|status| {
                    status.ends_with(":error")
                        || status.ends_with(":unknown")
                        || media_preprocess_outputs
                            .iter()
                            .any(|output| output.starts_with(&format!("{tool}:")))
                })
        });
        if !media_preprocess_tools.is_empty() && media_preprocess_surface_note_complete {
            metadata.insert(
                "media_preprocess_contract_core_complete".to_string(),
                "true".to_string(),
            );
            if media_preprocess_outputs_complete {
                metadata.insert(
                    "media_preprocess_contract_complete".to_string(),
                    "true".to_string(),
                );
            }
        }
        if media_preprocess_artifact_surface_note_complete {
            metadata.insert(
                "media_preprocess_artifact_contract_complete".to_string(),
                "true".to_string(),
            );
        }
        let media_preprocess_consumption_surface_note_complete = !media_preprocess_consumed_by
            .is_empty()
            && media_preprocess_consumed_by.len() == media_preprocess_consumption_routes.len();
        if media_preprocess_consumption_surface_note_complete {
            metadata.insert(
                "media_preprocess_consumption_surface_note_complete".to_string(),
                "true".to_string(),
            );
            metadata.insert(
                "media_preprocess_consumption_contract_complete".to_string(),
                "true".to_string(),
            );
        }
        if media_preprocess_outcome_surface_note_present {
            let categorized_routes_count = media_preprocess_preprocess_failed_routes.len()
                + media_preprocess_model_failed_routes.len()
                + media_preprocess_result_insufficient_routes.len();
            let media_preprocess_outcome_surface_note_complete = !media_preprocess_outcomes
                .is_empty()
                && media_preprocess_outcomes.len() == categorized_routes_count;
            if media_preprocess_outcome_surface_note_complete {
                metadata.insert(
                    "media_preprocess_outcome_surface_note_complete".to_string(),
                    "true".to_string(),
                );
                metadata.insert(
                    "media_preprocess_outcome_contract_complete".to_string(),
                    "true".to_string(),
                );
            }
        }
        if media_preprocess_strategy_surface_note_present {
            let categorized_routes_count = media_preprocess_attachment_fallback_routes.len()
                + media_preprocess_alternate_model_fallback_routes.len()
                + media_preprocess_clarification_routes.len();
            let media_preprocess_strategy_surface_note_complete =
                !media_preprocess_followup_strategies.is_empty()
                    && media_preprocess_followup_strategies.len() == categorized_routes_count;
            if media_preprocess_strategy_surface_note_complete {
                metadata.insert(
                    "media_preprocess_strategy_surface_note_complete".to_string(),
                    "true".to_string(),
                );
                metadata.insert(
                    "media_preprocess_strategy_contract_complete".to_string(),
                    "true".to_string(),
                );
            }
        }

        metadata
    }
}
