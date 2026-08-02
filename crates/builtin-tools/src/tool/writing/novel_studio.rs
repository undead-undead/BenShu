//! Governed long-form fiction project tool.
//!
//! The tool is intentionally scoped to long-form fiction governance. It does
//! not browse the web; source gathering remains owned by research/browser
//! workers under BenShu's routing. It does not own an LLM provider:
//! BenShu delegates the writing task to an equipped writer worker, and that
//! worker remains responsible for reasoning and prose generation. This tool
//! persists contracts, context packages, ledgers, checks, snapshots, and exports.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use benshu_infra::error::Error;
use benshu_infra::{SafetyLevel, Tool, ToolDefinition};
use benshu_state::{ArtifactLifecycle, ArtifactManager};

use crate::tool::{register_tool_output_artifact, ToolArtifactRegistration};

use super::chapter_quality::{self, ChapterMetadataGate, ChapterQualityGate};
use super::creation_contract::ContractReadinessScope;
use super::creation_contract_model::NovelCreationContract;
use super::longform_policy;
use super::naming;
use super::novel_bible;
use super::novel_contract_v2::{
    self, AntagonistPressure, ArtifactLedgerEntry, ChapterCharacterRegistration,
    ChapterCharacterRequest, ChapterExecutionContractV2, CharacterVoiceProfile, EmotionalContract,
    EmotionalStateLedgerEntry, GeographyModel, NarrationContract, NovelContractV2,
    PayoffMatrixEntry, PowerProgression, RelationshipLedgerEntry, ResourceEconomy, SocialOrder,
    TimeModel,
};
use super::novel_governance as governance;
use super::novel_pipeline as pipeline;
use super::novel_pipeline::lifecycle as chapter_lifecycle;
use super::novel_runner::{self as runner, is_chinese_language};
use super::path_recovery::recoverable_path_error_result;
use super::policy;
use super::surface_sanitizer::{self, strip_markdown_frontmatter as strip_frontmatter};

mod approval_transaction;
mod archive;
mod chapter_io;
mod chapter_metadata;
mod chapter_planning;
mod chapter_state;
mod context_packaging;
mod contract_terms;
mod creation_draft;
mod export;
mod input;
mod manifest;
mod model;
mod pathing;
mod project_cache;
mod project_config;
mod project_governance;
mod project_lifecycle;
mod project_lock;
mod prose_sanitizer;
mod quality_checks;
mod quality_gate;
mod rendering;
mod reporting;
mod review_approval;
mod runtime_records;
mod settlement;
mod snapshot;
mod state_truth;
mod status_export;
mod storage;
mod support;
mod text_surface;
mod tool_schema;

use archive::*;
use chapter_metadata::*;
use chapter_state::*;
use context_packaging::{
    approved_chapter_context_view, approved_prior_chapters, build_context_budget_telemetry,
    build_context_governance, build_context_payload, build_minimal_context_payload,
    build_prompt_context_payload, prompt_context_fingerprint, protected_prompt_context_char_limit,
};
use contract_terms::*;
use creation_draft::{
    apply_novel_draft_updates, approved_novel_creation_draft_from_manifest,
    draft_outline_with_naming_basis, draft_premise_with_naming_basis,
    init_project_title_conflicted, light_status_audit_manifest, novel_creation_draft_from_manifest,
    novel_draft_readiness_issues, novel_draft_summary, novel_draft_title_from_args,
    project_state_summary, project_state_summary_light, project_title_is_temporary_placeholder,
};
use manifest::*;
use model::{
    ApprovalJournal, ApprovalJournalState, ApprovalReceipt, ChapterPlanRecord,
    CharacterAuthorityRecord, ContextPackageRecord, DeliveryAdvisory, DeliveryAdvisoryWindowRecord,
    HookDebtReportRecord, LongformArchiveRecord, NovelCreationDraft, NovelProjectManifest,
    NovelStudioArgs, ReviewCycleRecord, ReviewReceipt, SettlementOutput, SnapshotRecord,
    SourceRecord, StateSettlementDisposition, StateValidationOutput, StyleProfileRecord,
    TitleState, TruthFileRecord, TruthValidationRecord, VolumeRecord, VolumeSummaryRecord,
};
pub(crate) use model::{ChapterArchitectureRecord, ChapterContractRecord, ChapterRecord};
pub(crate) use novel_bible::StoryContract;
#[cfg(test)]
use pathing::invalid_draft_path_as_project_path_result;
use pathing::{
    canonical_or_self, canonical_parent_join, find_existing_title_conflicts,
    normalize_project_lookup_key, project_path_looks_like_draft_file,
    project_path_points_to_draft_file, reject_parent_components, slugify, title_similarity,
    unique_child_path,
};
use project_config::{character_authority_fingerprint, govern_novel_creation_draft_authority};
use project_governance::{
    canonical_project_contract_projection, canonical_project_title,
    chapter_title_is_generic_stage_label, chapter_volume_pair,
    cjk_title_candidate_has_sentence_fragment_edge, cjk_title_core_has_prose_grammar_fragment,
    discard_chapter_character_registrations, ensure_character_authority_ledger,
    ensure_project_governance, ensure_story_bible_from_manifest, ensure_structured_contract_v2,
    ensure_volume_records_from_story_bible, final_chapter_title_from_body_with_metadata,
    invalidate_story_bible_planning_after, promote_approved_chapter_character_identity_markers,
    promote_chapter_character_registrations, rebuild_story_bible_from_contract_only,
    rebuild_story_bible_from_manifest, register_chapter_character_requests,
    title_is_default_chapter_heading, title_matches_project_or_volume, volume_for_chapter,
};
#[cfg(test)]
use project_governance::{final_chapter_title_from_body, title_needs_post_body_repair};
use prose_sanitizer::{
    is_chinese_noise_boundary, line_is_standalone_markup_residue, sanitize_chinese_script_noise,
    sanitize_saved_prose, strip_short_escape_residue_near_chinese_line,
};
#[cfg(test)]
use prose_sanitizer::{
    normalize_chinese_surface_punctuation, strip_adjacent_foreign_alpha_runs_from_chinese_text,
    strip_chinese_markup_residue_lines, strip_embedded_structured_field_residue_from_chinese_prose,
    strip_isolated_unexpected_scripts_from_chinese_text,
};
use quality_checks::*;

pub(super) fn contract_stable_character_pronoun_profile_in_text(
    content: &str,
    name: &str,
    other_character_names: &BTreeSet<String>,
) -> Option<&'static str> {
    stable_character_pronoun_profile_in_text(content, name, other_character_names)
}

pub(super) fn contract_stable_primary_pronoun_profile_in_text(
    content: &str,
) -> Option<&'static str> {
    stable_primary_pronoun_profile_in_text(content)
}

pub(super) fn contract_explicit_identity_profile_in_character_anchor(
    content: &str,
) -> Option<&'static str> {
    explicit_identity_profile_in_character_anchor(content)
}
#[cfg(test)]
use quality_gate::chapter_title_fatigue_issues;
use quality_gate::{
    chapter_completion_gate_json, chapter_metadata_gate, chapter_outcome_status,
    chapter_quality_gate, cross_chapter_duplicate_issues, extend_quality_gate_issues,
    mechanical_chapter_issues,
};
use rendering::{
    render_architecture_file, render_chapter_file, render_contract, render_plan_file,
    render_project_readme, render_review_file, render_source_file, render_style_file,
    render_truth_file, stable_chapter_path, truth_file_body,
};
use reporting::*;
use runtime_records::*;
#[cfg(test)]
use settlement::{deterministic_state_validation, parse_settlement_output};
use settlement::{
    payoff_continuity_update, validate_settlement_for_chapter,
    validated_settlement_from_final_body,
    validated_settlement_from_final_body_after_observer_exhaustion,
};
use storage::atomic_write_file;
use support::*;
use text_surface::{
    leading_line_looks_like_same_chapter_heading, markdown_heading_text,
    normalize_chapter_body_for_record, strip_markdown_heading,
    strip_redundant_leading_chapter_heading,
};
use tool_schema::novel_studio_parameters;

#[cfg(test)]
use export::sanitize_readable_chapter_body;

const SCHEMA_VERSION: &str = "benshu.novel_project.v1";
const MAX_SINGLE_TEXT_BYTES: usize = 8 * 1024 * 1024;
const PROMPT_TRUTH_FILE_CHARS: usize = 1_200;
const PROMPT_TRUTH_TOTAL_CHARS: usize = 5_000;
const PROMPT_SOURCE_EXCERPT_CHARS: usize = 800;
const PROMPT_CONTRACT_TEXT_CHARS: usize = 900;
const PROMPT_CONTRACT_ITEM_CHARS: usize = 180;
const PROMPT_CONTRACT_ARRAY_ITEMS: usize = 8;
const PROMPT_STORY_BIBLE_TEXT_CHARS: usize = 240;
const PROMPT_STORY_BIBLE_ARRAY_ITEMS: usize = 8;
const CONTEXT_RECENT_CHAPTER_LIMIT: usize = 3;
const CONTEXT_SOURCE_LIMIT: usize = 4;
const CONTEXT_ARCHIVE_LIMIT: usize = 3;
const ARCHIVE_ARC_CHAPTER_SPAN: usize = 20;
const ARCHIVE_VOLUME_CHAPTER_SPAN: usize = 100;
const ACTIVE_CONTINUITY_CHAPTER_LIMIT: usize = 20;
const ARCHIVE_EXCERPT_CHARS: usize = 1_200;
const AUTO_SNAPSHOT_CHAPTER_INTERVAL: usize = 5;
const CHAPTER_SUMMARY_MAX_CHARS: usize = 360;
const CHAPTER_FACT_MAX_CHARS: usize = 220;
const CHAPTER_FACT_LIMIT: usize = 12;
const CHAPTER_CONTINUITY_LIMIT: usize = 8;
const TRUTH_HOOKS_MAX_CHARS: usize = 900;
const TRUTH_SUMMARY_LINE_MAX_CHARS: usize = 420;

pub(crate) fn chapter_body_completion_issues(content: &str) -> Vec<String> {
    prose_ending_completeness_issues(content)
}

pub struct NovelStudioTool {
    workspace: PathBuf,
    artifact_manager: Option<Arc<ArtifactManager>>,
    agent_id: String,
}

impl NovelStudioTool {
    pub fn new(workspace: PathBuf, agent_id: impl Into<String>) -> Self {
        Self {
            workspace,
            artifact_manager: None,
            agent_id: agent_id.into(),
        }
    }

    pub fn with_artifact_manager(mut self, manager: Arc<ArtifactManager>) -> Self {
        self.artifact_manager = Some(manager);
        self
    }
}

#[async_trait]
impl Tool for NovelStudioTool {
    fn name(&self) -> String {
        "novel_studio".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "novel_studio".to_string(),
            description: "Create and maintain long-form fiction projects: source intake, story contract, assigned-worker chapter policy packets, continuity/truth ledger, drift audit, revision gates, snapshots, and TXT/Markdown export.".to_string(),
            parameters: novel_studio_parameters(),
            parameters_ts: Some(tool_schema::parameters_ts()),
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some(
                "Use after BenShu delegates a long-form fiction task to an equipped writer worker. It stores project state, continuity ledgers, audits, revisions, and exports; it does not browse, search, route, delegate, or call an LLM internally. Use run_next_chapter/run_project to obtain the policy packet for BenShu's assigned writer worker, which then reasons, drafts, audits, revises, and persists each stage through explicit actions. For a new story, create a fresh project title from the current task instead of listing or reusing old projects unless the user asked to continue an existing project.".to_string(),
            ),
            safety_level: SafetyLevel::Green,
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let value: serde_json::Value =
            serde_json::from_str(arguments).map_err(|e| Error::ToolArguments {
                tool_name: "novel_studio".into(),
                message: e.to_string(),
            })?;
        let value = input::normalize_novel_studio_arguments(value);
        if value
            .get("action")
            .and_then(|value| value.as_str())
            .is_none()
        {
            return Ok(serde_json::to_string_pretty(
                &input::missing_novel_action_result(),
            )?);
        }
        let args: NovelStudioArgs =
            serde_json::from_value(value).map_err(|e| Error::ToolArguments {
                tool_name: "novel_studio".into(),
                message: e.to_string(),
            })?;
        if let Some(result) = input::missing_required_content_result(&args) {
            return Ok(serde_json::to_string_pretty(&result)?);
        }
        if let Some(result) = input::invalid_source_content_result(&args) {
            return Ok(serde_json::to_string_pretty(&result)?);
        }
        if let Some(result) = pathing::invalid_draft_path_as_project_path_result(&args) {
            return Ok(serde_json::to_string_pretty(&result)?);
        }

        let output_root = self.output_root_for_args(&args);
        let _project_guard = match self.lock_project_operation(&args).await {
            Ok(guard) => guard,
            Err(error) => {
                if error.to_string().contains("project_busy:") {
                    return Ok(serde_json::to_string_pretty(&json!({
                        "success": false,
                        "recoverable": true,
                        "error_kind": "project_busy",
                        "error": error.to_string(),
                        "action": args.action,
                        "project_path": args.project_path,
                        "next_action": "retry_after_current_writer_finishes"
                    }))?);
                }
                if let Some(value) = recoverable_path_error_result(
                    &error,
                    "novel_studio",
                    &args.action,
                    &args.project_path,
                    &self.workspace,
                    output_root.as_ref(),
                ) {
                    return Ok(serde_json::to_string_pretty(&value)?);
                }
                return Err(error);
            }
        };

        macro_rules! run_action {
            ($future:expr) => {
                match Box::pin($future).await {
                    Ok(value) => value,
                    Err(error) => {
                        if let Some(value) = recoverable_path_error_result(
                            &error,
                            "novel_studio",
                            &args.action,
                            &args.project_path,
                            &self.workspace,
                            output_root.as_ref(),
                        ) {
                            value
                        } else {
                            return Err(error);
                        }
                    }
                }
            };
        }

        let mut result = match args.action.as_str() {
            "list_projects" => run_action!(self.list_projects(&args)),
            "draft_project" => run_action!(self.draft_project(&args)),
            "update_draft" => run_action!(self.update_draft(&args)),
            "show_draft" => run_action!(self.show_draft(&args)),
            "approve_draft" => run_action!(self.approve_draft(&args)),
            "discard_draft" => run_action!(self.discard_draft(&args)),
            "init_project" => run_action!(self.init_project(&args)),
            "update_project" => run_action!(self.update_project(&args)),
            "clone_project" => run_action!(self.clone_project(&args)),
            "add_source" => run_action!(self.add_source(&args)),
            "import_chapters" => run_action!(self.import_chapters(&args)),
            "update_style" => run_action!(self.update_style(&args)),
            "read_style" => run_action!(self.read_style(&args)),
            "set_contract" => run_action!(self.set_contract(&args)),
            // Internal compatibility surface kept for old artifacts/tests. These actions are
            // intentionally hidden from the public schema; new callers should use the canonical
            // public actions exposed by tool_schema::PUBLIC_ACTIONS.
            "plan_chapter" => run_action!(self.plan_chapter(&args)),
            "compose_chapter" if !args.content.trim().is_empty() => {
                run_action!(self.write_draft(&args))
            }
            "compose_chapter" => run_action!(self.compose_context(&args)),
            "architect_chapter" => run_action!(self.architect_chapter(&args)),
            "persist_execution_package" => run_action!(self.persist_execution_package(&args)),
            "write_draft" => run_action!(self.write_draft(&args)),
            "audit_chapter" => run_action!(self.audit_chapter(&args)),
            "record_candidate_decision" => {
                run_action!(self.record_candidate_decision(&args))
            }
            "prepare_delivery_advisory_window" => {
                run_action!(self.prepare_delivery_advisory_window(&args))
            }
            "commit_delivery_advisory_window" => {
                run_action!(self.commit_delivery_advisory_window(&args))
            }
            "repair_chapter_metadata" => run_action!(self.repair_latest_chapter_metadata(&args)),
            "revise_draft" => run_action!(self.revise_chapter(&args)),
            "run_next_chapter" => run_action!(self.run_next_chapter(&args)),
            "run_project" => run_action!(self.run_project(&args)),
            "settle_chapter_state" => run_action!(self.settle_chapter_state(&args)),
            "validate_chapter_state" => run_action!(self.validate_chapter_state(&args)),
            "repair_latest_chapter_metadata" => {
                run_action!(self.repair_latest_chapter_metadata(&args))
            }
            "repair_project_state" => run_action!(self.repair_project_state(&args)),
            "compose_context" => run_action!(self.compose_context(&args)),
            // Internal compatibility surface: workflow internals may still call this directly,
            // but LLM-visible callers should prefer persist_execution_package / run_next_chapter.
            "add_chapter_plan" => run_action!(self.add_chapter_plan(&args)),
            "add_chapter" => run_action!(self.add_chapter(&args)),
            "read_chapter" => run_action!(self.read_chapter(&args)),
            "review_chapter" => run_action!(self.review_chapter(&args)),
            "revise_chapter" => run_action!(self.revise_chapter(&args)),
            "approve_chapter" => run_action!(self.approve_chapter_transaction(&args)),
            "reject_chapter" => run_action!(self.reject_chapter_transaction(&args)),
            "approve_all" => run_action!(self.approve_all(&args)),
            "update_truth" => run_action!(self.update_truth(&args)),
            "read_truth" => run_action!(self.read_truth(&args)),
            "snapshot" => run_action!(self.snapshot(&args)),
            "restore_snapshot" => run_action!(self.restore_snapshot(&args)),
            "analytics" => run_action!(self.analytics(&args)),
            "audit" => run_action!(self.audit(&args)),
            "status" => run_action!(self.status(&args)),
            "export" => run_action!(self.export(&args)),
            other => input::wrong_novel_studio_action_result(other),
        };

        if let (Some(phase), Some(object)) = (
            pipeline::phase_for_action(&args.action),
            result.as_object_mut(),
        ) {
            object.entry("stage").or_insert_with(|| json!(phase));
        }

        let output = serde_json::to_string_pretty(&result)?;
        Ok(output)
    }
}

fn preview_chars(value: &str, max_chars: usize) -> String {
    let mut out = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() > max_chars {
        out.push_str("...");
    }
    out
}

fn route_cross_chapter_duplicate_issues(
    quality_gate: &mut ChapterQualityGate,
    duplicate_findings: Vec<chapter_quality::ChapterFinding>,
) {
    quality_gate.extend_findings(duplicate_findings);
}

#[cfg(test)]
#[path = "novel_studio_tests.rs"]
mod novel_studio_tests;
