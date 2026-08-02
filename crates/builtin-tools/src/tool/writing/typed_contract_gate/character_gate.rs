use super::*;
use crate::tool::writing::creation_contract::issue::{
    user_story_semantic_issue_kind, ContractIssue, ContractIssueDisposition, ContractIssueEvidence,
    ContractIssueKind, ContractIssueList,
};

pub(super) fn validate_superseded_character_name_residue(
    contract: &NovelCreationContract,
    issues: &mut ContractIssueList,
) {
    let mut previous_name_owners =
        std::collections::BTreeMap::<&str, std::collections::BTreeSet<&str>>::new();
    for character in &contract.characters {
        for previous_name in &character.previous_names {
            let previous_name = previous_name.trim();
            if !value_missing(previous_name) {
                previous_name_owners
                    .entry(previous_name)
                    .or_default()
                    .insert(character.canonical_name.trim());
            }
        }
    }
    for (previous_name, owners) in previous_name_owners {
        if owners.len() > 1 {
            issues.push(format!(
                "ContractBlocker: 历史角色名 `{previous_name}` 同时指向多个当前角色，必须明确唯一身份映射"
            ));
        }
    }

    let previous_names = contract
        .characters
        .iter()
        .filter(|character| character.name_source.trim() == "generated_by_writing_tool_policy")
        .flat_map(|character| character.previous_names.iter().map(String::as_str))
        .map(str::trim)
        .filter(|name| !value_missing(name))
        .collect::<std::collections::BTreeSet<_>>();
    if previous_names.is_empty() {
        return;
    }

    for previous_name in previous_names {
        for (label, value) in [
            ("故事简述", contract.brief.as_str()),
            ("故事前提", contract.premise.as_str()),
            ("终局方向", contract.ending.desired_resolution.as_str()),
            ("终局状态", contract.ending.final_state.as_str()),
            ("主角弧线", contract.protagonist_arc.as_str()),
            ("世界观意象", contract.world_imagery.as_str()),
            ("总主线因果链", contract.main_causal_spine.as_str()),
        ] {
            push_superseded_name_issue(label, value, previous_name, issues);
        }
        for (index, value) in contract.ending.must_resolve.iter().enumerate() {
            push_superseded_name_issue(
                &format!("终局方向必须兑现项[{index}]"),
                value,
                previous_name,
                issues,
            );
        }
        for (index, value) in contract.ending.allowed_open_questions.iter().enumerate() {
            push_superseded_name_issue(
                &format!("终局方向允许开放项[{index}]"),
                value,
                previous_name,
                issues,
            );
        }
        for character in &contract.characters {
            let owner = character.canonical_name.trim();
            for (field, value) in [
                ("欲望", character.desire.as_str()),
                ("恐惧", character.fear.as_str()),
                ("底线", character.bottom_line.as_str()),
                ("弧线起点", character.arc_start.as_str()),
                ("弧线终点", character.arc_end.as_str()),
                ("计划登场", character.planned_entry.as_str()),
                ("计划离场", character.planned_exit.as_str()),
            ] {
                push_superseded_name_issue(
                    &format!("角色 `{owner}` 的{field}锚点"),
                    value,
                    previous_name,
                    issues,
                );
            }
        }
        push_superseded_name_issue("大纲", &contract.outline.raw_outline, previous_name, issues);
        for (index, volume) in contract.outline.volumes.iter().enumerate() {
            for (field, value) in [
                ("卷名", volume.title.as_str()),
                ("目标", volume.objective.as_str()),
                ("卷尾变化", volume.ending_change.as_str()),
            ] {
                push_superseded_name_issue(
                    &format!("分卷[{index}]{field}"),
                    value,
                    previous_name,
                    issues,
                );
            }
        }
        for (index, chapter) in contract.outline.near_chapters.iter().enumerate() {
            for (field, value) in [
                ("章节目标", chapter.goal.as_str()),
                ("章节转折", chapter.expected_turn.as_str()),
            ] {
                push_superseded_name_issue(
                    &format!("近期章节[{index}]{field}"),
                    value,
                    previous_name,
                    issues,
                );
            }
        }
        for (index, value) in contract.themes.iter().enumerate() {
            push_superseded_name_issue(&format!("核心主题[{index}]"), value, previous_name, issues);
        }
        for (index, value) in contract.world_rules.iter().enumerate() {
            push_superseded_name_issue(&format!("世界规则[{index}]"), value, previous_name, issues);
        }
        for (index, value) in contract.style_rules.iter().enumerate() {
            push_superseded_name_issue(&format!("叙事风格[{index}]"), value, previous_name, issues);
        }
        for (index, value) in contract.must_avoid.iter().enumerate() {
            push_superseded_name_issue(&format!("必须避免[{index}]"), value, previous_name, issues);
        }
        if serde_json::to_value(&contract.structured)
            .ok()
            .is_some_and(|value| json_value_contains_character_reference(&value, previous_name))
        {
            issues.push(format!(
                "ContractBlocker: 小说合同结构化治理字段仍引用已废弃角色名 `{previous_name}`，必须只重写对应治理字段并保留当前角色权威表"
            ));
        }
    }
}

fn push_superseded_name_issue(
    label: &str,
    value: &str,
    previous_name: &str,
    issues: &mut ContractIssueList,
) {
    if superseded_name_is_explicit_person_reference(value, previous_name) {
        issues.push(format!(
            "ContractBlocker: 小说合同{label}仍引用已废弃角色名 `{previous_name}`，必须只重写该字段并保留当前角色权威表"
        ));
    }
}

fn superseded_name_is_explicit_person_reference(value: &str, previous_name: &str) -> bool {
    if !value.contains(previous_name) {
        return false;
    }
    if previous_name.chars().count() == 1
        && replace_character_anchor_reference(value, previous_name, "__CURRENT_CHARACTER__")
            != value
    {
        return true;
    }
    if previous_name.chars().count() >= 3 {
        return true;
    }
    character_field_person_references(value)
        .into_iter()
        .chain(primary_role_person_references(value))
        .any(|reference| reference == previous_name)
}

fn json_value_contains_character_reference(value: &serde_json::Value, reference: &str) -> bool {
    match value {
        serde_json::Value::String(text) => {
            superseded_name_is_explicit_person_reference(text, reference)
        }
        serde_json::Value::Array(items) => items
            .iter()
            .any(|item| json_value_contains_character_reference(item, reference)),
        serde_json::Value::Object(object) => object
            .values()
            .any(|item| json_value_contains_character_reference(item, reference)),
        _ => false,
    }
}

pub(super) fn validate_character_identity_invariants(
    contract: &NovelCreationContract,
    issues: &mut ContractIssueList,
) {
    issues.set_scope(
        "contract.character_authority",
        crate::tool::writing::creation_contract::issue::ContractIssueKind::Characters,
        "characters",
    );
    let mut names = std::collections::BTreeSet::new();
    let mut ids = std::collections::BTreeSet::new();
    let mut ids_by_name = std::collections::BTreeMap::new();

    for character in &contract.characters {
        let name = character.canonical_name.trim();
        let id = character.character_id.trim();
        if !value_missing(name) && !names.insert(name.to_string()) {
            issues.push(format!(
                "ContractBlocker: 角色权威表包含重复 canonical_name `{name}`"
            ));
        }
        if !value_missing(id) && !ids.insert(id.to_string()) {
            issues.push(format!(
                "ContractBlocker: 角色权威表包含重复 character_id `{id}`"
            ));
        }
        if !value_missing(name) && !value_missing(id) {
            ids_by_name.insert(name.to_string(), id.to_string());
        }
    }

    for (index, relation) in contract.structured.relationship_ledger.iter().enumerate() {
        let relation_names = relation
            .characters
            .iter()
            .map(|name| name.trim())
            .filter(|name| !value_missing(name))
            .collect::<Vec<_>>();
        let relation_ids = relation
            .character_ids
            .iter()
            .map(|id| id.trim())
            .filter(|id| !value_missing(id))
            .collect::<Vec<_>>();

        if relation_names.len()
            != relation_names
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
        {
            issues.push(format!(
                "ContractBlocker: 关系账本[{index}]形成角色与自身的关系边"
            ));
        }
        if relation_ids.len()
            != relation_ids
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
        {
            issues.push(format!(
                "ContractBlocker: 关系账本[{index}]包含重复 character_id，不能形成自环"
            ));
        }
        for id in &relation_ids {
            if !ids.contains(*id) {
                issues.push(format!(
                    "ContractBlocker: 关系账本[{index}]引用未知 character_id `{id}`"
                ));
            }
        }
        for name in &relation_names {
            if !names.contains(*name) {
                issues.push(format!(
                    "ContractBlocker: 关系账本[{index}]引用角色权威表之外的角色 `{name}`"
                ));
            }
        }
        if !relation_ids.is_empty() && relation_ids.len() != relation_names.len() {
            issues.push(format!(
                "ContractBlocker: 关系账本[{index}]的角色姓名与 character_id 数量不一致"
            ));
            continue;
        }
        for (name, id) in relation_names.iter().zip(relation_ids.iter()) {
            if let Some(expected) = ids_by_name.get(*name) {
                if expected != id {
                    issues.push(format!(
                        "ContractBlocker: 关系账本[{index}]中角色 `{name}` 的 character_id 与角色权威表不一致"
                    ));
                }
            }
        }
    }

    validate_gendered_primary_role_against_story_text(contract, issues);
}

fn validate_gendered_primary_role_against_story_text(
    contract: &NovelCreationContract,
    issues: &mut ContractIssueList,
) {
    if !(contract.language.to_ascii_lowercase().starts_with("zh")
        || contract.language.contains("中文"))
    {
        return;
    }
    let other_character_names = contract
        .characters
        .iter()
        .map(|character| character.canonical_name.trim())
        .filter(|name| !value_missing(name))
        .map(ToString::to_string)
        .collect::<std::collections::BTreeSet<_>>();
    let mut story_fields = vec![
        ("故事简述".to_string(), contract.brief.as_str()),
        ("故事前提".to_string(), contract.premise.as_str()),
        ("主角弧线".to_string(), contract.protagonist_arc.as_str()),
        (
            "总主线因果".to_string(),
            contract.main_causal_spine.as_str(),
        ),
        (
            "终局方向".to_string(),
            contract.ending.desired_resolution.as_str(),
        ),
        ("终局状态".to_string(), contract.ending.final_state.as_str()),
        ("大纲".to_string(), contract.outline.raw_outline.as_str()),
    ];
    for (index, volume) in contract.outline.volumes.iter().enumerate() {
        story_fields.extend([
            (format!("第{}卷卷名", index + 1), volume.title.as_str()),
            (format!("第{}卷目标", index + 1), volume.objective.as_str()),
            (
                format!("第{}卷卷尾变化", index + 1),
                volume.ending_change.as_str(),
            ),
        ]);
    }
    for (index, chapter) in contract.outline.near_chapters.iter().enumerate() {
        let number = chapter.number.unwrap_or(index + 1);
        story_fields.extend([
            (format!("第{number}章目标"), chapter.goal.as_str()),
            (format!("第{number}章转折"), chapter.expected_turn.as_str()),
        ]);
    }
    for character in &contract.characters {
        story_fields.extend([
            ("角色权威表欲望锚点".to_string(), character.desire.as_str()),
            ("角色权威表恐惧锚点".to_string(), character.fear.as_str()),
            (
                "角色权威表底线锚点".to_string(),
                character.bottom_line.as_str(),
            ),
            (
                "角色权威表弧线起点".to_string(),
                character.arc_start.as_str(),
            ),
            ("角色权威表弧线终点".to_string(), character.arc_end.as_str()),
        ]);
    }
    let story_text = story_fields
        .iter()
        .map(|(_, text)| *text)
        .collect::<Vec<_>>()
        .join("\n");
    let primary_specific_text =
        [contract.brief.as_str(), contract.protagonist_arc.as_str()].join("\n");

    for character in contract
        .characters
        .iter()
        .filter(|character| character.role_looks_primary())
    {
        let expected = if character.role.contains("女主") {
            Some("feminine")
        } else if character.role.contains("男主") {
            Some("masculine")
        } else {
            None
        };
        let Some(expected) = expected else {
            continue;
        };
        let name = character.canonical_name.trim();
        if value_missing(name) {
            continue;
        }
        if let Some((field, observed)) = [
            ("欲望", character.desire.as_str()),
            ("恐惧", character.fear.as_str()),
            ("底线", character.bottom_line.as_str()),
            ("弧线起点", character.arc_start.as_str()),
            ("弧线终点", character.arc_end.as_str()),
        ]
        .into_iter()
        .find_map(|(field, value)| {
            super::super::novel_studio::contract_explicit_identity_profile_in_character_anchor(
                value,
            )
            .filter(|observed| *observed != expected)
            .map(|observed| (field, observed))
        }) {
            let observed_label = if observed == "feminine" {
                "女性"
            } else {
                "男性"
            };
            issues.push(format!(
                "ContractBlocker: 角色 `{name}` 的{field}含{observed_label}身份称谓，与角色权威表定位 `{}` 冲突；必须统一角色定位和角色锚点",
                character.role.trim()
            ));
            continue;
        }
        let mut other_names = other_character_names.clone();
        other_names.remove(name);
        let primary_specific_mentions_other_character = other_names
            .iter()
            .any(|other_name| primary_specific_text.contains(other_name));
        let localized_observed = story_fields.iter().find_map(|(label, text)| {
            super::super::novel_studio::contract_stable_character_pronoun_profile_in_text(
                text,
                name,
                &other_names,
            )
            .filter(|observed| *observed != expected)
            .map(|observed| (label.as_str(), observed))
        });
        let grouped_story_fields = [
            (
                "角色权威表锚点",
                ContractIssueKind::Characters,
                story_fields
                    .iter()
                    .filter(|(label, _)| label.contains("角色权威表"))
                    .map(|(_, text)| *text)
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            (
                "剧情规划字段",
                ContractIssueKind::Plot,
                story_fields
                    .iter()
                    .filter(|(label, _)| {
                        user_story_semantic_issue_kind(label) == ContractIssueKind::Plot
                    })
                    .map(|(_, text)| *text)
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            (
                "故事骨架字段",
                ContractIssueKind::Skeleton,
                story_fields
                    .iter()
                    .filter(|(label, _)| {
                        !label.contains("角色权威表")
                            && user_story_semantic_issue_kind(label) == ContractIssueKind::Skeleton
                    })
                    .map(|(_, text)| *text)
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
        ];
        let grouped_observed = grouped_story_fields.iter().find_map(|(label, kind, text)| {
            super::super::novel_studio::contract_stable_character_pronoun_profile_in_text(
                text,
                name,
                &other_names,
            )
            .filter(|observed| *observed != expected)
            .map(|observed| (*label, *kind, observed))
        });
        let (source_label, observed) = if let Some(localized) = localized_observed {
            localized
        } else if let Some((source_label, _, observed)) = grouped_observed {
            (source_label, observed)
        } else {
            let observed =
                super::super::novel_studio::contract_stable_character_pronoun_profile_in_text(
                    &story_text,
                    name,
                    &other_names,
                )
                .or_else(|| {
                    (!primary_specific_mentions_other_character)
                        .then(|| {
                            super::super::novel_studio::contract_stable_primary_pronoun_profile_in_text(
                                &primary_specific_text,
                            )
                        })
                        .flatten()
                })
                .filter(|observed| *observed != expected);
            let Some(observed) = observed else {
                continue;
            };
            ("故事简述、故事前提、主角弧线或大纲", observed)
        };
        if observed == expected {
            continue;
        }
        let observed_label = if observed == "feminine" {
            "女性"
        } else {
            "男性"
        };
        let issue_kind = grouped_observed
            .filter(|(label, _, _)| *label == source_label)
            .map(|(_, kind, _)| kind)
            .unwrap_or_else(|| {
                if source_label.contains("角色权威表") {
                    ContractIssueKind::Characters
                } else {
                    user_story_semantic_issue_kind(source_label)
                }
            });
        let issue_text = format!(
            "ContractBlocker: 小说合同{source_label}把 `{name}` 稳定指代为{observed_label}，与角色权威表定位 `{}` 冲突；必须统一角色定位与该字段的角色指代",
            character.role.trim()
        );
        issues.push_issue(ContractIssue::new(
            "contract.character_story_identity",
            issue_kind,
            ContractIssueDisposition::HardBlock,
            ContractIssueEvidence::new(source_label, observed_label),
            issue_text,
        ));
    }
}

pub(super) fn validate_character_plan_volume_references(
    contract: &NovelCreationContract,
    issues: &mut ContractIssueList,
) {
    issues.set_scope(
        "contract.character_plan",
        crate::tool::writing::creation_contract::issue::ContractIssueKind::Characters,
        "characters",
    );
    let volume_count = contract.outline.volumes.len();
    for character in &contract.characters {
        let name = character.canonical_name.trim();
        let primary = character.role_looks_primary();
        for (label, value) in [
            ("计划登场", character.planned_entry.as_str()),
            ("计划离场", character.planned_exit.as_str()),
        ] {
            let planned_exit = label == "计划离场";
            let volume = character_plan_volume_reference(value, planned_exit);
            let Some(volume) = volume else {
                continue;
            };
            if volume > volume_count {
                issues.push(format!(
                    "ContractBlocker: 角色 `{name}` 的{label}锚点引用第{volume}卷，但合同只有{volume_count}卷；必须按实际分卷重写，不能把预计章节数当成卷数"
                ));
            } else if primary && label == "计划登场" && volume != 1 {
                issues.push(format!(
                    "ContractBlocker: 唯一主角 `{name}` 的计划登场锚点从第{volume}卷开始，但长篇主线从第1卷开始；必须让主角从首卷进入主线"
                ));
            } else if primary && label == "计划离场" && volume != volume_count {
                issues.push(format!(
                    "ContractBlocker: 唯一主角 `{name}` 的计划离场锚点停在第{volume}卷，但合同终局位于第{volume_count}卷；必须让主角持续到末卷终局"
                ));
            }
        }
    }
}

pub(crate) fn character_plan_anchor_needs_repair(
    value: &str,
    volume_count: usize,
    primary: bool,
    planned_exit: bool,
) -> bool {
    let Some(volume) = character_plan_volume_reference(value, planned_exit) else {
        return false;
    };
    volume > volume_count
        || (primary
            && if planned_exit {
                volume != volume_count
            } else {
                volume != 1
            })
}

pub(crate) fn first_volume_reference_outside_contract(
    value: &str,
    volume_count: usize,
) -> Option<usize> {
    volume_ordinals(value)
        .into_iter()
        .find(|volume| *volume > volume_count)
}

fn character_plan_volume_reference(value: &str, planned_exit: bool) -> Option<usize> {
    let ordinals = volume_ordinals(value);
    if planned_exit {
        ordinals.last().copied()
    } else {
        ordinals.first().copied()
    }
}

fn volume_ordinals(value: &str) -> Vec<usize> {
    let chars = value.char_indices().collect::<Vec<_>>();
    let mut ordinals = Vec::new();
    let ordinal_char = |ch: char| {
        ch.is_ascii_digit()
            || matches!(
                ch,
                '零' | '〇'
                    | '一'
                    | '二'
                    | '两'
                    | '三'
                    | '四'
                    | '五'
                    | '六'
                    | '七'
                    | '八'
                    | '九'
                    | '十'
                    | '百'
                    | '千'
                    | '万'
            )
    };
    for (start, (_, ch)) in chars.iter().enumerate() {
        let (number_start, suffix_required) = match *ch {
            '第' => (start + 1, true),
            '卷' => (start + 1, false),
            _ => continue,
        };
        let mut number_end = number_start;
        while number_end < chars.len()
            && number_end < number_start + 9
            && ordinal_char(chars[number_end].1)
        {
            number_end += 1;
        }
        if number_end == number_start
            || (suffix_required && (number_end >= chars.len() || chars[number_end].1 != '卷'))
        {
            continue;
        }
        let raw = chars[number_start..number_end]
            .iter()
            .map(|(_, ch)| *ch)
            .collect::<String>();
        if let Some(ordinal) =
            super::super::longform_guard::LongformArtifactGuard::parse_step_ordinal(&raw)
        {
            ordinals.push(ordinal);
        }
    }
    ordinals.sort_unstable();
    ordinals.dedup();
    ordinals
}

pub(super) fn validate_character_anchor_references(
    contract: &NovelCreationContract,
    authority_names: &[&str],
    non_character_terms: &[String],
    issues: &mut ContractIssueList,
) {
    for character in &contract.characters {
        let owner = character.canonical_name.trim();
        for (field, value) in [
            ("欲望", character.desire.as_str()),
            ("恐惧", character.fear.as_str()),
            ("底线", character.bottom_line.as_str()),
            ("弧线起点", character.arc_start.as_str()),
            ("弧线终点", character.arc_end.as_str()),
            ("计划登场", character.planned_entry.as_str()),
            ("计划离场", character.planned_exit.as_str()),
        ] {
            for reference in character_field_person_references(value) {
                if reference == owner
                    || reference_matches_authority_name(&reference, authority_names)
                    || reference_matches_authority_name_in_text(&reference, value, authority_names)
                    || authority_name_prefix_matches(&reference, owner)
                    || reference_matches_non_character_term(&reference, non_character_terms)
                {
                    continue;
                }
                issues.push(format!(
                    "ContractBlocker: 角色 `{owner}` 的{field}锚点引用了权威表外角色 `{reference}`"
                ));
            }
        }
    }
}

pub(super) fn validate_text_character_references(
    label: &str,
    text: &str,
    authority_names: &[&str],
    non_character_terms: &[String],
    issues: &mut ContractIssueList,
) {
    for reference in structured_text_person_references(text, authority_names) {
        if reference_matches_authority_name(&reference, authority_names)
            || reference_matches_authority_name_in_text(&reference, text, authority_names)
            || reference_matches_non_character_term(&reference, non_character_terms)
        {
            continue;
        }
        issues.push(format!(
            "ContractBlocker: 小说合同{label}引用了角色权威表外角色 `{reference}`"
        ));
    }
}

pub(super) fn validate_authority_names_not_used_as_non_character_entities(
    label: &str,
    text: &str,
    authority_names: &[&str],
    issues: &mut ContractIssueList,
) {
    for name in authority_names {
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        if authority_name_has_non_character_entity_context(text, name) {
            issues.push(format!(
                "ContractBlocker: 小说合同{label}把角色权威名 `{name}` 用作组织、地点或机构名"
            ));
        }
    }
}

fn structured_text_person_references(text: &str, authority_names: &[&str]) -> Vec<String> {
    let mut refs = role_prefixed_character_references(text);
    for reference in authority_surface_fragments(text, authority_names) {
        if !refs.iter().any(|existing| existing == &reference) {
            refs.push(reference);
        }
    }
    refs
}

fn authority_name_has_non_character_entity_context(text: &str, name: &str) -> bool {
    let mut rest = text;
    while let Some(index) = rest.find(name) {
        let before = &rest[..index];
        let after = &rest[index + name.len()..];
        if reference_has_entity_prefix_context(before)
            || reference_has_entity_suffix_context(after)
            || quoted_reference_has_entity_intro(before, after)
        {
            return true;
        }
        rest = &after[after
            .char_indices()
            .nth(1)
            .map(|(idx, _)| idx)
            .unwrap_or(after.len())..];
    }
    false
}

fn reference_has_entity_prefix_context(before: &str) -> bool {
    let compact = before
        .chars()
        .rev()
        .take(14)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>()
        .replace(char::is_whitespace, "")
        .trim_end_matches(|ch| matches!(ch, '"' | '\'' | '“' | '‘' | '「' | '『' | '《'))
        .to_string();
    [
        "公司",
        "集团",
        "企业",
        "机构",
        "平台",
        "组织",
        "部门",
        "团队",
        "项目组",
        "事务所",
        "交易所",
        "学校",
        "学院",
        "夜校",
        "宗门",
        "仙门",
        "门派",
        "商会",
        "协会",
        "联盟",
    ]
    .iter()
    .any(|marker| compact.ends_with(marker))
}

fn reference_has_entity_suffix_context(after: &str) -> bool {
    let compact = after
        .chars()
        .take(8)
        .collect::<String>()
        .replace(char::is_whitespace, "");
    if compact.starts_with("项目组") {
        return false;
    }
    if [
        "公司",
        "集团",
        "企业",
        "机构",
        "平台",
        "组织",
        "部门",
        "事务所",
        "交易所",
        "学校",
        "学院",
        "夜校",
        "宗门",
        "仙门",
        "门派",
        "商会",
        "协会",
        "联盟",
    ]
    .iter()
    .any(|marker| compact.starts_with(marker))
    {
        return true;
    }
    if [
        "协议", "系统", "算法", "程序", "项目", "装置", "设备", "代号", "代码",
    ]
    .iter()
    .any(|marker| compact.starts_with(marker))
    {
        return true;
    }
    let ambiguous_entity_suffixes = ["计划", "编号"];
    if ambiguous_entity_suffixes.iter().any(|marker| {
        compact.strip_prefix(*marker).is_some_and(|rest| {
            rest.is_empty()
                || rest.chars().next().is_some_and(|ch| {
                    matches!(
                        ch,
                        '、' | '，'
                            | '。'
                            | '；'
                            | ';'
                            | ','
                            | '：'
                            | ':'
                            | '/'
                            | '|'
                            | '和'
                            | '与'
                            | '被'
                            | '将'
                    )
                })
        })
    }) {
        return true;
    }
    let short_entity_suffixes = [
        "道", "网", "阵", "城", "门", "宗", "院", "校", "契", "法", "术", "阁", "塔", "桥", "街",
        "巷", "楼", "站", "会", "局", "署", "司", "厂", "铺", "店", "港", "城邦",
    ];
    short_entity_suffixes.iter().any(|marker| {
        compact == *marker
            || compact
                .strip_prefix(*marker)
                .and_then(|rest| rest.chars().next())
                .is_some_and(|ch| {
                    matches!(
                        ch,
                        '、' | '，'
                            | '。'
                            | '；'
                            | ';'
                            | ','
                            | '：'
                            | ':'
                            | '/'
                            | '|'
                            | '和'
                            | '与'
                    )
                })
    })
}

fn quoted_reference_has_entity_intro(before: &str, after: &str) -> bool {
    let before_compact = before
        .chars()
        .rev()
        .take(18)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>()
        .replace(char::is_whitespace, "");
    let quoted = before_compact.ends_with('“')
        || before_compact.ends_with('‘')
        || before_compact.ends_with('"')
        || before_compact.ends_with('\'')
        || before_compact.ends_with('「')
        || before_compact.ends_with('『')
        || before_compact.ends_with('《');
    if !quoted {
        return false;
    }
    let after_starts_quote = after
        .chars()
        .next()
        .is_some_and(|ch| matches!(ch, '”' | '’' | '"' | '\'' | '」' | '』' | '》'));
    after_starts_quote
        && [
            "入职",
            "加入",
            "任职",
            "创办",
            "进入",
            "投奔",
            "隶属",
            "供职",
            "行业巨头",
            "巨头",
            "公司",
            "集团",
            "企业",
            "机构",
            "平台",
            "宗门",
            "学院",
            "学校",
        ]
        .iter()
        .any(|marker| before_compact.contains(marker))
}

pub(super) fn character_field_person_references(text: &str) -> Vec<String> {
    let mut refs = role_prefixed_character_references(text);
    if let Some(reference) = leading_character_arc_subject(text) {
        if !refs.iter().any(|existing| existing == &reference) {
            refs.push(reference);
        }
    }
    refs
}

pub(super) fn leading_character_arc_subject(text: &str) -> Option<String> {
    let text = text
        .trim()
        .trim_start_matches(|ch: char| matches!(ch, '`' | '"' | '\'' | '“' | '‘' | '「' | '『'));
    for connector in ["原本", "起初", "曾经", "曾是", "从"] {
        let Some(index) = text.find(connector) else {
            continue;
        };
        let prefix = &text[..index];
        let candidate = prefix.trim();
        let count = candidate.chars().count();
        if !(2..=4).contains(&count)
            || !candidate.chars().all(surface_gate::is_cjk_unified)
            || reference_looks_like_common_contract_term(candidate)
        {
            continue;
        }
        return Some(candidate.to_string());
    }
    None
}

fn authority_surface_fragments(text: &str, authority_names: &[&str]) -> Vec<String> {
    let chars = text.chars().collect::<Vec<_>>();
    let mut refs = Vec::new();
    for known in authority_names {
        let known = known.trim();
        if known.is_empty() {
            continue;
        }
        let known_chars = known.chars().collect::<Vec<_>>();
        if known_chars.is_empty() || chars.len() < known_chars.len() {
            continue;
        }
        for index in 0..=chars.len() - known_chars.len() {
            if chars[index..index + known_chars.len()] != known_chars[..] {
                continue;
            }
            for extra in 0..=1 {
                let end = index + known_chars.len() + extra;
                if end > chars.len() {
                    continue;
                }
                if extra > 0
                    && !chars[index + known_chars.len()..end]
                        .iter()
                        .all(|ch| surface_gate::is_cjk_unified(*ch))
                {
                    continue;
                }
                let candidate = chars[index..end].iter().collect::<String>();
                if candidate == known {
                    continue;
                }
                if !refs.iter().any(|existing| existing == &candidate) {
                    refs.push(candidate);
                }
            }
        }
    }
    refs
}

pub(super) fn primary_role_person_references(text: &str) -> Vec<String> {
    let mut refs = Vec::new();
    for marker in ["主角", "主人公", "男主", "女主", "protagonist"] {
        for reference in marked_primary_role_person_references(text, marker) {
            if !refs.iter().any(|existing| existing == &reference) {
                refs.push(reference);
            }
        }
    }
    refs
}

pub(super) fn marked_primary_role_person_references(text: &str, marker: &str) -> Vec<String> {
    let mut refs = Vec::new();
    if marker.is_empty() {
        return refs;
    }
    let mut rest = text;
    while let Some(index) = rest.find(marker) {
        let after = &rest[index + marker.len()..];
        if let Some(reference) = direct_role_reference_name(after) {
            if !refs.iter().any(|existing| existing == &reference) {
                refs.push(reference);
            }
        }
        rest = &after[after
            .char_indices()
            .nth(1)
            .map(|(idx, _)| idx)
            .unwrap_or(after.len())..];
    }
    refs
}

fn reference_matches_authority_name(reference: &str, authority_names: &[&str]) -> bool {
    authority_names
        .iter()
        .any(|known| authority_name_prefix_matches(reference, known))
}

pub(super) fn reference_matches_authority_name_in_text(
    reference: &str,
    text: &str,
    authority_names: &[&str],
) -> bool {
    let reference = reference.trim();
    if reference.is_empty() {
        return false;
    }
    authority_names.iter().any(|known| {
        let known = known.trim();
        if known.is_empty() || reference == known {
            return false;
        }
        let Some(tail) = reference.strip_prefix(known) else {
            return false;
        };
        if !(1..=2).contains(&tail.chars().count()) {
            return false;
        }
        let tail_is_known_narrative_connector =
            tail.chars().all(role_reference_candidate_trailing_noise);
        if authority_name_is_followed_by_person_action(reference, known, text) {
            return true;
        }
        if !tail_is_known_narrative_connector
            && authority_reference_tail_precedes_person_action(reference, known, text)
        {
            return false;
        }
        reference_has_authority_surface_context(reference, text)
            || authority_name_is_followed_by_grammar_phrase(reference, known, text)
    })
}

fn authority_reference_tail_precedes_person_action(
    reference: &str,
    known: &str,
    text: &str,
) -> bool {
    let Some(tail) = reference.strip_prefix(known) else {
        return false;
    };
    let mut rest = text;
    while let Some(index) = rest.find(known) {
        let after = &rest[index + known.len()..];
        if let Some(after_tail) = after.strip_prefix(tail) {
            if text_after_reference_has_person_action_context(after_tail) {
                return true;
            }
        }
        rest = &after[after
            .char_indices()
            .nth(1)
            .map(|(idx, _)| idx)
            .unwrap_or(after.len())..];
    }
    false
}

fn authority_name_is_followed_by_grammar_phrase(reference: &str, known: &str, text: &str) -> bool {
    let Some(tail) = reference.strip_prefix(known) else {
        return false;
    };
    if tail.is_empty() {
        return false;
    }
    let mut rest = text;
    while let Some(index) = rest.find(known) {
        let after = &rest[index + known.len()..];
        if after.starts_with(tail)
            && [
                "所在", "所有", "所见", "所得", "所持", "所作", "所属", "所处", "所用", "所受",
                "所说", "所需", "所知", "所写", "所查", "作为", "正是",
            ]
            .iter()
            .any(|phrase| after.starts_with(phrase) && phrase.starts_with(tail))
        {
            return true;
        }
        rest = &after[after
            .char_indices()
            .nth(1)
            .map(|(idx, _)| idx)
            .unwrap_or(after.len())..];
    }
    false
}

pub(super) fn authority_name_is_followed_by_person_action(
    reference: &str,
    known: &str,
    text: &str,
) -> bool {
    let Some(tail) = reference.strip_prefix(known) else {
        return false;
    };
    let mut rest = text;
    while let Some(index) = rest.find(known) {
        let after = &rest[index + known.len()..];
        if after.starts_with(tail)
            && (text_after_reference_has_person_action_context(after)
                || reference_tail_is_followed_by_predicate_boundary(after, tail))
        {
            return true;
        }
        rest = &after[after
            .char_indices()
            .nth(1)
            .map(|(idx, _)| idx)
            .unwrap_or(after.len())..];
    }
    false
}

pub(super) fn authority_name_prefix_starts_person_action(
    reference: &str,
    known: &str,
    text: &str,
) -> bool {
    let Some(tail) = reference.strip_prefix(known) else {
        return false;
    };
    if tail.is_empty() {
        return false;
    }
    let mut rest = text;
    while let Some(index) = rest.find(known) {
        let after = &rest[index + known.len()..];
        if after.starts_with(tail) && text_after_reference_has_person_action_context(after) {
            return true;
        }
        rest = &after[after
            .char_indices()
            .nth(1)
            .map(|(idx, _)| idx)
            .unwrap_or(after.len())..];
    }
    false
}

fn reference_tail_is_followed_by_predicate_boundary(after: &str, tail: &str) -> bool {
    if tail.chars().count() != 1 {
        return false;
    }
    let Some(rest) = after.strip_prefix(tail) else {
        return false;
    };
    rest.chars().next().is_some_and(|ch| {
        ch.is_whitespace()
            || matches!(
                ch,
                '，' | ','
                    | '。'
                    | '.'
                    | '；'
                    | ';'
                    | '：'
                    | ':'
                    | '在'
                    | '向'
                    | '到'
                    | '了'
                    | '着'
                    | '过'
                    | '将'
                    | '把'
                    | '被'
                    | '由'
                    | '以'
                    | '为'
                    | '并'
                    | '而'
                    | '却'
            )
    })
}

fn reference_has_authority_surface_context(reference: &str, text: &str) -> bool {
    let mut rest = text;
    while let Some(index) = rest.find(reference) {
        let before = &rest[..index];
        let after = &rest[index + reference.len()..];
        if text_before_reference_has_role_marker(before) {
            return true;
        }
        if after
            .chars()
            .next()
            .is_some_and(reference_following_char_continues_compound_word)
        {
            return true;
        }
        rest = &after[after
            .char_indices()
            .nth(1)
            .map(|(idx, _)| idx)
            .unwrap_or(after.len())..];
    }
    false
}

fn text_after_reference_has_person_action_context(after: &str) -> bool {
    let compact = after.chars().take(8).collect::<String>();
    [
        "因",
        "为",
        "在",
        "把",
        "将",
        "被",
        "让",
        "使",
        "从",
        "向",
        "对",
        "正在",
        "正要",
        "正试图",
        "正准备",
        "亲自",
        "主动",
        "手动",
        "立即",
        "立刻",
        "独自",
        "共同",
        "联手",
        "再次",
        "继续",
        "试图",
        "开始",
        "决定",
        "认为",
        "面对",
        "面临",
        "面向",
        "买断",
        "夺走",
        "夺取",
        "瓦解",
        "背叛",
        "帮助",
        "保护",
        "藏",
        "击败",
        "控制",
        "追查",
        "核对",
        "针对",
        "反制",
        "公开",
        "守住",
        "学会",
        "成为",
        "愿意",
        "拒绝",
        "介入",
        "退出",
        "离开",
        "加入",
        "辞职",
        "辞官",
        "牺牲",
        "隐藏",
        "隐瞒",
        "揭露",
        "调查",
        "躲避",
        "收集",
        "修复",
        "寻找",
        "看到",
        "返回",
        "失踪",
        "遇难",
        "死亡",
        "留下",
        "出现",
        "发现",
        "重生",
        "也不",
        "也会",
        "也要",
        "也能",
        "也将",
        "也绝不",
    ]
    .iter()
    .any(|marker| compact.starts_with(marker))
}

fn text_before_reference_has_role_marker(before: &str) -> bool {
    let before = before.trim_end_matches(char::is_whitespace);
    [
        "主角",
        "男主",
        "女主",
        "反派",
        "对手",
        "导师",
        "盟友",
        "同伴",
        "关键角色",
        "人物",
        "角色",
        "姐姐",
        "妹妹",
        "哥哥",
        "弟弟",
        "母亲",
        "父亲",
        "妻子",
        "丈夫",
        "女儿",
        "儿子",
    ]
    .iter()
    .any(|marker| before.ends_with(marker))
}

fn reference_following_char_continues_compound_word(ch: char) -> bool {
    surface_gate::is_cjk_unified(ch)
        && !matches!(
            ch,
            '的' | '和'
                | '与'
                | '及'
                | '或'
                | '但'
                | '却'
                | '而'
                | '都'
                | '也'
                | '又'
                | '并'
                | '仍'
                | '在'
                | '是'
                | '为'
                | '把'
                | '被'
                | '让'
                | '使'
                | '会'
                | '能'
                | '将'
                | '已'
                | '未'
                | '不'
                | '了'
        )
}

pub(super) fn authority_name_prefix_matches(reference: &str, known: &str) -> bool {
    let reference = reference.trim();
    let known = known.trim();
    if known.is_empty() || reference.is_empty() {
        return false;
    }
    if reference == known {
        return true;
    }
    let Some(tail) = reference.strip_prefix(known) else {
        return false;
    };
    let tail_len = tail.chars().count();
    (1..=2).contains(&tail_len) && tail.chars().all(role_reference_candidate_trailing_noise)
}

fn reference_matches_non_character_term(reference: &str, non_character_terms: &[String]) -> bool {
    if reference_looks_like_common_contract_term(reference) {
        return true;
    }
    let reference = reference.trim();
    if reference.is_empty() {
        return false;
    }
    non_character_terms.iter().any(|term| {
        let term = term.trim();
        !term.is_empty() && reference == term
    })
}

fn role_prefixed_character_references(text: &str) -> Vec<String> {
    let mut refs = Vec::new();
    for marker in [
        "主角",
        "男主",
        "女主",
        "反派",
        "对手",
        "导师",
        "盟友",
        "同伴",
        "关键角色",
        "人物",
        "角色",
        "姐姐",
        "妹妹",
        "哥哥",
        "弟弟",
        "母亲",
        "父亲",
        "妻子",
        "丈夫",
        "女儿",
        "儿子",
    ] {
        let mut rest = text;
        while let Some(index) = rest.find(marker) {
            let after = &rest[index + marker.len()..];
            if let Some(reference) = direct_role_reference_name(after) {
                if !refs.iter().any(|existing| existing == &reference) {
                    refs.push(reference);
                }
            }
            rest = &after[after
                .char_indices()
                .nth(1)
                .map(|(idx, _)| idx)
                .unwrap_or(after.len())..];
        }
    }
    refs
}

fn reference_starts_with_compound_surname(reference: &str) -> bool {
    crate::tool::writing::naming::cjk_character_surname(reference)
        .is_some_and(|surname| surname.chars().count() == 2)
}

fn direct_role_reference_name(after_marker: &str) -> Option<String> {
    let after_separator =
        after_marker.trim_start_matches(|ch: char| matches!(ch, '：' | ':' | ' ' | '\t'));
    if after_separator.starts_with('的') {
        return None;
    }
    let trimmed = after_separator.trim_start_matches('是');
    if let Some(candidate) = quoted_role_reference_name(trimmed) {
        return Some(candidate);
    }
    let trimmed_chars = trimmed.char_indices().collect::<Vec<_>>();
    for name_len in [3, 2] {
        let end = trimmed_chars
            .get(name_len)
            .map(|(index, _)| *index)
            .unwrap_or(trimmed.len());
        if trimmed[..end].chars().count() != name_len {
            continue;
        }
        let raw_candidate = &trimmed[..end];
        let after = &trimmed[end..];
        let (candidate, predicate_after) =
            split_trailing_grammar_connector_from_role_name(raw_candidate, after);
        if role_reference_candidate_looks_like_person(&candidate)
            && (predicate_after.is_empty()
                || text_after_reference_has_person_action_context(&predicate_after))
        {
            return Some(candidate);
        }
    }

    let mut started = false;
    let mut candidate = String::new();
    for ch in after_marker.chars() {
        if !started && matches!(ch, '：' | ':' | ' ' | '\t' | '的' | '是') {
            continue;
        }
        if surface_gate::is_cjk_unified(ch) {
            started = true;
            candidate.push(ch);
            if candidate.chars().count() >= 4 {
                break;
            }
            continue;
        }
        break;
    }
    let candidate = trim_role_reference_candidate(&candidate)?;
    role_reference_candidate_looks_like_person(&candidate).then_some(candidate)
}

fn quoted_role_reference_name(value: &str) -> Option<String> {
    let value = value.trim_start();
    let mut chars = value.chars();
    let opening = chars.next()?;
    let closing = match opening {
        '“' => '”',
        '‘' => '’',
        '"' => '"',
        '\'' => '\'',
        '「' => '」',
        '『' => '』',
        '《' => '》',
        _ => return None,
    };
    let mut candidate = String::new();
    let mut closed = false;
    for ch in chars {
        if ch == closing {
            closed = true;
            break;
        }
        if !surface_gate::is_cjk_unified(ch) || candidate.chars().count() >= 4 {
            return None;
        }
        candidate.push(ch);
    }
    let len = candidate.chars().count();
    if !closed || !(1..=4).contains(&len) {
        return None;
    }
    if len == 1 || role_reference_candidate_looks_like_person(&candidate) {
        Some(candidate)
    } else {
        None
    }
}

fn split_trailing_grammar_connector_from_role_name(
    candidate: &str,
    after: &str,
) -> (String, String) {
    let mut normalized = candidate.to_string();
    let Some(last) = normalized.chars().last() else {
        return (normalized, after.to_string());
    };
    if normalized.chars().count() != 3
        || !matches!(
            last,
            '因' | '为' | '在' | '把' | '将' | '被' | '让' | '使' | '从' | '向' | '对'
        )
    {
        return (normalized, after.to_string());
    }
    normalized.pop();
    if !role_reference_candidate_looks_like_person(&normalized) {
        return (candidate.to_string(), after.to_string());
    }
    (normalized, format!("{last}{after}"))
}

fn trim_role_reference_candidate(candidate: &str) -> Option<String> {
    let mut value = candidate
        .trim()
        .trim_matches(|ch: char| {
            matches!(
                ch,
                '`' | '"'
                    | '\''
                    | '“'
                    | '”'
                    | '‘'
                    | '’'
                    | '《'
                    | '》'
                    | '「'
                    | '」'
                    | '『'
                    | '』'
                    | '('
                    | ')'
                    | '（'
                    | '）'
                    | '['
                    | ']'
                    | '【'
                    | '】'
            )
        })
        .to_string();
    for suffix in ["一人", "本人"] {
        if value.chars().count() == 4 && value.ends_with(suffix) {
            value.truncate(value.len() - suffix.len());
            break;
        }
    }
    while value
        .chars()
        .last()
        .is_some_and(role_reference_candidate_trailing_noise)
    {
        value.pop();
    }
    if role_reference_candidate_starts_with_grammar_connector(&value) {
        return None;
    }
    if role_reference_candidate_contains_grammar_connector(&value) {
        return None;
    }
    let len = value.chars().count();
    if !(2..=4).contains(&len) || !role_reference_candidate_looks_like_person(&value) {
        return None;
    }
    if reference_looks_like_common_contract_term(&value) {
        return None;
    }
    Some(value)
}

fn role_reference_candidate_starts_with_grammar_connector(candidate: &str) -> bool {
    candidate.chars().next().is_some_and(|ch| {
        matches!(
            ch,
            '和' | '与'
                | '及'
                | '并'
                | '或'
                | '把'
                | '被'
                | '让'
                | '使'
                | '对'
                | '为'
                | '因'
                | '从'
                | '在'
                | '向'
        )
    })
}

fn role_reference_candidate_contains_grammar_connector(candidate: &str) -> bool {
    candidate.chars().any(|ch| {
        matches!(
            ch,
            '和' | '与' | '及' | '并' | '或' | '、' | '/' | '\\' | '&'
        )
    })
}

fn reference_looks_like_common_contract_term(reference: &str) -> bool {
    let compact = reference.trim().replace(char::is_whitespace, "");
    if compact.is_empty() {
        return true;
    }
    if reference_looks_like_quantifier_or_asset_fragment(&compact) {
        return true;
    }
    if reference_looks_like_collective_or_organization(&compact) {
        return true;
    }
    let exact_terms = [
        "主角", "反派", "对手", "导师", "盟友", "同伴", "人物", "角色", "关系", "信任", "平衡",
        "升华", "利己", "利他", "自保", "公平", "规则", "秩序", "系统", "网络", "灵网", "算法",
        "资本", "制度", "资源", "真相", "证据", "身份", "利益", "同盟", "联盟", "城市", "世界",
        "考场", "考试", "阶层", "资格", "金融", "商业", "商界", "市场", "股市", "科技", "地产",
        "财富", "权力", "权威", "家族", "张扬",
    ];
    exact_terms.iter().any(|term| compact == *term)
        || [
            "盟友", "同伴", "关系", "系统", "网络", "规则", "制度", "资源", "证据", "资格",
        ]
        .iter()
        .any(|suffix| compact.ends_with(suffix))
}

fn reference_looks_like_quantifier_or_asset_fragment(reference: &str) -> bool {
    [
        "任何", "任意", "所有", "全部", "一切", "某个", "某些", "他人", "别人", "个体", "资产",
        "资源", "利益",
    ]
    .iter()
    .any(|term| reference == *term)
        || ["资产", "资源", "利益", "个体"]
            .iter()
            .any(|suffix| reference.ends_with(suffix))
        || ["任何", "任意", "所有", "全部"]
            .iter()
            .any(|prefix| reference.starts_with(prefix))
}

pub(super) fn reference_looks_like_collective_or_organization(reference: &str) -> bool {
    let len = reference.chars().count();
    (2..=6).contains(&len)
        && [
            "家",
            "氏",
            "族",
            "门",
            "派",
            "宗",
            "阁",
            "院",
            "府",
            "司",
            "局",
            "会",
            "盟",
            "社",
            "队",
            "组",
            "部",
            "团",
            "帮",
            "城",
            "市",
            "区",
            "县",
            "省",
            "国",
            "镇",
            "村",
            "街",
            "路",
            "港",
            "湾",
            "园",
            "场",
            "校",
            "厂",
            "店",
            "馆",
            "楼",
            "堂",
            "殿",
            "中心",
            "广场",
            "平台",
            "集团",
            "公司",
            "机构",
            "交易所",
            "政府",
            "政权",
            "议会",
            "帝国",
            "王国",
            "军队",
            "舰队",
        ]
        .iter()
        .any(|suffix| reference.ends_with(suffix))
}

fn role_reference_candidate_trailing_noise(ch: char) -> bool {
    matches!(
        ch,
        '的' | '和'
            | '与'
            | '及'
            | '或'
            | '公'
            | '崛'
            | '成'
            | '飞'
            | '突'
            | '建'
            | '维'
            | '改'
            | '打'
            | '夺'
            | '守'
            | '揭'
            | '对'
            | '连'
            | '贫'
            | '反'
            | '压'
            | '追'
            | '从'
            | '以'
            | '为'
            | '是'
            | '一'
            | '名'
            | '领'
            | '袖'
            | '员'
            | '市'
            | '省'
            | '区'
            | '县'
            | '国'
            | '街'
            | '路'
            | '镇'
            | '村'
            | '港'
            | '湾'
            | '园'
            | '场'
            | '经'
            | '济'
            | '商'
            | '业'
            | '资'
            | '产'
            | '权'
            | '力'
            | '原'
            | '曾'
            | '本'
            | '遭'
            | '作'
            | '只'
            | '在'
            | '把'
            | '将'
            | '会'
            | '能'
            | '仍'
            | '也'
            | '又'
            | '被'
            | '受'
            | '让'
            | '使'
            | '因'
            | '时'
            | '后'
            | '前'
            | '中'
            | '上'
            | '下'
            | '里'
            | '内'
            | '身'
    )
}

pub(super) fn role_reference_candidate_looks_like_person(candidate: &str) -> bool {
    let Some(first) = candidate.chars().next() else {
        return false;
    };
    if candidate_looks_like_abstract_domain_noun(candidate) {
        return false;
    }
    let common_surname = [
        '赵', '钱', '孙', '李', '林', '周', '吴', '郑', '王', '冯', '陈', '褚', '卫', '蒋', '沈',
        '韩', '杨', '朱', '秦', '尤', '许', '何', '吕', '施', '张', '孔', '曹', '严', '华', '金',
        '魏', '陶', '姜', '谢', '邹', '喻', '柏', '水', '窦', '章', '云', '苏', '潘', '葛', '奚',
        '范', '彭', '郎', '鲁', '韦', '昌', '马', '苗', '凤', '花', '方', '俞', '任', '袁', '柳',
        '酆', '鲍', '史', '唐', '费', '廉', '岑', '薛', '雷', '贺', '倪', '汤', '滕', '殷', '罗',
        '毕', '郝', '邬', '安', '常', '乐', '于', '时', '傅', '皮', '卞', '齐', '康', '伍', '余',
        '元', '卜', '顾', '孟', '平', '黄', '和', '穆', '萧', '尹', '钟', '闻', '祝', '辛', '白',
        '温', '晏', '裴', '梁', '宋', '宁', '阮', '程', '段', '景', '洛', '司', '南', '陆',
    ]
    .contains(&first)
        || reference_starts_with_compound_surname(candidate);
    if !common_surname {
        return false;
    }
    if !candidate.chars().all(surface_gate::is_cjk_unified) {
        return false;
    }
    if candidate.chars().any(|ch| {
        matches!(
            ch,
            '用' | '或'
                | '变'
                | '化'
                | '案'
                | '卷'
                | '分'
                | '歧'
                | '次'
                | '常'
                | '带'
                | '压'
                | '迫'
                | '员'
        )
    }) {
        return false;
    }
    !reference_looks_like_common_contract_term(candidate)
}

pub(super) fn replace_character_anchor_reference(
    text: &str,
    reference: &str,
    replacement: &str,
) -> String {
    let Some(reference) = clean_character_anchor_replacement_reference(reference) else {
        return text.to_string();
    };
    if replacement.trim().is_empty() || text.is_empty() {
        return text.to_string();
    }
    if reference.chars().count() == 1 {
        if reference.chars().all(surface_gate::is_cjk_unified) {
            return replace_governed_single_cjk_character_reference(text, &reference, replacement);
        }
        return replace_single_ascii_character_reference(text, &reference, replacement);
    }
    text.replace(&reference, replacement)
}

fn replace_governed_single_cjk_character_reference(
    text: &str,
    reference: &str,
    replacement: &str,
) -> String {
    let Some(target) = reference.chars().next() else {
        return text.to_string();
    };
    let mut rewritten = String::with_capacity(text.len());
    for (index, ch) in text.char_indices() {
        if ch != target {
            rewritten.push(ch);
            continue;
        }
        let before = &text[..index];
        let after = &text[index + ch.len_utf8()..];
        if single_cjk_character_reference_is_explicit_person(before, after) {
            rewritten.push_str(replacement);
        } else {
            rewritten.push(ch);
        }
    }
    rewritten
}

fn single_cjk_character_reference_is_explicit_person(before: &str, after: &str) -> bool {
    let before = before.trim_end_matches(char::is_whitespace);
    let after = after.trim_start_matches(char::is_whitespace);
    let quoted = before
        .chars()
        .last()
        .is_some_and(|ch| matches!(ch, '“' | '‘' | '"' | '\'' | '「' | '『' | '《'))
        && after
            .chars()
            .next()
            .is_some_and(|ch| matches!(ch, '”' | '’' | '"' | '\'' | '」' | '』' | '》'));
    quoted
        || text_before_reference_has_role_marker(before)
        || after.starts_with('的')
        || text_after_reference_has_person_action_context(after)
        || after.chars().next().is_some_and(|ch| {
            matches!(
                ch,
                '，' | ',' | '。' | '.' | '；' | ';' | '：' | ':' | '！' | '!' | '？' | '?'
            )
        })
}

fn replace_single_ascii_character_reference(
    text: &str,
    reference: &str,
    replacement: &str,
) -> String {
    let Some(target) = reference.chars().next() else {
        return text.to_string();
    };
    if !target.is_ascii_alphanumeric() {
        return text.to_string();
    }
    let chars = text.chars().collect::<Vec<_>>();
    let mut rewritten = String::with_capacity(text.len());
    for (index, ch) in chars.iter().copied().enumerate() {
        let left_is_identifier = index > 0 && ascii_identifier_character(chars[index - 1]);
        let right_is_identifier = chars
            .get(index + 1)
            .copied()
            .is_some_and(ascii_identifier_character);
        if ch == target && !left_is_identifier && !right_is_identifier {
            rewritten.push_str(replacement);
        } else {
            rewritten.push(ch);
        }
    }
    rewritten
}

fn ascii_identifier_character(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn clean_character_anchor_replacement_reference(reference: &str) -> Option<String> {
    let value = reference
        .trim()
        .trim_matches(|ch: char| {
            matches!(
                ch,
                '`' | '"'
                    | '\''
                    | '“'
                    | '”'
                    | '‘'
                    | '’'
                    | '《'
                    | '》'
                    | '「'
                    | '」'
                    | '『'
                    | '』'
                    | '('
                    | ')'
                    | '（'
                    | '）'
                    | '['
                    | ']'
                    | '【'
                    | '】'
            )
        })
        .to_string();
    let len = value.chars().count();
    if len == 1
        && !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || surface_gate::is_cjk_unified(ch))
    {
        return None;
    }
    if !(1..=8).contains(&len) {
        return None;
    }
    if reference_looks_like_quantifier_or_asset_fragment(&value) {
        return None;
    }
    Some(value)
}

fn candidate_looks_like_abstract_domain_noun(candidate: &str) -> bool {
    let chars = candidate.chars().collect::<Vec<_>>();
    if chars.len() != 2 {
        return false;
    }
    matches!(
        chars[1],
        '代' | '界'
            | '业'
            | '域'
            | '制'
            | '法'
            | '力'
            | '权'
            | '局'
            | '势'
            | '潮'
            | '场'
            | '圈'
            | '网'
            | '链'
            | '端'
            | '流'
            | '线'
            | '面'
            | '体'
            | '层'
            | '阶'
            | '级'
            | '序'
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::writing::creation_contract_model::{CharacterContract, OutlineContract};

    #[test]
    fn authority_name_followed_by_hide_action_is_not_a_new_character() {
        let text = "梁晏白隐藏了账本原件，等待听证会公开证据。";
        assert!(reference_matches_authority_name_in_text(
            "梁晏白隐",
            text,
            &["梁晏白"]
        ));
    }

    #[test]
    fn ambiguous_superseded_two_character_name_is_left_for_semantic_review() {
        let contract = NovelCreationContract {
            premise: "城中恢复安宁后，安宁才决定离开。".to_string(),
            outline: OutlineContract {
                raw_outline: "安宁查明旧案后公开证据。".to_string(),
                ..Default::default()
            },
            characters: vec![CharacterContract {
                canonical_name: "钟望宁".to_string(),
                name_source: "generated_by_writing_tool_policy".to_string(),
                previous_names: vec!["安宁".to_string()],
                role: "主角".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut issues = ContractIssueList::default();

        validate_superseded_character_name_residue(&contract, &mut issues);

        assert_eq!(contract.premise, "城中恢复安宁后，安宁才决定离开。");
        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn shared_previous_name_blocks_ambiguous_identity_authority() {
        let contract = NovelCreationContract {
            characters: vec![
                CharacterContract {
                    canonical_name: "韩知朔".to_string(),
                    previous_names: vec!["陈默".to_string()],
                    ..Default::default()
                },
                CharacterContract {
                    canonical_name: "闻望言".to_string(),
                    previous_names: vec!["陈默".to_string()],
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let mut issues = ContractIssueList::default();

        validate_superseded_character_name_residue(&contract, &mut issues);

        assert!(issues.iter().any(|issue| {
            issue.contains("陈默") && issue.contains("同时指向多个当前角色")
        }));
    }

    #[test]
    fn character_arc_subject_detects_short_name_before_transition() {
        assert_eq!(
            leading_character_arc_subject("林远从依赖技术炫技的独奏者成长为乐团领袖"),
            Some("林远".to_string())
        );
        assert_eq!(
            leading_character_arc_subject("从只相信自己到学会信任同伴"),
            None
        );
    }

    #[test]
    fn character_field_references_detect_destroy_target_names() {
        let refs = character_field_person_references("摧毁对手：林烬，并维护家族利益");

        assert!(
            refs.iter().any(|reference| reference == "林烬"),
            "character anchor validation should catch unregistered person references after destructive verbs: {refs:?}"
        );
    }

    #[test]
    fn unqualified_character_references_require_compound_surname_for_four_cjk_chars() {
        let ordinary_phrase = character_field_person_references(
            "资金短缺与施工困难共同拖慢民宿修缮，但团队决定继续推进。",
        );
        assert!(
            !ordinary_phrase.iter().any(|value| value == "施工困难"),
            "a four-character domain phrase must not become a person merely because its first character is a surname: {ordinary_phrase:?}"
        );

        let compound_name =
            character_field_person_references("团队决定帮助盟友：欧阳景澜，守住祖宅。");
        assert!(
            compound_name.iter().any(|value| value == "欧阳景澜"),
            "an unqualified four-character name with a compound surname should remain detectable: {compound_name:?}"
        );
    }

    #[test]
    fn character_field_references_detect_intervention_action_names() {
        let refs = character_field_person_references("不得干涉对手：景朔棠，突破");

        assert!(
            refs.iter().any(|reference| reference == "景朔棠"),
            "character anchor validation should catch stale person references before action suffixes: {refs:?}"
        );
    }

    #[test]
    fn character_field_references_do_not_extract_quantifier_asset_phrases() {
        let refs = character_field_person_references("为了效率可以牺牲任何资产。");

        assert!(
            refs.is_empty(),
            "generic quantifier/resource phrases are not person references: {refs:?}"
        );
    }

    #[test]
    fn character_field_references_trim_grammar_suffixes_and_quotes() {
        let refs = character_field_person_references(
            "对手：唐澈白，只想守住底线；盟友：洛晴声，拒绝妥协。",
        );

        assert!(
            refs.iter().any(|reference| reference == "唐澈白"),
            "{refs:?}"
        );
        assert!(
            refs.iter().any(|reference| reference == "洛晴声"),
            "{refs:?}"
        );
        assert!(
            !refs.iter().any(|reference| reference == "唐澈白只")
                && !refs.iter().any(|reference| reference == "洛晴声`"),
            "grammar suffixes or quote residue must not become standalone names: {refs:?}"
        );
    }

    #[test]
    fn character_field_references_trim_sentence_tail_fragments() {
        let refs = character_field_person_references("主角阮桥声为商界领袖。");

        assert!(
            refs.iter().any(|reference| reference == "阮桥声"),
            "{refs:?}"
        );
        assert!(
            !refs.iter().any(|reference| reference == "阮桥声为"),
            "sentence tail fragments must not become character names: {refs:?}"
        );

        for (raw, expected) in [("姜望安领", "姜望安"), ("裴庭禾袖", "裴庭禾")] {
            let refs = character_field_person_references(&format!("角色{raw}"));
            assert!(
                refs.iter().any(|reference| reference == expected),
                "{raw}: {refs:?}"
            );
            assert!(
                !refs.iter().any(|reference| reference == raw),
                "field tail fragments must not become character names: {raw}: {refs:?}"
            );
        }
    }

    #[test]
    fn character_field_references_trim_body_verb_prefix_after_name() {
        let refs =
            character_field_person_references("主角宁珩棠身负残缺功法，必须在劫灰时代求生。");

        assert!(
            refs.iter().any(|reference| reference == "宁珩棠"),
            "{refs:?}"
        );
        assert!(
            !refs.iter().any(|reference| reference == "宁珩棠身"),
            "`身负` is a verb phrase after the authority name, not part of the name: {refs:?}"
        );
    }

    #[test]
    fn character_field_references_do_not_keep_occupation_tail_noise() {
        let refs =
            character_field_person_references("宁闻棠员只是模型把角色名和员工尾字粘连后的噪声。");

        assert!(
            !refs.iter().any(|reference| reference == "宁闻棠员"),
            "occupation tail noise must not become a character name: {refs:?}"
        );
    }

    #[test]
    fn character_authority_validation_covers_planned_entry_and_exit() {
        let contract = NovelCreationContract {
            characters: vec![
                crate::tool::writing::creation_contract_model::CharacterContract {
                    canonical_name: "许望川".to_string(),
                    role: "主角".to_string(),
                    ..Default::default()
                },
                crate::tool::writing::creation_contract_model::CharacterContract {
                    canonical_name: "秦衡野".to_string(),
                    role: "导师".to_string(),
                    planned_exit: "决赛关键时刻助攻对手：林远，完成绝杀".to_string(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let authority = ["许望川", "秦衡野"];
        let mut issues = ContractIssueList::default();

        validate_character_anchor_references(&contract, &authority, &[], &mut issues);

        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("计划离场") && issue.contains("林远")),
            "planned character lifecycle fields must obey the same authority table: {issues:?}"
        );
    }

    #[test]
    fn authority_prefix_match_rejects_arbitrary_cjk_suffix_for_three_char_names() {
        let authority = ["温棠晚", "程岑舟"];
        let mut issues = ContractIssueList::default();

        validate_text_character_references(
            "主题母题",
            "温棠晚块与程岑舟虽都只是模型把权威名和后续字粘连后的表面噪声。",
            &authority,
            &[],
            &mut issues,
        );

        assert!(
            !issues.is_empty(),
            "arbitrary CJK suffixes after known names may be real drift and must not be silently accepted"
        );
    }

    #[test]
    fn authority_prefix_match_accepts_instrumental_grammar_particle() {
        let authority = ["温星真"];
        let mut issues = ContractIssueList::default();

        validate_text_character_references(
            "大纲",
            "工程局温星真以枯水期不便作业为由推迟探查。",
            &authority,
            &[],
            &mut issues,
        );

        assert!(
            issues.is_empty(),
            "the particle in `温星真以……为由` must not become part of a new character name: {issues:?}"
        );
    }

    #[test]
    fn authority_prefix_match_accepts_relationship_surface_tail_noise() {
        let authority = ["谢晴白", "裴庭舟"];
        let mut issues = ContractIssueList::default();

        validate_text_character_references(
            "兑现矩阵",
            "谢晴白连接现代商业与旧武馆规则，裴庭舟贫寒出身造成早期压力。",
            &authority,
            &[],
            &mut issues,
        );

        assert!(
            issues.is_empty(),
            "relationship and state tail noise after known authority names should not create fake characters: {issues:?}"
        );
    }

    #[test]
    fn authority_prefix_match_accepts_role_and_compound_surface_noise() {
        let authority = ["洛望序", "姜桥遥"];
        let mut issues = ContractIssueList::default();

        validate_text_character_references(
            "对手行动",
            "主角洛望序奇，却因背叛破产入狱；姜桥遥商业版图开始扩张。",
            &authority,
            &[],
            &mut issues,
        );

        assert!(
            issues.is_empty(),
            "role markers and compound words after known names should not create fake characters: {issues:?}"
        );
    }

    #[test]
    fn authority_prefix_match_accepts_as_role_grammar_after_name() {
        let authority = ["秦岑川", "闻栖棠"];
        let mut issues = ContractIssueList::default();

        validate_text_character_references(
            "兑现矩阵",
            "富豪雇佣秦岑川作为载体，闻栖棠作为证人公开记忆证据。",
            &authority,
            &[],
            &mut issues,
        );

        assert!(
            issues.is_empty(),
            "the grammar in `作为` after an authority name must not create a fake character: {issues:?}"
        );
    }

    #[test]
    fn authority_prefix_match_accepts_identity_grammar_after_name() {
        let authority = ["裴知遥", "祝望岚"];
        let mut issues = ContractIssueList::default();

        validate_text_character_references(
            "大纲",
            "裴知遥逐渐发现祝望岚正是时钟的另一半。",
            &authority,
            &[],
            &mut issues,
        );

        assert!(
            issues.is_empty(),
            "the identity grammar in `正是` after an authority name must not create a fake character: {issues:?}"
        );
    }

    #[test]
    fn authority_context_accepts_primary_name_followed_by_action_compound() {
        assert!(reference_matches_authority_name_in_text(
            "阮晴禾击",
            "主角阮晴禾击败所有主要竞争对手",
            &["阮晴禾"],
        ));
        assert!(reference_matches_authority_name_in_text(
            "司桥舟圈",
            "主角司桥舟圈层破局",
            &["司桥舟"],
        ));
        assert!(reference_matches_authority_name_in_text(
            "许庭川学",
            "许庭川学会在失败后信任队友",
            &["许庭川"],
        ));
    }

    #[test]
    fn authority_context_accepts_name_followed_by_structural_grammar_phrase() {
        let authority = ["林深", "苏青"];
        let mut issues = ContractIssueList::default();

        validate_text_character_references(
            "大纲",
            "工程师林深所在的观测站发生数据真空，苏青所见的异常与记录一致。",
            &authority,
            &[],
            &mut issues,
        );

        assert!(
            issues.is_empty(),
            "structural grammar after an authority name must not become a new character: {issues:?}"
        );
    }

    #[test]
    fn authority_context_accepts_name_followed_by_opinion_verb() {
        let authority = ["陶听桥"];
        let mut issues = ContractIssueList::default();

        validate_text_character_references(
            "大纲",
            "陶听桥认为日志空白不是硬件故障。",
            &authority,
            &[],
            &mut issues,
        );

        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn character_field_references_detect_unknown_name_inside_trust_boundary() {
        let refs = character_field_person_references("只信任盟友：林野，一人");

        assert!(
            refs.iter().any(|reference| reference == "林野"),
            "unknown person names inside relationship/trust phrases should remain visible to the authority gate: {refs:?}"
        );
    }

    #[test]
    fn character_field_references_detect_unknown_name_before_action_verb() {
        let refs = character_field_person_references("协助盟友：林浅，找出真相");

        assert!(
            refs.iter().any(|reference| reference == "林浅"),
            "unknown person names before action verbs should remain visible to the authority gate: {refs:?}"
        );
    }

    #[test]
    fn character_field_references_detect_named_family_member() {
        let refs = character_field_person_references("查明姐姐林柔失踪真相并完成告别");

        assert!(
            refs.iter().any(|reference| reference == "林柔"),
            "a named family member is still a character authority reference: {refs:?}"
        );
    }

    #[test]
    fn possessive_role_or_family_phrase_is_not_parsed_as_a_person_name() {
        for value in [
            "不以妹妹的安全换取核心秘密",
            "必须守住母亲的尊严",
            "主角的底线是不伪造证据",
            "盟友的失踪迫使调查提前",
        ] {
            let refs = character_field_person_references(value);
            assert!(
                refs.is_empty(),
                "a possessive role phrase must not become an unknown character: {value} => {refs:?}"
            );
        }
    }

    #[test]
    fn character_field_references_detect_unknown_name_inside_example_phrase() {
        let refs =
            character_field_person_references("被外部变量（如对手：林远，使用的探测仪）干扰");

        assert!(
            refs.iter().any(|reference| reference == "林远"),
            "unknown person names inside example phrases should remain visible to the authority gate: {refs:?}"
        );
    }

    #[test]
    fn character_field_references_do_not_extract_overlap_inside_trust_phrase() {
        let refs = character_field_person_references("敢于在绝境中信任他人");

        assert!(
            !refs.iter().any(|reference| reference == "任他人"),
            "a sliding CJK fragment inside an ordinary trust phrase is not a person: {refs:?}"
        );
    }

    #[test]
    fn character_field_references_do_not_promote_conjoined_abstract_state_to_person() {
        let refs = character_field_person_references("通过修缮祖宅找回内心的秩序与安宁");

        assert!(
            refs.is_empty(),
            "a conjunction before a surname-shaped two-character state is not person evidence: {refs:?}"
        );
    }

    #[test]
    fn character_field_references_detect_unknown_name_in_possessive_agency_phrase() {
        let refs =
            character_field_person_references("自己的贪腐链条被对手：林远，使用技术审计彻底切断");

        assert!(
            refs.iter().any(|reference| reference == "林远"),
            "a possessive phrase describing human agency must remain visible to the authority gate: {refs:?}"
        );
    }

    #[test]
    fn character_field_references_detect_unknown_name_in_passive_possessive_phrase() {
        let refs = character_field_person_references("被盟友：陈默，依靠社区凝聚力击败");

        assert!(
            refs.iter().any(|reference| reference == "陈默"),
            "a passive possessive phrase describing a person's effect must remain visible to the authority gate: {refs:?}"
        );
    }

    #[test]
    fn ambiguous_two_character_sentence_subject_is_not_a_hard_person_reference() {
        let refs = character_field_person_references("陈默也不卖变质的菜给街坊");

        assert!(
            !refs.iter().any(|reference| reference == "陈默"),
            "an unqualified two-character sentence subject is too ambiguous for a hard contract blocker: {refs:?}"
        );
    }

    #[test]
    fn lifetime_adverb_is_not_promoted_to_person_by_following_action() {
        let refs = character_field_person_references("愿用毕生守住家业，毕生愿意承担所有代价");

        assert!(
            !refs.iter().any(|reference| reference == "毕生"),
            "a lifetime adverb must not become a person solely because an action follows it: {refs:?}"
        );
    }

    #[test]
    fn character_field_references_do_not_promote_possessive_abstract_state_to_person() {
        let refs = character_field_person_references("害怕失去安宁的生活");

        assert!(
            !refs.iter().any(|reference| reference == "安宁"),
            "an abstract state owning an ordinary noun is not a person: {refs:?}"
        );
    }

    #[test]
    fn character_anchor_reference_gate_blocks_unregistered_action_target() {
        use crate::tool::writing::creation_contract_model::{
            CharacterContract, NovelCreationContract,
        };

        let mut contract = NovelCreationContract::default();
        contract.characters = vec![
            CharacterContract {
                canonical_name: "秦晴禾".to_string(),
                aliases: Vec::new(),
                role: "主角".to_string(),
                desire: "在咨询行业站稳脚跟".to_string(),
                fear: "再次被边缘化".to_string(),
                bottom_line: "不伪造核心数据".to_string(),
                arc_start: "谨慎自保".to_string(),
                arc_end: "主动破局".to_string(),
                ..Default::default()
            },
            CharacterContract {
                canonical_name: "景栖序".to_string(),
                aliases: Vec::new(),
                role: "关键同伴".to_string(),
                desire: "协助盟友：林浅，找出真相".to_string(),
                fear: "被卷入高层斗争".to_string(),
                bottom_line: "只与靠谱的人合作".to_string(),
                arc_start: "冷眼旁观".to_string(),
                arc_end: "共同公开证据".to_string(),
                ..Default::default()
            },
        ];
        let authority = ["秦晴禾", "景栖序"];
        let mut issues = ContractIssueList::default();

        validate_character_anchor_references(&contract, &authority, &[], &mut issues);

        assert!(
            issues.iter().any(|issue| issue.contains("林浅")),
            "unregistered names in character anchors must be blocked: {issues:?}"
        );
    }

    #[test]
    fn character_field_references_skip_phrase_fragments_that_start_with_surnames() {
        let refs = character_field_person_references("白带压迫不是角色名，常用案卷也不是角色名。");

        assert!(
            refs.is_empty(),
            "phrase fragments must not be promoted to person references: {refs:?}"
        );
    }

    #[test]
    fn character_field_references_ignore_connector_prefixed_resource_phrase_fragments() {
        let refs = character_field_person_references("从认知错位和积累资源开始逆袭。");

        assert!(
            !refs.iter().any(|reference| reference == "和积累资"),
            "connector-prefixed resource phrases are not character names: {refs:?}"
        );
    }

    #[test]
    fn character_field_references_ignore_contract_state_terms() {
        let refs = character_field_person_references(
            "从只相信自己，到学会信任盟友，并在利己、利他之间找到平衡。",
        );

        assert!(
            !refs
                .iter()
                .any(|reference| reference == "任盟友" || reference == "平衡"),
            "contract state terms must not become external character names: {refs:?}"
        );
    }

    #[test]
    fn character_field_references_ignore_collective_or_organization_terms() {
        let refs = character_field_person_references("苏家想垄断资源，林家控制旧城商会。");

        assert!(
            refs.is_empty(),
            "family, sect, company, alliance, and other collective names must not be treated as character authority names: {refs:?}"
        );
    }

    #[test]
    fn primary_role_references_read_quoted_incomplete_names() {
        assert_eq!(
            primary_role_person_references("主角“默”是一名记忆修复师"),
            ["默"]
        );
        assert_eq!(
            primary_role_person_references("主人公是「顾晟衡」，他追查被删除的案件"),
            ["顾晟衡"]
        );
    }

    #[test]
    fn authority_name_prefix_accepts_ordinary_narrative_tail() {
        assert!(authority_name_prefix_matches("温星声本", "温星声"));
        assert!(authority_name_prefix_matches("温星声本是", "温星声"));
    }

    #[test]
    fn authority_name_followed_by_targeted_action_is_not_extended_into_a_new_name() {
        let authority = ["顾景岚"];
        assert!(reference_matches_authority_name_in_text(
            "顾景岚针",
            "姜谨声因寒玉依赖导致根基脆弱，被顾景岚针对性击败。",
            &authority,
        ));
    }

    #[test]
    fn authority_name_followed_by_verification_action_is_not_extended_into_a_new_name() {
        let authority = ["陶泊衡"];
        let mut issues = ContractIssueList::default();

        validate_text_character_references(
            "兑现矩阵",
            "叶望真与陶泊衡核对贡香账页",
            &authority,
            &[],
            &mut issues,
        );

        assert!(
            issues.is_empty(),
            "a verification predicate after an authority name must not become a fake character: {issues:?}"
        );
    }

    #[test]
    fn primary_role_references_ignore_city_and_economy_tail_noise() {
        let refs = primary_role_person_references(
            "主角南岑安市不是人物，主角宋栖声经济线是模型把人名和经济词粘连后的噪声。",
        );

        assert!(
            !refs.iter().any(|reference| reference == "南岑安市"),
            "city names after role wording must not become protagonist references: {refs:?}"
        );
        assert!(
            !refs.iter().any(|reference| reference == "宋栖声济"),
            "economy tail noise after a generated name must not become protagonist references: {refs:?}"
        );
    }

    #[test]
    fn primary_role_references_split_names_from_grammar_connectors() {
        let refs = primary_role_person_references(
            "男主林深因实验事故失忆，女主苏念为保护事务所与他签订契约。",
        );

        assert_eq!(refs, vec!["林深".to_string(), "苏念".to_string()]);
    }

    #[test]
    fn primary_role_references_split_names_before_facing_predicates() {
        let refs = primary_role_person_references(
            "男主南晏舟面对突如其来的选择，女主沈知遥面临家族压力。",
        );

        assert_eq!(refs, vec!["南晏舟".to_string(), "沈知遥".to_string()]);
    }

    #[test]
    fn authority_name_entity_context_blocks_short_world_suffix_but_not_verbs() {
        let authority = ["段知棠"];
        let mut issues = ContractIssueList::default();

        validate_authority_names_not_used_as_non_character_entities(
            "世界观意象",
            "段知棠道、蒸汽核心、旧城阀门",
            &authority,
            &mut issues,
        );
        assert!(
            issues.iter().any(|issue| issue.contains("段知棠")),
            "character name glued to a world/entity suffix should be blocked: {issues:?}"
        );

        let mut ok = ContractIssueList::default();
        validate_authority_names_not_used_as_non_character_entities(
            "故事前提",
            "段知棠道歉后仍决定公开证据。",
            &authority,
            &mut ok,
        );
        assert!(
            ok.is_empty(),
            "ordinary action words after a character name must not be treated as entity pollution: {ok:?}"
        );
    }

    #[test]
    fn authority_name_entity_context_blocks_protocol_and_identifier_suffixes() {
        let authority = ["秦景舟"];
        for text in ["发现秦景舟编号，然后协议激活", "秦景舟协议覆盖了日志"]
        {
            let mut issues = ContractIssueList::default();
            validate_authority_names_not_used_as_non_character_entities(
                "故事前提",
                text,
                &authority,
                &mut issues,
            );
            assert!(!issues.is_empty(), "{text}: {issues:?}");
        }

        let mut ordinary_action = ContractIssueList::default();
        validate_authority_names_not_used_as_non_character_entities(
            "故事前提",
            "秦景舟编号了全部样本并提交复核。",
            &authority,
            &mut ordinary_action,
        );
        assert!(ordinary_action.is_empty(), "{ordinary_action:?}");
    }

    #[test]
    fn authority_name_followed_by_person_led_team_is_not_an_entity_collision() {
        let authority = ["姜维桥"];
        for text in [
            "老技工姜维桥团队从抵触转向合作。",
            "姜维桥项目组保留手工备份并继续核验数据。",
        ] {
            let mut issues = ContractIssueList::default();
            validate_authority_names_not_used_as_non_character_entities(
                "大纲",
                text,
                &authority,
                &mut issues,
            );
            assert!(
                issues.is_empty(),
                "a team led by an authority character must not be mistaken for an organization named after the character: {text}: {issues:?}"
            );
        }
    }

    #[test]
    fn authority_name_preceded_by_personal_industry_status_is_not_an_entity_collision() {
        let authority = ["韩晏言"];
        for text in [
            "在行业巨头韩晏言的垄断夹缝中建立起晨曦医疗。",
            "遭遇最大竞争对手、医疗器械巨头韩晏言的轻视。",
        ] {
            let mut issues = ContractIssueList::default();
            validate_authority_names_not_used_as_non_character_entities(
                "大纲",
                text,
                &authority,
                &mut issues,
            );
            assert!(
                issues.is_empty(),
                "a person's industry status must not be mistaken for an organization bearing the person's name: {text}: {issues:?}"
            );
        }
    }

    #[test]
    fn character_references_ignore_business_domain_terms_and_origin_tail() {
        let refs = character_field_person_references(
            "主角司砚白原为底层青年，张扬不是角色，金融只是产业压力。",
        );

        assert!(
            refs.iter().any(|reference| reference == "司砚白"),
            "authority-like names should remain visible after trimming origin prose: {refs:?}"
        );
        assert!(
            !refs.iter().any(|reference| reference == "司砚白原"
                || reference == "张扬"
                || reference == "金融"),
            "business/domain/common words must not become authority references: {refs:?}"
        );
    }

    #[test]
    fn character_field_references_ignore_abstract_domain_and_connector_fragments() {
        let refs =
            character_field_person_references("从被时代抛弃，到重建金融与实体经济之间的公平规则。");

        assert!(
            !refs.iter().any(|reference| reference == "时代"
                || reference == "金融与实"
                || reference.contains('与')),
            "abstract domain words and connector-spliced phrases must not become character names: {refs:?}"
        );
    }

    #[test]
    fn structured_references_ignore_organization_and_location_compounds() {
        let refs = structured_text_person_references(
            "主角要让金融中心重新接受底层交易所的公开规则。",
            &["钟望宁"],
        );

        assert!(
            !refs.iter().any(|reference| reference == "金融中心"
                || reference == "交易所"
                || reference == "金融中"),
            "organization/location compounds must not be treated as people: {refs:?}"
        );
    }

    #[test]
    fn gendered_primary_role_rejects_stable_opposite_pronouns_in_story_fields() {
        let mut contract = NovelCreationContract::default();
        contract.language = "zh-CN".to_string();
        contract.brief = "谢云宁发现城市核心异常，他决定追查低语来源。".to_string();
        contract.premise = "谢云宁接入旧芯片后，他成为唯一能听见主脑的人。".to_string();
        contract.characters.push(CharacterContract {
            canonical_name: "谢云宁".to_string(),
            role: "女主".to_string(),
            ..Default::default()
        });

        let mut issues = ContractIssueList::default();
        validate_character_identity_invariants(&contract, &mut issues);

        assert!(
            issues.iter().any(|issue| issue.contains("稳定指代为男性")
                && issue.contains("女主")
                && issue.contains("谢云宁")
                && issue.kind == ContractIssueKind::Skeleton),
            "gendered primary role must agree with protagonist-specific story prose: {issues:?}"
        );
    }

    #[test]
    fn gendered_primary_role_accepts_matching_story_pronouns() {
        let mut contract = NovelCreationContract::default();
        contract.language = "zh-CN".to_string();
        contract.brief = "谢云宁发现城市核心异常，她决定追查低语来源。".to_string();
        contract.premise = "谢云宁接入旧芯片后，她成为唯一能听见主脑的人。".to_string();
        contract.characters.push(CharacterContract {
            canonical_name: "谢云宁".to_string(),
            role: "女主".to_string(),
            ..Default::default()
        });

        let mut issues = ContractIssueList::default();
        validate_character_identity_invariants(&contract, &mut issues);

        assert!(
            !issues.iter().any(|issue| issue.contains("稳定指代")),
            "matching protagonist identity must remain valid: {issues:?}"
        );
    }

    #[test]
    fn gendered_primary_role_uses_outline_identity_markers_as_contract_evidence() {
        let mut contract = NovelCreationContract::default();
        contract.language = "zh-CN".to_string();
        contract.brief = "无灵根少年谢听野以燃烧骨骼为代价对抗神族。".to_string();
        contract.outline.raw_outline = "故事讲述底层少年谢听野攀登天梯并改变灵气秩序。".to_string();
        contract.characters.push(CharacterContract {
            canonical_name: "谢听野".to_string(),
            role: "女主".to_string(),
            ..Default::default()
        });

        let mut issues = ContractIssueList::default();
        validate_character_identity_invariants(&contract, &mut issues);

        assert!(
            issues.iter().any(|issue| issue.contains("稳定指代为男性")
                && issue.contains("女主")
                && issue.contains("谢听野")
                && issue.contains("大纲")
                && issue.kind == ContractIssueKind::Plot),
            "outline and brief identity markers must protect the locked protagonist profile: {issues:?}"
        );
    }

    #[test]
    fn gendered_primary_role_uses_unnamed_primary_specific_story_fields() {
        let mut contract = NovelCreationContract::default();
        contract.language = "zh-CN".to_string();
        contract.brief =
            "凡骨少年以燃烧自身为代价撬动天道，从卑微薪火成长为规则改写者。".to_string();
        contract.protagonist_arc =
            "从畏惧消耗、苟且生存的卑微少年，成长为主动定义规则的守护者。".to_string();
        contract.characters.push(CharacterContract {
            canonical_name: "季予声".to_string(),
            role: "女主".to_string(),
            ..Default::default()
        });

        let mut issues = ContractIssueList::default();
        validate_character_identity_invariants(&contract, &mut issues);

        assert!(
            issues.iter().any(|issue| issue.contains("稳定指代为男性")
                && issue.contains("女主")
                && issue.contains("季予声")),
            "brief and protagonist arc are primary-specific identity evidence: {issues:?}"
        );
    }

    #[test]
    fn gendered_primary_role_does_not_assign_supporting_identity_to_primary() {
        let mut contract = NovelCreationContract::default();
        contract.language = "zh-CN".to_string();
        contract.brief =
            "男记者周既明调查旧城失踪案，女法医岑秋棠协助验尸，她坚持保留关键证物。".to_string();
        contract.protagonist_arc = "周既明从独断调查者成长为愿意信任同伴的记者。".to_string();
        contract.structured.emotional_contract.emotional_promise =
            "见证女法医守住职业底线，并与主角建立互信。".to_string();
        contract.characters.extend([
            CharacterContract {
                canonical_name: "周既明".to_string(),
                role: "男主".to_string(),
                ..Default::default()
            },
            CharacterContract {
                canonical_name: "岑秋棠".to_string(),
                role: "同伴".to_string(),
                ..Default::default()
            },
        ]);

        let mut issues = ContractIssueList::default();
        validate_character_identity_invariants(&contract, &mut issues);

        assert!(
            !issues
                .iter()
                .any(|issue| issue.contains("周既明") && issue.contains("女性")),
            "a supporting woman's role-bound identity must not be assigned to the male protagonist: {issues:?}"
        );
    }

    #[test]
    fn gendered_primary_role_follows_pronoun_at_next_sentence_start() {
        let mut contract = NovelCreationContract::default();
        contract.language = "zh-CN".to_string();
        contract.outline.raw_outline = "季维言搬入静安公寓修养。入住当晚，他发现午夜噪音异常。通过录音设备，季维言确认失踪案与噪音有关。他决定继续追查。".to_string();
        contract.characters.push(CharacterContract {
            canonical_name: "季维言".to_string(),
            role: "女主".to_string(),
            ..Default::default()
        });

        let mut issues = ContractIssueList::default();
        validate_character_identity_invariants(&contract, &mut issues);

        assert!(
            issues.iter().any(|issue| issue.contains("稳定指代为男性")
                && issue.contains("女主")
                && issue.contains("季维言")
                && issue.contains("大纲")),
            "a pronoun starting the immediately following sentence still refers to the named protagonist: {issues:?}"
        );
    }

    #[test]
    fn gendered_primary_role_follows_next_sentence_after_long_subject_action() {
        let mut contract = NovelCreationContract::default();
        contract.language = "zh-CN".to_string();
        contract.premise = "青云宗晋升依赖灵脉供给，边荒药圃弟子商屿桥从十年病害记录中发现高层掩盖灵脉枯竭，他必须在被灭口前公开真相。".to_string();
        contract.outline.raw_outline = "商屿桥通过十年病害记录发现灵植病害与灵脉枯竭同步，并确认宗门高层一直掩盖边荒真相。在遭遇第一次灭口危机后，他逃往边荒深处寻找灵脉源头。".to_string();
        contract.characters.push(CharacterContract {
            canonical_name: "商屿桥".to_string(),
            role: "女主".to_string(),
            ..Default::default()
        });

        let mut issues = ContractIssueList::default();
        validate_character_identity_invariants(&contract, &mut issues);

        assert!(
            issues.iter().any(|issue| issue.contains("稳定指代为男性")
                && issue.contains("女主")
                && issue.contains("商屿桥")),
            "long same-sentence and next-sentence evidence must remain attached to the named subject: {issues:?}"
        );
    }

    #[test]
    fn gendered_primary_role_checks_near_chapters_and_character_anchors() {
        let mut contract = NovelCreationContract::default();
        contract.language = "zh-CN".to_string();
        contract.outline.near_chapters.push(
            super::super::super::creation_contract_model::ChapterSeedContract {
                number: Some(1),
                goal: "祝承原在酸雨中拾得记忆碎片".to_string(),
                expected_turn: "祝承原看到不属于他的童年记忆，碎片与他的神经产生共鸣".to_string(),
            },
        );
        contract.characters.extend([
            CharacterContract {
                canonical_name: "祝承原".to_string(),
                role: "女主".to_string(),
                ..Default::default()
            },
            CharacterContract {
                canonical_name: "季砚岚".to_string(),
                role: "同伴".to_string(),
                bottom_line: "无论祝承原变成何种形态，誓死守住他的神经接口安全".to_string(),
                ..Default::default()
            },
        ]);

        let mut issues = ContractIssueList::default();
        validate_character_identity_invariants(&contract, &mut issues);

        assert!(
            issues.iter().any(|issue| issue.contains("稳定指代为男性")
                && issue.contains("女主")
                && issue.contains("祝承原")),
            "bounded chapter plans and character anchors belong to the same identity authority: {issues:?}"
        );
        assert!(
            issues
                .iter()
                .filter(|issue| issue.contains("祝承原") && issue.contains("稳定指代为男性"))
                .all(|issue| issue.kind == ContractIssueKind::Plot),
            "near-chapter identity conflicts must be repaired by the plot owner: {issues:?}"
        );
    }

    #[test]
    fn gendered_primary_role_rejects_single_opposite_identity_noun_in_own_anchor() {
        let mut contract = NovelCreationContract::default();
        contract.language = "zh-CN".to_string();
        contract.characters.push(CharacterContract {
            canonical_name: "钟星岚".to_string(),
            role: "女主".to_string(),
            arc_start: "初出茅庐、理想主义的寒门士子".to_string(),
            ..Default::default()
        });

        let mut issues = ContractIssueList::default();
        validate_character_identity_invariants(&contract, &mut issues);

        assert!(
            issues.iter().any(|issue| {
                issue.contains("钟星岚")
                    && issue.contains("弧线起点")
                    && issue.contains("男性身份称谓")
            }),
            "a character-owned anchor needs only one explicit identity noun: {issues:?}"
        );
    }

    #[test]
    fn gendered_primary_role_does_not_treat_relative_in_desire_as_self_identity() {
        let mut contract = NovelCreationContract::default();
        contract.language = "zh-CN".to_string();
        contract.characters.push(CharacterContract {
            canonical_name: "姜予朔".to_string(),
            role: "男主".to_string(),
            desire: "找回妹妹被篡改的童年记忆真相".to_string(),
            ..Default::default()
        });

        let mut issues = ContractIssueList::default();
        validate_character_identity_invariants(&contract, &mut issues);

        assert!(
            !issues.iter().any(|issue| issue.contains("女性身份称谓")),
            "a relative who is the object of a character goal is not the character's own identity: {issues:?}"
        );
    }

    #[test]
    fn gendered_primary_role_does_not_treat_altruistic_compound_as_male_pronoun() {
        let mut contract = NovelCreationContract::default();
        contract.language = "zh-CN".to_string();
        contract.characters.push(CharacterContract {
            canonical_name: "韩予白".to_string(),
            role: "女主".to_string(),
            arc_end: "愿意为集体未来承担牺牲的女领袖，完成利他升华".to_string(),
            ..Default::default()
        });

        let mut issues = ContractIssueList::default();
        validate_character_identity_invariants(&contract, &mut issues);

        assert!(
            !issues.iter().any(|issue| issue.contains("男性身份称谓")),
            "lexical compounds containing 他 are not masculine identity evidence: {issues:?}"
        );
    }

    #[test]
    fn gendered_primary_role_does_not_assign_interacted_youth_to_female_protagonist() {
        let mut contract = NovelCreationContract::default();
        contract.language = "zh-CN".to_string();
        contract.brief = "女画师梁知棠借残卷追查宫廷旧案。".to_string();
        contract.premise =
            "梁知棠在坊市偶遇卖画少年，随后跟踪少年至破庙，并从少年手中取得线索。".to_string();
        contract.characters.push(CharacterContract {
            canonical_name: "梁知棠".to_string(),
            role: "女主".to_string(),
            arc_start: "谨慎自保的底层女画师".to_string(),
            arc_end: "敢于公开真相的女史官".to_string(),
            ..Default::default()
        });

        let mut issues = ContractIssueList::default();
        validate_character_identity_invariants(&contract, &mut issues);

        assert!(
            !issues.iter().any(|issue| issue.contains("稳定指代为男性")),
            "an interacted unnamed youth is not the protagonist's identity: {issues:?}"
        );
    }

    #[test]
    fn gendered_primary_role_does_not_assign_object_pronoun_or_mixed_plural_to_subject() {
        let mut contract = NovelCreationContract::default();
        contract.language = "zh-CN".to_string();
        contract.brief =
            "调查记者唐景宁与建筑师韩屿弦从相互质疑到并肩追查旧城安全记录。".to_string();
        contract.outline.raw_outline =
            "唐景宁与韩屿弦从对立走向合作。他们联手取证并揭露利益链。".to_string();
        contract.outline.near_chapters.push(
            super::super::super::creation_contract_model::ChapterSeedContract {
                number: Some(2),
                goal: "韩屿弦在工地遭遇小型坍塌".to_string(),
                expected_turn: "韩屿弦为护住图纸手臂受伤，唐景宁第一次看到他脆弱的一面".to_string(),
            },
        );
        contract.characters.extend([
            CharacterContract {
                canonical_name: "唐景宁".to_string(),
                role: "女主".to_string(),
                ..Default::default()
            },
            CharacterContract {
                canonical_name: "韩屿弦".to_string(),
                role: "男主".to_string(),
                ..Default::default()
            },
        ]);

        let mut issues = ContractIssueList::default();
        validate_character_identity_invariants(&contract, &mut issues);

        assert!(
            !issues.iter().any(|issue| {
                issue.contains("唐景宁")
                    && issue.contains("稳定指代为男性")
                    && issue.contains("女主")
            }),
            "an object pronoun and a mixed-character plural do not identify the female subject as male: {issues:?}"
        );
    }

    #[test]
    fn character_plan_rejects_chapter_count_mislabeled_as_volume_number() {
        let mut contract = NovelCreationContract::default();
        contract.outline.volumes = vec![
            super::super::super::creation_contract_model::VolumeContract::default(),
            super::super::super::creation_contract_model::VolumeContract::default(),
        ];
        contract.characters.push(CharacterContract {
            canonical_name: "温景衡".to_string(),
            planned_entry: "第一卷：签订契约".to_string(),
            planned_exit: "第四十卷：建立独立研究所".to_string(),
            ..Default::default()
        });

        let mut issues = ContractIssueList::default();
        validate_character_plan_volume_references(&contract, &mut issues);

        assert!(
            issues.iter().any(|issue| issue.contains("第40卷")
                && issue.contains("只有2卷")
                && issue.contains("预计章节数")),
            "chapter count must not leak into volume anchors: {issues:?}"
        );
    }

    #[test]
    fn character_plan_cannot_reference_volumes_when_outline_has_none() {
        let mut contract = NovelCreationContract::default();
        contract.characters.push(CharacterContract {
            canonical_name: "祝承原".to_string(),
            role: "女主".to_string(),
            planned_entry: "卷一进入主线".to_string(),
            planned_exit: "持续至卷五终局".to_string(),
            ..Default::default()
        });

        let mut issues = ContractIssueList::default();
        validate_character_plan_volume_references(&contract, &mut issues);

        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("引用第1卷") && issue.contains("只有0卷")),
            "explicit volume anchors require an actual volume plan: {issues:?}"
        );
    }

    #[test]
    fn primary_character_plan_must_span_first_through_final_volume() {
        let mut contract = NovelCreationContract::default();
        contract.outline.volumes = (0..5)
            .map(|_| super::super::super::creation_contract_model::VolumeContract::default())
            .collect();
        contract.characters.push(CharacterContract {
            canonical_name: "季予声".to_string(),
            role: "女主".to_string(),
            planned_entry: "第二卷".to_string(),
            planned_exit: "第四卷".to_string(),
            ..Default::default()
        });

        let mut issues = ContractIssueList::default();
        validate_character_plan_volume_references(&contract, &mut issues);

        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("从第2卷开始") && issue.contains("第1卷")),
            "primary entry must anchor the opening volume: {issues:?}"
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("停在第4卷") && issue.contains("第5卷")),
            "primary exit must reach the terminal volume: {issues:?}"
        );
    }

    #[test]
    fn primary_exit_uses_the_last_volume_ordinal_in_a_range_description() {
        let mut contract = NovelCreationContract::default();
        contract.outline.volumes = (0..5)
            .map(|_| super::super::super::creation_contract_model::VolumeContract::default())
            .collect();
        contract.characters.push(CharacterContract {
            canonical_name: "季予声".to_string(),
            role: "女主".to_string(),
            planned_entry: "第一卷进入主线".to_string(),
            planned_exit: "第一卷登场并持续至第五卷终局".to_string(),
            ..Default::default()
        });

        let mut issues = ContractIssueList::default();
        validate_character_plan_volume_references(&contract, &mut issues);

        assert!(
            !issues.iter().any(|issue| issue.contains("计划离场锚点")),
            "the final ordinal is the exit anchor: {issues:?}"
        );
    }
}
