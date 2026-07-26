use serde::{Deserialize, Serialize};

use crate::tool::writing::creation_contract_model::NovelCreationContract;
use crate::tool::writing::longform_policy::GenreGovernanceProfile;
use crate::tool::writing::novel_contract_v2::{ChapterCharacterRegistration, NovelContractV2};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ChapterStateEventType {
    Character,
    Relationship,
    World,
    Power,
    Resource,
    #[serde(alias = "hook_open", alias = "hook_opened")]
    HookSeed,
    HookAdvance,
    #[serde(alias = "hook_payoff", alias = "hook_paid_off")]
    HookPayOff,
    HookDefer,
    Incidental,
}

impl Default for ChapterStateEventType {
    fn default() -> Self {
        Self::Incidental
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StateChangeAllowance {
    Contract,
    BoundedIncidental,
    Rejected,
    Unchecked,
}

impl Default for StateChangeAllowance {
    fn default() -> Self {
        Self::Unchecked
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct ChapterBodyEvidence {
    #[serde(default)]
    pub start_char: usize,
    #[serde(default)]
    pub end_char: usize,
    pub excerpt: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct ChapterStateChange {
    #[serde(default)]
    pub change_id: String,
    pub entity_id: String,
    pub event_type: ChapterStateEventType,
    pub value: String,
    pub evidence: ChapterBodyEvidence,
    #[serde(default)]
    pub authority_path: String,
    #[serde(default)]
    pub authority_excerpt: String,
    #[serde(default)]
    pub allowance: StateChangeAllowance,
    #[serde(default)]
    pub defer_until_chapter: Option<usize>,
    #[serde(default)]
    pub changes_identity: bool,
    #[serde(default)]
    pub changes_core_ability: bool,
    #[serde(default)]
    pub changes_bottom_line: bool,
    #[serde(default)]
    pub changes_world_hard_rule: bool,
    #[serde(default)]
    pub pays_future_hook_early: bool,
    #[serde(default)]
    pub opens_new_mainline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CompletionDebt {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct StoryBible {
    pub schema_version: String,
    pub title: String,
    pub language: String,
    pub genre: String,
    pub brief: String,
    pub ending_contract: EndingContract,
    pub narrative_graph: NarrativeGraph,
    pub world_database: WorldDatabase,
    pub character_ledger: Vec<CharacterAnchor>,
    pub hook_ledger: Vec<HookLedgerEntry>,
    pub genre_governance: GenreGovernanceProfile,
    pub theme_ledger: Vec<ThemeLedgerEntry>,
    pub timeline: Vec<TimelineEntry>,
    /// Runtime projection derived from the confirmed contract and approved chapters.
    /// It is not an independent contract authority.
    #[serde(default)]
    pub structured_contract_v2: NovelContractV2,
    pub chapter_summaries: Vec<ChapterContinuitySummary>,
    #[serde(default)]
    pub source_contract_revision: u64,
    #[serde(default)]
    pub last_rebuilt_chapter: Option<usize>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct StoryContract {
    pub premise: String,
    pub themes: Vec<String>,
    pub characters: Vec<String>,
    pub world_rules: Vec<String>,
    pub style_rules: Vec<String>,
    pub must_avoid: Vec<String>,
    pub outline: String,
    #[serde(default)]
    pub structured_contract_v2: NovelContractV2,
    #[serde(default)]
    pub authority_contract: Option<NovelCreationContract>,
    pub updated_at: String,
}

/// Storage-independent event accepted by the story-bible reducer.
#[derive(Debug, Clone, Default)]
pub(crate) struct ApprovedChapterDelta {
    pub number: usize,
    pub title: String,
    pub summary: String,
    pub unit_count: usize,
    pub key_facts: Vec<String>,
    pub continuity_updates: Vec<String>,
    pub character_registrations: Vec<ChapterCharacterRegistration>,
    pub state_changes: Vec<ChapterStateChange>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct EndingContract {
    pub desired_resolution: String,
    pub final_state: String,
    pub open_questions_allowed: Vec<String>,
    pub must_resolve: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct NarrativeGraph {
    pub global_spine: String,
    pub reverse_design_notes: Vec<String>,
    pub volume_arcs: Vec<NarrativeArc>,
    pub chapter_goals: Vec<ChapterGoal>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct NarrativeArc {
    pub id: String,
    pub title: String,
    pub goal: String,
    pub start_chapter: Option<usize>,
    pub end_chapter: Option<usize>,
    pub resolves_toward: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct ChapterGoal {
    pub chapter_number: usize,
    pub goal: String,
    pub depends_on: Vec<usize>,
    pub moves_toward_ending: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct WorldDatabase {
    pub rules: Vec<WorldRule>,
    pub locations: Vec<WorldEntity>,
    pub factions: Vec<WorldEntity>,
    pub resources: Vec<WorldEntity>,
    pub constraints: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct WorldRule {
    pub id: String,
    pub rule: String,
    pub cost_or_limit: String,
    pub narrative_effect: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct WorldEntity {
    pub id: String,
    pub name: String,
    pub role: String,
    pub known_facts: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct CharacterAnchor {
    pub id: String,
    pub name: String,
    pub role: String,
    pub desire: String,
    pub fear: String,
    pub bottom_line: String,
    pub wound_or_flaw: String,
    pub current_state: String,
    pub relationship_anchors: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct HookLedgerEntry {
    pub id: String,
    pub title: String,
    pub introduced_chapter: Option<usize>,
    pub introduced_when: String,
    pub knowers: Vec<String>,
    pub reader_knows: String,
    pub planned_payoff_window: String,
    #[serde(default)]
    pub planned_payoff_chapter: Option<usize>,
    pub payoff_chapter: Option<usize>,
    #[serde(default)]
    pub last_advanced_chapter: Option<usize>,
    #[serde(default)]
    pub deferred_until_chapter: Option<usize>,
    pub emotional_effect: String,
    pub status: HookStatus,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HookStatus {
    #[default]
    Open,
    Seeded,
    Advancing,
    Deferred,
    Overdue,
    PaidOff,
    Dropped,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct ThemeLedgerEntry {
    pub theme: String,
    pub function: String,
    pub recurrence_rule: String,
    pub last_touched_chapter: Option<usize>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct TimelineEntry {
    pub chapter_number: Option<usize>,
    pub label: String,
    pub event: String,
    pub causal_link: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct ChapterContinuitySummary {
    pub chapter_number: usize,
    pub title: String,
    pub summary: String,
    pub key_facts: Vec<String>,
    pub continuity_updates: Vec<String>,
    pub unit_count: usize,
}
