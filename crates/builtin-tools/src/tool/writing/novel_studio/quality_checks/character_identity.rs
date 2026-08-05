use super::*;

pub(in crate::tool::writing::novel_studio) fn contract_character_anchor_issues(
    manifest: &NovelProjectManifest,
    _chapter: &ChapterRecord,
    content: &str,
) -> Vec<String> {
    if manifest.contract.is_none() {
        return Vec::new();
    }
    let anchors = manifest_character_anchors(manifest);
    if anchors.is_empty() {
        return Vec::new();
    }

    let mut issues = Vec::new();
    let primary = contract_primary_character_anchors(manifest);
    if !primary.is_empty()
        && !primary
            .iter()
            .any(|anchor| content.contains(anchor.as_str()))
    {
        issues.push(format!(
            "chapter body does not preserve the primary character anchor from the story contract: {}",
            primary.join(", ")
        ));
    }
    issues.extend(contract_supporting_character_dominates_primary_issues(
        &anchors, &primary, content,
    ));

    let lowered = content.to_ascii_lowercase();
    let declares_character_roster = content.contains("主要角色")
        || content.contains("人物设定")
        || lowered.contains("main characters")
        || lowered.contains("character roster");
    if declares_character_roster {
        let missing = anchors
            .iter()
            .filter(|anchor| !content.contains(anchor.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            issues.push(format!(
                "declared character roster is missing stable contract anchors: {}",
                missing.join(", ")
            ));
        }
    }
    issues
}

fn contract_supporting_character_dominates_primary_issues(
    anchors: &[String],
    primary: &[String],
    content: &str,
) -> Vec<String> {
    if primary.is_empty() {
        return Vec::new();
    }
    let primary_mentions = primary
        .iter()
        .map(|anchor| content.matches(anchor.as_str()).count())
        .max()
        .unwrap_or(0);
    if primary_mentions == 0 {
        return Vec::new();
    }
    let primary_set = primary.iter().cloned().collect::<BTreeSet<_>>();
    let mut ranked_supporting = anchors
        .iter()
        .filter(|anchor| !primary_set.contains(*anchor))
        .map(|anchor| (anchor, content.matches(anchor.as_str()).count()))
        .filter(|(_, count)| *count >= 4)
        .collect::<Vec<_>>();
    ranked_supporting.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(right.0)));
    let Some((supporting, count)) = ranked_supporting.first() else {
        return Vec::new();
    };
    if *count <= primary_mentions.saturating_mul(2).max(4) {
        return Vec::new();
    }
    vec![format!(
        "contract supporting character `{supporting}` appears to replace the primary character line; primary anchor `{}` appears {} times while supporting anchor appears {} times",
        primary.join(", "),
        primary_mentions,
        count
    )]
}

pub(in crate::tool::writing::novel_studio) fn contract_character_drift_issues(
    manifest: &NovelProjectManifest,
    chapter: &ChapterRecord,
    content: &str,
) -> Vec<String> {
    if !is_chinese_language(&manifest.language) {
        return Vec::new();
    }
    if manifest.contract.is_none() {
        return Vec::new();
    }
    let anchors = manifest_character_anchors(manifest);
    if anchors.is_empty() {
        return Vec::new();
    }
    let authority_view = contract_term_authority_view(manifest);
    let known = known_character_like_terms(manifest, &anchors);
    let mut issues = Vec::new();
    for (forbidden, canonical) in forbidden_character_name_replacements(manifest) {
        if replace_forbidden_character_name_reference(
            content,
            &forbidden,
            &canonical,
            &authority_view,
        ) != content
        {
            issues.push(format!(
                "superseded character name `{forbidden}` appears in chapter body; canonical authority is `{canonical}`"
            ));
        }
    }
    for anchor in &anchors {
        for candidate in near_anchor_cjk_name_variants(content, anchor) {
            if known.contains(&candidate) || authority_view.is_non_character_term(&candidate) {
                continue;
            }
            issues.push(format!(
                "possible character name drift: `{candidate}` is close to stable contract character `{anchor}` but is not recorded in the story contract or truth ledger"
            ));
        }
    }
    issues.extend(unrecorded_character_issues(
        manifest,
        chapter,
        content,
        &known,
        &authority_view.non_character_terms(),
    ));
    issues.sort();
    issues.dedup();
    issues
}

pub(in crate::tool::writing::novel_studio) fn unregistered_character_candidate_issues(
    manifest: &NovelProjectManifest,
    chapter: &ChapterRecord,
) -> Vec<String> {
    if !is_chinese_language(&manifest.language) || manifest.contract.is_none() {
        return Vec::new();
    }
    let anchors = manifest_character_anchors(manifest);
    let authority_view = contract_term_authority_view(manifest);
    let known = known_character_like_terms(manifest, &anchors);
    let trusted_character_names = known.iter().cloned().collect::<Vec<_>>();
    let mut issues = chapter_character_candidates(chapter)
        .into_iter()
        .filter(|candidate| !known.contains(candidate))
        .filter(|candidate| !authority_view.is_non_character_term(candidate))
        .filter(|candidate| {
            !cjk_anchor_is_contaminated_by_trusted_name(candidate, &trusted_character_names)
        })
        .map(|candidate| {
            format!(
                "unregistered character `{candidate}` appears in chapter metadata and prose; every named character must be declared by the chapter execution contract before prose generation"
            )
        })
        .collect::<Vec<_>>();
    issues.sort();
    issues.dedup();
    issues
}

pub(in crate::tool::writing::novel_studio) fn contract_character_pronoun_drift_issues(
    manifest: &NovelProjectManifest,
    _chapter: &ChapterRecord,
    content: &str,
) -> Vec<String> {
    if !is_chinese_language(&manifest.language) {
        return Vec::new();
    }
    let authority_view = contract_term_authority_view(manifest);
    let mut issues = Vec::new();
    for (name, markers) in &authority_view.character_identity_markers {
        let expected = if markers.contains("pronoun_profile:feminine")
            || markers.contains("inferred_pronoun_profile:feminine")
        {
            Some("feminine")
        } else if markers.contains("pronoun_profile:masculine")
            || markers.contains("inferred_pronoun_profile:masculine")
        {
            Some("masculine")
        } else {
            None
        };
        let Some(expected) = expected else {
            continue;
        };
        let other_character_names = authority_view
            .character_names
            .iter()
            .filter(|other| *other != name)
            .cloned()
            .collect::<BTreeSet<_>>();
        let evidence = character_pronoun_evidence_near_name(
            content,
            name,
            &other_character_names,
            PronounEvidenceScope::ChapterHardGate,
        );
        let contradicts = match expected {
            "feminine" => evidence.masculine,
            "masculine" => evidence.feminine,
            _ => 0,
        };
        if contradicts >= 2 {
            issues.push(format!(
                "character pronoun/appellation drift: `{name}` has established {expected} identity markers, but this chapter uses contradictory references near the same character"
            ));
        }
    }
    issues.sort();
    issues.dedup();
    issues
}

pub(in crate::tool::writing) fn compact_planning_text_conflicts_with_character_identity(
    content: &str,
    character_identity_markers: &BTreeMap<String, BTreeSet<String>>,
) -> bool {
    let mentioned = character_identity_markers
        .iter()
        .filter(|(name, _)| !name.trim().is_empty() && content.contains(name.as_str()))
        .filter_map(|(name, markers)| {
            let expected = if markers.contains("pronoun_profile:feminine")
                || markers.contains("inferred_pronoun_profile:feminine")
            {
                Some("feminine")
            } else if markers.contains("pronoun_profile:masculine")
                || markers.contains("inferred_pronoun_profile:masculine")
            {
                Some("masculine")
            } else {
                None
            }?;
            Some((name, expected))
        })
        .collect::<Vec<_>>();
    if mentioned.is_empty() {
        return false;
    }

    for (name, expected) in &mentioned {
        let other_character_names = character_identity_markers
            .keys()
            .filter(|other| *other != *name)
            .cloned()
            .collect::<BTreeSet<_>>();
        let evidence = character_pronoun_evidence_near_name(
            content,
            name,
            &other_character_names,
            PronounEvidenceScope::ContractInference,
        );
        let (contradicts, supports) = if *expected == "feminine" {
            (evidence.masculine, evidence.feminine)
        } else {
            (evidence.feminine, evidence.masculine)
        };
        let opposite = if *expected == "feminine" {
            "他"
        } else {
            "她"
        };
        if contradicts > 0
            && contradicts > supports
            && !opposite_pronoun_follows_unnamed_person_introduction(content, name, opposite)
        {
            return true;
        }
    }

    let profiles = mentioned
        .iter()
        .map(|(_, profile)| *profile)
        .collect::<BTreeSet<_>>();
    if profiles.len() != 1 {
        return false;
    }
    let expected = *profiles.iter().next().expect("one profile");
    let (opposite, matching) = if expected == "feminine" {
        ("他", "她")
    } else {
        ("她", "他")
    };
    explicit_identity_marker_count(content, opposite)
        > explicit_identity_marker_count(content, matching)
        && !mentioned.iter().any(|(name, _)| {
            opposite_pronoun_follows_unnamed_person_introduction(content, name, opposite)
        })
}

fn opposite_pronoun_follows_unnamed_person_introduction(
    content: &str,
    established_name: &str,
    opposite_pronoun: &str,
) -> bool {
    content
        .match_indices(established_name)
        .any(|(name_index, name)| {
            let after_name = &content[name_index + name.len()..];
            after_name
                .match_indices(opposite_pronoun)
                .any(|(pronoun_index, _)| {
                    let between = &after_name[..pronoun_index];
                    [
                        "一名", "一位", "一个", "某名", "某位", "某个", "这名", "那名",
                    ]
                    .iter()
                    .any(|marker| between.contains(marker))
                })
        })
}

#[derive(Debug, Clone, Copy, Default)]
struct CharacterPronounEvidence {
    feminine: usize,
    masculine: usize,
}

const FEMININE_IDENTITY_MARKERS: &[&str] = &[
    "她", "女人", "女孩", "少女", "姑娘", "女子", "女修", "小姐", "姐姐", "妹妹", "母亲", "妻子",
    "女儿", "侍女",
];
const FEMININE_ROLE_MARKERS: &[&str] = &[
    "夫人",
    "太太",
    "姨太",
    "女主人",
    "小姐",
    "姑娘",
    "女孩",
    "少女",
    "女人",
    "女子",
    "妹妹",
    "姐姐",
    "母亲",
    "妻子",
    "女儿",
    "侍女",
];
const MASCULINE_IDENTITY_MARKERS: &[&str] = &[
    "他", "男人", "男孩", "少年", "青年", "男子", "男修", "先生", "哥哥", "弟弟", "父亲", "丈夫",
    "儿子", "士子", "书生",
];
const MASCULINE_ROLE_MARKERS: &[&str] = &[
    "先生", "少爷", "老爷", "男人", "男子", "男孩", "少年", "青年", "哥哥", "弟弟", "父亲", "丈夫",
    "儿子", "士子", "书生",
];
const FEMININE_SELF_IDENTITY_MARKERS: &[&str] = &[
    "女性",
    "女主",
    "女人",
    "女孩",
    "少女",
    "姑娘",
    "女子",
    "女修",
    "小姐",
    "妻子",
    "未婚妻",
    "前妻",
    "亡妻",
    "女友",
    "女性伴侣",
    "侍女",
];
const MASCULINE_SELF_IDENTITY_MARKERS: &[&str] = &[
    "男性",
    "男主",
    "男人",
    "男孩",
    "少年",
    "青年",
    "男子",
    "男修",
    "先生",
    "士子",
    "书生",
    "丈夫",
    "未婚夫",
    "前夫",
    "亡夫",
    "男友",
    "男性伴侣",
];

fn stable_pronoun_profile(evidence: CharacterPronounEvidence) -> Option<&'static str> {
    if evidence.feminine >= 2 && evidence.feminine >= evidence.masculine.saturating_add(2) {
        return Some("feminine");
    }
    if evidence.masculine >= 2 && evidence.masculine >= evidence.feminine.saturating_add(2) {
        return Some("masculine");
    }
    None
}

pub(in crate::tool::writing) fn stable_character_pronoun_profile_in_text(
    content: &str,
    name: &str,
    other_character_names: &BTreeSet<String>,
) -> Option<&'static str> {
    stable_pronoun_profile(character_pronoun_evidence_near_name(
        content,
        name,
        other_character_names,
        PronounEvidenceScope::ContractInference,
    ))
}

pub(in crate::tool::writing) fn stable_approved_character_pronoun_profile_in_text(
    content: &str,
    name: &str,
    other_character_names: &BTreeSet<String>,
) -> Option<&'static str> {
    stable_pronoun_profile(character_pronoun_evidence_near_name(
        content,
        name,
        other_character_names,
        PronounEvidenceScope::ApprovedSettlement,
    ))
}

pub(in crate::tool::writing) fn approved_character_pronoun_profile_hint_in_text(
    content: &str,
    name: &str,
    other_character_names: &BTreeSet<String>,
) -> Option<&'static str> {
    let evidence = character_pronoun_evidence_near_name(
        content,
        name,
        other_character_names,
        PronounEvidenceScope::ChapterHardGate,
    );
    match (evidence.feminine, evidence.masculine) {
        (feminine, 0) if feminine > 0 => Some("feminine"),
        (0, masculine) if masculine > 0 => Some("masculine"),
        _ => None,
    }
}

pub(in crate::tool::writing) fn stable_primary_pronoun_profile_in_text(
    content: &str,
) -> Option<&'static str> {
    let marker_count = |markers: &[&str]| {
        markers
            .iter()
            .map(|marker| explicit_identity_marker_count(content, marker))
            .sum()
    };
    stable_pronoun_profile(CharacterPronounEvidence {
        feminine: marker_count(FEMININE_IDENTITY_MARKERS),
        masculine: marker_count(MASCULINE_IDENTITY_MARKERS),
    })
}

pub(in crate::tool::writing) fn explicit_identity_profile_in_character_anchor(
    content: &str,
) -> Option<&'static str> {
    let feminine = explicit_self_identity_marker_count(content, FEMININE_SELF_IDENTITY_MARKERS);
    let masculine = explicit_self_identity_marker_count(content, MASCULINE_SELF_IDENTITY_MARKERS);
    if feminine == masculine {
        None
    } else if feminine > masculine {
        Some("feminine")
    } else {
        Some("masculine")
    }
}

fn explicit_self_identity_marker_count(content: &str, markers: &[&str]) -> usize {
    let content = content.trim();
    markers
        .iter()
        .map(|marker| {
            content
                .match_indices(marker)
                .filter(|(index, _)| {
                    identity_marker_occurrence_is_explicit(content, *index, marker)
                        && identity_marker_describes_anchor_subject(content, *index, marker)
                })
                .count()
        })
        .sum()
}

fn identity_marker_describes_anchor_subject(content: &str, index: usize, marker: &str) -> bool {
    let before = content[..index].trim_end_matches(char::is_whitespace);
    let after = content[index + marker.len()..]
        .trim_start_matches(char::is_whitespace)
        .trim_start_matches(['，', '、', '：', ':']);
    before.is_empty()
        || after.is_empty()
        || [
            "是",
            "为",
            "作为",
            "身为",
            "成为",
            "变成",
            "本是",
            "原是",
            "仍是",
            "是一名",
            "是一个",
            "是个",
            "是一位",
            "是位",
        ]
        .iter()
        .any(|cue| before.ends_with(cue))
}

fn character_pronoun_evidence_near_name(
    content: &str,
    name: &str,
    other_character_names: &BTreeSet<String>,
    scope: PronounEvidenceScope,
) -> CharacterPronounEvidence {
    CharacterPronounEvidence {
        feminine: direct_identity_marker_count_for_name(
            content,
            name,
            other_character_names,
            FEMININE_IDENTITY_MARKERS,
            FEMININE_ROLE_MARKERS,
            scope,
        ),
        masculine: direct_identity_marker_count_for_name(
            content,
            name,
            other_character_names,
            MASCULINE_IDENTITY_MARKERS,
            MASCULINE_ROLE_MARKERS,
            scope,
        ),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PronounEvidenceScope {
    ContractInference,
    ChapterHardGate,
    ApprovedSettlement,
}

fn direct_identity_marker_count_for_name(
    window: &str,
    name: &str,
    other_character_names: &BTreeSet<String>,
    markers: &[&str],
    same_profile_role_markers: &[&str],
    scope: PronounEvidenceScope,
) -> usize {
    window
        .match_indices(name)
        .map(|(index, matched)| {
            let after = &window[index + matched.len()..];
            let before = &window[..index];
            let direct = nearby_identity_marker_count(
                after,
                markers,
                same_profile_role_markers,
                other_character_names,
                true,
                scope,
            ) + nearby_identity_marker_count(
                before,
                same_profile_role_markers,
                same_profile_role_markers,
                other_character_names,
                false,
                scope,
            );
            direct
                + if scope == PronounEvidenceScope::ChapterHardGate {
                    0
                } else {
                    following_sentence_identity_marker_count(
                        after,
                        markers,
                        other_character_names,
                        scope,
                    )
                }
        })
        .sum()
}

fn following_sentence_identity_marker_count(
    text: &str,
    markers: &[&str],
    other_character_names: &BTreeSet<String>,
    scope: PronounEvidenceScope,
) -> usize {
    let boundary = text
        .char_indices()
        .find(|(_, ch)| matches!(ch, '。' | '！' | '？' | '；' | '\n'));
    let Some((index, boundary)) = boundary else {
        return 0;
    };
    if text[..index].chars().count() > 48
        || other_character_names
            .iter()
            .any(|name| !name.is_empty() && text[..index].contains(name))
    {
        return 0;
    }
    let start = index + boundary.len_utf8();
    let next = text[start..]
        .trim_start_matches(char::is_whitespace)
        .split(|ch| matches!(ch, '。' | '！' | '？' | '；' | '\n'))
        .next()
        .unwrap_or("")
        .to_string();
    if next.is_empty()
        || other_character_names
            .iter()
            .any(|name| !name.is_empty() && next.contains(name))
    {
        return 0;
    }
    if scope == PronounEvidenceScope::ApprovedSettlement {
        return markers
            .iter()
            .filter(|marker| matches!(**marker, "他" | "她"))
            .filter(|marker| next.starts_with(**marker))
            .filter(|marker| identity_marker_occurrence_is_explicit(&next, 0, **marker))
            .count();
    }
    let first_personal_pronoun =
        first_attributable_personal_pronoun(&next, 18, PronounEvidenceScope::ContractInference);
    markers
        .iter()
        .filter(|marker| {
            next.match_indices(**marker).any(|(index, _)| {
                if !identity_marker_occurrence_is_explicit(&next, index, marker) {
                    return false;
                }
                if matches!(**marker, "他" | "她") {
                    if first_personal_pronoun != Some((index, marker.chars().next().unwrap())) {
                        return false;
                    }
                    return personal_pronoun_directly_attributes_nearby_name(
                        &next[..index],
                        true,
                        18,
                        marker.chars().next().unwrap(),
                        PronounEvidenceScope::ContractInference,
                    );
                }
                next[..index].chars().count() <= 8
            })
        })
        .count()
}

fn nearby_identity_marker_count(
    text: &str,
    markers: &[&str],
    same_profile_role_markers: &[&str],
    other_character_names: &BTreeSet<String>,
    forward: bool,
    scope: PronounEvidenceScope,
) -> usize {
    let chars = if forward {
        text.split(|ch| matches!(ch, '。' | '！' | '？' | '；' | '\n'))
            .next()
            .unwrap_or("")
            .to_string()
    } else {
        text.chars()
            .rev()
            .take(12)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<String>()
    };
    let sentence_boundary = |ch| matches!(ch, '。' | '！' | '？' | '；' | '\n');
    let sentence_fragment = if forward {
        chars.split(sentence_boundary).next()
    } else {
        chars.rsplit(sentence_boundary).next()
    }
    .unwrap_or("")
    .trim();
    if sentence_fragment.is_empty() {
        return 0;
    }
    let first_personal_pronoun = if forward {
        first_attributable_personal_pronoun(sentence_fragment, 48, scope)
    } else {
        None
    };
    markers
        .iter()
        .map(|marker| {
            sentence_fragment
                .match_indices(marker)
                .filter(|(index, _)| {
                    if !identity_marker_occurrence_is_explicit(sentence_fragment, *index, marker) {
                        return false;
                    }
                    let between = if forward {
                        &sentence_fragment[..*index]
                    } else {
                        &sentence_fragment[index + marker.len()..]
                    };
                    if other_character_names
                        .iter()
                        .any(|other| !other.is_empty() && between.contains(other))
                    {
                        return false;
                    }
                    if matches!(*marker, "他" | "她")
                        && (first_personal_pronoun
                            != Some((*index, marker.chars().next().unwrap()))
                            || !personal_pronoun_directly_attributes_nearby_name(
                                between,
                                forward,
                                48,
                                marker.chars().next().unwrap(),
                                scope,
                            ))
                    {
                        return false;
                    }
                    if same_profile_role_markers.contains(marker)
                        && !role_marker_directly_attributes_nearby_name(between, forward)
                    {
                        return false;
                    }
                    !same_profile_role_markers
                        .iter()
                        .any(|role_marker| between.contains(role_marker))
                })
                .count()
        })
        .sum()
}

fn first_attributable_personal_pronoun(
    text: &str,
    max_distance: usize,
    scope: PronounEvidenceScope,
) -> Option<(usize, char)> {
    ["他", "她"]
        .into_iter()
        .flat_map(|marker| {
            text.match_indices(marker)
                .filter(move |(index, _)| {
                    identity_marker_occurrence_is_explicit(text, *index, marker)
                        && personal_pronoun_directly_attributes_nearby_name(
                            &text[..*index],
                            true,
                            max_distance,
                            marker.chars().next().unwrap(),
                            scope,
                        )
                })
                .map(move |(index, _)| (index, marker.chars().next().unwrap()))
        })
        .min_by_key(|(index, _)| *index)
}

fn personal_pronoun_directly_attributes_nearby_name(
    between: &str,
    forward: bool,
    max_distance: usize,
    pronoun: char,
    scope: PronounEvidenceScope,
) -> bool {
    if !forward {
        return false;
    }
    let compact = between
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    if compact.chars().count() > max_distance {
        return false;
    }
    if compact.is_empty() {
        return true;
    }
    let Some((boundary, _)) = compact
        .char_indices()
        .rev()
        .find(|(_, ch)| matches!(ch, '，' | ',' | '。' | '！' | '!' | '？' | '?' | '；' | ';'))
    else {
        return false;
    };
    let continuation =
        compact[boundary..].trim_start_matches(['，', ',', '。', '！', '!', '？', '?', '；', ';']);
    let prior_clause = &compact[..boundary];
    let prior_clause_object_pronoun = ["他", "她"]
        .into_iter()
        .flat_map(|marker| {
            prior_clause
                .match_indices(marker)
                .filter(move |(index, _)| {
                    identity_marker_occurrence_is_explicit(prior_clause, *index, marker)
                })
                .map(move |(index, _)| (index, marker.chars().next().unwrap()))
        })
        .max_by_key(|(index, _)| *index)
        .map(|(_, marker)| marker);
    (match scope {
        PronounEvidenceScope::ContractInference => prior_clause_object_pronoun.is_some(),
        PronounEvidenceScope::ChapterHardGate | PronounEvidenceScope::ApprovedSettlement => {
            prior_clause_object_pronoun.is_some_and(|object_pronoun| object_pronoun != pronoun)
        }
    }) || matches!(
        continuation,
        "" | "而"
            | "但"
            | "然而"
            | "却"
            | "随后"
            | "然后"
            | "于是"
            | "接着"
            | "此时"
            | "这时"
            | "最终"
            | "仍"
            | "仍然"
            | "仍旧"
            | "依然"
            | "也"
            | "又"
            | "便"
            | "就"
            | "则"
            | "转而"
            | "忽然"
            | "突然"
            | "旋即"
            | "随即"
            | "紧接着"
            | "下一刻"
    )
}

#[cfg(test)]
mod pronoun_tests {
    use super::*;

    #[test]
    fn relationship_role_identity_is_explicit_but_object_reference_is_not() {
        assert_eq!(
            explicit_identity_profile_in_character_anchor("妻子兼关键关系对象"),
            Some("feminine")
        );
        assert_eq!(
            explicit_identity_profile_in_character_anchor("丈夫兼关键关系对象"),
            Some("masculine")
        );
        assert_eq!(
            explicit_identity_profile_in_character_anchor("寻找失踪妻子的调查者"),
            None
        );
    }

    #[test]
    fn compact_planning_authority_rejects_opposite_pronoun_for_same_profile_pair() {
        let authority = BTreeMap::from([
            (
                "程听野".to_string(),
                BTreeSet::from(["inferred_pronoun_profile:masculine".to_string()]),
            ),
            (
                "闻照桥".to_string(),
                BTreeSet::from(["inferred_pronoun_profile:masculine".to_string()]),
            ),
        ]);

        assert!(compact_planning_text_conflicts_with_character_identity(
            "程听野开始意识到闻照桥的规划不仅是为了城市，更是为了她。",
            &authority,
        ));
        assert!(!compact_planning_text_conflicts_with_character_identity(
            "程听野开始意识到闻照桥的规划不仅是为了城市，更是为了他。",
            &authority,
        ));
        assert!(!compact_planning_text_conflicts_with_character_identity(
            "程听野与闻照桥共同完成结构缓冲方案。",
            &authority,
        ));
    }

    #[test]
    fn compact_planning_authority_does_not_assign_unnamed_person_pronoun_to_named_character() {
        let authority = BTreeMap::from([(
            "商予真".to_string(),
            BTreeSet::from(["inferred_pronoun_profile:feminine".to_string()]),
        )]);

        assert!(!compact_planning_text_conflicts_with_character_identity(
            "商予真看见一名守卫，他立即转身逃走。",
            &authority,
        ));
    }

    #[test]
    fn object_pronoun_does_not_hide_later_subject_pronoun_for_nearby_name() {
        let content = "沈砚看向她，却发现他自己的手正在发抖。";
        let other_names = BTreeSet::new();

        let masculine = direct_identity_marker_count_for_name(
            content,
            "沈砚",
            &other_names,
            MASCULINE_IDENTITY_MARKERS,
            MASCULINE_ROLE_MARKERS,
            PronounEvidenceScope::ChapterHardGate,
        );
        let feminine = direct_identity_marker_count_for_name(
            content,
            "沈砚",
            &other_names,
            FEMININE_IDENTITY_MARKERS,
            FEMININE_ROLE_MARKERS,
            PronounEvidenceScope::ChapterHardGate,
        );

        assert!(masculine > 0);
        assert_eq!(feminine, 0);
    }

    #[test]
    fn repeated_object_pronoun_is_not_attributed_to_named_observer() {
        let content = "沈砚看着她，能感受到她眼中那种紧迫的焦虑。";
        let other_names = BTreeSet::from(["顾晚".to_string()]);

        let feminine = direct_identity_marker_count_for_name(
            content,
            "沈砚",
            &other_names,
            FEMININE_IDENTITY_MARKERS,
            FEMININE_ROLE_MARKERS,
            PronounEvidenceScope::ChapterHardGate,
        );

        assert_eq!(feminine, 0);
    }

    #[test]
    fn prior_addressee_title_is_not_attributed_to_next_speaker() {
        let content = "“顾小姐，”沈砚收回视线，重新回到工作台旁。";
        let other_names = BTreeSet::from(["顾晚".to_string()]);

        let feminine = direct_identity_marker_count_for_name(
            content,
            "沈砚",
            &other_names,
            FEMININE_IDENTITY_MARKERS,
            FEMININE_ROLE_MARKERS,
            PronounEvidenceScope::ChapterHardGate,
        );

        assert_eq!(feminine, 0);
    }

    #[test]
    fn object_clause_pronoun_after_comma_is_not_attributed_to_prior_name() {
        let content = "唐承序正在构建他的帝国。唐承序的意志仿佛化作巨浪，试图冲垮她建立的防御。每个连接处都承受着来自唐承序秩序的挤压。如果她不能及时锁死零件，韩予真就会失去最后的防线。";
        let other_names = BTreeSet::from(["韩予真".to_string()]);

        let masculine = direct_identity_marker_count_for_name(
            content,
            "唐承序",
            &other_names,
            MASCULINE_IDENTITY_MARKERS,
            MASCULINE_ROLE_MARKERS,
            PronounEvidenceScope::ChapterHardGate,
        );
        let feminine = direct_identity_marker_count_for_name(
            content,
            "唐承序",
            &other_names,
            FEMININE_IDENTITY_MARKERS,
            FEMININE_ROLE_MARKERS,
            PronounEvidenceScope::ChapterHardGate,
        );

        assert_eq!(masculine, 0);
        assert_eq!(feminine, 0);
    }

    #[test]
    fn discourse_connector_keeps_following_subject_pronoun_attributable() {
        let content = "唐承序停在门前，随后他推开了门。";
        let other_names = BTreeSet::new();

        let masculine = direct_identity_marker_count_for_name(
            content,
            "唐承序",
            &other_names,
            MASCULINE_IDENTITY_MARKERS,
            MASCULINE_ROLE_MARKERS,
            PronounEvidenceScope::ChapterHardGate,
        );

        assert_eq!(masculine, 1);
    }

    #[test]
    fn implicit_cross_sentence_subject_switch_is_not_hard_attributed_to_previous_name() {
        let content = "唐承序停在门前，随后他推开了门。她重新看向坐标点。";
        let other_names = BTreeSet::from(["韩予真".to_string()]);

        let masculine = direct_identity_marker_count_for_name(
            content,
            "唐承序",
            &other_names,
            MASCULINE_IDENTITY_MARKERS,
            MASCULINE_ROLE_MARKERS,
            PronounEvidenceScope::ChapterHardGate,
        );
        let feminine = direct_identity_marker_count_for_name(
            content,
            "唐承序",
            &other_names,
            FEMININE_IDENTITY_MARKERS,
            FEMININE_ROLE_MARKERS,
            PronounEvidenceScope::ChapterHardGate,
        );

        assert_eq!(masculine, 1);
        assert_eq!(feminine, 0);
    }
}

fn role_marker_directly_attributes_nearby_name(between: &str, forward: bool) -> bool {
    if !forward
        && between.chars().any(|ch| {
            matches!(
                ch,
                '，' | ','
                    | '。'
                    | '！'
                    | '!'
                    | '？'
                    | '?'
                    | '；'
                    | ';'
                    | '“'
                    | '”'
                    | '"'
                    | '‘'
                    | '’'
            )
        })
    {
        return false;
    }
    let compact = between
        .chars()
        .filter(|ch| !ch.is_whitespace() && !matches!(ch, '，' | ',' | '：' | ':' | '（' | '('))
        .collect::<String>();
    if compact.is_empty() {
        return true;
    }
    if !forward {
        return compact.chars().count() <= 4
            && ![
                "遇", "见", "找", "跟", "追", "护", "救", "带", "和", "与", "向", "对",
            ]
            .iter()
            .any(|marker| compact.contains(marker));
    }
    [
        "是",
        "为",
        "仍是",
        "已是",
        "原是",
        "本是",
        "作为",
        "身为",
        "成为",
        "原本是",
        "曾是",
        "乃是",
    ]
    .iter()
    .any(|attribution| compact == *attribution || compact.ends_with(attribution))
}

fn explicit_identity_marker_count(content: &str, marker: &str) -> usize {
    content
        .match_indices(marker)
        .filter(|(index, _)| identity_marker_occurrence_is_explicit(content, *index, marker))
        .count()
}

fn identity_marker_occurrence_is_explicit(content: &str, index: usize, marker: &str) -> bool {
    if !matches!(marker, "他" | "她") {
        return true;
    }
    let previous = content[..index].chars().next_back();
    let next = content[index + marker.len()..].chars().next();
    !next.is_some_and(|ch| matches!(ch, '们' | '俩'))
        && (marker != "他"
            || (!previous.is_some_and(|ch| matches!(ch, '利' | '其' | '吉' | '排' | '维'))
                && !next.is_some_and(|ch| {
                    matches!(
                        ch,
                        '人' | '者' | '方' | '乡' | '处' | '物' | '国' | '校' | '日' | '年'
                    )
                })))
}

pub(in crate::tool::writing::novel_studio) async fn approved_chapter_integrity_blockers(
    project_dir: &Path,
    manifest: &NovelProjectManifest,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let mut blockers = Vec::new();
    for chapter in manifest
        .chapters
        .iter()
        .filter(|chapter| chapter_is_approved(chapter))
    {
        let raw = tokio::fs::read_to_string(project_dir.join(&chapter.path))
            .await
            .unwrap_or_default();
        let content = strip_frontmatter(&raw);
        let gate =
            super::super::quality_gate::chapter_quality_gate(manifest, chapter, &content, &[]);
        let findings = gate
            .findings
            .into_iter()
            .filter(|finding| finding.hard_blocking())
            .filter(|finding| {
                matches!(
                    finding.class,
                    chapter_quality::ChapterFindingClass::Contract
                        | chapter_quality::ChapterFindingClass::Continuity
                        | chapter_quality::ChapterFindingClass::State
                )
            })
            .collect::<Vec<_>>();
        if !findings.is_empty() {
            blockers.push(json!({
                "chapter_number": chapter.number,
                "chapter_title": chapter.title,
                "findings": findings,
                "next_action": "repair_project_state"
            }));
        }
    }
    Ok(blockers)
}

pub(in crate::tool::writing::novel_studio) fn contract_primary_character_anchors(
    manifest: &NovelProjectManifest,
) -> Vec<String> {
    let Some(contract) = &manifest.contract else {
        return Vec::new();
    };
    let mut anchors = contract
        .characters
        .iter()
        .filter(|value| character_contract_line_marks_primary(value))
        .filter_map(|value| stable_anchor_token(value).map(ToString::to_string))
        .filter(|value| character_anchor_name_for_language(value, &manifest.language).is_some())
        .collect::<Vec<_>>();
    anchors.sort();
    anchors.dedup();
    anchors
}

pub(in crate::tool::writing::novel_studio) fn character_contract_line_marks_primary(
    value: &str,
) -> bool {
    let lowered = value.to_ascii_lowercase();
    value.contains("主角")
        || value.contains("主人公")
        || value.contains("男主")
        || value.contains("女主")
        || lowered.contains("protagonist")
        || lowered.contains("main character")
}

pub(in crate::tool::writing::novel_studio) fn unrecorded_character_issues(
    manifest: &NovelProjectManifest,
    _chapter: &ChapterRecord,
    content: &str,
    known: &BTreeSet<String>,
    non_character_terms: &BTreeSet<String>,
) -> Vec<String> {
    let mut issues = malformed_single_cjk_character_candidates(content)
        .into_iter()
        .map(|candidate| {
            format!(
                "malformed single-character identity `{candidate}` appears in chapter prose; named characters must use a complete name declared by the chapter execution contract, otherwise keep the person unnamed and use only a role label"
            )
        })
        .collect::<Vec<_>>();
    let primary = contract_primary_character_anchors(manifest);
    if primary.is_empty() {
        return issues;
    }
    let evidence = content;
    let primary_mentions = primary
        .iter()
        .map(|anchor| evidence.matches(anchor.as_str()).count())
        .max()
        .unwrap_or(0);
    let trusted_character_names = known
        .iter()
        .filter(|name| stable_character_anchor_name(name).is_some())
        .cloned()
        .collect::<Vec<_>>();

    let mut counts = BTreeMap::<String, usize>::new();
    for candidate in cjk_name_like_candidates(&evidence) {
        if known.contains(&candidate) {
            continue;
        }
        if cjk_anchor_is_contaminated_by_trusted_name(&candidate, &trusted_character_names) {
            continue;
        }
        if !cjk_candidate_has_body_person_context(evidence, &candidate) {
            continue;
        }
        let occurrence_count = evidence.matches(&candidate).count().max(1);
        if candidate.chars().count() <= 2
            && !cjk_candidate_has_strong_person_identity_context(evidence, &candidate)
            && !(primary_mentions == 0 && occurrence_count >= 2)
        {
            continue;
        }
        if candidate_looks_like_address_for_known_character(&candidate, known) {
            continue;
        }
        if non_character_terms.contains(&candidate) {
            continue;
        }
        if stable_character_anchor_name(&candidate).is_none() {
            continue;
        }
        let has_strong_person_context =
            cjk_candidate_has_strong_person_identity_context(evidence, &candidate);
        if !has_strong_person_context
            && occurrence_count < 4
            && !(primary_mentions == 0 && occurrence_count >= 2)
        {
            continue;
        }
        *counts.entry(candidate).or_insert(0) += occurrence_count;
    }
    let mut ranked = counts.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    let Some((candidate, count)) = ranked.first() else {
        return issues;
    };
    if primary_mentions == 0 && *count >= 2 {
        issues.push(format!(
            "possible protagonist replacement: unrecorded character `{candidate}` dominates this chapter body while primary contract anchor `{}` appears {} times",
            primary.join(", "),
            primary_mentions
        ));
        return issues;
    }
    if *count >= 3 && *count > primary_mentions.saturating_mul(2).max(3) {
        issues.push(format!(
            "possible protagonist replacement: unrecorded character `{candidate}` dominates this chapter body while primary contract anchor `{}` appears {} times",
            primary.join(", "),
            primary_mentions
        ));
        return issues;
    }
    issues.extend(
        ranked
        .into_iter()
        .filter(|(_, count)| *count >= 3)
        .map(|(candidate, _)| {
            format!(
                "unregistered character `{candidate}` appears repeatedly in chapter prose; every named character must be declared by the chapter execution contract before prose generation"
            )
        })
    );
    issues.sort();
    issues.dedup();
    issues
}

pub(in crate::tool::writing::novel_studio) fn candidate_looks_like_address_for_known_character(
    candidate: &str,
    known: &BTreeSet<String>,
) -> bool {
    let chars = candidate.chars().collect::<Vec<_>>();
    if chars.len() < 3 {
        return false;
    }
    let Some(first) = chars.first().copied() else {
        return false;
    };
    let suffix = chars[1..].iter().collect::<String>();
    if !matches!(
        suffix.as_str(),
        "师弟"
            | "师兄"
            | "师姐"
            | "师妹"
            | "师叔"
            | "师伯"
            | "师尊"
            | "先生"
            | "老师"
            | "长老"
            | "前辈"
    ) {
        return false;
    }
    known.iter().any(|name| {
        name.chars()
            .next()
            .is_some_and(|known_first| known_first == first)
    })
}

pub(in crate::tool::writing::novel_studio) fn repair_contract_character_name_typos(
    manifest: &NovelProjectManifest,
    content: &str,
) -> String {
    if !is_chinese_language(&manifest.language) {
        return content.to_string();
    }
    if manifest.contract.is_none() {
        return content.to_string();
    }
    let anchors = manifest_character_anchors(manifest);
    if anchors.is_empty() {
        return content.to_string();
    }
    let known = known_character_like_terms(manifest, &anchors);
    let authority_view = contract_term_authority_view(manifest);
    let mut repaired = content.to_string();
    for (forbidden, canonical) in forbidden_character_name_replacements(manifest) {
        repaired = replace_forbidden_character_name_reference(
            &repaired,
            &forbidden,
            &canonical,
            &authority_view,
        );
    }
    for anchor in &anchors {
        repaired = repair_repeated_anchor_suffix(&repaired, anchor);
        repaired = repair_truncated_anchor_prefix_before_boundary(&repaired, anchor);
        for candidate in near_anchor_cjk_name_variants(&repaired, anchor) {
            if known.contains(&candidate) {
                continue;
            }
            if cjk_name_variant_is_safe_to_repair(&candidate, anchor) {
                repaired = repaired.replace(&candidate, anchor);
            }
        }
    }
    repaired
}

fn replace_forbidden_character_name_reference(
    content: &str,
    forbidden: &str,
    canonical: &str,
    authority_view: &ContractTermAuthorityView,
) -> String {
    if forbidden.trim().is_empty() || canonical.trim().is_empty() || !content.contains(forbidden) {
        return content.to_string();
    }
    let mut protected_terms = authority_view
        .non_character_terms()
        .into_iter()
        .filter(|term| term.contains(forbidden) && term != forbidden)
        .collect::<Vec<_>>();
    for suffix in [
        "市", "城", "港", "站", "山", "河", "湖", "路", "街", "区", "村", "镇", "院", "馆", "塔",
        "公司", "集团", "机构", "学院", "学校", "宗门", "系统", "协议", "计划", "工程", "装置",
    ] {
        let compound = format!("{forbidden}{suffix}");
        if content.contains(&compound) {
            protected_terms.push(compound);
        }
    }
    protected_terms.sort_by_key(|term| std::cmp::Reverse(term.chars().count()));
    protected_terms.dedup();

    let mut protected = content.to_string();
    let mut restorations = Vec::new();
    for (index, term) in protected_terms.into_iter().enumerate() {
        let sentinel = format!("\u{e000}protected-character-term-{index}\u{e001}");
        if protected.contains(&term) {
            protected = protected.replace(&term, &sentinel);
            restorations.push((sentinel, term));
        }
    }
    let reference_match = if forbidden.chars().count() == 2 && forbidden.chars().all(is_cjk_unified)
    {
        crate::tool::writing::typed_contract_gate::CharacterReferenceMatch::DerivedShortIdentity
    } else {
        crate::tool::writing::typed_contract_gate::CharacterReferenceMatch::AuthorityAnchor
    };
    protected = crate::tool::writing::typed_contract_gate::replace_character_anchor_reference(
        &protected,
        forbidden,
        canonical,
        reference_match,
    );
    for (sentinel, term) in restorations {
        protected = protected.replace(&sentinel, &term);
    }
    protected
}

fn forbidden_character_name_replacements(manifest: &NovelProjectManifest) -> Vec<(String, String)> {
    let mut owners = BTreeMap::<String, BTreeSet<String>>::new();
    for character in &manifest.character_ledger {
        let canonical = character.canonical_name.trim();
        if canonical.is_empty() {
            continue;
        }
        for forbidden in &character.forbidden_renames {
            let forbidden = forbidden.trim();
            if !forbidden.is_empty() && forbidden != canonical {
                owners
                    .entry(forbidden.to_string())
                    .or_default()
                    .insert(canonical.to_string());
            }
        }
    }
    if let Some(authority) = manifest
        .contract
        .as_ref()
        .and_then(|contract| contract.authority_contract.as_ref())
    {
        for character in &authority.characters {
            let canonical = character.canonical_name.trim();
            if canonical.is_empty() {
                continue;
            }
            for forbidden in &character.previous_names {
                let forbidden = forbidden.trim();
                if !forbidden.is_empty() && forbidden != canonical {
                    owners
                        .entry(forbidden.to_string())
                        .or_default()
                        .insert(canonical.to_string());
                }
            }
        }
    }
    owners
        .into_iter()
        .filter_map(|(forbidden, owners)| {
            (owners.len() == 1).then(|| (forbidden, owners.into_iter().next().unwrap_or_default()))
        })
        .collect()
}

pub(in crate::tool::writing::novel_studio) fn clean_contract_character_name_typos(
    manifest: &NovelProjectManifest,
    values: Vec<String>,
) -> Vec<String> {
    values
        .into_iter()
        .map(|value| repair_contract_character_name_typos(manifest, &value))
        .filter(|value| !value.trim().is_empty())
        .collect()
}

pub(in crate::tool::writing::novel_studio) fn repair_repeated_anchor_suffix(
    content: &str,
    anchor: &str,
) -> String {
    let Some(last) = anchor.chars().last() else {
        return content.to_string();
    };
    let repeated = format!("{anchor}{last}");
    content.replace(&repeated, anchor)
}

pub(in crate::tool::writing::novel_studio) fn repair_truncated_anchor_prefix_before_boundary(
    content: &str,
    anchor: &str,
) -> String {
    let anchor_chars = anchor.chars().collect::<Vec<_>>();
    if anchor_chars.len() < 3 || !anchor_chars.iter().all(|ch| is_cjk_unified(*ch)) {
        return content.to_string();
    }
    let repeated_final_anchor = anchor_chars
        .get(anchor_chars.len().saturating_sub(1))
        .zip(anchor_chars.get(anchor_chars.len().saturating_sub(2)))
        .is_some_and(|(last, previous)| last == previous);
    if !content.contains(anchor) && !repeated_final_anchor {
        return content.to_string();
    }
    let prefix = anchor_chars[..anchor_chars.len() - 1]
        .iter()
        .collect::<String>();
    let chars = content.chars().collect::<Vec<_>>();
    let prefix_chars = prefix.chars().collect::<Vec<_>>();
    let mut out = String::new();
    let mut index = 0usize;
    while index < chars.len() {
        let end = index + prefix_chars.len();
        if end <= chars.len()
            && chars[index..end] == prefix_chars
            && index
                .checked_sub(1)
                .and_then(|idx| chars.get(idx))
                .copied()
                .is_none_or(|ch| !is_cjk_unified(ch))
            && chars.get(end).copied().is_some_and(|ch| {
                is_name_boundary_after_short_cjk_name(ch)
                    || is_high_confidence_truncated_name_follow_char(ch)
            })
        {
            out.push_str(anchor);
            index = end;
            continue;
        }
        out.push(chars[index]);
        index += 1;
    }
    out
}

pub(in crate::tool::writing::novel_studio) fn is_high_confidence_truncated_name_follow_char(
    ch: char,
) -> bool {
    matches!(
        ch,
        '微' | '一'
            | '皱'
            | '怔'
            | '抬'
            | '低'
            | '回'
            | '转'
            | '看'
            | '望'
            | '听'
            | '说'
            | '问'
            | '答'
            | '笑'
            | '沉'
            | '停'
            | '站'
            | '坐'
            | '走'
            | '伸'
            | '握'
            | '把'
            | '将'
            | '被'
            | '在'
            | '向'
            | '从'
    )
}

pub(in crate::tool::writing::novel_studio) fn cjk_name_variant_is_safe_to_repair(
    candidate: &str,
    anchor: &str,
) -> bool {
    let candidate_chars = candidate.chars().collect::<Vec<_>>();
    let anchor_chars = anchor.chars().collect::<Vec<_>>();
    let len = anchor_chars.len();
    if !(3..=4).contains(&len)
        || !(candidate_chars.len() == len || candidate_chars.len() == len + 1)
        || !candidate_chars.iter().all(|ch| is_cjk_unified(*ch))
        || !anchor_chars.iter().all(|ch| is_cjk_unified(*ch))
    {
        return false;
    }
    if candidate_is_full_anchor_plus_predicate(&candidate_chars, &anchor_chars) {
        return false;
    }
    if candidate_chars.len() == len + 1 {
        return candidate_chars.iter().enumerate().any(|(skip, _)| {
            candidate_chars
                .iter()
                .enumerate()
                .filter_map(|(index, ch)| (index != skip).then_some(*ch))
                .eq(anchor_chars.iter().copied())
        });
    }
    if cjk_name_is_adjacent_transposition(&candidate_chars, &anchor_chars) {
        return true;
    }
    let distance = candidate_chars
        .iter()
        .zip(anchor_chars.iter())
        .filter(|(left, right)| left != right)
        .count();
    distance == 1
        && candidate_chars[..len.saturating_sub(1)] == anchor_chars[..len.saturating_sub(1)]
}

fn candidate_is_full_anchor_plus_predicate(candidate: &[char], anchor: &[char]) -> bool {
    candidate.len() == anchor.len() + 1
        && candidate[..anchor.len()] == *anchor
        && candidate
            .last()
            .copied()
            .is_some_and(|ch| is_name_predicate_after_short_cjk_name(ch) || matches!(ch, '在'))
}

pub(in crate::tool::writing::novel_studio) fn known_character_like_terms(
    manifest: &NovelProjectManifest,
    anchors: &[String],
) -> BTreeSet<String> {
    let mut known = anchors.iter().cloned().collect::<BTreeSet<_>>();
    let authority_view = contract_term_authority_view(manifest);
    known.extend(authority_view.character_names.clone());
    if let Some(contract) = &manifest.contract {
        for value in &contract.characters {
            for candidate in cjk_name_like_candidates(value) {
                known.insert(candidate);
            }
        }
    }
    for truth in &manifest.truth_files {
        for candidate in cjk_name_like_candidates(&truth.section) {
            known.insert(candidate);
        }
    }
    known
}

pub(in crate::tool::writing::novel_studio) fn near_anchor_cjk_name_variants(
    content: &str,
    anchor: &str,
) -> BTreeSet<String> {
    let anchor_chars = anchor.chars().collect::<Vec<_>>();
    let anchor_len = anchor_chars.len();
    if !(3..=4).contains(&anchor_len) || !anchor_chars.iter().all(|ch| is_cjk_unified(*ch)) {
        return BTreeSet::new();
    }

    let chars = content.chars().collect::<Vec<_>>();
    let mut out = BTreeSet::new();
    for index in 0..chars.len() {
        for candidate_len in [anchor_len, anchor_len + 1] {
            if index + candidate_len > chars.len() {
                continue;
            }
            let candidate = &chars[index..index + candidate_len];
            if !candidate.iter().all(|ch| is_cjk_unified(*ch)) {
                continue;
            }
            if candidate == anchor_chars.as_slice() {
                continue;
            }
            if candidate_len == anchor_len
                && candidate[..anchor_len.saturating_sub(1)]
                    == anchor_chars[..anchor_len.saturating_sub(1)]
                && chars.get(index + candidate_len) == anchor_chars.last()
            {
                // The next character completes an anchor with one inserted glyph
                // (for example `黎启落洄`). Let the anchor_len + 1 branch own it;
                // repairing the shorter prefix first would duplicate the suffix.
                continue;
            }
            let distance = if candidate_len == anchor_len
                && cjk_name_is_adjacent_transposition(candidate, &anchor_chars)
            {
                1
            } else if candidate_len == anchor_len {
                candidate
                    .iter()
                    .zip(anchor_chars.iter())
                    .filter(|(left, right)| left != right)
                    .count()
            } else if candidate.iter().enumerate().any(|(skip, _)| {
                candidate
                    .iter()
                    .enumerate()
                    .filter_map(|(idx, ch)| (idx != skip).then_some(*ch))
                    .eq(anchor_chars.iter().copied())
            }) {
                1
            } else {
                usize::MAX
            };
            if distance != 1 {
                continue;
            }
            let candidate_text = candidate.iter().collect::<String>();
            if short_cjk_variant_looks_like_common_phrase(&candidate_text) {
                continue;
            }
            if candidate_tail_continues_common_cjk_word(&chars, index, candidate_len) {
                continue;
            }
            if !cjk_candidate_has_name_like_boundary(&chars, index, candidate_len)
                && !cjk_name_candidate_tail_has_person_action_context(
                    &chars[index + candidate_len..],
                )
            {
                continue;
            }

            let shares_stable_prefix = if candidate_len == anchor_len + 1 {
                candidate.first() == anchor_chars.first() && candidate.last() == anchor_chars.last()
            } else if cjk_name_is_adjacent_transposition(candidate, &anchor_chars) {
                true
            } else if anchor_len == 2 {
                candidate.first() == anchor_chars.first()
                    && !candidate
                        .get(1)
                        .copied()
                        .is_some_and(short_cjk_name_variant_tail_looks_like_grammar)
                    && cjk_candidate_has_person_context(content, &candidate_text)
            } else {
                candidate[..anchor_len - 1] == anchor_chars[..anchor_len - 1]
            };
            if shares_stable_prefix {
                out.insert(candidate_text);
            }
        }
    }
    out
}

fn candidate_tail_continues_common_cjk_word(chars: &[char], start: usize, len: usize) -> bool {
    let Some(last) = chars.get(start + len.saturating_sub(1)).copied() else {
        return false;
    };
    let Some(next) = chars.get(start + len).copied() else {
        return false;
    };
    candidate_tail_and_next_form_common_cjk_word(last, next)
}

fn cjk_name_is_adjacent_transposition(candidate: &[char], anchor: &[char]) -> bool {
    if candidate.len() != anchor.len() || candidate.len() < 3 || candidate == anchor {
        return false;
    }
    let mismatches = candidate
        .iter()
        .zip(anchor.iter())
        .enumerate()
        .filter_map(|(index, (left, right))| (left != right).then_some(index))
        .collect::<Vec<_>>();
    let [left, right] = mismatches.as_slice() else {
        return false;
    };
    *right == left.saturating_add(1)
        && candidate[*left] == anchor[*right]
        && candidate[*right] == anchor[*left]
}

pub(in crate::tool::writing::novel_studio) fn cjk_candidate_has_name_like_boundary(
    chars: &[char],
    start: usize,
    len: usize,
) -> bool {
    let prev = start.checked_sub(1).and_then(|idx| chars.get(idx)).copied();
    let next = chars.get(start + len).copied();
    if let (Some(last), Some(next)) = (chars.get(start + len.saturating_sub(1)).copied(), next) {
        if candidate_tail_and_next_form_common_cjk_word(last, next) {
            return false;
        }
    }
    let prev_ok = prev.is_none_or(|ch| !is_cjk_unified(ch));
    let next_ok = next.is_none_or(|ch| {
        !is_cjk_unified(ch)
            || is_name_particle_after_short_cjk_name(ch)
            || is_name_predicate_after_short_cjk_name(ch)
    });
    prev_ok && next_ok
}

pub(in crate::tool::writing::novel_studio) fn candidate_tail_and_next_form_common_cjk_word(
    last: char,
    next: char,
) -> bool {
    matches!(
        (last, next),
        ('感', '知')
            | ('意', '识')
            | ('发', '现')
            | ('听', '见')
            | ('看', '见')
            | ('察', '觉')
            | ('觉', '得')
            | ('想', '到')
            | ('知', '道')
            | ('明', '白')
    )
}

pub(in crate::tool::writing::novel_studio) fn short_cjk_name_variant_tail_looks_like_grammar(
    ch: char,
) -> bool {
    is_name_particle_after_short_cjk_name(ch)
        || is_name_predicate_after_short_cjk_name(ch)
        || matches!(
            ch,
            '到' | '地' | '得' | '着' | '过' | '了' | '也' | '都' | '却' | '便' | '又'
        )
}

pub(in crate::tool::writing::novel_studio) fn short_cjk_variant_looks_like_common_phrase(
    candidate: &str,
) -> bool {
    let chars = candidate.chars().collect::<Vec<_>>();
    if chars.len() != 2 {
        return false;
    }
    if two_char_cjk_time_or_state_phrase(&chars) {
        return true;
    }
    matches!(
        chars[1],
        '间' | '中' | '里' | '内' | '外' | '前' | '后' | '上' | '下' | '边' | '旁' | '侧'
    )
}

pub(in crate::tool::writing::novel_studio) fn two_char_cjk_time_or_state_phrase(
    chars: &[char],
) -> bool {
    chars.len() == 2
        && matches!(chars[0], '时' | '期' | '瞬' | '片')
        && matches!(
            chars[1],
            '刻' | '间' | '候' | '光' | '序' | '期' | '点' | '段' | '存' | '在'
        )
}

pub(in crate::tool::writing::novel_studio) fn is_name_boundary_after_short_cjk_name(
    ch: char,
) -> bool {
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
                | '"'
                | '\''
                | '“'
                | '”'
                | '‘'
                | '’'
                | '、'
        )
}

pub(in crate::tool::writing::novel_studio) fn is_name_particle_after_short_cjk_name(
    ch: char,
) -> bool {
    matches!(
        ch,
        '的' | '与' | '和' | '在' | '向' | '把' | '被' | '从' | '将' | '对' | '给' | '为'
    )
}

pub(in crate::tool::writing::novel_studio) fn is_name_predicate_after_short_cjk_name(
    ch: char,
) -> bool {
    matches!(
        ch,
        '说' | '问'
            | '答'
            | '喊'
            | '叫'
            | '走'
            | '跑'
            | '退'
            | '停'
            | '站'
            | '坐'
            | '看'
            | '望'
            | '听'
            | '想'
            | '知'
            | '握'
            | '追'
            | '压'
            | '提'
            | '抬'
            | '低'
            | '转'
            | '推'
            | '拉'
            | '伸'
            | '冲'
            | '踏'
            | '入'
            | '出'
            | '回'
            | '来'
            | '去'
            | '醒'
            | '笑'
            | '哭'
            | '怒'
            | '惊'
            | '没'
    )
}

pub(in crate::tool::writing::novel_studio) fn cjk_name_like_candidates(
    content: &str,
) -> BTreeSet<String> {
    const SURNAME_CHARS: &str = "赵钱孙李周吴郑王冯陈褚卫蒋沈韩杨朱秦尤许何吕施张孔曹严华金魏陶姜谢邹喻柏水窦章云苏潘葛奚范彭郎鲁韦昌马苗凤花方俞任袁柳鲍史唐费廉岑薛雷贺倪汤滕殷罗毕郝邬安常乐于时傅皮卞齐康伍余元卜顾孟平黄和穆萧尹姚邵湛汪祁毛禹狄米贝明臧计伏成戴谈宋庞熊纪舒屈项祝董梁杜阮蓝闵席季麻强贾路娄危江童颜郭梅盛林刁钟徐邱骆高夏蔡田胡凌霍虞万支柯昝管卢莫经房裘缪干解应宗丁宣邓郁单杭洪包诸左石崔吉龚程嵇邢裴陆荣翁荀羊於惠甄曲封储靳汲邴糜松井段富巫乌焦巴弓牧隗山谷车侯宓蓬全郗班仰秋仲伊宫宁仇栾暴甘斜厉戎祖武符刘景詹龙叶幸司韶郜黎蓟薄印宿白怀蒲台从鄂索咸籍赖卓蔺屠蒙池乔阴胥能苍双闻莘党翟谭贡劳逄姬申扶堵冉宰郦雍璩桑桂濮牛寿通边扈燕冀郏浦尚农温别庄晏柴瞿阎充慕连茹习宦艾鱼容向古易慎戈廖庾终暨居衡步都耿满弘匡国文寇广禄阙东欧利师巩聂关荆司马欧阳上官诸葛东方独孤南宫";
    let chars = content.chars().collect::<Vec<_>>();
    let mut candidates = BTreeSet::new();
    let mut index = 0usize;
    while index < chars.len() {
        if !SURNAME_CHARS.contains(chars[index]) {
            index += 1;
            continue;
        }
        for len in [2usize, 3, 4] {
            if index + len > chars.len() {
                continue;
            }
            let slice = &chars[index..index + len];
            if slice.iter().all(|ch| is_cjk_unified(*ch)) {
                if !cjk_name_candidate_has_extraction_boundary(&chars, index, len) {
                    continue;
                }
                if len > 2
                    && slice[1..]
                        .iter()
                        .any(|ch| is_name_particle_after_short_cjk_name(*ch))
                {
                    continue;
                }
                let next = chars.get(index + len).copied();
                if next.is_some_and(is_cjk_unified)
                    && len < 4
                    && !next.is_some_and(is_name_particle_after_short_cjk_name)
                    && !next.is_some_and(is_name_predicate_after_short_cjk_name)
                    && !cjk_name_candidate_tail_has_person_action_context(&chars[index + len..])
                {
                    continue;
                }
                candidates.insert(slice.iter().collect::<String>());
            }
        }
        index += 1;
    }
    candidates
}

fn malformed_single_cjk_character_candidates(content: &str) -> BTreeSet<String> {
    const NON_NAME_SINGLE_CJK: &str =
        "我你您他她它咱谁人这那其某各每本该有无是在把被将与和或而但也都又便却就才仍还只更最很太真若如因由从向对给为之者们";
    let chars = content.chars().collect::<Vec<_>>();
    chars
        .iter()
        .enumerate()
        .filter_map(|(index, ch)| {
            (is_cjk_unified(*ch)
                && !NON_NAME_SINGLE_CJK.contains(*ch)
                && single_cjk_identity_occurrence_is_explicit(&chars, index))
            .then(|| ch.to_string())
        })
        .collect()
}

fn single_cjk_identity_occurrence_is_explicit(chars: &[char], index: usize) -> bool {
    let before = chars[..index]
        .iter()
        .rev()
        .take(24)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    let after = chars.get(index + 1).copied();
    let has_terminal = after.is_none()
        || after.is_some_and(is_name_boundary_after_short_cjk_name)
        || after == Some('的');
    if !has_terminal {
        return false;
    }

    if ["名叫", "称作", "自称"]
        .iter()
        .any(|marker| before.ends_with(marker))
    {
        return true;
    }
    if before.ends_with("叫") {
        if after != Some('的') {
            return true;
        }
        let named_person_tail = chars[index + 2..].iter().take(6).collect::<String>();
        if [
            "人", "人物", "家伙", "男人", "女人", "少年", "少女", "老人", "孩子", "选手", "队员",
            "队友", "同伴", "对手", "角色",
        ]
        .iter()
        .any(|referent| named_person_tail.starts_with(referent))
        {
            return true;
        }
    }

    let separated_before = index == 0
        || chars
            .get(index.saturating_sub(1))
            .copied()
            .is_some_and(is_name_boundary_after_short_cjk_name);
    if !separated_before {
        return false;
    }
    let role_before_apposition = before
        .trim_end_matches(is_name_boundary_after_short_cjk_name)
        .trim_end();
    [
        "主角",
        "主人公",
        "男主",
        "女主",
        "反派",
        "导师",
        "同伴",
        "盟友",
        "队友",
        "对手",
        "队长",
        "选手",
        "裁判",
        "债主",
        "老板",
        "医生",
        "警察",
        "司机",
        "记者",
        "男人",
        "女人",
        "少年",
        "少女",
        "老人",
        "朋友",
        "父亲",
        "母亲",
        "丈夫",
        "妻子",
        "哥哥",
        "姐姐",
        "弟弟",
        "妹妹",
    ]
    .iter()
    .any(|role| role_before_apposition.ends_with(role))
}

fn cjk_name_candidate_has_extraction_boundary(chars: &[char], start: usize, len: usize) -> bool {
    cjk_candidate_has_name_like_boundary(chars, start, len)
        || cjk_candidate_has_identity_prefix(chars, start)
}

fn cjk_candidate_has_identity_prefix(chars: &[char], start: usize) -> bool {
    let prefix = chars[..start]
        .iter()
        .rev()
        .take(6)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    [
        "姓名",
        "角色",
        "人物",
        "主角",
        "主人公",
        "男主",
        "女主",
        "反派",
        "妹妹",
        "姐姐",
        "哥哥",
        "弟弟",
        "父亲",
        "母亲",
        "女儿",
        "儿子",
        "妻子",
        "丈夫",
        "亲人",
        "家人",
        "失踪者",
        "当事人",
        "名叫",
        "叫",
        "称作",
        "自称",
    ]
    .iter()
    .any(|marker| prefix.ends_with(marker))
}

pub(in crate::tool::writing::novel_studio) fn contract_character_anchors(
    contract: &StoryContract,
) -> Vec<String> {
    let mut anchors = contract
        .characters
        .iter()
        .filter_map(|value| stable_anchor_token(value).map(ToString::to_string))
        .filter(|value| value.chars().count() >= 2)
        .collect::<Vec<_>>();
    anchors.sort();
    anchors.dedup();
    anchors
}

pub(in crate::tool::writing::novel_studio) fn contract_character_anchor_set(
    manifest: &NovelProjectManifest,
) -> Vec<String> {
    manifest
        .contract
        .as_ref()
        .map(contract_character_anchors)
        .unwrap_or_default()
        .into_iter()
        .filter(|anchor| character_anchor_name_for_language(anchor, &manifest.language).is_some())
        .collect()
}

pub(in crate::tool::writing::novel_studio) fn manifest_character_anchors(
    manifest: &NovelProjectManifest,
) -> Vec<String> {
    let primary = contract_primary_character_anchors(manifest);
    let mut anchors = explicit_manifest_character_anchors(manifest);
    if is_chinese_language(&manifest.language) {
        anchors.retain(|anchor| stable_character_anchor_name(anchor).is_some());
    } else {
        anchors.retain(|anchor| {
            let trimmed = anchor.trim();
            !trimmed.is_empty() && trimmed.chars().count() <= 80
        });
    }
    role_prioritize_character_anchors(primary, anchors)
}

pub(in crate::tool::writing::novel_studio) fn explicit_manifest_character_anchors(
    manifest: &NovelProjectManifest,
) -> Vec<String> {
    let primary = contract_primary_character_anchors(manifest);
    let authority_view = contract_term_authority_view(manifest);
    let trusted_contract_anchors = contract_character_anchor_set(manifest);
    let mut anchors = authority_view
        .character_names
        .into_iter()
        .filter(|anchor| character_anchor_name_for_language(anchor, &manifest.language).is_some())
        .collect::<Vec<_>>();
    anchors.extend(trusted_contract_anchors.clone());
    if let Some(bible) = manifest.story_bible.as_ref() {
        anchors.extend(
            bible
                .character_ledger
                .iter()
                .filter_map(|character| {
                    character_anchor_name_for_language(&character.name, &manifest.language)
                })
                .filter(|name| {
                    !is_chinese_language(&manifest.language)
                        || !cjk_anchor_is_contaminated_by_trusted_name(
                            name,
                            &trusted_contract_anchors,
                        )
                })
                .map(ToString::to_string),
        );
    }
    if is_chinese_language(&manifest.language) {
        anchors.retain(|anchor| stable_character_anchor_name(anchor).is_some());
    } else {
        anchors.retain(|anchor| {
            let trimmed = anchor.trim();
            !trimmed.is_empty() && trimmed.chars().count() <= 80
        });
    }
    role_prioritize_character_anchors(primary, anchors)
}

fn role_prioritize_character_anchors(primary: Vec<String>, anchors: Vec<String>) -> Vec<String> {
    let primary_set = primary.iter().cloned().collect::<BTreeSet<_>>();
    let mut prioritized = primary;
    let mut rest = anchors
        .into_iter()
        .filter(|anchor| !primary_set.contains(anchor))
        .collect::<Vec<_>>();
    prioritized.sort();
    prioritized.dedup();
    rest.sort();
    rest.dedup();
    prioritized.extend(rest);
    prioritized
}

pub(in crate::tool::writing::novel_studio) fn cjk_anchor_is_contaminated_by_trusted_name(
    candidate: &str,
    trusted: &[String],
) -> bool {
    trusted.iter().any(|anchor| {
        let candidate_len = candidate.chars().count();
        let anchor_len = anchor.chars().count();
        candidate != anchor
            && ((candidate.starts_with(anchor) && candidate_len <= anchor_len + 2)
                || (anchor.starts_with(candidate) && anchor_len <= candidate_len + 2))
    })
}

pub(in crate::tool::writing::novel_studio) fn cjk_candidate_has_person_context(
    text: &str,
    candidate: &str,
) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() || candidate.trim().is_empty() {
        return false;
    }
    let lowered = trimmed.to_ascii_lowercase();
    if (trimmed.contains("name:") || trimmed.contains("姓名") || trimmed.contains("角色"))
        && trimmed.contains(candidate)
    {
        return true;
    }
    let role_markers = [
        "主角",
        "主人公",
        "男主",
        "女主",
        "反派",
        "角色",
        "人物",
        "导师",
        "同伴",
        "盟友",
        "protagonist",
        "antagonist",
        "character",
        "mentor",
        "ally",
    ];
    if role_markers
        .iter()
        .any(|marker| trimmed.contains(marker) || lowered.contains(marker))
        && trimmed.contains(candidate)
    {
        return true;
    }
    let chars = trimmed.chars().collect::<Vec<_>>();
    let candidate_chars = candidate.chars().collect::<Vec<_>>();
    let candidate_len = candidate_chars.len();
    for index in 0..chars.len() {
        if index + candidate_len > chars.len()
            || chars[index..index + candidate_len] != candidate_chars
        {
            continue;
        }
        let before = index.checked_sub(1).and_then(|idx| chars.get(idx)).copied();
        let after = chars.get(index + candidate_len).copied();
        if before.is_some_and(|ch| matches!(ch, '‘' | '“' | '"' | '\'' | '《'))
            || after.is_some_and(|ch| matches!(ch, '’' | '”' | '"' | '\'' | '》'))
        {
            continue;
        }
        if after.is_some_and(is_name_predicate_after_short_cjk_name)
            || after.is_some_and(|ch| matches!(ch, '重' | '醒' | '逃' | '战' | '在' | '与' | '和'))
        {
            return true;
        }
        let after_tail = chars[index + candidate_len..]
            .iter()
            .take(4)
            .collect::<String>();
        if after_tail.starts_with("意识")
            || after_tail.starts_with("发现")
            || after_tail.starts_with("决定")
            || after_tail.starts_with("知道")
            || after_tail.starts_with("明白")
            || after_tail.starts_with("成为")
            || after_tail.starts_with("重生")
        {
            return true;
        }
        if cjk_name_candidate_tail_has_person_action_context(&chars[index + candidate_len..]) {
            return true;
        }
        let before_head = chars[..index]
            .iter()
            .rev()
            .take(4)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<String>();
        if before_head.ends_with("主角")
            || before_head.ends_with("角色")
            || before_head.ends_with("人物")
            || before_head.ends_with("同伴")
            || before_head.ends_with("反派")
        {
            return true;
        }
    }
    false
}

pub(in crate::tool::writing::novel_studio) fn cjk_candidate_has_body_person_context(
    text: &str,
    candidate: &str,
) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() || candidate.trim().is_empty() || !trimmed.contains(candidate) {
        return false;
    }
    let chars = trimmed.chars().collect::<Vec<_>>();
    let candidate_chars = candidate.chars().collect::<Vec<_>>();
    let candidate_len = candidate_chars.len();
    for index in 0..chars.len() {
        if index + candidate_len > chars.len()
            || chars[index..index + candidate_len] != candidate_chars
        {
            continue;
        }
        let before = index.checked_sub(1).and_then(|idx| chars.get(idx)).copied();
        let after = chars.get(index + candidate_len).copied();
        if before.is_some_and(|ch| matches!(ch, '‘' | '“' | '"' | '\'' | '《'))
            || after.is_some_and(|ch| matches!(ch, '’' | '”' | '"' | '\'' | '》'))
        {
            continue;
        }
        let local_start = index.saturating_sub(8);
        let local_end = (index + candidate_len + 8).min(chars.len());
        let local_window = chars[local_start..local_end].iter().collect::<String>();
        if local_window.contains("姓名")
            || local_window.contains("角色")
            || local_window.contains("人物")
            || local_window.contains("主角")
            || local_window.to_ascii_lowercase().contains("character")
        {
            return true;
        }
        if after.is_some_and(is_name_predicate_after_short_cjk_name)
            || after.is_some_and(|ch| matches!(ch, '重' | '醒' | '逃' | '战' | '在' | '与' | '和'))
        {
            return true;
        }
        let after_tail = chars[index + candidate_len..]
            .iter()
            .take(4)
            .collect::<String>();
        if after_tail.starts_with("意识")
            || after_tail.starts_with("发现")
            || after_tail.starts_with("决定")
            || after_tail.starts_with("知道")
            || after_tail.starts_with("明白")
            || after_tail.starts_with("成为")
            || after_tail.starts_with("重生")
        {
            return true;
        }
        if cjk_name_candidate_tail_has_person_action_context(&chars[index + candidate_len..]) {
            return true;
        }
        let before_head = chars[..index]
            .iter()
            .rev()
            .take(6)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<String>();
        if before_head.ends_with("主角")
            || before_head.ends_with("角色")
            || before_head.ends_with("人物")
            || before_head.ends_with("同伴")
            || before_head.ends_with("反派")
            || before_head.ends_with("名叫")
            || before_head.ends_with("叫")
        {
            return true;
        }
    }
    false
}

pub(in crate::tool::writing::novel_studio) fn cjk_candidate_has_strong_person_identity_context(
    text: &str,
    candidate: &str,
) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() || candidate.trim().is_empty() || !trimmed.contains(candidate) {
        return false;
    }
    let chars = trimmed.chars().collect::<Vec<_>>();
    let candidate_chars = candidate.chars().collect::<Vec<_>>();
    let candidate_len = candidate_chars.len();
    for index in 0..chars.len() {
        if index + candidate_len > chars.len()
            || chars[index..index + candidate_len] != candidate_chars
        {
            continue;
        }
        let after = chars.get(index + candidate_len).copied();
        if after.is_some_and(|ch| matches!(ch, '说' | '问' | '答' | '喊' | '叫')) {
            return true;
        }
        let before_head = chars[..index]
            .iter()
            .rev()
            .take(12)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<String>();
        let before_head = before_head.trim_end();
        if [
            "姓名",
            "姓名：",
            "姓名:",
            "名叫",
            "叫作",
            "角色",
            "人物",
            "主角",
            "主人公",
            "男主",
            "女主",
            "反派",
            "导师",
            "同伴",
            "盟友",
            "name:",
            "character:",
        ]
        .iter()
        .any(|marker| before_head.ends_with(marker))
        {
            return true;
        }
        let after_tail = chars[index + candidate_len..]
            .iter()
            .take(12)
            .collect::<String>();
        let after_tail = after_tail.trim_start();
        if [
            "是主角",
            "为主角",
            "是主人公",
            "为主人公",
            "是男主",
            "是女主",
            "是反派",
            "是导师",
            "是同伴",
            "是盟友",
            "; role:",
            "；role:",
            ";role:",
            "；角色:",
        ]
        .iter()
        .any(|marker| after_tail.starts_with(marker))
        {
            return true;
        }
    }
    false
}

pub(in crate::tool::writing::novel_studio) fn cjk_name_candidate_tail_has_person_action_context(
    tail: &[char],
) -> bool {
    let mut index = 0usize;
    while let Some(ch) = tail.get(index).copied() {
        if ch.is_whitespace()
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
                    | '、'
                    | '…'
            )
        {
            index += 1;
            continue;
        }
        break;
    }
    while let Some(ch) = tail.get(index).copied() {
        if matches!(
            ch,
            '快' | '步' | '先' | '立' | '忽' | '猛' | '低' | '高' | '再' | '又'
        ) {
            index += 1;
            continue;
        }
        break;
    }
    tail.get(index)
        .copied()
        .is_some_and(is_name_predicate_after_short_cjk_name)
}

pub(in crate::tool::writing::novel_studio) fn stable_character_anchor_name(
    value: &str,
) -> Option<&str> {
    let trimmed = value.trim();
    let char_count = trimmed.chars().count();
    if !(2..=4).contains(&char_count) || !trimmed.chars().all(is_cjk_unified) {
        return None;
    }
    if matches!(
        trimmed,
        "主角"
            | "少女"
            | "少年"
            | "老人"
            | "敌人"
            | "坏人"
            | "老师"
            | "老板"
            | "队长"
            | "执事"
            | "护卫"
            | "同伴"
            | "左臂"
            | "右臂"
            | "能力"
            | "权力"
            | "战力"
            | "修为"
            | "灵能"
            | "体力"
            | "精力"
            | "能用"
            | "能地"
            | "系统"
            | "城市"
            | "世界"
            | "关系"
            | "线索"
            | "伏笔"
            | "主线"
            | "支线"
            | "剧情"
            | "章节"
            | "标题"
            | "大纲"
            | "状态"
            | "姓名"
            | "名字"
            | "名称"
            | "角色名"
            | "主角名"
            | "主角姓名"
            | "主人公名"
            | "主人公姓名"
            | "男主姓名"
            | "女主姓名"
            | "反派姓名"
            | "对手姓名"
            | "环境"
            | "身份"
            | "能量"
            | "余量"
            | "数量"
            | "重量"
            | "容量"
            | "严酷"
            | "因为"
    ) {
        return None;
    }
    if trimmed.starts_with("因为") || (trimmed.ends_with("主角") && trimmed != "主角") {
        return None;
    }
    if char_count == 2
        && trimmed
            .chars()
            .last()
            .is_some_and(|ch| matches!(ch, '量' | '度' | '性' | '感' | '率' | '力' | '能'))
    {
        return None;
    }
    if char_count == 2
        && trimmed
            .chars()
            .next()
            .is_some_and(|ch| matches!(ch, '能' | '可' | '会' | '要' | '将' | '被' | '已' | '再'))
    {
        return None;
    }
    let chars = trimmed.chars().collect::<Vec<_>>();
    if two_char_cjk_time_or_state_phrase(&chars) {
        return None;
    }
    let phrase_noise = [
        "关系", "变化", "线索", "进展", "通过", "意识", "感知", "使用", "利用", "发现", "决定",
        "进入", "回到", "离开", "状态", "环境", "位置", "物理", "逻辑", "核心", "规则", "主题",
        "前提", "身份", "真相", "资源", "来源", "伏笔", "阶段", "世界", "成长", "宣布", "增加",
        "减少", "能力", "权力", "战力", "体力", "精力", "老板", "护卫", "执事", "监督", "队长",
        "时刻", "时间", "时候", "存在", "因为",
    ];
    if phrase_noise.iter().any(|term| trimmed.contains(term)) {
        return None;
    }
    Some(trimmed)
}

fn character_anchor_name_for_language<'a>(value: &'a str, language: &str) -> Option<&'a str> {
    if is_chinese_language(language) {
        return stable_character_anchor_name(value);
    }
    let trimmed = value.trim();
    (!trimmed.is_empty() && trimmed.chars().count() <= 80).then_some(trimmed)
}

pub(in crate::tool::writing::novel_studio) fn stable_anchor_token(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    if trimmed.chars().count() < 2 {
        return None;
    }
    if let Some(name) = labeled_name_anchor(trimmed) {
        return Some(name);
    }
    let token = trimmed
        .split(|ch: char| ch.is_whitespace() || matches!(ch, ':' | '：' | ',' | '，' | ';' | '；'))
        .find(|part| part.chars().count() >= 2)
        .unwrap_or(trimmed);
    Some(token)
}

pub(in crate::tool::writing::novel_studio) fn labeled_name_anchor(value: &str) -> Option<&str> {
    for field in value.split(|ch| matches!(ch, ';' | '；' | '\n' | '\r')) {
        for label in [
            "name",
            "canonical_name",
            "姓名",
            "名字",
            "名称",
            "角色名",
            "角色",
            "主角名",
            "主角姓名",
            "主角名字",
            "主人公名",
            "主人公姓名",
            "主人公名字",
            "男主姓名",
            "女主姓名",
            "反派姓名",
            "对手姓名",
            "关键配角姓名",
        ] {
            for separator in [":", "："] {
                let prefix = format!("{label}{separator}");
                if let Some(rest) = field.trim_start().strip_prefix(&prefix) {
                    let candidate = rest
                        .split(|ch: char| {
                            ch.is_whitespace() || matches!(ch, ',' | '，' | '|' | '\t')
                        })
                        .find(|part| part.chars().count() >= 2)?;
                    return Some(candidate.trim());
                }
            }
        }
    }
    None
}
