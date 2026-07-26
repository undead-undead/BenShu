use super::core::Agent;
use super::runtime_support::RuntimeExecutionSeed;
use crate::agent::protocol::ChatOutcome;
use crate::agent::provider::Provider;
use benshu_telemetry::{RuntimeStage, RuntimeStageTrace, TraceStatus};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

impl<P: Provider> Agent<P> {
    pub(crate) fn build_stage_trace_metadata(
        &self,
        seed: &RuntimeExecutionSeed,
    ) -> HashMap<String, String> {
        let mut metadata = HashMap::new();
        metadata.insert("run_id".to_string(), seed.run_id.to_string());
        metadata.insert("task_id".to_string(), seed.task_id.to_string());
        metadata.insert("thread_id".to_string(), seed.thread_id.clone());
        if let Some(session_id) = &seed.session_id {
            metadata.insert("session_id".to_string(), session_id.clone());
        }
        metadata
    }

    pub(crate) fn build_runtime_stage_traces(
        &self,
        seed: &RuntimeExecutionSeed,
        outcome: &ChatOutcome,
        finished_at: DateTime<Utc>,
        metadata_seed: &HashMap<String, String>,
    ) -> Vec<RuntimeStageTrace> {
        let captured = self.runtime_stage_capture.read().clone();
        if captured.is_empty() {
            return vec![RuntimeStageTrace {
                stage: RuntimeStage::TraceAudit,
                status: TraceStatus::Degraded,
                started_at: seed.started_at,
                finished_at: Some(finished_at),
                detail: Some(format!(
                    "runtime stage capture missing; fallback trace built from outcome thoughts={} tool_calls={}",
                    outcome.thoughts.len(),
                    outcome.tool_calls.len()
                )),
                metadata: self.build_stage_trace_metadata(seed),
            }];
        }

        let stage_order = [
            RuntimeStage::Ingress,
            RuntimeStage::Governance,
            RuntimeStage::ContextBuild,
            RuntimeStage::Reasoning,
            RuntimeStage::ToolPlanningFiltering,
            RuntimeStage::Execution,
            RuntimeStage::PersistenceMemory,
            RuntimeStage::TraceAudit,
            RuntimeStage::Egress,
        ];

        let mut traces = Vec::new();
        for stage in stage_order {
            let stage_signals: Vec<_> = captured
                .iter()
                .filter(|signal| signal.stage == stage)
                .cloned()
                .collect();
            if stage_signals.is_empty() {
                continue;
            }

            let first_signal = &stage_signals[0];
            let started_signal = stage_signals
                .iter()
                .find(|signal| matches!(signal.status, TraceStatus::Started))
                .unwrap_or(first_signal);
            let terminal_signal = stage_signals.last().unwrap_or(first_signal);
            let mut metadata = self.build_stage_trace_metadata(seed);
            metadata.insert("signal_count".to_string(), stage_signals.len().to_string());
            self.apply_stage_runtime_metadata(stage, &mut metadata, metadata_seed);

            traces.push(RuntimeStageTrace {
                stage,
                status: terminal_signal.status.clone(),
                started_at: started_signal.at,
                finished_at: if matches!(terminal_signal.status, TraceStatus::Started) {
                    None
                } else {
                    Some(terminal_signal.at)
                },
                detail: terminal_signal
                    .detail
                    .clone()
                    .or_else(|| first_signal.detail.clone()),
                metadata,
            });
        }

        traces
    }

    pub(crate) fn apply_stage_runtime_metadata(
        &self,
        stage: RuntimeStage,
        stage_metadata: &mut HashMap<String, String>,
        runtime_metadata: &HashMap<String, String>,
    ) {
        let stage_keys: &[&str] = match stage {
            RuntimeStage::Reasoning => &[
                "tactical_slm_present",
                "tactical_slm_model_id",
                "tactical_slm_factory_id",
                "tactical_slm_source",
                "tactical_slm_roles",
                "tactical_slm_contract_complete",
                "background_used_slm",
            ],
            RuntimeStage::ToolPlanningFiltering => &[
                "deferred_tool_filter_active",
                "deferred_tool_visible_count",
                "deferred_tool_total_count",
                "deferred_tool_deferred_count",
                "deferred_tool_surface_note_present",
                "deferred_tool_surface_note_complete",
                "matched_skill_manuals",
                "matched_skill_assets",
                "skill_surface_classifications",
                "skill_surface_executions",
                "skill_surface_kinds",
                "skill_manual_gate_active",
                "skill_asset_gate_active",
                "skill_loading_contract_core_complete",
                "skill_loading_surface_note_core_complete",
                "skill_surface_contract_core_complete",
            ],
            RuntimeStage::Execution => &[
                "tool_error_tools",
                "tool_error_surface_tools",
                "tool_error_surface_present",
                "tool_error_contract_complete",
                "degraded_tool_names",
                "loop_guard_tools",
                "read_skill_manuals",
                "read_skill_assets",
                "skill_surface_runtimes",
                "skill_surface_contract_happened",
                "skill_manual_read_happened",
                "skill_asset_read_happened",
                "skill_asset_followups",
                "skill_asset_execution_surfaces",
                "skill_asset_followup_happened",
                "skill_asset_execution_surface_happened",
                "skill_loading_contract_complete",
                "skill_loading_surface_note_complete",
                "skill_surface_contract_complete",
            ],
            RuntimeStage::PersistenceMemory => &[
                "memory_owner",
                "approval_owner",
                "session_title",
                "session_title_source",
                "session_title_present",
                "memory_session_contract_core_complete",
                "memory_session_contract_complete",
                "memory_session_surface_core_complete",
                "memory_session_surface_complete",
                "memory_session_surface_note_present",
                "memory_session_surface_note_complete",
                "subagent_budget_surface_note_present",
                "subagent_budget_surface_note_complete",
                "title_surface_note_present",
                "title_surface_note_complete",
                "summarization_surface_note_present",
                "summarization_surface_note_complete",
                "memory_session_orchestration_contract_core_complete",
                "memory_session_orchestration_contract_complete",
                "background_present",
                "background_revision",
                "background_previous_revision",
                "background_update_reason",
                "background_quality_signal",
                "background_persona_present",
                "background_relationship_present",
                "background_session_present",
                "background_recent_window_present",
                "background_source_ref_count",
                "background_decision",
                "background_used_slm",
                "background_session_persistence_status",
                "background_durable_promotion_pending",
                "background_durable_promotion_status",
                "background_review_reason",
                "background_review_source",
                "background_total_attempts",
                "background_skip_count",
                "background_reject_count",
                "background_refresh_session_count",
                "background_promote_relationship_count",
                "background_rewrite_count",
                "background_contract_complete",
            ],
            RuntimeStage::Egress => &[
                "runtime_finish_reason",
                "post_run_summary",
                "background_revision",
                "background_quality_signal",
                "background_decision",
                "background_session_persistence_status",
                "background_durable_promotion_status",
                "background_review_reason",
                "background_review_source",
                "background_total_attempts",
                "background_skip_count",
                "background_reject_count",
                "background_refresh_session_count",
                "background_promote_relationship_count",
                "background_rewrite_count",
            ],
            RuntimeStage::TraceAudit => &[
                "runtime_evidence_contract_core_complete",
                "runtime_evidence_contract_complete",
                "background_contract_complete",
            ],
            _ => &[],
        };

        for key in stage_keys {
            if let Some(value) = runtime_metadata.get(*key) {
                stage_metadata.insert((*key).to_string(), value.clone());
            }
        }
        self.apply_provider_stage_runtime_metadata(stage, stage_metadata, runtime_metadata);
        self.apply_media_stage_runtime_metadata(stage, stage_metadata, runtime_metadata);
        self.apply_clarification_stage_runtime_metadata(stage, stage_metadata, runtime_metadata);
        self.apply_forge_stage_runtime_metadata(stage, stage_metadata, runtime_metadata);
        self.apply_windows_native_stage_runtime_metadata(stage, stage_metadata, runtime_metadata);

        if stage == RuntimeStage::ToolPlanningFiltering {
            let manual_chain_complete = !runtime_metadata.contains_key("matched_skill_manuals")
                || runtime_metadata.contains_key("read_skill_manuals");
            let asset_chain_complete = !runtime_metadata.contains_key("matched_skill_assets")
                || runtime_metadata.contains_key("read_skill_assets");
            let surface_contract_core_complete = !runtime_metadata
                .contains_key("matched_skill_manuals")
                || (runtime_metadata.contains_key("skill_surface_classifications")
                    && runtime_metadata.contains_key("skill_surface_executions")
                    && runtime_metadata.contains_key("skill_surface_kinds"));

            if (runtime_metadata.contains_key("matched_skill_manuals")
                || runtime_metadata.contains_key("matched_skill_assets")
                || runtime_metadata.contains_key("read_skill_manuals")
                || runtime_metadata.contains_key("read_skill_assets")
                || runtime_metadata.contains_key("skill_manual_gate_active")
                || runtime_metadata.contains_key("skill_asset_gate_active"))
                && manual_chain_complete
                && asset_chain_complete
                && surface_contract_core_complete
            {
                stage_metadata.insert(
                    "skill_loading_contract_core_complete".to_string(),
                    "true".to_string(),
                );
                stage_metadata.insert(
                    "skill_surface_contract_core_complete".to_string(),
                    "true".to_string(),
                );
            }
        } else if stage == RuntimeStage::Execution {
            let manual_chain_complete = !runtime_metadata.contains_key("matched_skill_manuals")
                || runtime_metadata.contains_key("read_skill_manuals");
            let asset_chain_complete = !runtime_metadata.contains_key("matched_skill_assets")
                || runtime_metadata.contains_key("read_skill_assets");
            let followup_chain_complete = !runtime_metadata.contains_key("skill_asset_followups")
                || runtime_metadata.contains_key("skill_asset_read_happened");
            let execution_surface_chain_complete = !runtime_metadata
                .contains_key("skill_asset_followups")
                || runtime_metadata.contains_key("skill_asset_execution_surfaces");
            let surface_contract_complete = !runtime_metadata.contains_key("matched_skill_manuals")
                || (runtime_metadata.contains_key("skill_surface_classifications")
                    && runtime_metadata.contains_key("skill_surface_executions")
                    && runtime_metadata.contains_key("skill_surface_kinds")
                    && runtime_metadata.contains_key("skill_surface_runtimes"));

            if (runtime_metadata.contains_key("matched_skill_manuals")
                || runtime_metadata.contains_key("matched_skill_assets")
                || runtime_metadata.contains_key("read_skill_manuals")
                || runtime_metadata.contains_key("read_skill_assets")
                || runtime_metadata.contains_key("skill_manual_gate_active")
                || runtime_metadata.contains_key("skill_asset_gate_active"))
                && manual_chain_complete
                && asset_chain_complete
                && followup_chain_complete
                && execution_surface_chain_complete
                && surface_contract_complete
            {
                stage_metadata.insert(
                    "skill_loading_contract_complete".to_string(),
                    "true".to_string(),
                );
                stage_metadata.insert(
                    "skill_surface_contract_complete".to_string(),
                    "true".to_string(),
                );
            }
        } else if stage == RuntimeStage::TraceAudit {
            for key in [
                "runtime_evidence_contract_core_complete",
                "runtime_evidence_contract_complete",
            ] {
                if let Some(value) = runtime_metadata.get(key) {
                    stage_metadata.insert(key.to_string(), value.clone());
                }
            }
        }
    }
}
