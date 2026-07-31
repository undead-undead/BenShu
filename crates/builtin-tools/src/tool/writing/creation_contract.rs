//! Writing creation-contract draft lifecycle and contract orchestration.
//!
//! This module is owned by the writing tools. Gateway/chat code may call it as
//! an adapter, but writing-specific contract rules should live here.
//!
//! Boundary rule: this module may transition creation-draft lifecycle state and
//! coordinate contract candidates. Final typed readiness belongs to
//! `typed_contract_gate`; naming quality belongs to `naming`; user-visible
//! display belongs to `session_surface`.

use crate::tool::delegation::DelegateTool;
use crate::tool::writing::creation_contract_model::{
    value_missing, ChapterSeedContract, CharacterContract, CharacterRoleSlotCoverage,
    EndingContract, NovelCreationContract, OutlineContract, TitleContract, TitleSource,
    VolumeContract,
};
use crate::tool::writing::creation_contract_normalizer;
use crate::tool::writing::intent_policy::{self, WritingIntent, WritingIntentInput};
use crate::tool::writing::longform_policy;
use crate::tool::writing::naming;
use crate::tool::writing::novel_contract_v2::{
    self, AntagonistPressure, ChapterEndingRotation, CharacterVoiceProfile, ConflictPressureCurve,
    EmotionalContract, EmotionalStateLedgerEntry, MotifLedgerEntry, NarrationContract,
    NovelContractV2, PayoffMatrixEntry, ReaderPromise, RelationshipInteractionQuota,
    RelationshipLedgerEntry, RevealScheduleEntry, SceneTypeMix,
};
use crate::tool::writing::surface_sanitizer;
use crate::tool::writing::typed_contract_gate;
use async_trait::async_trait;
use benshu_compression::preview_text;
use benshu_runtime_policy_core::{
    detect_creation_artifact_kind, evaluate_creation_intake, resolve_language_contract,
};
use generated_gate::contract_gate_from_findings;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

fn record_chapter_unit_band_normalization_note(
    draft: &mut SessionCreationDraftState,
    raw: Option<usize>,
    normalized: Option<usize>,
) {
    let (Some(raw), Some(normalized)) = (raw, normalized) else {
        return;
    };
    if raw == normalized {
        return;
    }
    let note = format!(
        "用户请求每章约 {raw} 字；小说每章字数仅支持 {}，已自动归一到 {normalized}。",
        longform_policy::novel_chapter_unit_band_label()
    );
    draft.planning_notes = merge_list(&draft.planning_notes, &[note]);
}

pub const CREATION_PLANNING_DIALOGUE_MARKER: &str = "[BENSHU_CREATION_PLANNING_DIALOGUE]";
pub const NOVEL_CONTENT_OPERATION_MARKER: &str = "[BENSHU_NOVEL_CONTENT_OPERATION]";
pub const DIRECT_WRITER_CONTINUATION_MARKER: &str = "[BENSHU_DIRECT_WRITER_CONTINUATION]";

#[derive(Debug, Clone)]
pub struct CreationDraftUserResponse {
    pub response: String,
    pub chat_route: String,
    pub tool_surface_mode: String,
    pub runtime_persistence_status: String,
}

impl CreationDraftUserResponse {
    pub fn new(response: impl Into<String>, tool_surface_mode: impl Into<String>) -> Self {
        Self {
            response: response.into(),
            chat_route: "coordinator::creation_intake".to_string(),
            tool_surface_mode: tool_surface_mode.into(),
            runtime_persistence_status: "not_needed".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum CreationDraftTurnOutcome {
    Respond(CreationDraftUserResponse),
    ContinueWithMessage(String),
}

pub fn creation_draft_planning_response(
    draft: &SessionCreationDraftState,
    latest_user: &str,
) -> CreationDraftUserResponse {
    CreationDraftUserResponse::new(
        creation_draft_planning_response_text(draft, latest_user),
        draft.artifact_kind.clone(),
    )
}

#[cfg(test)]
mod boundary_text_gate;
mod chat_flow;
mod contract_candidate;
mod contract_text;
mod draft_contract_bridge;
mod draft_lifecycle;
mod draft_readiness;
mod draft_state;
mod gate;
mod generated_gate;
mod genre_patch_profile;
pub(crate) mod issue;
mod lifecycle;
mod patch;
mod patch_normalizer;
mod patch_prompt;
#[cfg(test)]
mod planning_gate;
mod repair_coordinator;
mod staged_prompts;
mod validation;

pub use chat_flow::{
    creation_draft_metadata_key, creation_intake_response, handle_creation_draft_chat,
    infer_project_artifact_kind, intent_requests_existing_work_continuation, CreationDraftRuntime,
};
pub(crate) use chat_flow::{
    intent_requests_existing_work_generation, intent_requests_existing_work_read_only_status,
};
pub(crate) use contract_candidate::*;
pub(crate) use contract_text::*;
pub(crate) use draft_contract_bridge::*;
pub use draft_lifecycle::build_initial_creation_draft;
#[cfg(test)]
pub(crate) use draft_lifecycle::pending_explicit_contract_revision_issue;
#[cfg(test)]
pub(crate) use draft_lifecycle::FORBIDDEN_CHARACTER_NAMING_PREFIX;
pub(crate) use draft_lifecycle::{
    apply_message_to_creation_draft, clear_applied_explicit_contract_revisions,
    clear_contract_quality_blocker_diagnostic, clear_fiction_contract_fields,
    creation_contract_repair_only_message, fiction_concept_replacement_requested,
    forbidden_naming_authority, pending_explicit_contract_revision_findings,
    record_contract_quality_blocker_diagnostic, CONTRACT_QUALITY_BLOCKER_DIAGNOSTIC_PREFIX,
};
pub(crate) use draft_readiness::creation_draft_contract_blocking_findings_for_scope;
#[cfg(test)]
pub(crate) use draft_readiness::creation_draft_contract_blocking_issues;
pub(crate) use draft_readiness::creation_draft_contract_blocking_issues_for_scope;
pub(crate) use draft_state::ContractReadinessScope;
pub use draft_state::SessionCreationDraftState;
pub(crate) use gate::{ContractGateResult, ContractGateStatus, ContractSubmissionOutcome};
#[cfg(test)]
pub(crate) use generated_gate::{
    generated_contract_advisory_issues, generated_contract_completion_quality_issues,
    generated_contract_gate_result, generated_contract_quality_issues,
};
pub(crate) use genre_patch_profile::GenrePatchProfile;
pub use issue::creation_contract_issue_summary;
pub use lifecycle::CreationDraftLifecycleStatus;
pub(crate) use patch::*;
#[cfg(test)]
pub(crate) use patch_normalizer::infer_book_title_from_rationale_text;
pub(crate) use patch_normalizer::{
    derive_plot_contract_from_outline_text, normalize_creation_contract_patch_boundary,
    strip_plot_control_segments_from_outline_text,
};
#[cfg(test)]
pub(crate) use planning_gate::generated_fiction_contract_planning_issues;
pub use repair_coordinator::{
    maybe_repair_creation_planning_outcome, CreationContractRepairRuntime,
};
pub(crate) use staged_prompts::{
    final_prompt_from_initial_contract_batch, final_prompt_from_staged_contract_completion,
};
pub use validation::{creation_contract_draft_is_confirmable, CreationContractSurfaceState};
pub(crate) use validation::{latest_contract_status_issues, ContractValidationReport};

pub fn apply_continuation_controls_to_creation_draft(
    draft: &mut SessionCreationDraftState,
    message: &str,
) {
    let language = resolve_language_contract(message).artifact_language;
    if !language.trim().is_empty() {
        draft.language = language;
    }
    let requested_title_value = requested_title(message);
    if let Some(title) = requested_title_value.as_ref() {
        draft.title = title.clone();
    }
    if let Some(target) = requested_total_unit_target(message) {
        draft.target_units = Some(target);
    }
    let raw_chapter_unit_target = requested_raw_chapter_unit_target(message);
    if let Some(target) = raw_chapter_unit_target.map(nearest_novel_chapter_unit_band) {
        draft.chapter_unit_target = Some(target);
        record_chapter_unit_band_normalization_note(draft, raw_chapter_unit_target, Some(target));
    }
    if let Some(target) = requested_section_unit_target(message) {
        draft.section_unit_target = Some(target);
    }
    if let Some(count) = requested_max_chapters_per_turn(message) {
        draft.max_chapters_per_turn = Some(count);
    }
    if let Some(format) = requested_export_format(message) {
        draft.export_format = format;
    }
    if draft.artifact_kind == "fiction" {
        if let Some(genre) =
            infer_followup_fiction_genre(message).or_else(|| infer_fiction_genre(message))
        {
            if fiction_concept_replacement_requested(message) {
                draft.genre = genre;
                if requested_title_value.is_none() {
                    draft.title.clear();
                }
                clear_fiction_contract_fields(draft);
            } else {
                draft.genre = merge_short_field(&draft.genre, &genre);
            }
        }
    }
    draft.updated_at = chrono::Utc::now().to_rfc3339();
}

mod surface;

pub(crate) use surface::*;

mod intent;

pub(crate) use intent::*;
pub use intent::{
    message_requests_metadata_only_content_operation, project_path_from_approved_creation_draft,
    sync_creation_draft_from_approval,
};

mod tool_args;

pub use tool_args::creation_draft_tool_args;

#[path = "creation_contract_tests.rs"]
mod creation_contract_tests;
