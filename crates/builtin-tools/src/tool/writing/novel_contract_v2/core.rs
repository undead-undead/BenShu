//! Structured fiction contract fields owned by the writing tools.
//!
//! These structs are intentionally data-only. They let novel projects persist a
//! richer contract without moving writing policy into runtime-policy-core or
//! requiring users to fill a form.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(crate) const NOVEL_CONTRACT_V2_SCHEMA_VERSION: &str = "benshu.novel_contract.v2";

fn default_schema_version() -> String {
    NOVEL_CONTRACT_V2_SCHEMA_VERSION.to_string()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NovelContractV2 {
    #[serde(default = "default_schema_version")]
    pub schema_version: String,
    #[serde(default)]
    pub revision: u64,
    #[serde(default)]
    pub field_requirements: BTreeMap<String, String>,
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
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResourceEconomy {
    #[serde(default)]
    pub currency: String,
    #[serde(default)]
    pub value_scale: String,
    #[serde(default)]
    pub resource_types: Vec<String>,
    #[serde(default)]
    pub income_sources: Vec<String>,
    #[serde(default)]
    pub cost_examples: Vec<String>,
    #[serde(default)]
    pub scarcity_rules: Vec<String>,
    #[serde(default)]
    pub trade_rules: Vec<String>,
    #[serde(default)]
    pub class_impact: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EmotionalContract {
    #[serde(default)]
    pub primary_emotion: String,
    #[serde(default)]
    pub emotional_promise: String,
    #[serde(default)]
    pub emotional_beats: Vec<String>,
    #[serde(default)]
    pub relief_beats: Vec<String>,
    #[serde(default)]
    pub payoff_requirements: Vec<String>,
    #[serde(default)]
    pub ending_emotional_state: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EmotionalStateLedgerEntry {
    #[serde(default)]
    pub character: String,
    #[serde(default)]
    pub current_emotion: String,
    #[serde(default)]
    pub pressure: String,
    #[serde(default)]
    pub desire: String,
    #[serde(default)]
    pub fear: String,
    #[serde(default)]
    pub expected_next_shift: String,
    #[serde(default)]
    pub payoff_target: String,
    #[serde(default)]
    pub last_changed_chapter: Option<usize>,
    #[serde(default)]
    pub transition_history: Vec<EmotionalTransition>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EmotionalTransition {
    #[serde(default)]
    pub chapter_number: Option<usize>,
    #[serde(default)]
    pub from_emotion: String,
    #[serde(default)]
    pub to_emotion: String,
    #[serde(default)]
    pub trigger_event: String,
    #[serde(default)]
    pub relationship_effect: String,
    #[serde(default)]
    pub evidence: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RelationshipLedgerEntry {
    #[serde(default)]
    pub character_ids: Vec<String>,
    #[serde(default)]
    pub characters: Vec<String>,
    #[serde(default)]
    pub arc_type: String,
    #[serde(default)]
    pub relationship_type: String,
    #[serde(default)]
    pub stage: String,
    #[serde(default)]
    pub next_expected_stage: String,
    #[serde(default)]
    pub start_state: String,
    #[serde(default)]
    pub current_state: String,
    #[serde(default)]
    pub desired_end_state: String,
    #[serde(default)]
    pub evidence: String,
    #[serde(default)]
    pub conflicts: Vec<String>,
    #[serde(default)]
    pub secrets: Vec<String>,
    #[serde(default)]
    pub turning_points: Vec<String>,
    #[serde(default)]
    pub transition_history: Vec<RelationshipTransition>,
    #[serde(default)]
    pub last_changed_chapter: Option<usize>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RelationshipTransition {
    #[serde(default)]
    pub chapter_number: Option<usize>,
    #[serde(default)]
    pub from_state: String,
    #[serde(default)]
    pub to_state: String,
    #[serde(default)]
    pub from_stage: String,
    #[serde(default)]
    pub to_stage: String,
    #[serde(default)]
    pub event: String,
    #[serde(default)]
    pub evidence: String,
    #[serde(default)]
    pub relationship_delta: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PowerProgression {
    #[serde(default)]
    pub system_name: String,
    #[serde(default)]
    pub levels: Vec<String>,
    #[serde(default)]
    pub advancement_costs: Vec<String>,
    #[serde(default)]
    pub bottlenecks: Vec<String>,
    #[serde(default)]
    pub failure_consequences: Vec<String>,
    #[serde(default)]
    pub anti_power_creep_rules: Vec<String>,
    #[serde(default)]
    pub character_current_levels: Vec<CharacterProgressionState>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CharacterProgressionState {
    #[serde(default)]
    pub character: String,
    #[serde(default)]
    pub level: String,
    #[serde(default)]
    pub evidence: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SocialOrder {
    #[serde(default)]
    pub institutions: Vec<String>,
    #[serde(default)]
    pub rank_system: String,
    #[serde(default)]
    pub exam_or_promotion_rules: Vec<String>,
    #[serde(default)]
    pub laws: Vec<String>,
    #[serde(default)]
    pub class_structure: String,
    #[serde(default)]
    pub authority_conflicts: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GeographyModel {
    #[serde(default)]
    pub regions: Vec<String>,
    #[serde(default)]
    pub important_locations: Vec<LocationRecord>,
    #[serde(default)]
    pub distance_rules: Vec<String>,
    #[serde(default)]
    pub travel_constraints: Vec<String>,
    #[serde(default)]
    pub location_changes: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LocationRecord {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub known_facts: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TimeModel {
    #[serde(default)]
    pub calendar: String,
    #[serde(default)]
    pub story_start_time: String,
    #[serde(default)]
    pub elapsed_time: String,
    #[serde(default)]
    pub age_progression: Vec<AgeProgressionState>,
    #[serde(default)]
    pub deadline_events: Vec<String>,
    #[serde(default)]
    pub time_skip_rules: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgeProgressionState {
    #[serde(default)]
    pub character: String,
    #[serde(default)]
    pub start_age: String,
    #[serde(default)]
    pub current_age: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ArtifactLedgerEntry {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub owner: String,
    #[serde(default)]
    pub origin: String,
    #[serde(default)]
    pub ability: String,
    #[serde(default)]
    pub cost_or_limit: String,
    #[serde(default)]
    pub last_seen_chapter: Option<usize>,
    #[serde(default)]
    pub status: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AntagonistPressure {
    #[serde(default)]
    pub primary_pressure: String,
    #[serde(default)]
    pub antagonists: Vec<AntagonistRecord>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AntagonistRecord {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub goal: String,
    #[serde(default)]
    pub resources: Vec<String>,
    #[serde(default)]
    pub knowledge_state: String,
    #[serde(default)]
    pub current_move: String,
    #[serde(default)]
    pub escalation_plan: Vec<String>,
    #[serde(default)]
    pub defeat_condition: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PayoffMatrixEntry {
    #[serde(default)]
    pub promise: String,
    #[serde(default)]
    pub introduced_chapter: Option<usize>,
    #[serde(default)]
    pub payoff_target: String,
    #[serde(default)]
    pub payoff_chapter: Option<usize>,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NarrationContract {
    #[serde(default)]
    pub pov: String,
    #[serde(default)]
    pub tense: String,
    #[serde(default)]
    pub narrative_distance: String,
    #[serde(default)]
    pub dialogue_style: String,
    #[serde(default)]
    pub description_density: String,
    #[serde(default)]
    pub chapter_pacing: String,
    #[serde(default)]
    pub forbidden_style_drift: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SceneTypeMix {
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub dialogue: String,
    #[serde(default)]
    pub everyday: String,
    #[serde(default)]
    pub reveal: String,
    #[serde(default)]
    pub emotional: String,
    #[serde(default)]
    pub turning_point: String,
    #[serde(default)]
    pub balance_rule: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CharacterVoiceProfile {
    #[serde(default)]
    pub character: String,
    #[serde(default)]
    pub voice_style: String,
    #[serde(default)]
    pub catchphrases: Vec<String>,
    #[serde(default)]
    pub forbidden_expressions: Vec<String>,
    #[serde(default)]
    pub dialogue_rules: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReaderPromise {
    #[serde(default)]
    pub core_hook: String,
    #[serde(default)]
    pub pleasure_points: Vec<String>,
    #[serde(default)]
    pub curiosity_engine: String,
    #[serde(default)]
    pub payoff_style: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChapterEndingRotation {
    #[serde(default)]
    pub planned_rotation: Vec<String>,
    #[serde(default)]
    pub avoid_repetition_rule: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConflictPressureCurve {
    #[serde(default)]
    pub global_curve: Vec<PressureBeat>,
    #[serde(default)]
    pub release_strategy: String,
    #[serde(default)]
    pub peak_policy: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PressureBeat {
    #[serde(default)]
    pub range: String,
    #[serde(default)]
    pub pressure_level: String,
    #[serde(default)]
    pub function: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MotifLedgerEntry {
    #[serde(default)]
    pub motif: String,
    #[serde(default)]
    pub meaning: String,
    #[serde(default)]
    pub evolution: Vec<String>,
    #[serde(default)]
    pub payoff_target: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RevealScheduleEntry {
    #[serde(default)]
    pub secret: String,
    #[serde(default)]
    pub reader_knows: String,
    #[serde(default)]
    pub protagonist_knows: String,
    #[serde(default)]
    pub antagonist_knows: String,
    #[serde(default)]
    pub reveal_window: String,
    #[serde(default)]
    pub status: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RelationshipInteractionQuota {
    #[serde(default)]
    pub relationship: String,
    #[serde(default)]
    pub characters: Vec<String>,
    #[serde(default)]
    pub cadence: String,
    #[serde(default)]
    pub next_due: String,
    #[serde(default)]
    pub required_interaction: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChapterExecutionContractV2 {
    #[serde(default)]
    pub scene_goal: String,
    #[serde(default)]
    pub conflict: String,
    #[serde(default)]
    pub choice: String,
    #[serde(default)]
    pub cost: String,
    #[serde(default)]
    pub reveal: String,
    #[serde(default)]
    pub emotional_beat: String,
    #[serde(default)]
    pub new_state_after_chapter: String,
    #[serde(default)]
    pub relationship_delta: String,
    #[serde(default)]
    pub power_delta: String,
    #[serde(default)]
    pub resource_delta: String,
    #[serde(default)]
    pub hook_opened: Vec<String>,
    #[serde(default)]
    pub hook_paid_off: Vec<String>,
    #[serde(default)]
    pub character_change: String,
    #[serde(default)]
    pub world_change: String,
    #[serde(default)]
    pub payoff_target: String,
    #[serde(default)]
    pub new_character_requests: Vec<ChapterCharacterRequest>,
    #[serde(default)]
    pub character_registrations: Vec<ChapterCharacterRegistration>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChapterCharacterRequest {
    #[serde(default)]
    pub request_id: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub importance: String,
    #[serde(default)]
    pub narrative_purpose: String,
    #[serde(default)]
    pub planned_entry: String,
    #[serde(default)]
    pub planned_exit: String,
    #[serde(default)]
    pub relationship_to_existing: String,
    #[serde(default)]
    pub desire: String,
    #[serde(default)]
    pub fear: String,
    #[serde(default)]
    pub bottom_line: String,
    #[serde(default)]
    pub arc_start: String,
    #[serde(default)]
    pub arc_end: String,
    #[serde(default)]
    pub voice_style: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChapterCharacterRegistration {
    #[serde(default)]
    pub request_id: String,
    #[serde(default)]
    pub character_id: String,
    #[serde(default)]
    pub canonical_name: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub importance: String,
    #[serde(default)]
    pub narrative_purpose: String,
    #[serde(default)]
    pub planned_entry: String,
    #[serde(default)]
    pub planned_exit: String,
    #[serde(default)]
    pub relationship_to_existing: String,
    #[serde(default)]
    pub desire: String,
    #[serde(default)]
    pub fear: String,
    #[serde(default)]
    pub bottom_line: String,
    #[serde(default)]
    pub arc_start: String,
    #[serde(default)]
    pub arc_end: String,
    #[serde(default)]
    pub voice_style: String,
}

impl NovelContractV2 {
    pub(crate) fn normalize(&mut self) {
        super::normalization::normalize(self);
    }

    pub(crate) fn bump_revision(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }

    pub(crate) fn has_authored_content(&self) -> bool {
        !self.resource_economy.currency.trim().is_empty()
            || !self.resource_economy.value_scale.trim().is_empty()
            || !self.resource_economy.resource_types.is_empty()
            || !self.emotional_contract.primary_emotion.trim().is_empty()
            || !self.emotional_contract.emotional_promise.trim().is_empty()
            || !self.emotional_contract.emotional_beats.is_empty()
            || !self.emotional_state_ledger.is_empty()
            || !self.relationship_ledger.is_empty()
            || !self.power_progression.system_name.trim().is_empty()
            || !self.power_progression.levels.is_empty()
            || !self.social_order.institutions.is_empty()
            || !self.social_order.rank_system.trim().is_empty()
            || !self.geography_model.regions.is_empty()
            || !self.geography_model.important_locations.is_empty()
            || !self.time_model.calendar.trim().is_empty()
            || !self.time_model.story_start_time.trim().is_empty()
            || !self.artifact_ledger.is_empty()
            || !self.antagonist_pressure.primary_pressure.trim().is_empty()
            || !self.antagonist_pressure.antagonists.is_empty()
            || !self.payoff_matrix.is_empty()
            || !self.narration_contract.pov.trim().is_empty()
            || !self.scene_type_mix.balance_rule.trim().is_empty()
            || !self.character_voice_ledger.is_empty()
            || !self.reader_promise.core_hook.trim().is_empty()
            || !self.chapter_ending_rotation.planned_rotation.is_empty()
            || !self.conflict_pressure_curve.global_curve.is_empty()
            || !self.motif_ledger.is_empty()
            || !self.reveal_schedule.is_empty()
            || !self.relationship_interaction_quotas.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_preserves_authored_contract_content() {
        let mut contract = NovelContractV2 {
            social_order: SocialOrder {
                laws: vec!["城邦法律法规禁止私自改写居民记忆".to_string()],
                ..Default::default()
            },
            narration_contract: NarrationContract {
                dialogue_style: " 甲方与乙方是故事中的两个阵营称号 ".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };

        contract.normalize();

        assert_eq!(
            contract.social_order.laws,
            vec!["城邦法律法规禁止私自改写居民记忆"]
        );
        assert_eq!(
            contract.narration_contract.dialogue_style,
            "甲方与乙方是故事中的两个阵营称号"
        );
    }

    #[test]
    fn normalize_does_not_generate_story_policy_defaults() {
        let mut contract = NovelContractV2::default();

        contract.normalize();

        assert!(contract.field_requirements.is_empty());
        assert!(contract.emotional_contract.relief_beats.is_empty());
        assert!(contract.chapter_ending_rotation.planned_rotation.is_empty());
    }

    #[test]
    fn normalize_preserves_required_field_strength() {
        let mut contract = NovelContractV2 {
            field_requirements: BTreeMap::from([
                (" reader_promise ".to_string(), " required ".to_string()),
                ("motif_ledger".to_string(), "genre_required".to_string()),
            ]),
            ..Default::default()
        };

        contract.normalize();

        assert_eq!(
            contract.field_requirements.get("reader_promise"),
            Some(&"required".to_string())
        );
        assert_eq!(
            contract.field_requirements.get("motif_ledger"),
            Some(&"genre_required".to_string())
        );
    }

    #[test]
    fn normalize_does_not_rewrite_character_voice_references() {
        let mut contract = NovelContractV2 {
            character_voice_ledger: vec![CharacterVoiceProfile {
                character: "林默".to_string(),
                voice_style: "林默说话简短".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };

        contract.normalize();

        assert_eq!(contract.character_voice_ledger[0].character, "林默");
        assert_eq!(
            contract.character_voice_ledger[0].voice_style,
            "林默说话简短"
        );
    }

    #[test]
    fn legacy_json_receives_schema_metadata_without_losing_content() {
        let mut contract: NovelContractV2 = serde_json::from_value(serde_json::json!({
            "reader_promise": {"core_hook": "揭开城市停电的真实代价"},
            "revision": 7
        }))
        .expect("legacy contract");

        contract.normalize();

        assert_eq!(contract.schema_version, NOVEL_CONTRACT_V2_SCHEMA_VERSION);
        assert_eq!(contract.revision, 7);
        assert_eq!(contract.reader_promise.core_hook, "揭开城市停电的真实代价");
    }

    #[test]
    fn normalize_is_idempotent() {
        let mut contract = NovelContractV2 {
            resource_economy: ResourceEconomy {
                resource_types: vec!["  灵石  ".to_string(), "".to_string()],
                ..Default::default()
            },
            ..Default::default()
        };

        contract.normalize();
        let once = serde_json::to_value(&contract).expect("normalized contract");
        contract.normalize();
        let twice = serde_json::to_value(&contract).expect("normalized contract twice");

        assert_eq!(once, twice);
    }
}
