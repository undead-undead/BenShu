use super::{creation_draft_contract_blocking_issues_for_scope, CreationDraftLifecycleStatus};
use crate::tool::writing::novel_contract_v2::{
    AntagonistPressure, ArtifactLedgerEntry, ChapterEndingRotation, CharacterVoiceProfile,
    ConflictPressureCurve, EmotionalContract, EmotionalStateLedgerEntry, GeographyModel,
    MotifLedgerEntry, NarrationContract, NovelContractV2, PayoffMatrixEntry, PowerProgression,
    ReaderPromise, RelationshipInteractionQuota, RelationshipLedgerEntry, ResourceEconomy,
    RevealScheduleEntry, SceneTypeMix, SocialOrder, TimeModel,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContractReadinessScope {
    DisplayContract,
    LockedAuthorityContract,
    #[cfg(test)]
    FullLongformContract,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCreationDraftState {
    pub schema_version: String,
    pub session_id: String,
    pub artifact_kind: String,
    pub tool_name: String,
    pub draft_path: String,
    #[serde(default)]
    pub project_path: String,
    pub title: String,
    pub language: String,
    pub genre: String,
    pub brief: String,
    pub document_type: String,
    pub audience: String,
    pub purpose: String,
    pub thesis_or_premise: String,
    pub target_units: Option<usize>,
    #[serde(default)]
    pub target_units_user_specified: bool,
    pub chapter_unit_target: Option<usize>,
    #[serde(default)]
    pub chapter_unit_target_user_specified: bool,
    /// The normalized chapter tier captured from the user's request.  This is
    /// kept separately from the mutable draft projection so generated
    /// contract candidates and approval responses cannot replace it.
    #[serde(default)]
    pub chapter_unit_target_user_authority: Option<usize>,
    pub section_unit_target: Option<usize>,
    pub max_chapters_per_turn: Option<usize>,
    pub export_format: String,
    pub export_when_complete: bool,
    pub approved_only: bool,
    pub required_structure: Vec<String>,
    pub evidence_rules: Vec<String>,
    pub style_rules: Vec<String>,
    #[serde(default)]
    pub planning_notes: Vec<String>,
    #[serde(default)]
    pub diagnostics: Vec<String>,
    #[serde(default)]
    pub current_contract: Option<Value>,
    #[serde(default)]
    pub pending_contract_candidate: Option<Value>,
    #[serde(default)]
    pub fiction_premise: String,
    #[serde(default)]
    pub fiction_themes: Vec<String>,
    #[serde(default)]
    pub fiction_characters: Vec<String>,
    #[serde(default)]
    pub fiction_world_rules: Vec<String>,
    #[serde(default)]
    pub fiction_style_rules: Vec<String>,
    #[serde(default)]
    pub fiction_must_avoid: Vec<String>,
    #[serde(default)]
    pub fiction_outline: String,
    #[serde(default)]
    pub fiction_ending_direction: String,
    #[serde(default)]
    pub fiction_protagonist_arc: String,
    #[serde(default)]
    pub fiction_world_imagery: String,
    #[serde(default)]
    pub fiction_main_causal_spine: String,
    #[serde(default)]
    pub fiction_title_rationale: String,
    #[serde(default)]
    pub field_requirements: BTreeMap<String, String>,
    #[serde(default)]
    pub structured_contract_schema_version: String,
    #[serde(default)]
    pub structured_contract_revision: u64,
    #[serde(default)]
    pub resource_economy: ResourceEconomy,
    #[serde(default)]
    pub emotional_contract: EmotionalContract,
    #[serde(default)]
    pub emotional_state_ledger: Vec<EmotionalStateLedgerEntry>,
    #[serde(default)]
    pub relationship_ledger: Vec<RelationshipLedgerEntry>,
    #[serde(default)]
    pub power_progression: PowerProgression,
    #[serde(default)]
    pub social_order: SocialOrder,
    #[serde(default)]
    pub geography_model: GeographyModel,
    #[serde(default)]
    pub time_model: TimeModel,
    #[serde(default)]
    pub artifact_ledger: Vec<ArtifactLedgerEntry>,
    #[serde(default)]
    pub antagonist_pressure: AntagonistPressure,
    #[serde(default)]
    pub payoff_matrix: Vec<PayoffMatrixEntry>,
    #[serde(default)]
    pub narration_contract: NarrationContract,
    #[serde(default)]
    pub scene_type_mix: SceneTypeMix,
    #[serde(default)]
    pub character_voice_ledger: Vec<CharacterVoiceProfile>,
    #[serde(default)]
    pub reader_promise: ReaderPromise,
    #[serde(default)]
    pub chapter_ending_rotation: ChapterEndingRotation,
    #[serde(default)]
    pub conflict_pressure_curve: ConflictPressureCurve,
    #[serde(default)]
    pub motif_ledger: Vec<MotifLedgerEntry>,
    #[serde(default)]
    pub reveal_schedule: Vec<RevealScheduleEntry>,
    #[serde(default)]
    pub relationship_interaction_quotas: Vec<RelationshipInteractionQuota>,
    pub created_at: String,
    pub updated_at: String,
    pub status: String,
}

impl SessionCreationDraftState {
    pub(crate) fn user_chapter_unit_target(&self) -> Option<usize> {
        self.chapter_unit_target_user_authority.or_else(|| {
            self.chapter_unit_target_user_specified
                .then_some(self.chapter_unit_target)
                .flatten()
        })
    }

    pub fn lifecycle_status(&self) -> CreationDraftLifecycleStatus {
        CreationDraftLifecycleStatus::from_str(&self.status)
            .unwrap_or(CreationDraftLifecycleStatus::DraftingContract)
    }

    pub fn set_lifecycle_status(&mut self, status: CreationDraftLifecycleStatus) {
        self.status = status.as_str().to_string();
    }

    pub(crate) fn contract_v2(&self) -> NovelContractV2 {
        let mut contract = NovelContractV2 {
            schema_version: self.structured_contract_schema_version.clone(),
            revision: self.structured_contract_revision,
            field_requirements: self.field_requirements.clone(),
            resource_economy: self.resource_economy.clone(),
            emotional_contract: self.emotional_contract.clone(),
            emotional_state_ledger: self.emotional_state_ledger.clone(),
            relationship_ledger: self.relationship_ledger.clone(),
            power_progression: self.power_progression.clone(),
            social_order: self.social_order.clone(),
            geography_model: self.geography_model.clone(),
            time_model: self.time_model.clone(),
            artifact_ledger: self.artifact_ledger.clone(),
            antagonist_pressure: self.antagonist_pressure.clone(),
            payoff_matrix: self.payoff_matrix.clone(),
            narration_contract: self.narration_contract.clone(),
            scene_type_mix: self.scene_type_mix.clone(),
            character_voice_ledger: self.character_voice_ledger.clone(),
            reader_promise: self.reader_promise.clone(),
            chapter_ending_rotation: self.chapter_ending_rotation.clone(),
            conflict_pressure_curve: self.conflict_pressure_curve.clone(),
            motif_ledger: self.motif_ledger.clone(),
            reveal_schedule: self.reveal_schedule.clone(),
            relationship_interaction_quotas: self.relationship_interaction_quotas.clone(),
        };
        contract.normalize();
        contract
    }

    pub(crate) fn set_contract_v2(&mut self, mut contract: NovelContractV2) {
        contract.normalize();
        self.structured_contract_schema_version = contract.schema_version.clone();
        self.structured_contract_revision = contract.revision;
        self.field_requirements = contract.field_requirements;
        self.resource_economy = contract.resource_economy;
        self.emotional_contract = contract.emotional_contract;
        self.emotional_state_ledger = contract.emotional_state_ledger;
        self.relationship_ledger = contract.relationship_ledger;
        self.power_progression = contract.power_progression;
        self.social_order = contract.social_order;
        self.geography_model = contract.geography_model;
        self.time_model = contract.time_model;
        self.artifact_ledger = contract.artifact_ledger;
        self.antagonist_pressure = contract.antagonist_pressure;
        self.payoff_matrix = contract.payoff_matrix;
        self.narration_contract = contract.narration_contract;
        self.scene_type_mix = contract.scene_type_mix;
        self.character_voice_ledger = contract.character_voice_ledger;
        self.reader_promise = contract.reader_promise;
        self.chapter_ending_rotation = contract.chapter_ending_rotation;
        self.conflict_pressure_curve = contract.conflict_pressure_curve;
        self.motif_ledger = contract.motif_ledger;
        self.reveal_schedule = contract.reveal_schedule;
        self.relationship_interaction_quotas = contract.relationship_interaction_quotas;
    }

    pub fn is_approved(&self) -> bool {
        self.lifecycle_status() == CreationDraftLifecycleStatus::Approved
    }

    pub fn can_accept_contract_candidate(&self) -> bool {
        matches!(
            self.lifecycle_status(),
            CreationDraftLifecycleStatus::DraftingContract | CreationDraftLifecycleStatus::Blocked
        )
    }

    pub fn refresh_contract_status_from_validation(&mut self) {
        if self.is_approved() {
            return;
        }
        if creation_draft_contract_blocking_issues_for_scope(
            self,
            ContractReadinessScope::LockedAuthorityContract,
        )
        .is_empty()
        {
            self.set_lifecycle_status(CreationDraftLifecycleStatus::ContractReady);
        } else {
            self.set_lifecycle_status(CreationDraftLifecycleStatus::DraftingContract);
        }
    }
}
