use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

use super::chapter_quality::ChapterFinding;
use super::novel_contract_v2::{ChapterCharacterRegistration, ChapterCharacterRequest};
use super::novel_runner::DraftOutput;
use super::novel_studio::{ChapterArchitectureRecord, ChapterContractRecord};

const GOVERNANCE_VERSION: &str = "benshu.novel_governance.v1";
const SEALED_AUTHORITY_VERSION: &str = "benshu.sealed_chapter_authority.v2";

pub(crate) type AuthorityFingerprint = String;

fn compact_event_probe_text(value: &str) -> String {
    value
        .to_ascii_lowercase()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || ('\u{4e00}'..='\u{9fff}').contains(ch))
        .collect()
}

fn compact_event_segments(value: &str, cjk: bool) -> Vec<String> {
    value
        .split(['；', ';', '。', '.', '！', '!', '？', '?', '，', ',', '\n'])
        .map(str::trim)
        .filter(|segment| {
            if cjk {
                segment
                    .chars()
                    .filter(|ch| {
                        ('\u{4e00}'..='\u{9fff}').contains(ch) || ch.is_ascii_alphanumeric()
                    })
                    .count()
                    >= 2
            } else {
                segment.split_whitespace().count() >= 2
            }
        })
        .map(|segment| {
            if cjk {
                compact_event_probe_text(segment)
            } else {
                segment.to_ascii_lowercase()
            }
        })
        .filter(|segment| !segment.is_empty())
        .collect()
}

fn contains_distinctive_cjk_span(haystack: &str, source: &str, minimum: usize) -> bool {
    let source = source.chars().collect::<Vec<_>>();
    if source.len() < minimum {
        return false;
    }
    (minimum..=source.len().min(12)).rev().any(|width| {
        source.windows(width).any(|window| {
            let candidate = window.iter().collect::<String>();
            haystack.contains(&candidate)
        })
    })
}

fn contains_distinctive_cjk_span_absent_from(
    haystack: &str,
    source: &str,
    excluded: &str,
    minimum: usize,
) -> bool {
    let source = source.chars().collect::<Vec<_>>();
    if source.len() < minimum {
        return false;
    }
    (minimum..=source.len().min(12)).rev().any(|width| {
        source.windows(width).any(|window| {
            let candidate = window.iter().collect::<String>();
            haystack.contains(&candidate) && !excluded.contains(&candidate)
        })
    })
}

fn contains_distinctive_english_span(haystack: &str, source: &str) -> bool {
    let haystack = haystack
        .split_whitespace()
        .map(|word| word.trim_matches(|ch: char| !ch.is_ascii_alphanumeric()))
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    let source = source
        .split_whitespace()
        .map(|word| word.trim_matches(|ch: char| !ch.is_ascii_alphanumeric()))
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    source.len() >= 3
        && source
            .windows(3)
            .any(|needle| haystack.windows(3).any(|window| window == needle))
}

fn without_shared_leading_cjk_subject(segment: &str, current_seed: &str) -> String {
    let shared = segment
        .chars()
        .zip(current_seed.chars())
        .take_while(|(left, right)| left == right)
        .count();
    if (2..=4).contains(&shared) && segment.chars().count() > shared {
        segment.chars().skip(shared).collect()
    } else {
        segment.to_string()
    }
}

pub(crate) fn event_text_is_grounded_in_current_chapter(
    field: &str,
    current_seed: &str,
    cjk: bool,
) -> bool {
    let event_text = compact_event_probe_text(field);
    let current_seed_compact = compact_event_probe_text(current_seed);
    if event_text.is_empty() || current_seed_compact.is_empty() {
        return false;
    }
    if cjk {
        contains_distinctive_cjk_span(&event_text, &current_seed_compact, 4)
            || contains_distinctive_cjk_span(&current_seed_compact, &event_text, 4)
    } else {
        contains_distinctive_english_span(field, current_seed)
            || contains_distinctive_english_span(current_seed, field)
    }
}

pub(crate) fn text_consumes_future_chapter(
    text: &str,
    current_seed: &str,
    next_seed: &str,
    cjk: bool,
) -> bool {
    let event_text = compact_event_probe_text(text);
    let current_seed_compact = compact_event_probe_text(current_seed);
    let current_seed_lower = current_seed.to_ascii_lowercase();
    if event_text.is_empty() {
        return false;
    }
    compact_event_segments(next_seed, cjk)
        .into_iter()
        .filter(|segment| {
            if cjk {
                !current_seed_compact.contains(segment.as_str())
            } else {
                !current_seed_lower.contains(segment.as_str())
            }
        })
        .any(|segment| {
            let segment = if cjk {
                without_shared_leading_cjk_subject(&segment, &current_seed_compact)
            } else {
                segment
            };
            if (cjk && event_text.contains(&segment))
                || (!cjk && text.to_ascii_lowercase().contains(&segment))
            {
                return true;
            }
            if cjk {
                let distinctive = contains_distinctive_cjk_span_absent_from(
                    &event_text,
                    &segment,
                    &current_seed_compact,
                    4,
                );
                let mixed_short_anchor = segment.chars().any(|ch| ch.is_ascii_alphanumeric())
                    && contains_distinctive_cjk_span_absent_from(
                        &event_text,
                        &segment,
                        &current_seed_compact,
                        2,
                    );
                distinctive || mixed_short_anchor
            } else {
                contains_distinctive_english_span(text, &segment)
                    && !contains_distinctive_english_span(current_seed, &segment)
            }
        })
}

fn sentence_reports_completed_outcome(sentence: &str, cjk: bool) -> bool {
    if cjk {
        if sentence.contains('已') {
            return true;
        }
        if [
            "确认", "发现", "查明", "证明", "证实", "显示", "表明", "直指", "揭示", "揭开", "透露",
            "获得", "得到", "拿到", "完成", "抵达", "进入", "击败", "解决", "达成", "交换", "识破",
            "暴露", "来自", "源自", "属于",
        ]
        .iter()
        .any(|marker| sentence.contains(marker))
        {
            return true;
        }
        let chars = sentence.chars().collect::<Vec<_>>();
        return chars.iter().enumerate().any(|(index, ch)| {
            if *ch != '了' {
                return false;
            }
            let previous = index.checked_sub(1).and_then(|offset| chars.get(offset));
            let next = chars.get(index + 1);
            !previous.is_some_and(|ch| matches!(ch, '为' | '除' | '得' | '不'))
                && !next.is_some_and(|ch| matches!(ch, '解' | '望' | '然' | '如'))
        });
    }
    let lowered = sentence.to_ascii_lowercase();
    [
        "confirmed",
        "discovered",
        "found",
        "proved",
        "revealed",
        "obtained",
        "received",
        "completed",
        "arrived",
        "entered",
        "defeated",
        "resolved",
        "exposed",
        "came from",
        "belonged to",
    ]
    .iter()
    .any(|marker| lowered.contains(marker))
}

pub(crate) fn final_body_future_consumption_evidence(
    body: &str,
    current_seed: &str,
    next_seed: &str,
    cjk: bool,
) -> Option<String> {
    body.split_inclusive(['。', '！', '？', '.', '!', '?', '\n'])
        .map(str::trim)
        .filter(|sentence| !sentence.is_empty())
        .find(|sentence| {
            sentence
                .split(['，', ',', '；', ';'])
                .map(str::trim)
                .filter(|clause| !clause.is_empty())
                .any(|clause| {
                    sentence_reports_completed_outcome(clause, cjk)
                        && text_consumes_future_chapter(clause, current_seed, next_seed, cjk)
                })
        })
        .map(ToString::to_string)
}

pub(crate) fn unresolved_character_request_ids(
    requests: &[ChapterCharacterRequest],
    registrations: &[ChapterCharacterRegistration],
) -> Vec<String> {
    let registered = registrations
        .iter()
        .map(|registration| registration.request_id.trim())
        .filter(|request_id| !request_id.is_empty())
        .collect::<BTreeSet<_>>();
    requests
        .iter()
        .map(|request| request.request_id.trim())
        .filter(|request_id| request_id.is_empty() || !registered.contains(request_id))
        .map(|request_id| {
            if request_id.is_empty() {
                "<missing-request-id>".to_string()
            } else {
                request_id.to_string()
            }
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CandidateProvenance {
    InitialDraft,
    RecoveredBest,
    LegacyCandidate,
    LocalCleanup,
    LengthTopup,
    TailCompletion,
    MetadataRepair,
    SemanticRevision,
    Regenerated,
    TruncatedRecovery,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RevisionQualityVector {
    pub hard_blockers: usize,
    pub authority_conflicts: usize,
    pub state_conflicts: usize,
    pub required_outcomes_missing: usize,
    pub protected_facts_lost: usize,
    pub new_high_priority_blockers: usize,
    pub material_deletion_ratio: u16,
    pub incomplete_body: bool,
    pub contaminated_body: bool,
    pub degenerate_repetition: bool,
    pub length_violation: usize,
    #[serde(default)]
    pub length_shortfall: usize,
    #[serde(default)]
    pub length_blockers: usize,
    pub deterministic_repairs: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DraftCandidateRecord {
    pub candidate_id: String,
    pub parent_candidate_id: Option<String>,
    pub authority_fingerprint: String,
    pub body_fingerprint: String,
    pub metadata_fingerprint: String,
    pub draft: DraftOutput,
    pub findings: Vec<ChapterFinding>,
    pub quality_vector: RevisionQualityVector,
    pub provenance: CandidateProvenance,
    pub accepted_as_best: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AuthorityRole {
    Writer,
    Auditor,
    Reviser,
    Observer,
}

impl AuthorityRole {
    pub(crate) const ALL: [Self; 4] = [Self::Writer, Self::Auditor, Self::Reviser, Self::Observer];

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Writer => "writer",
            Self::Auditor => "auditor",
            Self::Reviser => "reviser",
            Self::Observer => "observer",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AuthorityProjectionRecord {
    pub role: AuthorityRole,
    pub payload: Value,
    pub fingerprint: AuthorityFingerprint,
    #[serde(default)]
    pub protected_core_fingerprint: AuthorityFingerprint,
    #[serde(default)]
    pub included_paths: Vec<String>,
    #[serde(default)]
    pub excluded_paths: Vec<String>,
    #[serde(default)]
    pub truncated_paths: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct AuthorityCoverage {
    pub required_paths: Vec<String>,
    pub present_paths: Vec<String>,
    pub missing_paths: Vec<String>,
    pub complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SealedChapterAuthority {
    pub schema_version: String,
    pub chapter_number: usize,
    pub canonical_contract: Value,
    pub truth_as_of_chapter: Value,
    pub truth_cutoff_chapter: usize,
    pub context_package: ContextPackage,
    pub rule_stack: RuleStack,
    pub trace: ChapterTrace,
    pub chapter_contract: ChapterContractRecord,
    pub chapter_architecture: ChapterArchitectureRecord,
    pub character_registrations: Vec<ChapterCharacterRegistration>,
    pub role_projections: BTreeMap<AuthorityRole, AuthorityProjectionRecord>,
    pub authority_root_fingerprint: AuthorityFingerprint,
    pub protected_coverage: AuthorityCoverage,
    pub sealed_at: String,
}

impl SealedChapterAuthority {
    pub(crate) fn projection(&self, role: AuthorityRole) -> Option<&AuthorityProjectionRecord> {
        self.role_projections.get(&role)
    }
}

pub(crate) fn authority_fingerprint<T: Serialize>(value: &T) -> AuthorityFingerprint {
    let encoded = serde_json::to_vec(value).unwrap_or_default();
    hex::encode(Sha256::digest(encoded))
}

pub(crate) fn build_authority_coverage(
    chapter_number: usize,
    canonical_contract: &Value,
    truth_as_of_chapter: &Value,
    context_package: &ContextPackage,
    rule_stack: &RuleStack,
    trace: &ChapterTrace,
    chapter_contract: &ChapterContractRecord,
    chapter_architecture: &ChapterArchitectureRecord,
) -> AuthorityCoverage {
    let canonical_contract_complete = canonical_contract
        .get("premise")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
        && canonical_contract
            .get("characters")
            .and_then(Value::as_array)
            .is_some_and(|items| !items.is_empty())
        && canonical_contract
            .get("world_rules")
            .and_then(Value::as_array)
            .is_some_and(|items| !items.is_empty())
        && canonical_contract
            .get("target_units")
            .and_then(Value::as_u64)
            .is_some_and(|value| value > 0)
        && canonical_contract
            .get("chapter_unit_target")
            .and_then(Value::as_u64)
            .is_some_and(|value| matches!(value, 2500 | 5000));
    let required = [
        ("canonical_contract", !canonical_contract.is_null()),
        (
            "canonical_contract.required_authority_fields",
            canonical_contract_complete,
        ),
        ("truth_as_of_chapter", !truth_as_of_chapter.is_null()),
        (
            "context_package",
            context_package.chapter_number == chapter_number,
        ),
        ("rule_stack", rule_stack.chapter_number == chapter_number),
        ("trace", trace.chapter_number == chapter_number),
        (
            "chapter_contract",
            chapter_contract.number == chapter_number,
        ),
        (
            "chapter_architecture",
            chapter_architecture.number == chapter_number,
        ),
    ];
    let required_paths = required
        .iter()
        .map(|(path, _)| (*path).to_string())
        .collect::<Vec<_>>();
    let present_paths = required
        .iter()
        .filter(|(_, present)| *present)
        .map(|(path, _)| (*path).to_string())
        .collect::<Vec<_>>();
    let missing_paths = required
        .iter()
        .filter(|(_, present)| !*present)
        .map(|(path, _)| (*path).to_string())
        .collect::<Vec<_>>();
    AuthorityCoverage {
        required_paths,
        present_paths,
        complete: missing_paths.is_empty(),
        missing_paths,
    }
}

pub(crate) fn build_authority_projection(
    role: AuthorityRole,
    protected_payload: &Value,
    excluded_paths: &[String],
) -> AuthorityProjectionRecord {
    let payload = json!({
        "schema_version": SEALED_AUTHORITY_VERSION,
        "role": role,
        "authority": protected_payload
    });
    AuthorityProjectionRecord {
        role,
        fingerprint: authority_fingerprint(&payload),
        protected_core_fingerprint: authority_fingerprint(protected_payload),
        included_paths: protected_payload
            .as_object()
            .map(|object| object.keys().map(|key| format!("/{key}")).collect())
            .unwrap_or_default(),
        excluded_paths: excluded_paths.to_vec(),
        truncated_paths: Vec::new(),
        payload,
    }
}

pub(crate) fn model_authority_projection_payload(
    role: AuthorityRole,
    protected_payload: &Value,
    authority_root_fingerprint: &str,
) -> Value {
    let chapter_number = protected_payload
        .get("chapter_number")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or_default();
    let mut canonical_contract = protected_payload
        .get("canonical_contract")
        .cloned()
        .unwrap_or(Value::Null);
    if let Some(outline) = canonical_contract
        .get_mut("outline")
        .and_then(Value::as_object_mut)
    {
        outline.remove("raw_outline");
        if let Some(chapters) = outline
            .get_mut("near_chapters")
            .and_then(Value::as_array_mut)
        {
            chapters.retain(|chapter| {
                chapter
                    .get("number")
                    .or_else(|| chapter.get("chapter_number"))
                    .and_then(Value::as_u64)
                    .is_none_or(|number| number <= chapter_number as u64)
            });
        }
    }
    let mut truth_as_of_chapter = protected_payload
        .get("truth_as_of_chapter")
        .cloned()
        .unwrap_or(Value::Null);
    for pointer in [
        "/story_state/narrative_graph/chapter_goals",
        "/narrative_graph/chapter_goals",
    ] {
        if let Some(chapters) = truth_as_of_chapter
            .pointer_mut(pointer)
            .and_then(Value::as_array_mut)
        {
            chapters.retain(|chapter| {
                chapter
                    .get("number")
                    .or_else(|| chapter.get("chapter_number"))
                    .and_then(Value::as_u64)
                    .is_none_or(|number| number <= chapter_number as u64)
            });
        }
    }
    let context_selection_trace = protected_payload
        .pointer("/context_package/selected_context")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    json!({
                        "source": item.get("source").cloned().unwrap_or(Value::Null),
                        "reason": item.get("reason").cloned().unwrap_or(Value::Null),
                        "layer": item.get("layer").cloned().unwrap_or(Value::Null),
                        "original_chars": item
                            .get("original_chars")
                            .cloned()
                            .unwrap_or(Value::Null),
                        "selected_chars": item
                            .get("selected_chars")
                            .cloned()
                            .unwrap_or(Value::Null),
                        "truncated": item.get("truncated").cloned().unwrap_or(Value::Null)
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let authority = json!({
        "chapter_number": protected_payload
            .get("chapter_number")
            .cloned()
            .unwrap_or(Value::Null),
        "canonical_contract": canonical_contract,
        "truth_as_of_chapter": truth_as_of_chapter,
        "truth_cutoff_chapter": protected_payload
            .get("truth_cutoff_chapter")
            .cloned()
            .unwrap_or(Value::Null),
        "working_context": protected_payload
            .get("working_context")
            .cloned()
            .unwrap_or(Value::Null),
        "context_selection_trace": context_selection_trace,
        "rule_stack": protected_payload
            .get("rule_stack")
            .cloned()
            .unwrap_or(Value::Null),
        "chapter_plan": protected_payload
            .get("chapter_plan")
            .cloned()
            .unwrap_or(Value::Null),
        "chapter_contract": protected_payload
            .get("chapter_contract")
            .cloned()
            .unwrap_or(Value::Null),
        "chapter_architecture": protected_payload
            .get("chapter_architecture")
            .cloned()
            .unwrap_or(Value::Null),
        "character_registrations": protected_payload
            .get("character_registrations")
            .cloned()
            .unwrap_or_else(|| json!([]))
    });
    json!({
        "schema_version": "benshu.model_authority_projection.v1",
        "role": role,
        "authority_root_fingerprint": authority_root_fingerprint,
        "authority": authority
    })
}

pub(crate) fn sealed_authority_version() -> &'static str {
    SEALED_AUTHORITY_VERSION
}

pub(crate) fn replace_character_request_ids_in_value(
    value: &mut Value,
    registrations: &[ChapterCharacterRegistration],
) {
    match value {
        Value::String(text) => {
            for registration in registrations {
                let request_id = registration.request_id.trim();
                let canonical_name = registration.canonical_name.trim();
                if !request_id.is_empty() && !canonical_name.is_empty() {
                    *text = text.replace(request_id, canonical_name);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                replace_character_request_ids_in_value(item, registrations);
            }
        }
        Value::Object(map) => {
            for item in map.values_mut() {
                replace_character_request_ids_in_value(item, registrations);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ChapterControlContract {
    pub schema_version: String,
    pub chapter_number: usize,
    pub title: String,
    pub goal: String,
    pub raw_directive: String,
    pub source_refs: Vec<String>,
    pub must_keep: Vec<String>,
    pub must_avoid: Vec<String>,
    pub acceptance_checks: Vec<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ContextSource {
    pub source: String,
    pub reason: String,
    pub excerpt: Option<String>,
    #[serde(default)]
    pub layer: String,
    #[serde(default)]
    pub original_chars: usize,
    #[serde(default)]
    pub selected_chars: usize,
    #[serde(default)]
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ContextPackage {
    pub schema_version: String,
    pub chapter_number: usize,
    pub selected_context: Vec<ContextSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RuleStack {
    pub schema_version: String,
    pub chapter_number: usize,
    pub hard: Vec<String>,
    pub soft: Vec<String>,
    pub diagnostic: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ChapterTrace {
    pub schema_version: String,
    pub chapter_number: usize,
    pub planner_inputs: Vec<String>,
    pub composer_inputs: Vec<String>,
    pub selected_sources: Vec<String>,
    pub notes: Vec<String>,
    #[serde(default)]
    pub selection_decisions: Vec<ContextSelectionDecision>,
    #[serde(default)]
    pub prompt_context_fingerprint: String,
    #[serde(default)]
    pub context_budget: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ContextSelectionDecision {
    pub source: String,
    pub layer: String,
    pub reason: String,
    pub original_chars: usize,
    pub selected_chars: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TruthValidation {
    pub schema_version: String,
    pub chapter_number: usize,
    pub verdict: String,
    pub issues: Vec<String>,
    pub checked_items: Vec<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ReviewCycle {
    pub schema_version: String,
    pub chapter_number: usize,
    pub iteration: usize,
    pub verdict: String,
    pub issues: Vec<String>,
    pub next_action: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct HookDebtReport {
    pub schema_version: String,
    pub chapter_number: usize,
    pub debts: Vec<String>,
    pub created_at: String,
}

pub(crate) fn build_chapter_control_contract(
    chapter_number: usize,
    title: &str,
    raw_directive: &str,
    source_refs: Vec<String>,
    must_keep: Vec<String>,
    must_avoid: Vec<String>,
    now: String,
) -> ChapterControlContract {
    let goal = first_meaningful_line(raw_directive)
        .or_else(|| non_empty(title))
        .unwrap_or_else(|| format!("Chapter {chapter_number}"));
    let mut acceptance_checks = vec![
        "draft_has_title".to_string(),
        "draft_has_summary".to_string(),
        "draft_records_key_facts".to_string(),
        "draft_records_continuity_updates".to_string(),
        "truth_validation_passes_or_is_explicitly_degraded".to_string(),
    ];
    if !source_refs.is_empty() {
        acceptance_checks.push("context_package_carries_source_references".to_string());
    }
    if !must_avoid.is_empty() {
        acceptance_checks.push("draft_avoids_contract_forbidden_items".to_string());
    }

    ChapterControlContract {
        schema_version: GOVERNANCE_VERSION.to_string(),
        chapter_number,
        title: non_empty(title).unwrap_or_else(|| format!("Chapter {chapter_number}")),
        goal,
        raw_directive: raw_directive.trim().to_string(),
        source_refs: clean_strings(source_refs),
        must_keep: clean_strings(must_keep),
        must_avoid: clean_strings(must_avoid),
        acceptance_checks,
        created_at: now,
    }
}

pub(crate) fn build_context_package(
    chapter_number: usize,
    selected_context: Vec<ContextSource>,
) -> ContextPackage {
    ContextPackage {
        schema_version: GOVERNANCE_VERSION.to_string(),
        chapter_number,
        selected_context,
    }
}

pub(crate) fn build_rule_stack(
    chapter_number: usize,
    has_contract: bool,
    has_chapter_contract: bool,
    truth_count: usize,
    source_count: usize,
    has_architecture: bool,
) -> RuleStack {
    let mut hard = Vec::new();
    if has_contract {
        hard.push("story_contract".to_string());
    }
    if has_chapter_contract {
        hard.push("chapter_control_contract".to_string());
    }
    if truth_count > 0 {
        hard.push("truth_files".to_string());
    }

    let mut soft = Vec::new();
    if source_count > 0 {
        soft.push("source_material".to_string());
    }
    if has_architecture {
        soft.push("chapter_architecture".to_string());
    }
    soft.push("recent_continuity".to_string());

    RuleStack {
        schema_version: GOVERNANCE_VERSION.to_string(),
        chapter_number,
        hard,
        soft,
        diagnostic: vec![
            "mechanical_chapter_audit".to_string(),
            "truth_validation".to_string(),
            "hook_debt_report".to_string(),
            "review_cycle".to_string(),
        ],
    }
}

pub(crate) fn build_trace(
    chapter_number: usize,
    planner_inputs: Vec<String>,
    composer_inputs: Vec<String>,
    selected_sources: Vec<String>,
) -> ChapterTrace {
    ChapterTrace {
        schema_version: GOVERNANCE_VERSION.to_string(),
        chapter_number,
        planner_inputs,
        composer_inputs,
        selected_sources,
        notes: Vec::new(),
        selection_decisions: Vec::new(),
        prompt_context_fingerprint: String::new(),
        context_budget: json!({}),
    }
}

pub(crate) fn validate_truth_against_chapter(
    chapter_number: usize,
    content: &str,
    key_facts: &[String],
    continuity_updates: &[String],
    now: String,
) -> TruthValidation {
    let mut issues = Vec::new();
    let mut checked_items = Vec::new();
    for item in key_facts.iter().chain(continuity_updates.iter()) {
        let trimmed = item.trim();
        if trimmed.is_empty() {
            continue;
        }
        checked_items.push(trimmed.to_string());
        if !truth_item_supported_by_chapter(trimmed, content) {
            issues.push(format!(
                "truth item lacks visible support in chapter body: {trimmed}"
            ));
        }
    }
    TruthValidation {
        schema_version: GOVERNANCE_VERSION.to_string(),
        chapter_number,
        verdict: if issues.is_empty() {
            "passed".to_string()
        } else {
            "needs_attention".to_string()
        },
        issues,
        checked_items,
        created_at: now,
    }
}

pub(crate) fn retain_truth_items_supported_by_chapter(
    items: &mut Vec<String>,
    content: &str,
) -> Vec<String> {
    let mut removed = Vec::new();
    items.retain(|item| {
        let trimmed = item.trim();
        if trimmed.is_empty() {
            return false;
        }
        let supported = truth_item_supported_by_chapter(trimmed, content);
        if !supported {
            removed.push(trimmed.to_string());
        }
        supported
    });
    removed
}

pub(crate) fn build_review_cycle(
    chapter_number: usize,
    previous_iterations: usize,
    verdict: &str,
    issues: Vec<String>,
    now: String,
) -> ReviewCycle {
    let iteration = previous_iterations + 1;
    let next_action = if verdict == "passed" {
        "approve_chapter".to_string()
    } else {
        // Review history is telemetry, not a hidden revision budget. The unified
        // chapter controller owns typed budgets and records an explicit blocker.
        "revise_draft".to_string()
    };
    ReviewCycle {
        schema_version: GOVERNANCE_VERSION.to_string(),
        chapter_number,
        iteration,
        verdict: verdict.to_string(),
        issues,
        next_action,
        created_at: now,
    }
}

pub(crate) fn build_hook_debt_report(
    chapter_number: usize,
    planned_without_draft: Vec<usize>,
    architecture_without_draft: Vec<usize>,
    chapters_missing_continuity: Vec<usize>,
    truth_issues: &[String],
    now: String,
) -> HookDebtReport {
    let mut debts = Vec::new();
    for number in planned_without_draft {
        debts.push(format!("planned chapter {number} has not been drafted"));
    }
    for number in architecture_without_draft {
        debts.push(format!("architected chapter {number} has not been drafted"));
    }
    for number in chapters_missing_continuity {
        debts.push(format!("chapter {number} has no continuity update ledger"));
    }
    for issue in truth_issues {
        debts.push(format!("truth validation debt: {issue}"));
    }
    HookDebtReport {
        schema_version: GOVERNANCE_VERSION.to_string(),
        chapter_number,
        debts,
        created_at: now,
    }
}

pub(crate) fn context_source(source: &str, reason: &str, excerpt: Option<String>) -> ContextSource {
    let original_chars = excerpt
        .as_deref()
        .map(|value| value.chars().count())
        .unwrap_or_default();
    // Selection callers already own the source-specific compression budget.
    // Applying a second generic 1,200-character cut here silently truncated
    // protected contract, StoryBible, truth, plan, and architecture sources
    // before authority sealing, so a role could never consume the canonical
    // package it claimed to validate. Preserve the selected payload exactly;
    // compressible sources remain bounded at their selection sites.
    let excerpt = excerpt.filter(|value| !value.trim().is_empty());
    let selected_chars = excerpt
        .as_deref()
        .map(|value| value.chars().count())
        .unwrap_or_default();
    ContextSource {
        source: source.to_string(),
        reason: reason.to_string(),
        excerpt,
        layer: String::new(),
        original_chars,
        selected_chars,
        truncated: selected_chars < original_chars,
    }
}

pub(crate) fn render_contract_markdown(contract: &ChapterControlContract) -> String {
    [
        format!("# Chapter {} Control Contract", contract.chapter_number),
        format!("- Title: {}", contract.title),
        format!("- Goal: {}", contract.goal),
        String::new(),
        "## Raw Directive".to_string(),
        non_empty(&contract.raw_directive).unwrap_or_else(|| "(none)".to_string()),
        String::new(),
        "## Source Refs".to_string(),
        render_list(&contract.source_refs),
        String::new(),
        "## Authority Constraints (apply when relevant; never quote or force into this chapter)"
            .to_string(),
        render_list(&contract.must_keep),
        String::new(),
        "## Must Avoid".to_string(),
        render_list(&contract.must_avoid),
        String::new(),
        "## Acceptance Checks".to_string(),
        render_list(&contract.acceptance_checks),
    ]
    .join("\n")
}

pub(crate) fn render_rule_stack_yaml(stack: &RuleStack) -> String {
    format!(
        "schema_version: {}\nchapter_number: {}\nhard:\n{}soft:\n{}diagnostic:\n{}",
        stack.schema_version,
        stack.chapter_number,
        render_yaml_list(&stack.hard),
        render_yaml_list(&stack.soft),
        render_yaml_list(&stack.diagnostic),
    )
}

pub(crate) fn review_cycle_json(cycle: &ReviewCycle) -> Value {
    json!(cycle)
}

pub(crate) fn truth_item_supported_by_chapter(item: &str, content: &str) -> bool {
    if contains_unexpected_script_residue(item, content) {
        return false;
    }
    let required_anchors = required_entity_anchors(item);
    if required_anchors
        .iter()
        .any(|anchor| !content.contains(anchor))
    {
        return false;
    }
    let item_terms = stable_terms(item);
    if item_terms.is_empty() {
        return true;
    }
    let content_lower = content.to_ascii_lowercase();
    let supported = item_terms
        .iter()
        .filter(|term| {
            if term.is_ascii() {
                content_lower.contains(&term.to_ascii_lowercase())
            } else {
                content.contains(term.as_str())
            }
        })
        .count();
    supported >= item_terms.len().min(2)
        || (required_anchors.is_empty() && cjk_bigram_supports(item, content))
}

fn contains_unexpected_script_residue(value: &str, content: &str) -> bool {
    value.chars().any(|ch| {
        matches!(
            ch as u32,
            0x0370..=0x03FF
                | 0x0400..=0x052F
                | 0x0590..=0x05FF
                | 0x0600..=0x06FF
                | 0x0900..=0x097F
                | 0x0E00..=0x0E7F
                | 0x3040..=0x30FF
                | 0xAC00..=0xD7AF
        ) && !content.contains(ch)
    })
}

fn required_entity_anchors(value: &str) -> Vec<String> {
    let compact = cjk_compact(strip_truth_label(value));
    if compact.chars().count() < 4 {
        return Vec::new();
    }
    let Some(index) = [
        "通过", "进入", "发现", "决定", "开始", "选择", "成功", "失败", "意识", "收到", "听见",
        "看见", "锁定", "保护", "离开", "回到", "面对", "承认", "拒绝",
    ]
    .iter()
    .filter_map(|marker| compact.find(marker))
    .min() else {
        return Vec::new();
    };
    let prefix = compact[..index].to_string();
    let count = prefix.chars().count();
    if (2..=4).contains(&count) && !generic_cjk_anchor(&prefix) {
        vec![prefix]
    } else {
        Vec::new()
    }
}

fn generic_cjk_anchor(value: &str) -> bool {
    matches!(
        value,
        "主角"
            | "少年"
            | "少女"
            | "众人"
            | "敌人"
            | "老师"
            | "学生"
            | "队伍"
            | "核心"
            | "系统"
            | "城市"
            | "学院"
            | "飞船"
            | "基地"
            | "世界"
            | "真相"
            | "危机"
            | "计划"
    )
}

fn stable_terms(value: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let value = strip_truth_label(value);
    for token in value.split(|ch: char| {
        ch.is_whitespace()
            || matches!(
                ch,
                ',' | '，'
                    | '.'
                    | '。'
                    | ';'
                    | '；'
                    | ':'
                    | '：'
                    | '!'
                    | '！'
                    | '?'
                    | '？'
                    | '('
                    | ')'
                    | '（'
                    | '）'
                    | '['
                    | ']'
                    | '【'
                    | '】'
            )
    }) {
        let trimmed = token.trim();
        let char_count = trimmed.chars().count();
        if char_count >= 2
            && trimmed
                .chars()
                .all(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch))
        {
            if char_count > 4 {
                let chars = trimmed.chars().collect::<Vec<_>>();
                for pair in chars.windows(2).take(24) {
                    terms.push(pair.iter().collect());
                }
            } else {
                terms.push(trimmed.to_string());
            }
        } else if char_count >= 2 {
            terms.push(trimmed.to_string());
        }
    }
    if terms.is_empty() && value.chars().count() >= 2 {
        terms.push(value.chars().take(8).collect());
    }
    terms.into_iter().take(24).collect()
}

fn cjk_bigram_supports(item: &str, content: &str) -> bool {
    let item_bigrams = cjk_bigrams(item);
    if item_bigrams.is_empty() {
        return false;
    }
    let content_cjk = cjk_compact(content);
    if content_cjk.is_empty() {
        return false;
    }
    let matched = item_bigrams
        .iter()
        .filter(|bigram| content_cjk.contains(bigram.as_str()))
        .count();
    let needed = item_bigrams.len().min(4).max(2);
    matched >= needed
}

fn cjk_bigrams(value: &str) -> Vec<String> {
    let chars: Vec<char> = cjk_compact(value).chars().collect();
    let mut out = Vec::new();
    for pair in chars.windows(2) {
        let value: String = pair.iter().collect();
        if !out.contains(&value) {
            out.push(value);
        }
    }
    out
}

fn cjk_compact(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ('\u{4e00}'..='\u{9fff}').contains(ch))
        .collect()
}

fn strip_truth_label(value: &str) -> &str {
    let trimmed = value.trim();
    let Some(separator_index) = trimmed.find([':', '：']) else {
        return trimmed;
    };
    let (label, rest_with_separator) = trimmed.split_at(separator_index);
    let rest = rest_with_separator
        .chars()
        .next()
        .map(|separator| &rest_with_separator[separator.len_utf8()..])
        .unwrap_or("");
    let label_chars = label.chars().count();
    if label_chars <= 8 && rest.chars().count() >= 2 {
        rest.trim()
    } else {
        trimmed
    }
}

fn clean_strings(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .filter_map(|value| non_empty(&value))
        .collect()
}

fn first_meaningful_line(value: &str) -> Option<String> {
    value
        .lines()
        .map(|line| line.trim().trim_start_matches(['-', '*', '#', ' ']))
        .find(|line| line.chars().count() >= 2)
        .map(|line| line.chars().take(80).collect())
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn render_list(items: &[String]) -> String {
    if items.is_empty() {
        return "- none".to_string();
    }
    items
        .iter()
        .map(|item| format!("- {item}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_yaml_list(items: &[String]) -> String {
    if items.is_empty() {
        return "  - none\n".to_string();
    }
    items
        .iter()
        .map(|item| format!("  - {}\n", serde_json::to_string(item).unwrap_or_default()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truth_support_allows_paraphrased_chinese_frequency_fact() {
        let content = "林墨在灵核失控边缘捕捉到一丝异样。那不是常规灵气的流动，而是一种微弱、极其稳定的频率。";
        let validation = validate_truth_against_chapter(
            1,
            content,
            &["林墨在失控中感知到了一种微弱、稳定且不同于常规灵气的流动频率。".to_string()],
            &[],
            "now".to_string(),
        );
        assert_eq!(validation.verdict, "passed");
    }

    #[test]
    fn truth_support_rejects_wrong_named_actor_despite_shared_action_words() {
        let content = "黎启洄在脉冲核心过载时保持清醒，靠手动校准把频率重新压回安全区。";
        let validation = validate_truth_against_chapter(
            1,
            content,
            &["林墨通过手动过载成功锁定了频率。".to_string()],
            &[],
            "now".to_string(),
        );
        assert_eq!(validation.verdict, "needs_attention");
        assert!(validation.issues.iter().any(|issue| issue.contains("林墨")));
    }

    #[test]
    fn truth_support_rejects_unexpected_script_residue() {
        let content = "黎启洄把核心频率稳定在安全阈值附近，所有人都听见了低沉的回响。";
        let validation = validate_truth_against_chapter(
            1,
            content,
            &["核心频率稳定在44.나122 kHz。".to_string()],
            &[],
            "now".to_string(),
        );
        assert_eq!(validation.verdict, "needs_attention");
    }

    #[test]
    fn review_history_does_not_own_semantic_revision_budget() {
        let cycle = build_review_cycle(
            1,
            999,
            "needs_revision",
            vec!["typed blocker remains".to_string()],
            "now".to_string(),
        );
        assert_eq!(cycle.next_action, "revise_draft");
    }

    #[test]
    fn truth_support_accepts_script_used_by_the_chapter() {
        let content = "ユナは扉を開いた。続いて仲間へ合図した。";
        let validation = validate_truth_against_chapter(
            1,
            content,
            &["ユナは扉を開いた。".to_string()],
            &[],
            "now".to_string(),
        );
        assert_eq!(validation.verdict, "passed");
    }

    #[test]
    fn role_projections_share_one_protected_root_without_sharing_fingerprints() {
        let protected = json!({
            "chapter_number": 7,
            "canonical_contract": {"premise": "同一合同"},
            "truth_as_of_chapter": {"cutoff_chapter": 6}
        });
        let root = authority_fingerprint(&protected);
        let projections = AuthorityRole::ALL
            .into_iter()
            .map(|role| {
                build_authority_projection(role, &protected, &["chapters.number>=7".to_string()])
            })
            .collect::<Vec<_>>();

        assert!(projections.iter().all(|projection| {
            projection.payload.get("authority") == Some(&protected)
                && authority_fingerprint(
                    projection
                        .payload
                        .get("authority")
                        .expect("protected payload"),
                ) == root
                && authority_fingerprint(&projection.payload) == projection.fingerprint
                && projection.protected_core_fingerprint == root
                && projection.truncated_paths.is_empty()
        }));
        let fingerprints = projections
            .iter()
            .map(|projection| projection.fingerprint.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(fingerprints.len(), AuthorityRole::ALL.len());
    }

    #[test]
    fn model_projection_hides_future_chapters_but_keeps_sealed_root_identity() {
        let protected = json!({
            "chapter_number": 2,
            "canonical_contract": {
                "outline": {
                    "raw_outline": "第一章；第二章；第三章未来揭示",
                    "near_chapters": [
                        {"number": 2, "goal": "当前章行动"},
                        {"number": 3, "goal": "未来章揭示"}
                    ]
                }
            },
            "truth_as_of_chapter": {
                "story_state": {
                    "narrative_graph": {
                        "chapter_goals": [
                            {"chapter_number": 2, "goal": "当前章行动"},
                            {"chapter_number": 3, "goal": "未来章揭示"}
                        ]
                    }
                }
            },
            "working_context": {
                "next_chapter_boundary": [
                    {"number": 3, "goal": "未来章揭示"}
                ]
            },
            "chapter_architecture": {
                "architecture": "下一章边界：未来章揭示"
            }
        });
        let root = authority_fingerprint(&protected);
        let projection =
            model_authority_projection_payload(AuthorityRole::Writer, &protected, &root);

        assert_eq!(
            projection
                .get("authority_root_fingerprint")
                .and_then(Value::as_str),
            Some(root.as_str())
        );
        assert!(projection
            .pointer("/authority/canonical_contract/outline/raw_outline")
            .is_none());
        assert_eq!(
            projection
                .pointer("/authority/canonical_contract/outline/near_chapters")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(1)
        );
        assert_eq!(
            projection
                .pointer("/authority/truth_as_of_chapter/story_state/narrative_graph/chapter_goals")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(1)
        );
        assert_eq!(
            projection
                .pointer("/authority/working_context/next_chapter_boundary/0/number")
                .and_then(Value::as_u64),
            Some(3)
        );
    }

    #[test]
    fn final_body_future_boundary_detection_ignores_intent_but_catches_completed_reveal() {
        let current = "闻望宁发现被覆盖的原始共同记忆并私自保留胶囊";
        let next = "闻望宁前往黑市鉴定记忆胶囊来源；确认其源自上层区主脑核心的原始备份；黑市商人透露原主人身份";
        let body = "闻望宁收好胶囊，决定明天前往黑市。";
        assert!(
            final_body_future_consumption_evidence(body, current, next, true).is_none(),
            "preparing the next action must leave that boundary open"
        );

        let consumed = "但这段被三次覆盖的原始记忆却显示，收割的源头直指上层区的主脑核心。";
        assert_eq!(
            final_body_future_consumption_evidence(consumed, current, next, true).as_deref(),
            Some(consumed)
        );
    }

    #[test]
    fn final_body_future_boundary_detection_handles_generic_completed_aspect() {
        let current = "岑晏白在外门药园劳作并忍受管事刁难";
        let next = "岑晏白首次用真视之眼辨识变异灵草；救下一株濒死仙草；引起管事注意";
        let consumed = "他没想到，自己随手的一举，竟然真的救活了一株濒死仙草。";
        assert_eq!(
            final_body_future_consumption_evidence(consumed, current, next, true).as_deref(),
            Some(consumed)
        );

        let intent = "为了救活那株濒死仙草，他决定明天再去药园查找药材。";
        assert!(
            final_body_future_consumption_evidence(intent, current, next, true).is_none(),
            "purpose and future-intent clauses must not be treated as completed outcomes"
        );
    }

    #[test]
    fn final_body_future_boundary_detection_ignores_shared_subject_prefixes() {
        let current = "阮听舟在测脉时看到异常数据，发现矿脉寿命流失过快；测出数据与宗门记录不符，阮听舟被监工责罚";
        let next = "阮听舟深夜潜入废弃矿坑，遇到微服私访的钟景原；钟景原认出阮听舟手中的罗盘，两人达成初步合作";

        assert!(
            final_body_future_consumption_evidence(
                "阮听舟深吸了一口气，强行压下心底的那丝疲惫。",
                current,
                next,
                true,
            )
            .is_none(),
            "a character name plus one shared action character is not future-event evidence"
        );
        let consumed = "入夜后，阮听舟已经潜入废弃矿坑。";
        assert_eq!(
            final_body_future_consumption_evidence(consumed, current, next, true).as_deref(),
            Some(consumed)
        );
    }

    #[test]
    fn final_body_future_boundary_detection_handles_resultative_aspect() {
        let current = "南晏原遭到袭击后逃离，并从妹妹留下的线索得知天穹的存在";
        let next =
            "南晏原求助黑客温启遥并与她达成合作；温启遥破解代码，发现通往天穹塔主服务器的隐藏通道";
        let consumed = "通往天穹塔主服务器的隐藏通道已开启。";

        assert_eq!(
            final_body_future_consumption_evidence(consumed, current, next, true).as_deref(),
            Some(consumed)
        );
    }

    #[test]
    fn unresolved_character_requests_require_matching_registrations() {
        let requests = vec![ChapterCharacterRequest {
            request_id: "chapter-keeper".to_string(),
            role: "药园管事".to_string(),
            narrative_purpose: "施加外门压力".to_string(),
            ..ChapterCharacterRequest::default()
        }];
        assert_eq!(
            unresolved_character_request_ids(&requests, &[]),
            vec!["chapter-keeper"]
        );
        assert!(unresolved_character_request_ids(
            &requests,
            &[ChapterCharacterRegistration {
                request_id: "chapter-keeper".to_string(),
                canonical_name: "赵铁山".to_string(),
                ..ChapterCharacterRegistration::default()
            }]
        )
        .is_empty());
    }

    #[test]
    fn character_request_ids_are_replaced_recursively_before_sealing() {
        let mut value = json!({
            "goal": "让 station-informant 交出钥匙",
            "scenes": ["station-informant 拒绝", {"turn": "station-informant 改变决定"}]
        });
        replace_character_request_ids_in_value(
            &mut value,
            &[ChapterCharacterRegistration {
                request_id: "station-informant".to_string(),
                canonical_name: "楚辞尘".to_string(),
                ..ChapterCharacterRegistration::default()
            }],
        );

        let encoded = serde_json::to_string(&value).expect("encoded");
        assert!(!encoded.contains("station-informant"));
        assert_eq!(encoded.matches("楚辞尘").count(), 3);
    }

    #[test]
    fn model_authority_projection_keeps_authority_without_context_excerpt_mirrors() {
        let protected = json!({
            "chapter_number": 4,
            "canonical_contract": {"title": "银梭云枢"},
            "truth_as_of_chapter": {"cutoff_chapter": 3},
            "truth_cutoff_chapter": 3,
            "working_context": {"next_chapter_boundary": "第5章才进入气象塔"},
            "context_package": {
                "selected_context": [{
                    "source": "story_bible.prompt.json",
                    "reason": "protected story authority",
                    "layer": "protected",
                    "original_chars": 9000,
                    "selected_chars": 9000,
                    "truncated": false,
                    "excerpt": "不应再次复制进模型视图"
                }]
            },
            "rule_stack": {"hard": ["story_contract"]},
            "chapter_plan": {"plan": "只处理风眼窗口"},
            "chapter_contract": {"goal": "校准风眼"},
            "chapter_architecture": {"architecture": {"scenes": []}},
            "character_registrations": []
        });
        let projection =
            model_authority_projection_payload(AuthorityRole::Reviser, &protected, "root");
        let encoded = serde_json::to_string(&projection).expect("model projection");

        assert_eq!(
            projection
                .get("authority_root_fingerprint")
                .and_then(Value::as_str),
            Some("root")
        );
        assert!(encoded.contains("第5章才进入气象塔"));
        assert!(encoded.contains("story_bible.prompt.json"));
        assert!(!encoded.contains("不应再次复制进模型视图"));
    }
}
