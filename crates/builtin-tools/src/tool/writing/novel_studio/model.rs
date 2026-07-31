use super::*;
use crate::tool::writing::creation_contract_model::ChapterSeedContract;
use crate::tool::writing::novel_contract_v2::{
    ChapterCharacterRegistration, ChapterCharacterRequest, ChapterEndingRotation,
    CharacterVoiceProfile, ConflictPressureCurve, MotifLedgerEntry, ReaderPromise,
    RelationshipInteractionQuota, RevealScheduleEntry, SceneTypeMix,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct SettlementOutput {
    #[serde(default)]
    pub(super) chapter_fingerprint: String,
    #[serde(default)]
    pub(super) body_fingerprint: String,
    #[serde(default)]
    pub(super) authority_fingerprint: String,
    #[serde(default)]
    pub(super) state_changes: Vec<novel_bible::ChapterStateChange>,
    #[serde(default)]
    pub(super) degraded_reason: String,
    #[serde(default)]
    pub(super) current_state: String,
    #[serde(default)]
    pub(super) pending_hooks: String,
    #[serde(default)]
    pub(super) chapter_summary: String,
    #[serde(default)]
    pub(super) continuity_updates: Vec<String>,
    #[serde(default)]
    pub(super) resolved_hooks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct StateValidationOutput {
    #[serde(default)]
    pub(super) passed: bool,
    #[serde(default)]
    pub(super) warnings: Vec<String>,
    #[serde(default)]
    pub(super) advisories: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct NovelStudioArgs {
    pub(super) action: String,
    #[serde(default)]
    pub(super) project_path: String,
    #[serde(default)]
    pub(super) draft_path: String,
    #[serde(default)]
    pub(super) output_root: String,
    #[serde(default)]
    pub(super) source_project_path: String,
    #[serde(default)]
    pub(super) snapshot_id: String,
    #[serde(default)]
    pub(super) overwrite: bool,
    #[serde(default)]
    pub(super) allow_title_conflict: bool,
    #[serde(default)]
    pub(super) approved_only: bool,
    #[serde(default)]
    pub(super) include_draft: bool,
    #[serde(default)]
    pub(super) minimal_context: bool,
    #[serde(default)]
    pub(super) candidate_only: bool,
    #[serde(default)]
    pub(super) administrative_override: bool,
    #[serde(default)]
    pub(super) title: String,
    #[serde(default)]
    pub(super) language: String,
    #[serde(default)]
    pub(super) genre: String,
    #[serde(default)]
    pub(super) brief: String,
    #[serde(default)]
    pub(super) target_units: Option<usize>,
    #[serde(default)]
    pub(super) chapter_unit_target: Option<usize>,
    #[serde(default)]
    pub(super) max_chapters_per_turn: Option<usize>,
    #[serde(default)]
    pub(super) source_title: String,
    #[serde(default)]
    pub(super) source_url: String,
    #[serde(default)]
    pub(super) notes: String,
    /// Internal workflow signal. It is intentionally omitted from the public
    /// tool schema and must not be encoded into user-facing free text.
    #[serde(default)]
    pub(super) observer_attempts_exhausted: bool,
    #[serde(default)]
    pub(super) content: String,
    #[serde(default)]
    pub(super) split_pattern: String,
    #[serde(default)]
    pub(super) premise: String,
    #[serde(default)]
    pub(super) ending_direction: String,
    #[serde(default)]
    pub(super) authority_contract: Option<NovelCreationContract>,
    #[serde(default)]
    pub(super) protagonist_arc: String,
    #[serde(default)]
    pub(super) world_imagery: String,
    #[serde(default)]
    pub(super) main_causal_spine: String,
    #[serde(default)]
    pub(super) title_rationale: String,
    #[serde(default)]
    pub(super) themes: Vec<String>,
    #[serde(default)]
    pub(super) characters: Vec<String>,
    #[serde(default)]
    pub(super) world_rules: Vec<String>,
    #[serde(default)]
    pub(super) style_rules: Vec<String>,
    #[serde(default)]
    pub(super) must_avoid: Vec<String>,
    #[serde(default)]
    pub(super) outline: String,
    #[serde(default)]
    pub(super) field_requirements: BTreeMap<String, String>,
    #[serde(default)]
    pub(super) resource_economy: ResourceEconomy,
    #[serde(default)]
    pub(super) emotional_contract: EmotionalContract,
    #[serde(default)]
    pub(super) emotional_state_ledger: Vec<EmotionalStateLedgerEntry>,
    #[serde(default)]
    pub(super) relationship_ledger: Vec<RelationshipLedgerEntry>,
    #[serde(default)]
    pub(super) power_progression: PowerProgression,
    #[serde(default)]
    pub(super) social_order: SocialOrder,
    #[serde(default)]
    pub(super) geography_model: GeographyModel,
    #[serde(default)]
    pub(super) time_model: TimeModel,
    #[serde(default)]
    pub(super) artifact_ledger: Vec<ArtifactLedgerEntry>,
    #[serde(default)]
    pub(super) antagonist_pressure: AntagonistPressure,
    #[serde(default)]
    pub(super) payoff_matrix: Vec<PayoffMatrixEntry>,
    #[serde(default)]
    pub(super) narration_contract: NarrationContract,
    #[serde(default)]
    pub(super) scene_type_mix: SceneTypeMix,
    #[serde(default)]
    pub(super) character_voice_ledger: Vec<CharacterVoiceProfile>,
    #[serde(default)]
    pub(super) reader_promise: ReaderPromise,
    #[serde(default)]
    pub(super) chapter_ending_rotation: ChapterEndingRotation,
    #[serde(default)]
    pub(super) conflict_pressure_curve: ConflictPressureCurve,
    #[serde(default)]
    pub(super) motif_ledger: Vec<MotifLedgerEntry>,
    #[serde(default)]
    pub(super) reveal_schedule: Vec<RevealScheduleEntry>,
    #[serde(default)]
    pub(super) relationship_interaction_quotas: Vec<RelationshipInteractionQuota>,
    #[serde(default)]
    pub(super) plan: String,
    #[serde(default)]
    pub(super) chapter_number: Option<usize>,
    #[serde(default)]
    pub(super) chapter_title: String,
    #[serde(default)]
    pub(super) scene_goal: String,
    #[serde(default)]
    pub(super) conflict: String,
    #[serde(default)]
    pub(super) choice: String,
    #[serde(default)]
    pub(super) cost: String,
    #[serde(default)]
    pub(super) reveal: String,
    #[serde(default)]
    pub(super) emotional_beat: String,
    #[serde(default)]
    pub(super) new_state_after_chapter: String,
    #[serde(default)]
    pub(super) relationship_delta: String,
    #[serde(default)]
    pub(super) power_delta: String,
    #[serde(default)]
    pub(super) resource_delta: String,
    #[serde(default)]
    pub(super) hook_opened: Vec<String>,
    #[serde(default)]
    pub(super) hook_paid_off: Vec<String>,
    #[serde(default)]
    pub(super) character_change: String,
    #[serde(default)]
    pub(super) world_change: String,
    #[serde(default)]
    pub(super) payoff_target: String,
    #[serde(default)]
    pub(super) future_chapters: Vec<ChapterSeedContract>,
    #[serde(default)]
    pub(super) new_character_requests: Vec<ChapterCharacterRequest>,
    #[serde(default)]
    pub(super) summary: String,
    #[serde(default)]
    pub(super) key_facts: Vec<String>,
    #[serde(default)]
    pub(super) continuity_updates: Vec<String>,
    #[serde(default)]
    pub(super) issues: Vec<String>,
    #[serde(default)]
    pub(super) findings: Vec<chapter_quality::ChapterFinding>,
    #[serde(default)]
    pub(super) advisories: Vec<String>,
    #[serde(default)]
    pub(super) score: Option<u8>,
    #[serde(default)]
    pub(super) attempt_kind: String,
    #[serde(default)]
    pub(super) candidate_fingerprint: String,
    #[serde(default)]
    pub(super) quality_vector: serde_json::Value,
    #[serde(default)]
    pub(super) accepted_as_best: bool,
    #[serde(default)]
    pub(super) best_candidate_path: String,
    #[serde(default)]
    pub(super) feedback: String,
    #[serde(default)]
    pub(super) verdict: String,
    #[serde(default)]
    pub(super) section: String,
    #[serde(default)]
    pub(super) revision_notes: String,
    #[serde(default)]
    pub(super) status: String,
    #[serde(default)]
    pub(super) format: String,
    #[serde(default)]
    pub(super) output: String,
    #[serde(default)]
    pub(super) export_when_complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct NovelProjectManifest {
    pub(super) schema_version: String,
    pub(super) title: String,
    #[serde(default)]
    pub(super) title_state: TitleState,
    pub(super) language: String,
    pub(super) genre: String,
    pub(super) brief: String,
    pub(super) target_units: Option<usize>,
    pub(super) chapter_unit_target: Option<usize>,
    #[serde(default)]
    pub(super) max_chapters_per_turn: Option<usize>,
    #[serde(default)]
    pub(super) export_format: Option<String>,
    #[serde(default)]
    pub(super) export_when_complete: bool,
    #[serde(default)]
    pub(super) approved_only: bool,
    pub(super) created_at: String,
    pub(super) updated_at: String,
    #[serde(default)]
    pub(super) sources: Vec<SourceRecord>,
    #[serde(default)]
    pub(super) chapter_plans: Vec<ChapterPlanRecord>,
    #[serde(default)]
    pub(super) chapter_contracts: Vec<ChapterContractRecord>,
    #[serde(default)]
    pub(super) context_packages: Vec<ContextPackageRecord>,
    #[serde(default)]
    pub(super) chapter_architectures: Vec<ChapterArchitectureRecord>,
    #[serde(default)]
    pub(super) chapters: Vec<ChapterRecord>,
    #[serde(default)]
    pub(super) reviews: Vec<ReviewReceipt>,
    #[serde(default)]
    pub(super) review_cycles: Vec<ReviewCycleRecord>,
    #[serde(default)]
    pub(super) truth_validations: Vec<TruthValidationRecord>,
    #[serde(default)]
    pub(super) hook_debt_reports: Vec<HookDebtReportRecord>,
    #[serde(default)]
    pub(super) truth_files: Vec<TruthFileRecord>,
    #[serde(default)]
    pub(super) archives: Vec<LongformArchiveRecord>,
    #[serde(default)]
    pub(super) contract: Option<StoryContract>,
    #[serde(default)]
    pub(super) snapshots: Vec<SnapshotRecord>,
    #[serde(default)]
    pub(super) style_profiles: Vec<StyleProfileRecord>,
    #[serde(default)]
    pub(super) volumes: Vec<VolumeRecord>,
    #[serde(default)]
    pub(super) volume_summaries: Vec<VolumeSummaryRecord>,
    #[serde(default)]
    pub(super) character_ledger: Vec<CharacterAuthorityRecord>,
    #[serde(default)]
    pub(super) story_bible: Option<novel_bible::StoryBible>,
    #[serde(default)]
    pub(super) structured_contract_v2: NovelContractV2,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct TitleState {
    pub(super) provisional_title: String,
    pub(super) canonical_title: String,
    pub(super) source: String,
    pub(super) locked: bool,
    pub(super) rationale: String,
    pub(super) updated_at: String,
}

impl Default for TitleState {
    fn default() -> Self {
        Self {
            provisional_title: String::new(),
            canonical_title: String::new(),
            source: String::new(),
            locked: false,
            rationale: String::new(),
            updated_at: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct SourceRecord {
    pub(super) id: String,
    pub(super) title: String,
    pub(super) path: String,
    pub(super) source_url: Option<String>,
    pub(super) notes: Option<String>,
    pub(super) unit_count: usize,
    pub(super) created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ChapterRecord {
    pub(crate) number: usize,
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) volume_id: String,
    #[serde(default)]
    pub(crate) volume_title: String,
    pub(crate) path: String,
    pub(crate) summary: String,
    pub(crate) unit_count: usize,
    pub(crate) status: String,
    pub(crate) key_facts: Vec<String>,
    pub(crate) continuity_updates: Vec<String>,
    pub(crate) created_at: String,
    #[serde(default)]
    pub(crate) updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct VolumeRecord {
    pub(super) id: String,
    pub(super) title: String,
    pub(super) start_chapter: usize,
    #[serde(default)]
    pub(super) end_chapter: Option<usize>,
    #[serde(default)]
    pub(super) objective: String,
    #[serde(default)]
    pub(super) key_results: Vec<String>,
    #[serde(default)]
    pub(super) emotional_curve: String,
    #[serde(default)]
    pub(super) must_open: Vec<String>,
    #[serde(default)]
    pub(super) must_payoff: Vec<String>,
    #[serde(default)]
    pub(super) ending_change: String,
    #[serde(default)]
    pub(super) status: String,
    #[serde(default)]
    pub(super) summary: Option<String>,
    pub(super) created_at: String,
    pub(super) updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct VolumeSummaryRecord {
    pub(super) volume_id: String,
    pub(super) summary: String,
    pub(super) resolved_hooks: Vec<String>,
    pub(super) new_hooks: Vec<String>,
    pub(super) character_changes: Vec<String>,
    pub(super) world_changes: Vec<String>,
    pub(super) next_volume_pressure: String,
    pub(super) updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct CharacterAuthorityRecord {
    pub(super) id: String,
    pub(super) canonical_name: String,
    #[serde(default)]
    pub(super) name_source: String,
    #[serde(default)]
    pub(super) aliases: Vec<String>,
    #[serde(default)]
    pub(super) identity_markers: Vec<String>,
    pub(super) role: String,
    pub(super) desire: String,
    pub(super) fear: String,
    pub(super) bottom_line: String,
    pub(super) arc_start: String,
    pub(super) arc_end: String,
    #[serde(default)]
    pub(super) planned_entry: String,
    #[serde(default)]
    pub(super) planned_exit: String,
    #[serde(default)]
    pub(super) forbidden_renames: Vec<String>,
    #[serde(default)]
    pub(super) status: String,
    pub(super) updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ChapterPlanRecord {
    pub(super) number: usize,
    pub(super) title: String,
    pub(super) path: String,
    pub(super) plan: String,
    pub(super) status: String,
    pub(super) created_at: String,
    pub(super) updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ChapterContractRecord {
    pub(crate) number: usize,
    pub(crate) title: String,
    pub(crate) path: String,
    pub(crate) markdown_path: String,
    pub(crate) goal: String,
    #[serde(default)]
    pub(crate) scene_goal: String,
    #[serde(default)]
    pub(crate) conflict: String,
    #[serde(default)]
    pub(crate) choice: String,
    #[serde(default)]
    pub(crate) cost: String,
    #[serde(default)]
    pub(crate) reveal: String,
    #[serde(default)]
    pub(crate) emotional_beat: String,
    #[serde(default)]
    pub(crate) new_state_after_chapter: String,
    #[serde(default)]
    pub(crate) relationship_delta: String,
    #[serde(default)]
    pub(crate) power_delta: String,
    #[serde(default)]
    pub(crate) resource_delta: String,
    #[serde(default)]
    pub(crate) hook_opened: Vec<String>,
    #[serde(default)]
    pub(crate) hook_paid_off: Vec<String>,
    #[serde(default)]
    pub(crate) character_change: String,
    #[serde(default)]
    pub(crate) world_change: String,
    #[serde(default)]
    pub(crate) payoff_target: String,
    #[serde(default)]
    pub(crate) new_character_requests: Vec<ChapterCharacterRequest>,
    #[serde(default)]
    pub(crate) character_registrations: Vec<ChapterCharacterRegistration>,
    pub(crate) status: String,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ContextPackageRecord {
    pub(super) number: usize,
    pub(super) path: String,
    pub(super) rules_path: String,
    pub(super) trace_path: String,
    pub(super) selected_sources: usize,
    #[serde(default)]
    pub(super) context_budget: serde_json::Value,
    #[serde(default)]
    pub(super) authority_root_fingerprint: String,
    #[serde(default)]
    pub(super) sealed: bool,
    #[serde(default)]
    pub(super) sealed_at: String,
    #[serde(default)]
    pub(super) chapter_contract_fingerprint: String,
    #[serde(default)]
    pub(super) canonical_contract_fingerprint: String,
    #[serde(default)]
    pub(super) truth_fingerprint: String,
    #[serde(default)]
    pub(super) truth_cutoff_chapter: usize,
    #[serde(default)]
    pub(super) role_projection_fingerprints: BTreeMap<String, String>,
    #[serde(default)]
    pub(super) protected_coverage: serde_json::Value,
    #[serde(default)]
    pub(super) excluded_future_paths: Vec<String>,
    pub(super) created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ChapterArchitectureRecord {
    pub(crate) number: usize,
    pub(crate) title: String,
    pub(crate) path: String,
    pub(crate) architecture: String,
    pub(crate) status: String,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ReviewReceipt {
    pub(super) chapter_number: usize,
    #[serde(default)]
    pub(super) chapter_fingerprint: String,
    #[serde(default)]
    pub(super) authority_fingerprint: String,
    #[serde(default)]
    pub(super) findings: Vec<chapter_quality::ChapterFinding>,
    #[serde(default)]
    pub(super) advisories: Vec<String>,
    #[serde(default)]
    pub(super) score: Option<u8>,
    #[serde(default)]
    pub(super) locally_validated: bool,
    pub(super) verdict: String,
    pub(super) issues: Vec<String>,
    pub(super) feedback: String,
    pub(super) created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ApprovalReceipt {
    pub(super) transaction_id: String,
    pub(super) chapter_number: usize,
    pub(super) body_fingerprint: String,
    pub(super) metadata_fingerprint: String,
    pub(super) authority_fingerprint: String,
    pub(super) review_fingerprint: String,
    pub(super) settlement_fingerprint: String,
    #[serde(default)]
    pub(super) truth_fingerprint: String,
    pub(super) committed_at: String,
    #[serde(default)]
    pub(super) legacy: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ApprovalJournalState {
    Prepared,
    Committed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ApprovalJournal {
    pub(super) transaction_id: String,
    pub(super) chapter_number: usize,
    pub(super) state: ApprovalJournalState,
    pub(super) body_fingerprint: String,
    pub(super) authority_fingerprint: String,
    pub(super) prepared_at: String,
    #[serde(default)]
    pub(super) committed_at: String,
    #[serde(default)]
    pub(super) receipt_path: String,
    #[serde(default)]
    pub(super) backup_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ReviewCycleRecord {
    pub(super) chapter_number: usize,
    pub(super) path: String,
    pub(super) iteration: usize,
    pub(super) verdict: String,
    pub(super) next_action: String,
    #[serde(default)]
    pub(super) attempt_kind: String,
    #[serde(default)]
    pub(super) candidate_fingerprint: String,
    #[serde(default)]
    pub(super) quality_vector: serde_json::Value,
    #[serde(default)]
    pub(super) accepted_as_best: bool,
    #[serde(default)]
    pub(super) best_candidate_path: String,
    pub(super) created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct TruthValidationRecord {
    pub(super) chapter_number: usize,
    #[serde(default)]
    pub(super) chapter_fingerprint: String,
    pub(super) path: String,
    pub(super) verdict: String,
    pub(super) issues: Vec<String>,
    pub(super) created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct HookDebtReportRecord {
    pub(super) chapter_number: usize,
    pub(super) path: String,
    pub(super) debts: Vec<String>,
    pub(super) created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct TruthFileRecord {
    pub(super) section: String,
    pub(super) path: String,
    pub(super) unit_count: usize,
    pub(super) updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct LongformArchiveRecord {
    pub(super) kind: String,
    pub(super) range_start: usize,
    pub(super) range_end: usize,
    pub(super) path: String,
    pub(super) unit_count: usize,
    pub(super) created_at: String,
    pub(super) updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct SnapshotRecord {
    pub(super) id: String,
    pub(super) path: String,
    pub(super) reason: String,
    pub(super) created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct StyleProfileRecord {
    pub(super) id: String,
    pub(super) title: String,
    pub(super) path: String,
    pub(super) unit_count: usize,
    pub(super) created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct NovelCreationDraft {
    pub(super) schema_version: String,
    pub(super) title: String,
    pub(super) language: String,
    pub(super) genre: String,
    pub(super) brief: String,
    pub(super) target_units: Option<usize>,
    pub(super) chapter_unit_target: Option<usize>,
    pub(super) max_chapters_per_turn: Option<usize>,
    pub(super) export_format: String,
    pub(super) export_when_complete: bool,
    pub(super) approved_only: bool,
    pub(super) premise: String,
    #[serde(default)]
    pub(super) ending_direction: String,
    #[serde(default)]
    pub(super) authority_contract: Option<NovelCreationContract>,
    #[serde(default)]
    pub(super) protagonist_arc: String,
    #[serde(default)]
    pub(super) world_imagery: String,
    #[serde(default)]
    pub(super) main_causal_spine: String,
    #[serde(default)]
    pub(super) title_rationale: String,
    pub(super) themes: Vec<String>,
    pub(super) characters: Vec<String>,
    pub(super) world_rules: Vec<String>,
    pub(super) style_rules: Vec<String>,
    pub(super) must_avoid: Vec<String>,
    pub(super) outline: String,
    #[serde(default)]
    pub(super) structured_contract_v2: NovelContractV2,
    pub(super) created_at: String,
    pub(super) updated_at: String,
}
