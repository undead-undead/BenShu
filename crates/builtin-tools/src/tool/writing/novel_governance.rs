use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub(crate) const ROLLING_OUTLINE_LOOKAHEAD_CHAPTERS: usize = 3;
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
            haystack.contains(&candidate)
                && !excluded.contains(&candidate)
                && cjk_future_overlap_is_distinctive(&candidate)
                && cjk_future_overlap_has_completed_context(haystack, &candidate)
        })
    })
}

fn cjk_future_overlap_has_completed_context(haystack: &str, candidate: &str) -> bool {
    let process_markers = [
        "的过程中",
        "过程中",
        "正在",
        "持续",
        "不断",
        "试图",
        "准备",
        "决定",
        "打算",
    ];
    haystack.match_indices(candidate).any(|(start, _)| {
        let end = start + candidate.len();
        let suffix = haystack.get(end..).unwrap_or_default();
        let prefix = &haystack[..start];
        !process_markers
            .iter()
            .any(|marker| suffix.starts_with(marker))
            && !["正在", "试图", "准备", "决定", "打算"]
                .iter()
                .any(|marker| prefix.ends_with(marker))
    })
}

fn cjk_future_overlap_is_distinctive(candidate: &str) -> bool {
    let chars = candidate.chars().collect::<Vec<_>>();
    if chars.len() > 6 {
        return true;
    }
    if chars.len() < 4 {
        return false;
    }

    // Short overlaps are only useful as semantic evidence when they are not
    // bounded by Chinese grammatical/location glue. A shared phrase such as
    // “在黑雾中” identifies a setting, not completion of the future event
    // that happens there. Longer spans still carry enough event information
    // to be evaluated with the completed-outcome check above.
    !matches!(
        chars.first(),
        Some(
            '的' | '了'
                | '着'
                | '过'
                | '在'
                | '与'
                | '和'
                | '向'
                | '从'
                | '把'
                | '被'
                | '将'
                | '为'
                | '因'
                | '于'
                | '对'
        )
    ) && !matches!(
        chars.last(),
        Some(
            '的' | '了'
                | '着'
                | '过'
                | '中'
                | '内'
                | '里'
                | '上'
                | '下'
                | '前'
                | '后'
                | '间'
                | '边'
                | '在'
        )
    ) && !cjk_short_overlap_is_context_only(candidate)
}

fn cjk_short_overlap_is_context_only(candidate: &str) -> bool {
    ["内部", "外部", "附近", "周围", "之间", "当中", "其中"]
        .iter()
        .any(|marker| candidate.ends_with(marker))
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
    let segment_chars = segment.chars().collect::<Vec<_>>();
    let shared = segment_chars
        .iter()
        .zip(current_seed.chars())
        .take_while(|(left, right)| **left == *right)
        .count();
    if (2..=4).contains(&shared) && segment_chars.len() > shared {
        return segment_chars[shared..].iter().collect();
    }
    for prefix_len in (2..=4).rev() {
        if segment_chars.len() <= prefix_len {
            continue;
        }
        let prefix = segment_chars[..prefix_len].iter().collect::<String>();
        let remainder = segment_chars[prefix_len..].iter().collect::<String>();
        let begins_subject_predicate = [
            "在", "被", "将", "向", "与", "和", "从", "用", "把", "遭", "因", "为",
        ]
        .iter()
        .any(|marker| remainder.starts_with(marker));
        if begins_subject_predicate
            && current_seed.contains(&prefix)
            && !generic_cjk_anchor(&prefix)
        {
            return segment_chars[prefix_len..].iter().collect();
        }
    }
    segment.to_string()
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

/// Verifies that a bounded final-body excerpt supports the event portion of a
/// sealed contract change, rather than passing merely because both strings
/// contain the same character name. This is intentionally stricter than
/// `truth_item_supported_by_chapter`, which is designed for advisory summary
/// cleanup rather than durable state admission.
pub(crate) fn contract_change_supported_by_final_evidence(
    authority_value: &str,
    evidence: &str,
    cjk: bool,
    ignored_entity_surfaces: &[String],
) -> bool {
    contract_change_evidence_score(authority_value, evidence, cjk, ignored_entity_surfaces) >= 2
}

/// Returns the number of distinctive event terms shared by a sealed authority
/// value and one bounded final-body evidence span.  The boolean admission gate
/// above remains the single owner of the minimum threshold; settlement
/// recovery uses this score only to choose the strongest already-valid span
/// when several adjacent spans paraphrase the same event.
pub(crate) fn contract_change_evidence_score(
    authority_value: &str,
    evidence: &str,
    cjk: bool,
    ignored_entity_surfaces: &[String],
) -> usize {
    if text_reports_completed_outcome(authority_value, cjk)
        && !text_reports_completed_outcome(evidence, cjk)
        && text_reports_non_realized_intent(evidence, cjk)
    {
        return 0;
    }
    raw_contract_change_evidence_score(authority_value, evidence, cjk, ignored_entity_surfaces)
}

fn raw_contract_change_evidence_score(
    authority_value: &str,
    evidence: &str,
    cjk: bool,
    ignored_entity_surfaces: &[String],
) -> usize {
    if contains_unexpected_script_residue(authority_value, evidence) {
        return 0;
    }
    if cjk {
        let mut authority = compact_event_probe_text(strip_truth_label(authority_value));
        let mut evidence = compact_event_probe_text(evidence);
        for surface in ignored_entity_surfaces {
            let surface = compact_event_probe_text(surface);
            if surface.chars().count() >= 2 {
                authority = authority.replace(&surface, "");
                evidence = evidence.replace(&surface, "");
            }
        }
        if authority.is_empty() || evidence.is_empty() {
            return 0;
        }
        let authority_event = without_shared_leading_cjk_subject(&authority, &evidence);
        let evidence_event = without_shared_leading_cjk_subject(&evidence, &authority);
        return super::chapter_quality::shared_distinctive_bigram_count(
            &authority_event,
            &evidence_event,
        );
    }

    let authority_terms = distinctive_english_event_terms(authority_value, evidence);
    let evidence_terms = distinctive_english_event_terms(evidence, authority_value)
        .into_iter()
        .collect::<BTreeSet<_>>();
    authority_terms
        .into_iter()
        .filter(|term| evidence_terms.contains(term))
        .collect::<BTreeSet<_>>()
        .len()
}

fn distinctive_english_event_terms(value: &str, other: &str) -> Vec<String> {
    let mut terms = value
        .split_whitespace()
        .map(|term| {
            term.trim_matches(|ch: char| !ch.is_ascii_alphanumeric())
                .to_ascii_lowercase()
        })
        .filter(|term| {
            !term.is_empty()
                && !matches!(
                    term.as_str(),
                    "the"
                        | "a"
                        | "an"
                        | "and"
                        | "or"
                        | "to"
                        | "of"
                        | "in"
                        | "on"
                        | "at"
                        | "is"
                        | "was"
                        | "are"
                        | "were"
                )
        })
        .collect::<Vec<_>>();
    let other_terms = other
        .split_whitespace()
        .map(|term| {
            term.trim_matches(|ch: char| !ch.is_ascii_alphanumeric())
                .to_ascii_lowercase()
        })
        .filter(|term| !term.is_empty())
        .collect::<Vec<_>>();
    let shared_leading = terms
        .iter()
        .zip(other_terms.iter())
        .take_while(|(left, right)| left == right)
        .count()
        .min(3);
    if shared_leading > 0 && terms.len() > shared_leading {
        terms.drain(..shared_leading);
    }
    terms
}

pub(crate) fn text_consumes_future_chapter(
    text: &str,
    current_seed: &str,
    next_seed: &str,
    cjk: bool,
) -> bool {
    text_consumes_future_chapter_with_required_anchors(text, current_seed, next_seed, cjk, &[])
}

fn text_consumes_future_chapter_with_required_anchors(
    text: &str,
    current_seed: &str,
    next_seed: &str,
    cjk: bool,
    required_character_anchors: &[String],
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
            if required_character_anchors
                .iter()
                .filter(|anchor| segment.contains(anchor.as_str()))
                .any(|anchor| !event_text.contains(anchor.as_str()))
            {
                return false;
            }
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
        // Future-boundary evidence must describe a completed outcome, not a
        // hypothetical or compelled future action. Check this before the
        // completed markers below: phrases such as “不得不进入……阶段” can
        // contain an outcome verb while still leaving that event for the next
        // chapter. This is the existing completion gate's intent check, made
        // sentence-wide so modal wording is not lost at clause boundaries.
        if text_reports_non_realized_intent(sentence, cjk) {
            return false;
        }
        if sentence.contains('已') {
            return true;
        }
        let explicit_outcome = [
            "确认", "发现", "查明", "证明", "证实", "显示", "表明", "直指", "揭示", "揭开", "透露",
            "获得", "得到", "拿到", "完成", "抵达", "进入", "击败", "解决", "达成", "交换", "识破",
            "暴露", "来自", "源自", "属于",
        ]
        .iter()
        .any(|marker| sentence.contains(marker));
        if explicit_outcome {
            return true;
        }
        // A generic “了” only proves a completed event when the clause is not
        // explicitly describing intent, anticipation, or an event in progress.
        // Otherwise normal chapter-end foreshadowing can consume the next
        // chapter even though its result has not happened yet.
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

fn text_reports_non_realized_intent(text: &str, cjk: bool) -> bool {
    if cjk {
        let modal_or_future = [
            "即将",
            "将要",
            "正要",
            "正在",
            "正从",
            "准备",
            "决定",
            "打算",
            "试图",
            "想要",
            "预示",
            "可能",
            "不得不",
            "计划",
            "预期",
            "一旦",
            "如果",
            "若是",
            "将会",
            "会让",
            "会在",
            "会导致",
        ]
        .iter()
        .any(|marker| text.contains(marker));
        // “需要/急需” are strong intent surfaces unless the same bounded
        // clause explicitly says that need has already been satisfied. Keep
        // them out of the general future-marker list so noun-like completed
        // states such as “需要已经得到满足” remain admissible evidence.
        let unmet_need =
            ["需要", "急需"].iter().any(|marker| text.contains(marker)) && !text.contains('已');
        return modal_or_future || unmet_need;
    }
    let lowered = text.to_ascii_lowercase();
    [
        "will ",
        "would ",
        "plans to",
        "planned to",
        "prepares to",
        "prepared to",
        "tries to",
        "tried to",
        "needs to",
        "needed to",
        "intends to",
        "intended to",
        "hopes to",
        "hoped to",
    ]
    .iter()
    .any(|marker| lowered.contains(marker))
}

fn completed_outcome_segments(text: &str, cjk: bool) -> Vec<&str> {
    text.split_inclusive(['。', '！', '？', '.', '!', '?', '\n'])
        .map(str::trim)
        .filter(|sentence| !sentence.is_empty())
        .filter(|sentence| {
            sentence
                .split(['，', ',', '；', ';'])
                .map(str::trim)
                .filter(|clause| !clause.is_empty())
                .any(|clause| sentence_reports_completed_outcome(clause, cjk))
        })
        .collect()
}

fn text_reports_completed_outcome(text: &str, cjk: bool) -> bool {
    !completed_outcome_segments(text, cjk).is_empty()
}

pub(crate) fn final_body_future_consumption_evidence(
    body: &str,
    current_seed: &str,
    next_seed: &str,
    cjk: bool,
    required_character_anchors: &[String],
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
                        && text_consumes_future_chapter_with_required_anchors(
                            clause,
                            current_seed,
                            next_seed,
                            cjk,
                            required_character_anchors,
                        )
                })
        })
        .map(ToString::to_string)
}

/// Accepts the sealed final-body observer's semantic decision only as one
/// bounded, unique, verbatim span from the immutable final body. The workflow
/// may fall back to the deterministic completed-event check against the same
/// sealed current/next boundary, but generic quality checks must not scan
/// mutable manifest seeds. Observer paraphrases never become authority.
pub(crate) fn validated_future_boundary_observer_evidence(
    body: &str,
    evidence: &str,
    current_seed: &str,
    next_seed: &str,
    cjk: bool,
    required_character_anchors: &[String],
) -> Option<String> {
    let evidence = evidence.trim();
    if evidence.is_empty() || evidence.chars().count() > 320 {
        return None;
    }
    let mut matches = body.match_indices(evidence);
    matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    final_body_future_consumption_evidence(
        evidence,
        current_seed,
        next_seed,
        cjk,
        required_character_anchors,
    )?;
    Some(evidence.to_string())
}

pub(crate) fn distinct_future_boundary_character_anchors(
    authority: &SealedChapterAuthority,
    current_seed: &str,
    next_seed: &str,
) -> Vec<String> {
    authority
        .canonical_contract
        .get("characters")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|character| character.get("canonical_name").and_then(Value::as_str))
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .filter(|name| next_seed.contains(name) && !current_seed.contains(name))
        .map(ToString::to_string)
        .collect()
}

pub(crate) struct SealedFutureChapterBoundaries {
    pub(crate) current_chapter_seed: String,
    pub(crate) approved_history_seed: String,
    pub(crate) future: Vec<(usize, String, String)>,
}

/// Reads the current seed and bounded future window from the already sealed
/// authority projection. This keeps all consumers on the same read-only
/// boundary instead of reconstructing another outline view from mutable state.
pub(crate) fn sealed_current_and_future_chapter_seeds(
    authority: &SealedChapterAuthority,
) -> Option<SealedFutureChapterBoundaries> {
    let projection = authority.projection(AuthorityRole::Observer)?;
    let current_seed = [
        authority.chapter_contract.goal.as_str(),
        authority.chapter_contract.scene_goal.as_str(),
    ]
    .into_iter()
    .map(str::trim)
    .filter(|value| !value.is_empty())
    .collect::<Vec<_>>()
    .join("；");
    if current_seed.is_empty() {
        return None;
    }
    let future =
        future_chapter_seeds_from_projection(&projection.payload, authority.chapter_number);
    let future = future.into_values().collect::<Vec<_>>();
    (!future.is_empty()).then(|| SealedFutureChapterBoundaries {
        current_chapter_seed: current_seed,
        approved_history_seed: approved_truth_boundary_context(&authority.truth_as_of_chapter),
        future,
    })
}

/// Extends the current boundary with already-approved facts from the sealed
/// truth snapshot. Future-boundary detection must not classify a repeated
/// prior fact as a newly consumed future event merely because the current
/// chapter seed does not repeat that fact. Pending hooks and ending-contract
/// promises are deliberately excluded because they are not approved outcomes.
fn approved_truth_boundary_context(truth_as_of_chapter: &Value) -> String {
    truth_as_of_chapter
        .get("recent_approved_chapters")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|chapter| {
            [chapter.get("chapter_summary"), chapter.get("current_state")]
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .chain(
                    chapter
                        .get("continuity_updates")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(ToString::to_string),
                )
        })
        .collect::<Vec<_>>()
        .join("；")
}

/// Collect the bounded future window from both sealed boundary sources.  The
/// rolling window is preferred when it contains a chapter, while the durable
/// next-boundary pointer fills any gaps (especially the immediate next
/// chapter).  Keeping this merge in one helper prevents consumers from
/// choosing different authority sources for the same chapter.
fn future_chapter_seeds_from_projection(
    payload: &Value,
    chapter_number: usize,
) -> BTreeMap<usize, (usize, String, String)> {
    let last_number = chapter_number.saturating_add(ROLLING_OUTLINE_LOOKAHEAD_CHAPTERS);
    let mut future = BTreeMap::new();
    for boundary_pointer in [
        "/authority/working_context/rolling_outline_window",
        "/authority/working_context/next_chapter_boundary",
    ] {
        let Some(boundaries) = payload.pointer(boundary_pointer).and_then(Value::as_array) else {
            continue;
        };
        for (index, boundary) in boundaries.iter().enumerate() {
            let Some(number) = boundary
                .get("number")
                .or_else(|| boundary.get("chapter_number"))
                .and_then(Value::as_u64)
                .and_then(|number| usize::try_from(number).ok())
                .filter(|number| *number > chapter_number && *number <= last_number)
            else {
                continue;
            };
            let seed = ["goal", "expected_turn", "moves_toward_ending"]
                .into_iter()
                .filter_map(|key| boundary.get(key).and_then(Value::as_str))
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>()
                .join("；");
            if !seed.is_empty() {
                // The rolling window is visited first and is therefore the
                // preferred sealed source when both records contain a
                // chapter. The next-boundary source only fills a missing key.
                future
                    .entry(number)
                    .or_insert_with(|| (number, seed, format!("{boundary_pointer}/{index}")));
            }
        }
    }
    future
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
    #[serde(default)]
    pub length_topup_eligible: bool,
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
    let mut authority = json!({
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
    remove_internal_character_history(&mut authority);
    json!({
        "schema_version": "benshu.model_authority_projection.v1",
        "role": role,
        "authority_root_fingerprint": authority_root_fingerprint,
        "authority": authority
    })
}

fn remove_internal_character_history(value: &mut Value) {
    match value {
        Value::Array(items) => {
            for item in items {
                remove_internal_character_history(item);
            }
        }
        Value::Object(fields) => {
            fields.remove("previous_names");
            fields.remove("forbidden_renames");
            for item in fields.values_mut() {
                remove_internal_character_history(item);
            }
        }
        _ => {}
    }
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

pub(crate) fn required_entity_anchors(value: &str) -> Vec<String> {
    let compact = cjk_compact(strip_truth_label(value));
    if compact.chars().count() < 4 {
        return Vec::new();
    }
    let prefix_markers = [
        "通过", "进入", "发现", "决定", "开始", "选择", "成功", "失败", "意识", "收到", "听见",
        "看见", "锁定", "保护", "离开", "回到", "面对", "承认", "拒绝", "建立", "引发", "导致",
    ];
    if let Some(index) = prefix_markers
        .iter()
        .filter_map(|marker| compact.find(marker))
        .min()
    {
        let prefix = compact[..index].to_string();
        // A required outcome can lead with an object followed by its
        // property, for example “芯片病毒的扩散程度决定……”.  Reuse the
        // existing leading-entity extraction by keeping the canonical noun
        // before the first possessive marker instead of treating the whole
        // property phrase as an unresolvable entity.
        let parts = prefix.split('的').collect::<Vec<_>>();
        let leading = parts.first().copied().unwrap_or(prefix.as_str());
        let trailing = parts.last().copied().unwrap_or(leading);
        // Compound possessive subjects such as “唐云川派出的侦察机” put
        // an action phrase before the concrete entity.  If that leading
        // phrase is materially longer than the terminal noun, use the
        // terminal noun; ordinary property outcomes such as
        // “芯片病毒的扩散程度” keep their leading object anchor.
        let anchor = if parts.len() > 1 && leading.chars().count() > trailing.chars().count() + 1 {
            trailing
        } else {
            leading
        };
        let count = anchor.chars().count();
        if (2..=8).contains(&count) && !generic_cjk_anchor(anchor) {
            return vec![anchor.to_string()];
        }
    }

    // Some contracts phrase the required outcome as an encounter with an
    // object or institution (for example, “遭遇能源管理者的直接干预”), so
    // there is no leading character before the event verb. Reuse this same
    // anchor extractor for that grammatical form instead of introducing a
    // separate state-recovery path. The supporting sealed chapter fields and
    // final-body evidence still have to corroborate the candidate later.
    let object_markers = ["遭遇", "面对", "受到", "承受", "经历"];
    for marker in object_markers {
        let Some(index) = compact.find(marker) else {
            continue;
        };
        let tail = &compact[index + marker.len()..];
        let end = tail
            .char_indices()
            .find(|(_, ch)| {
                matches!(
                    *ch,
                    '的' | '了'
                        | '并'
                        | '和'
                        | '与'
                        | '在'
                        | '被'
                        | '将'
                        | '因'
                        | '为'
                        | '，'
                        | '。'
                        | '；'
                        | ';'
                        | ','
                        | '.'
                )
            })
            .map(|(offset, _)| offset)
            .unwrap_or(tail.len());
        let candidate = tail[..end].trim();
        let count = candidate.chars().count();
        if (2..=8).contains(&count) && !generic_cjk_anchor(candidate) {
            return vec![candidate.to_string()];
        }
    }
    Vec::new()
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
    let mut out = Vec::new();
    for bigram in super::chapter_quality::adjacent_bigrams(&cjk_compact(value)) {
        if !out.contains(&bigram) {
            out.push(bigram);
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
    fn contract_change_evidence_requires_event_support_beyond_character_name() {
        let authority = "宋泊禾发现影契正在加速衰老";

        assert!(!contract_change_supported_by_final_evidence(
            authority,
            "宋泊禾推开房门，窗外正下着雨。",
            true,
            &[],
        ));
        assert!(contract_change_supported_by_final_evidence(
            authority,
            "宋泊禾终于发现影契正在加速他的衰老。",
            true,
            &[],
        ));
    }

    #[test]
    fn completed_contract_change_rejects_intention_and_accepts_realized_outcome() {
        let authority = "韩照朔通过精准判断废旧物资价值，成功完成第一笔盈利交易";
        let ignored = vec!["韩照朔".to_string()];

        assert_eq!(
            contract_change_evidence_score(
                authority,
                "韩照朔需要第一笔启动资金，于是环视旧货摊寻找机会。",
                true,
                &ignored,
            ),
            0,
            "an intended transaction is not evidence of the sealed completed outcome"
        );
        assert!(
            contract_change_evidence_score(
                authority,
                "韩照朔以十块买入零件，又以三十五块卖出，第一笔盈利交易已经完成。",
                true,
                &ignored,
            ) >= 2
        );
    }

    #[test]
    fn required_entity_anchors_cover_established_state_outcomes() {
        assert_eq!(
            required_entity_anchors("周砚建立了一个不稳定的局部记忆阵地"),
            vec!["周砚".to_string()]
        );
    }

    #[test]
    fn required_entity_anchors_cover_object_event_outcomes() {
        assert_eq!(
            required_entity_anchors("遭遇能源管理者的直接干预"),
            vec!["能源管理者".to_string()]
        );
        assert_eq!(
            required_entity_anchors("芯片病毒的扩散程度决定了逃亡的紧迫性"),
            vec!["芯片病毒".to_string()]
        );
        assert_eq!(
            required_entity_anchors("唐云川派出的侦察机锁定了秦怀弦所在的废墟坐标"),
            vec!["侦察机".to_string()]
        );
    }

    #[test]
    fn contract_change_evidence_ignores_resolved_character_name_outside_subject_position() {
        let surfaces = vec!["裴予朔".to_string()];
        assert!(!contract_change_supported_by_final_evidence(
            "利用旧时代的遮蔽装置救下了即将暴露的裴予朔",
            "酸雨敲打窗棂，裴予朔听见机械脉搏般的回声。",
            true,
            &surfaces,
        ));
        assert!(contract_change_supported_by_final_evidence(
            "利用旧时代的遮蔽装置救下了即将暴露的裴予朔",
            "姜云野启动旧时代遮蔽装置，把即将暴露的裴予朔救进废墟。",
            true,
            &surfaces,
        ));
    }

    #[test]
    fn contract_change_evidence_accepts_shared_object_state_without_exact_wording() {
        assert!(contract_change_supported_by_final_evidence(
            "闻庭安取得铜钥匙",
            "闻庭安带着铜钥匙离开旧站。",
            true,
            &[],
        ));
    }

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
                ],
                "rolling_outline_window": [
                    {"number": 3, "goal": "未来章揭示"},
                    {"number": 4, "goal": "更远未来冲突"}
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
        assert_eq!(
            projection
                .pointer("/authority/working_context/rolling_outline_window")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(2)
        );
    }

    #[test]
    fn future_seed_window_fills_sparse_rolling_window_from_next_boundary() {
        let payload = json!({
            "authority": {
                "working_context": {
                    "rolling_outline_window": [
                        {"number": 4, "goal": "第四章转折"}
                    ],
                    "next_chapter_boundary": [
                        {"number": 2, "goal": "第二章承接"},
                        {"number": 4, "goal": "旧的第四章描述"}
                    ]
                }
            }
        });

        let seeds = future_chapter_seeds_from_projection(&payload, 1);

        assert_eq!(seeds.keys().copied().collect::<Vec<_>>(), vec![2, 4]);
        assert_eq!(seeds[&2].1, "第二章承接");
        assert_eq!(
            seeds[&2].2,
            "/authority/working_context/next_chapter_boundary/0"
        );
        assert_eq!(seeds[&4].1, "第四章转折");
        assert_eq!(
            seeds[&4].2,
            "/authority/working_context/rolling_outline_window/0"
        );
    }

    #[test]
    fn model_projection_hides_internal_character_name_history() {
        let protected = json!({
            "chapter_number": 1,
            "canonical_contract": {
                "characters": [{
                    "canonical_name": "顾屿野",
                    "previous_names": ["阮昭言"],
                    "forbidden_renames": ["林默"]
                }]
            },
            "working_context": {
                "character_ledger": [{
                    "canonical_name": "顾屿野",
                    "forbidden_renames": ["赵无极"]
                }]
            }
        });
        let projection =
            model_authority_projection_payload(AuthorityRole::Writer, &protected, "root");
        let encoded = serde_json::to_string(&projection).expect("model projection");

        assert!(encoded.contains("顾屿野"));
        assert!(!encoded.contains("阮昭言"));
        assert!(!encoded.contains("林默"));
        assert!(!encoded.contains("赵无极"));
        assert_eq!(projection["authority_root_fingerprint"], "root");
    }

    #[test]
    fn final_body_future_boundary_detection_ignores_intent_but_catches_completed_reveal() {
        let current = "闻望宁发现被覆盖的原始共同记忆并私自保留胶囊";
        let next = "闻望宁前往黑市鉴定记忆胶囊来源；确认其源自上层区主脑核心的原始备份；黑市商人透露原主人身份";
        let body = "闻望宁收好胶囊，决定明天前往黑市。";
        assert!(
            final_body_future_consumption_evidence(body, current, next, true, &[]).is_none(),
            "preparing the next action must leave that boundary open"
        );

        let consumed = "但这段被三次覆盖的原始记忆却显示，收割的源头直指上层区的主脑核心。";
        assert_eq!(
            final_body_future_consumption_evidence(consumed, current, next, true, &[]).as_deref(),
            Some(consumed)
        );
    }

    #[test]
    fn final_body_future_boundary_detection_handles_generic_completed_aspect() {
        let current = "岑晏白在外门药园劳作并忍受管事刁难";
        let next = "岑晏白首次用真视之眼辨识变异灵草；救下一株濒死仙草；引起管事注意";
        let consumed = "他没想到，自己随手的一举，竟然真的救活了一株濒死仙草。";
        assert_eq!(
            final_body_future_consumption_evidence(consumed, current, next, true, &[]).as_deref(),
            Some(consumed)
        );

        let intent = "为了救活那株濒死仙草，他决定明天再去药园查找药材。";
        assert!(
            final_body_future_consumption_evidence(intent, current, next, true, &[]).is_none(),
            "purpose and future-intent clauses must not be treated as completed outcomes"
        );
    }

    #[test]
    fn final_body_future_boundary_ignores_modal_plan_for_next_stage() {
        let current = "姜谨澜提交水道文化价值证据并引发规划方案辩论；规划方案开始面临修正压力";
        let next = "唐听朔在进度压力与文化保护之间做出权衡并调整技术方案；规划方案进入修正阶段";
        let body = "这个麻烦可能会让原本已经定稿的规划方案，不得不进入一个非常棘手的修正阶段。";

        assert!(
            final_body_future_consumption_evidence(body, current, next, true, &[]).is_none(),
            "a possible or compelled next-stage plan is not evidence that the next chapter stage completed"
        );
    }

    #[test]
    fn final_body_future_boundary_keeps_core_area_approach_distinct_from_arrival_and_fusion() {
        let truth = json!({
            "recent_approved_chapters": [{
                "chapter_summary": "商砚宁发现了能量核心外壳，并遭遇机械卫队合围。",
                "current_state": "商砚宁带着能量核心外壳躲在斜坡阴影中。",
                "continuity_updates": [
                    "商砚宁通过消耗生命力使碎片与外壳频率同步。"
                ],
                "pending_hooks": "商砚宁何时抵达矿脉核心区"
            }]
        });
        let current = format!(
            "商砚宁通过与闻清安的远程共振感应找到秘密通道；商砚宁成功通过秘密通道脱离包围，并获得闻清安的指引；{}",
            approved_truth_boundary_context(&truth)
        );
        let next = "商砚宁抵达能量核心所在的矿脉核心区，准备进行第一次真正的能量融合；商砚宁抵达能量核心点，准备进行融合仪式";
        let body = "由于刚才为了让碎片与能量核心外壳达成频率同步，商砚宁不得不驱动过多生命力，手部仍在剧烈颤抖。他发现自己已经站在废墟边缘，下方是深不见底的黑暗。这里是能量流动的边缘，也是通往矿脉核心区的必经之路。他意识到自己已经成功脱离表层包围，来到禁地边缘，真正的挑战才刚刚开始。";

        assert!(
            final_body_future_consumption_evidence(body, &current, next, true, &[]).is_none(),
            "an approved core-shell fact and an approach to the core area must not consume arrival or the first fusion"
        );
        assert!(
            !approved_truth_boundary_context(&truth).contains("何时抵达矿脉核心区"),
            "pending hooks must not weaken the future boundary"
        );

        let consumed = "商砚宁已经抵达能量核心所在的矿脉核心区，并完成了第一次真正的能量融合。";
        assert_eq!(
            final_body_future_consumption_evidence(consumed, &current, next, true, &[]).as_deref(),
            Some(consumed),
            "approved prior facts must not hide a genuinely completed future event"
        );
    }

    #[test]
    fn final_body_future_boundary_detection_ignores_ongoing_foreshadowing() {
        let current = "温望川带着南砚声逃入城市上层区；南砚声在数据流中觉醒";
        let next = "温望川遭遇企业安保系统的全方位扫描，被迫在数据缝隙中穿行";
        let foreshadowing = "他感觉到了一种压迫感正从高空降临，那是企业安保系统即将展开的扫描。";
        assert!(
            final_body_future_consumption_evidence(foreshadowing, current, next, true, &[])
                .is_none(),
            "ongoing or anticipated pressure must leave the next boundary unperformed"
        );

        let consumed = "企业安保系统已经完成全方位扫描，温望川被迫在数据缝隙中穿行。";
        assert_eq!(
            final_body_future_consumption_evidence(consumed, current, next, true, &[]).as_deref(),
            Some(consumed)
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
                &[],
            )
            .is_none(),
            "a character name plus one shared action character is not future-event evidence"
        );
        let consumed = "入夜后，阮听舟已经潜入废弃矿坑。";
        assert_eq!(
            final_body_future_consumption_evidence(consumed, current, next, true, &[]).as_deref(),
            Some(consumed)
        );
    }

    #[test]
    fn final_body_future_boundary_detection_ignores_reused_subject_with_different_action() {
        let current = "天穹特工突袭典当行；陆启朔被迫烧毁店铺，带走义体";
        let next = "陆启朔在贫民窟遇见秦知禾；秦知禾认出义体，提出合作，但无人机已锁定街区";
        let body = "跑了大约五分钟，陆启朔在一处岔路口停了下来。";

        assert!(
            final_body_future_consumption_evidence(body, current, next, true, &[]).is_none(),
            "a recurring subject plus ordinary grammar must not consume the next event"
        );
    }

    #[test]
    fn final_body_future_boundary_ignores_low_information_four_character_overlap() {
        let current = "陆云声在废料堆中发现导师留下的加密芯片；芯片激活时引发了陆云声的感官过载，数据流开始干扰视觉";
        let next =
            "陆云声向叶屿序寻求芯片解读方案；阮星禾派出的机械追猎者降临了下城区，打破了暂时的宁静";
        let body = "这枚芯片所释放出的逻辑波纹，似乎在某种程度上打破了下城区原本沉闷的秩序。";

        assert!(
            final_body_future_consumption_evidence(body, current, next, true, &[]).is_none(),
            "了下城区 is grammatical overlap, not evidence that the pursuer arrived"
        );
    }

    #[test]
    fn final_body_future_boundary_ignores_character_possessive_overlap() {
        let current =
            "陆昭白利用碎片散发的微弱灵力构建防御屏障，在乱石堆中躲避追击；闻照真挥剑挡下致命一击";
        let next = "陆昭白在闻照真的庇护下进入宗门外围，并观察当地灵气的流动规律；陆昭白发现灵气漩涡，季照桥锁定了陆昭白的位置";
        let body = "随着一声轻微的脆响，陆昭白的心猛地提到了嗓子眼。";

        assert!(
            final_body_future_consumption_evidence(body, current, next, true, &[]).is_none(),
            "陆昭白的 is a recurring subject plus possessive marker, not a future event"
        );
    }

    #[test]
    fn final_body_future_boundary_ignores_shared_location_phrase() {
        let current = "钟清序在坠落过程中邂逅阮启白；阮启白的航船差点撞上正在下坠的废墟";
        let next = "钟清序与阮启白共同应对岛屿崩塌；季屿舟的影子在黑雾中若隐若现，预示着追逐的开始";
        let body = "阮启白指向那片在黑雾中翻涌的深渊，提醒钟清序赶紧站稳。";

        assert!(
            final_body_future_consumption_evidence(body, current, next, true, &[]).is_none(),
            "a shared location phrase must not prove that the next chapter event completed"
        );
    }

    #[test]
    fn final_body_future_boundary_ignores_adjacent_status_overlap_without_next_action() {
        let current = "温维序利用结构缺陷的理论模型，在一次技术评审会上通过修正设计而非推翻设计的方式，向沈维言展示反击的可能性；温维序与沈维言达成初步的技术共识";
        let next = "裴启桥察觉到温维序的意图，通过调整材料参数试图掩盖缺陷，双方展开第一次技术层面的正面博弈；设计方案的修正过程引发了事务所内部权力的再次洗牌";
        let body = "但在沈维言这里，他已经重新夺回了在事务所内部的技术话语权。";

        assert!(
            final_body_future_consumption_evidence(body, current, next, true, &[]).is_none(),
            "a current-chapter authority shift must not prove material tampering, direct confrontation, or a later power reshuffle"
        );

        let consumed = "裴启桥已经调整材料参数来掩盖缺陷，双方第一次技术层面的正面博弈由此爆发。";
        assert_eq!(
            final_body_future_consumption_evidence(consumed, current, next, true, &[]).as_deref(),
            Some(consumed)
        );
    }

    #[test]
    fn final_body_future_boundary_requires_distinct_next_character_anchor() {
        let current = "叶云宁在酸雨覆盖的废墟中回收一段高价值的残缺记忆；记忆中出现宋家金色纹章";
        let next = "叶云宁因记忆容量不足面临身份降级，被迫在贫民窟边缘寻找生存空间；季维棠递给了他一枚散发着微弱蓝光的禁忌记忆芯片";
        let body = "随着污垢褪去，一枚闪烁着淡蓝色微光的记忆芯片显露了出来。";
        let required = vec!["季维棠".to_string()];

        assert!(
            final_body_future_consumption_evidence(body, current, next, true, &required).is_none(),
            "a similar object must not prove that the next chapter's distinct actor performed its action"
        );
        let consumed = "季维棠已经递给了他一枚散发着微弱蓝光的禁忌记忆芯片。";
        assert_eq!(
            final_body_future_consumption_evidence(consumed, current, next, true, &required)
                .as_deref(),
            Some(consumed)
        );
    }

    #[test]
    fn final_body_future_boundary_ignores_character_name_plus_location_particle_overlap() {
        let current = "秦景朔在逃亡中被迫开启祭鼎初步功能，释放冲击波；商谨川通过灵力波动发现了青铜鼎能够重塑灵气的惊人用途";
        let next = "秦景朔与岑听野在废土边缘建立临时据点；秦景朔意识到祭祀不仅消耗寿命，还能通过特定仪式改变周围的灵气浓度";
        let body = "虽然岑听野在刚才的关键时刻用纯粹的灵力卸掉了致命的攻势，但那份协作的默契并不能让他获得真正的喘息。";

        assert!(
            final_body_future_consumption_evidence(body, current, next, true, &[]).is_none(),
            "a recurring character name plus 在 is grammar/location setup, not completion of the next chapter's base"
        );

        let consumed =
            "秦景朔与岑听野已经在废土边缘建立临时据点，并通过特定仪式改变了周围的灵气浓度。";
        assert_eq!(
            final_body_future_consumption_evidence(consumed, current, next, true, &[]).as_deref(),
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
            final_body_future_consumption_evidence(consumed, current, next, true, &[]).as_deref(),
            Some(consumed)
        );
    }

    #[test]
    fn final_body_future_boundary_does_not_reuse_a_current_process_as_a_future_outcome() {
        let current = "岑清澜发现平衡重建后的规则变化，开始寻找重夺青莲控制权的新手段；岑清澜与南泊原展开新一轮的法则博弈";
        let next = "南泊原发现重塑循环后带来的副作用，准备迎接最终的结局挑战；南泊原彻底完成从散修到守望者的转变";
        let body = "南泊原需要强行引导那股新生的、有序的灵气进入已经变得松散的筑基根基，生命精元的流失速度在重塑循环的过程中被放大了数倍。";

        assert!(
            final_body_future_consumption_evidence(body, current, next, true, &[]).is_none(),
            "a shared phrase such as 重塑循环 describes the current process, not completion of the later副作用 outcome"
        );
    }

    #[test]
    fn final_observer_evidence_must_be_unique_verbatim_final_body_text() {
        let current = "陆昭岚完成第一笔交易";
        let next = "梁砚桥察觉到陆家庄园存在异常繁荣迹象，竞争压力开始显现";
        let body =
            "陆昭岚完成了第一笔交易。梁砚桥已经察觉到陆家庄园存在异常繁荣迹象，竞争压力开始显现。";
        let evidence = "梁砚桥已经察觉到陆家庄园存在异常繁荣迹象，竞争压力开始显现。";

        assert_eq!(
            validated_future_boundary_observer_evidence(body, evidence, current, next, true, &[])
                .as_deref(),
            Some(evidence)
        );
    }

    #[test]
    fn final_observer_cannot_block_with_absent_or_paraphrased_evidence() {
        let current = "陆昭岚完成第一笔交易";
        let next = "梁砚桥察觉到陆家庄园的异常繁荣，竞争压力开始显现";
        let body = "梁砚桥已经派人向陆家送去节礼，但无人讨论庄园近况。";

        assert!(validated_future_boundary_observer_evidence(
            body,
            "梁砚桥已察觉到陆家庄园的异常繁荣。",
            current,
            next,
            true,
            &[],
        )
        .is_none());
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
