//! Typed writing-contract readiness gate.
//!
//! This module is the single authority for whether a structured novel creation
//! contract is ready, repairable, or blocked. It may call naming/policy helpers,
//! but callers should not duplicate final contract-readiness checks.

use super::creation_contract::issue::{ContractIssueKind, ContractIssueList};
use super::creation_contract::{
    quoted_book_title_like_segments, quoted_segment_is_explicit_chapter_title,
    ContractReadinessScope, PatchFieldStrength,
};
use super::creation_contract_model::{value_missing, ContractBlockerReport, NovelCreationContract};
use super::naming;
use super::novel_contract_v2::PayoffMatrixEntry;
use super::surface_sanitizer;

mod character_gate;
mod outline_gate;
mod structured_gate;
mod surface_gate;

pub(crate) fn character_anchor_person_references(text: &str) -> Vec<String> {
    character_gate::character_field_person_references(text)
}

pub(crate) fn leading_character_arc_subject(text: &str) -> Option<String> {
    character_gate::leading_character_arc_subject(text)
}

pub(crate) fn replace_character_anchor_reference(
    text: &str,
    reference: &str,
    replacement: &str,
) -> String {
    character_gate::replace_character_anchor_reference(text, reference, replacement)
}

pub(crate) fn character_reference_extends_name_with_action(
    reference: &str,
    known_name: &str,
    text: &str,
) -> bool {
    character_gate::authority_name_prefix_starts_person_action(reference, known_name, text)
}

pub(crate) fn primary_role_person_references(text: &str) -> Vec<String> {
    character_gate::primary_role_person_references(text)
}

pub(crate) fn marked_primary_role_person_references(text: &str, marker: &str) -> Vec<String> {
    character_gate::marked_primary_role_person_references(text, marker)
}

pub(crate) fn reference_looks_like_collective_or_organization(reference: &str) -> bool {
    character_gate::reference_looks_like_collective_or_organization(reference)
}

pub(crate) fn character_plan_anchor_needs_repair(
    value: &str,
    volume_count: usize,
    primary: bool,
    planned_exit: bool,
) -> bool {
    character_gate::character_plan_anchor_needs_repair(value, volume_count, primary, planned_exit)
}

pub(crate) fn first_volume_reference_outside_contract(
    value: &str,
    volume_count: usize,
) -> Option<usize> {
    character_gate::first_volume_reference_outside_contract(value, volume_count)
}

pub(crate) fn non_character_contract_terms(contract: &NovelCreationContract) -> Vec<String> {
    structured_gate::non_character_contract_terms(contract)
}

pub(crate) fn contract_outline_text_is_polluted(value: &str) -> bool {
    outline_gate::outline_text_is_polluted(value)
}

pub(crate) fn payoff_matrix_entry_is_complete(entry: &PayoffMatrixEntry) -> bool {
    !value_missing(&entry.promise)
        && !value_missing(&entry.payoff_target)
        && !value_missing(&entry.status)
}

#[cfg(test)]
pub(crate) fn validate_novel_creation_contract(
    contract: &NovelCreationContract,
) -> ContractBlockerReport {
    validate_novel_creation_contract_for_scope(
        contract,
        ContractReadinessScope::FullLongformContract,
    )
}

pub(crate) fn validate_novel_creation_contract_for_scope(
    contract: &NovelCreationContract,
    scope: ContractReadinessScope,
) -> ContractBlockerReport {
    let mut issues =
        ContractIssueList::new("contract.surface", ContractIssueKind::Skeleton, "contract");
    contract.collect_surface_blockers(&mut issues);
    surface_gate::validate_creative_contract_field_pollution(contract, &mut issues);

    issues.set_scope("contract.title", ContractIssueKind::Skeleton, "title");
    let title = contract.title.canonical_title.trim();
    if value_missing(title) {
        issues.push("ContractBlocker: 小说合同缺少可锁定书名".to_string());
    }
    if !value_missing(title)
        && surface_gate::contract_title_surface_is_invalid_for_language(title, contract)
    {
        issues.push("ContractBlocker: 小说合同书名包含符号残片、外文残片或不像作品名".to_string());
    }
    if value_missing(&contract.title.rationale) {
        issues.push("ContractBlocker: 小说合同缺少书名理由".to_string());
    }
    if !value_missing(title) {
        if value_missing(&contract.title.rationale) {
            if let Some(issue) = naming::title_formality_issue(title, "书名") {
                issues.push(format!("ContractBlocker: {issue}"));
            }
        } else {
            let evidence = naming::BookTitleEvidence::new("书名", contract.story_basis_text());
            let decision = naming::select_book_title_candidate_decision(
                [naming::BookTitleCandidate::new(
                    title,
                    contract.title.rationale.as_str(),
                )],
                &evidence,
            );
            if !decision.accepted {
                if decision.reasons.is_empty() {
                    issues.push(
                        "ContractBlocker: 小说合同书名未通过读者钩子和故事依据质量门".to_string(),
                    );
                } else {
                    issues.extend(
                        decision
                            .reasons
                            .into_iter()
                            .map(|issue| format!("ContractBlocker: {issue}")),
                    );
                }
            }
        }
    }

    issues.set_scope(
        "contract.skeleton",
        ContractIssueKind::Skeleton,
        "story_authority",
    );
    if value_missing(&contract.ending.desired_resolution)
        && value_missing(&contract.ending.final_state)
    {
        issues.push("ContractBlocker: 小说合同缺少终局方向".to_string());
    }
    if value_missing(&contract.protagonist_arc) {
        issues.push("ContractBlocker: 小说合同缺少主角弧线".to_string());
    }
    if value_missing(&contract.world_imagery) {
        issues.push("ContractBlocker: 小说合同缺少世界观意象".to_string());
    }
    if value_missing(&contract.main_causal_spine) {
        issues.push("ContractBlocker: 小说合同缺少总主线因果链".to_string());
    }
    issues.set_scope(
        "contract.governance",
        ContractIssueKind::Governance,
        "governance",
    );
    for (label, values) in [
        ("核心主题", contract.themes.as_slice()),
        ("叙事风格", contract.style_rules.as_slice()),
        ("必须避免", contract.must_avoid.as_slice()),
    ] {
        if values.iter().all(|value| value_missing(value)) {
            issues.push(format!("ContractBlocker: 小说合同缺少{label}"));
        }
    }
    issues.set_scope(
        "contract.skeleton.surface",
        ContractIssueKind::Skeleton,
        "story_authority",
    );
    for (label, value) in [
        ("故事前提", contract.premise.as_str()),
        ("终局方向", contract.ending.desired_resolution.as_str()),
        ("终局状态", contract.ending.final_state.as_str()),
        ("总主线因果链", contract.main_causal_spine.as_str()),
        ("大纲", contract.outline.raw_outline.as_str()),
    ] {
        if story_field_contains_truncated_role_action(value) {
            issues.push(format!(
                "ContractBlocker: 小说合同{label}含有截断动作短语，必须补成完整行动"
            ));
        }
    }
    issues.set_scope(
        "contract.world_rules",
        ContractIssueKind::Governance,
        "world_rules",
    );
    if contract
        .world_rules
        .iter()
        .all(|value| value_missing(value))
    {
        issues.push("ContractBlocker: 小说合同缺少世界规则".to_string());
    } else if contract_list_is_only_repeating_anchor(&contract.world_rules, &contract.world_imagery)
    {
        issues.push(
            "ContractBlocker: 小说合同世界规则只是重复世界观意象，缺少可执行规则、代价或限制"
                .to_string(),
        );
    } else {
        for (index, rule) in contract.world_rules.iter().enumerate() {
            if world_rule_looks_truncated_or_not_actionable(rule) {
                issues.push(format!(
                    "ContractBlocker: 小说合同世界规则[{index}]不像可执行规则、代价或限制，疑似截断主线或角色锚点"
                ));
            }
        }
    }
    outline_gate::validate_outline_surface(contract, &mut issues, scope);

    issues.set_scope(
        "contract.structured_governance",
        ContractIssueKind::Governance,
        "structured",
    );
    structured_gate::validate_structured_contract_fields(contract, &mut issues, scope);
    issues.set_scope(
        "contract.character_authority",
        ContractIssueKind::Characters,
        "characters",
    );
    character_gate::validate_character_identity_invariants(contract, &mut issues);
    character_gate::validate_character_plan_volume_references(contract, &mut issues);

    let protagonists = contract
        .characters
        .iter()
        .filter(|character| character.role_looks_primary())
        .collect::<Vec<_>>();
    if protagonists.is_empty() {
        issues.push("ContractBlocker: 小说合同角色权威表缺少明确主角".to_string());
    } else if protagonists.len() > 1 {
        issues.push("ContractBlocker: 小说合同角色权威表包含多个主角槽位".to_string());
    }

    let non_primary_characters = contract
        .characters
        .iter()
        .filter(|character| {
            !character.role_looks_primary()
                && !value_missing(&character.canonical_name)
                && !value_missing(&character.role)
                && !character_role_is_generic_placeholder(&character.role)
        })
        .count();
    if non_primary_characters == 0 {
        issues.push(
            "ContractBlocker: 小说合同角色权威表缺少非主角关键角色、关系对象或对手".to_string(),
        );
    }
    let authority_names = contract
        .characters
        .iter()
        .map(|character| character.canonical_name.trim())
        .filter(|name| !value_missing(name))
        .collect::<Vec<_>>();
    for character in &contract.characters {
        issues.set_scope(
            "contract.character_authority",
            ContractIssueKind::Characters,
            "characters",
        );
        if value_missing(&character.canonical_name) || value_missing(&character.role) {
            continue;
        }
        let role = character.role.trim();
        let name = character.canonical_name.trim();
        if character_role_is_generic_placeholder(role) {
            issues.push(format!(
                "ContractBlocker: 角色 `{name}` 的角色定位过于泛化，必须改成主角、关系对象、盟友、导师、关键对手、反派或压力源等具体叙事功能"
            ));
        }
        if value_missing(&character.desire) {
            issues.set_scope(
                "contract.character_anchor",
                ContractIssueKind::Characters,
                "characters",
            );
            issues.push(format!(
                "ContractBlocker: 角色 `{name}`（{role}）缺少欲望锚点"
            ));
        }
        if value_missing(&character.fear) {
            issues.set_scope(
                "contract.character_anchor",
                ContractIssueKind::Characters,
                "characters",
            );
            issues.push(format!(
                "ContractBlocker: 角色 `{name}`（{role}）缺少恐惧锚点"
            ));
        } else if character_fear_ends_with_dangling_temporal_clause(&character.fear) {
            issues.set_scope(
                "contract.character_anchor",
                ContractIssueKind::Characters,
                "characters",
            );
            issues.push(format!(
                "ContractBlocker: 角色 `{name}`（{role}）的恐惧锚点像全书主线、截断残句或流程说明，必须改成短的角色级锚点"
            ));
        }
        if value_missing(&character.bottom_line) {
            issues.set_scope(
                "contract.character_anchor",
                ContractIssueKind::Characters,
                "characters",
            );
            issues.push(format!(
                "ContractBlocker: 角色 `{name}`（{role}）缺少底线锚点"
            ));
        }
        if character_anchor_uses_generic_placeholder(&character.desire)
            || character_anchor_uses_generic_placeholder(&character.fear)
            || character_anchor_uses_generic_placeholder(&character.bottom_line)
        {
            issues.set_scope(
                "contract.character_anchor",
                ContractIssueKind::Characters,
                "characters",
            );
            issues.push(format!(
                "ContractBlocker: 角色 `{name}`（{role}）仍使用通用兜底动机，必须根据当前故事重写欲望、恐惧和底线"
            ));
        }
        if character_bottom_line_lacks_boundary_action(&character.bottom_line) {
            issues.set_scope(
                "contract.character_anchor",
                ContractIssueKind::Characters,
                "characters",
            );
            issues.push(format!(
                "ContractBlocker: 角色 `{name}`（{role}）的底线锚点缺少明确边界、禁令或必须守住的行动"
            ));
        }
        for (field, value) in [
            ("欲望", character.desire.as_str()),
            ("恐惧", character.fear.as_str()),
            ("底线", character.bottom_line.as_str()),
            ("弧线起点", character.arc_start.as_str()),
            ("弧线终点", character.arc_end.as_str()),
        ] {
            if character_anchor_looks_like_storyline_or_truncated_surface(value) {
                issues.set_scope(
                    "contract.character_anchor",
                    ContractIssueKind::Characters,
                    "characters",
                );
                issues.push(format!(
                    "ContractBlocker: 角色 `{name}`（{role}）的{field}锚点像全书主线、截断残句或流程说明，必须改成短的角色级锚点"
                ));
            }
        }
    }
    issues.set_scope(
        "contract.character_authority",
        ContractIssueKind::Characters,
        "characters",
    );
    character_gate::validate_superseded_character_name_residue(contract, &mut issues);
    surface_gate::validate_primary_role_label_residue(contract, &authority_names, &mut issues);
    let non_character_terms = structured_gate::non_character_contract_terms(contract);
    character_gate::validate_character_anchor_references(
        contract,
        &authority_names,
        &non_character_terms,
        &mut issues,
    );
    for (label, text) in [
        ("故事前提", contract.premise.as_str()),
        ("终局方向", contract.ending.desired_resolution.as_str()),
        ("终局状态", contract.ending.final_state.as_str()),
        ("主角弧线", contract.protagonist_arc.as_str()),
        ("世界观意象", contract.world_imagery.as_str()),
        ("总主线因果链", contract.main_causal_spine.as_str()),
        ("大纲", contract.outline.raw_outline.as_str()),
    ] {
        let (code, kind, evidence_field) = if label == "大纲" {
            (
                "contract.outline.references",
                ContractIssueKind::Plot,
                "outline",
            )
        } else {
            (
                "contract.skeleton.references",
                ContractIssueKind::Skeleton,
                "story_authority",
            )
        };
        issues.set_scope(code, kind, evidence_field);
        character_gate::validate_authority_names_not_used_as_non_character_entities(
            label,
            text,
            &authority_names,
            &mut issues,
        );
        character_gate::validate_text_character_references(
            label,
            text,
            &authority_names,
            &non_character_terms,
            &mut issues,
        );
        if story_field_contains_incomplete_authority_rights_fragment(text, &authority_names) {
            issues.push(format!(
                "ContractBlocker: 小说合同{label}含有角色名与权属/控制对象的截断短语，必须补成完整行动"
            ));
        }
        if story_field_contains_dangling_authority_subject_fragment(text, &authority_names) {
            issues.push(format!(
                "ContractBlocker: 小说合同{label}含有角色名后接逗号的截断主语，必须补成完整行动"
            ));
        }
        if let Some(fragment) =
            story_field_authority_name_glued_to_ascii_entity(text, &authority_names)
        {
            issues.push(format!(
                "ContractBlocker: 小说合同{label}把中文角色名与 ASCII 实体直接粘连（`{fragment}`），必须只重写该字段并补成完整语义"
            ));
        }
    }
    issues.set_scope(
        "contract.world_rules.references",
        ContractIssueKind::Governance,
        "world_rules",
    );
    for (index, rule) in contract.world_rules.iter().enumerate() {
        character_gate::validate_authority_names_not_used_as_non_character_entities(
            &format!("世界规则[{index}]"),
            rule,
            &authority_names,
            &mut issues,
        );
        if story_field_contains_dangling_authority_subject_fragment(rule, &authority_names) {
            issues.push(format!(
                "ContractBlocker: 小说合同世界规则[{index}]含有角色名后接逗号的截断主语，必须补成完整规则"
            ));
        }
    }

    issues.set_scope(
        "contract.protagonist_authority",
        ContractIssueKind::Characters,
        "characters",
    );
    for character in protagonists {
        if value_missing(&character.canonical_name) {
            issues.push("ContractBlocker: 主角缺少稳定角色名".to_string());
        }
        if value_missing(&character.desire) {
            issues.push("ContractBlocker: 主角缺少欲望锚点".to_string());
        }
        if value_missing(&character.fear) {
            issues.push("ContractBlocker: 主角缺少恐惧锚点".to_string());
        }
        if value_missing(&character.bottom_line) {
            issues.push("ContractBlocker: 主角缺少底线锚点".to_string());
        }
        if value_missing(&character.arc_start) || value_missing(&character.arc_end) {
            issues.push("ContractBlocker: 主角缺少弧线起点或终点".to_string());
        }
    }

    issues.sort_dedup();
    ContractBlockerReport { issues }
}

pub(crate) fn character_anchor_uses_generic_placeholder(value: &str) -> bool {
    let compact = value.replace(char::is_whitespace, "");
    if compact.is_empty() {
        return false;
    }
    surface_sanitizer::contains_generic_contract_placeholder_residue(&compact)
        || [
            "完成故事合同中的核心目标",
            "完成本次故事合同中的核心目标",
            "改变自身命运",
            "再次失去选择权",
            "不违背合同确立的核心价值",
            "人物承诺",
            "动机必须清晰",
            "完成合同中的核心目标",
            "维护与主角目标冲突的秩序或利益",
            "自身秩序被主角选择改写",
            "反对必须由清晰动机推动",
            "被自己",
            "找到改写命运规则的路径并承担代价",
            "关键选择失败并失去主动权",
            "不无解释地背离用户设定与故事合同",
            "无法完成终局选择",
            "不以无代价捷径破坏终局选择",
            "阻止主线证据改变既有秩序",
            "主角抵达终局后旧秩序失效",
            "不能无解释地背离自身关系承诺",
            "不能无代价放弃当前秩序中的既得利益",
        ]
        .iter()
        .any(|marker| compact.contains(marker))
}

fn character_role_is_generic_placeholder(role: &str) -> bool {
    let compact = role.replace(char::is_whitespace, "");
    matches!(
        compact.as_str(),
        "角色" | "人物" | "关键角色" | "主要角色" | "配角" | "重要角色"
    )
}

pub(crate) fn character_anchor_looks_like_storyline_or_truncated_surface(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() {
        return false;
    }
    let compact = value.replace(char::is_whitespace, "");
    if compact.chars().count() > 56 {
        return true;
    }
    let len = compact.chars().count();
    if len <= 4 && compact.starts_with('被') && !compact.starts_with("被动") {
        return true;
    }
    if len <= 10
        && ["即使", "即便", "哪怕", "就算"]
            .iter()
            .any(|marker| compact.starts_with(marker))
        && !concessive_character_anchor_has_resolution(&compact)
    {
        return true;
    }
    if len <= 14 && subordinate_character_anchor_lacks_resolution(&compact) {
        return true;
    }
    if character_anchor_contains_incomplete_fixed_expression(&compact) {
        return true;
    }
    if len >= 6 && character_anchor_ends_like_truncated_clause(&compact) {
        return true;
    }
    compact.contains("->")
        || compact.contains("然后")
        || compact.ends_with('-')
        || compact.ends_with('—')
        || compact.ends_with('…')
        || compact.ends_with("...")
        || compact.matches('、').count() >= 3
}

fn character_anchor_contains_incomplete_fixed_expression(compact: &str) -> bool {
    [
        ("不为瓦全", "宁为玉碎"),
        ("唯利图", "唯利是图"),
        ("唯命从", "唯命是从"),
    ]
    .iter()
    .any(|(incomplete, complete)| compact.contains(incomplete) && !compact.contains(complete))
}

pub(crate) fn character_fear_ends_with_dangling_temporal_clause(value: &str) -> bool {
    let compact = value.trim().replace(char::is_whitespace, "");
    if compact.chars().count() < 5 {
        return false;
    }
    ["之后", "以前", "以后", "之时", "期间", "后", "前", "时"]
        .iter()
        .any(|suffix| compact.ends_with(suffix))
}

fn subordinate_character_anchor_lacks_resolution(compact: &str) -> bool {
    let starts_with_subordinate_clause = ["随着", "当", "如果", "一旦", "因为", "为了"]
        .iter()
        .any(|marker| compact.starts_with(marker));
    starts_with_subordinate_clause
        && ![
            "会", "将", "就", "便", "却", "而", "导致", "失去", "无法", "不能", "被迫",
        ]
        .iter()
        .any(|marker| compact.contains(marker))
}

fn character_anchor_ends_like_truncated_clause(compact: &str) -> bool {
    [
        "的", "以", "而", "为", "把", "被", "将", "向", "给", "从", "让", "用",
    ]
    .iter()
    .any(|suffix| compact.ends_with(suffix))
}

pub(crate) fn character_bottom_line_lacks_boundary_action(value: &str) -> bool {
    let compact = value.trim().replace(char::is_whitespace, "");
    if compact.is_empty()
        || value_missing(&compact)
        || character_anchor_uses_generic_placeholder(&compact)
    {
        return false;
    }
    if character_anchor_looks_like_storyline_or_truncated_surface(&compact) {
        return true;
    }
    if character_bottom_line_describes_unbounded_willingness(&compact) {
        return true;
    }
    let len = compact.chars().count();
    if len < 4 {
        return true;
    }
    if len < 8 && short_bottom_line_has_dangling_subject(&compact) {
        return true;
    }
    if short_bottom_line_has_only_relationship_target(&compact) {
        return true;
    }
    let has_boundary_marker = [
        "不", "拒绝", "必须", "不得", "禁止", "守住", "守护", "坚守", "保护", "保全", "救下",
        "护住", "维持", "维护", "确保", "捍卫", "宁可", "承诺", "坚持", "只", "仅",
    ]
    .iter()
    .any(|marker| compact.contains(marker));
    !has_boundary_marker
}

fn character_bottom_line_describes_unbounded_willingness(compact: &str) -> bool {
    if compact.contains("不惜代价") || compact.contains("不择手段") {
        return true;
    }
    let boundary_crossing = ["牺牲", "伤害", "欺骗", "伪造", "销毁", "出卖"];
    let describes_boundary_crossing = boundary_crossing.iter().any(|action| {
        compact.match_indices(action).any(|(index, _)| {
            let before = &compact[..index];
            ["愿意", "可以", "宁愿"]
                .iter()
                .any(|marker| before.ends_with(marker))
                || (before.ends_with('可')
                    && !before.ends_with("不可")
                    && !before.ends_with("宁可"))
        })
    });
    if !describes_boundary_crossing {
        return false;
    }
    let explicit_contrasting_boundary =
        ["但是", "但", "却", "不过", "然而"].iter().any(|contrast| {
            compact.split_once(contrast).is_some_and(|(_, boundary)| {
                [
                    "不", "不可", "不得", "拒绝", "禁止", "必须", "守住", "守护", "保护", "确保",
                    "捍卫", "坚守",
                ]
                .iter()
                .any(|marker| boundary.contains(marker))
            })
        });
    !explicit_contrasting_boundary
}

fn short_bottom_line_has_only_relationship_target(compact: &str) -> bool {
    ["只与", "只和", "仅与", "仅和", "不与", "不和"]
        .iter()
        .any(|prefix| {
            compact.strip_prefix(prefix).is_some_and(|target| {
                (2..=4).contains(&target.chars().count())
                    && target.chars().all(surface_gate::is_cjk_unified)
            })
        })
}

fn short_bottom_line_has_dangling_subject(compact: &str) -> bool {
    ["不能", "不可", "绝不", "不", "拒绝", "必须"]
        .iter()
        .filter_map(|marker| compact.find(marker).map(|index| (marker, index)))
        .any(|(marker, index)| {
            let subject_len = compact[..index].chars().count();
            let predicate_len = compact[index + marker.len()..].chars().count();
            subject_len >= 2 && predicate_len <= 1
        })
}

fn concessive_character_anchor_has_resolution(compact: &str) -> bool {
    [
        "也不", "也要", "仍不", "仍要", "绝不", "绝要", "不逃", "不退", "守住",
    ]
    .iter()
    .any(|marker| compact.contains(marker))
}

fn story_field_contains_truncated_role_action(value: &str) -> bool {
    let compact = value.replace(char::is_whitespace, "");
    if compact.is_empty() {
        return false;
    }
    if contains_truncated_power_loss_fragment(&compact) {
        return true;
    }
    if contains_orphaned_choice_suffix_before_action(&compact) {
        return true;
    }
    for role_noun in ["竞争对手", "对手", "反派", "敌人", "敌手"] {
        for stop in ["，", "。", "；", ",", ";", "、"] {
            if compact.contains(&format!("要{role_noun}{stop}")) {
                return true;
            }
        }
        if compact.ends_with(&format!("要{role_noun}")) {
            return true;
        }
    }
    false
}

fn contains_orphaned_choice_suffix_before_action(compact: &str) -> bool {
    const ACTIONS: &[&str] = &[
        "接受", "保留", "保护", "查明", "调查", "关闭", "公开", "恢复", "获得", "激活", "拒绝",
        "接管", "揭露", "进入", "离开", "启动", "切断", "牺牲", "删除", "释放", "上传", "提交",
        "停止", "终止", "执行", "追查", "封存", "修复", "销毁", "改写",
    ];

    let chars = compact.chars().collect::<Vec<_>>();
    for (index, current) in chars.iter().enumerate() {
        if *current != '择' {
            continue;
        }
        if index > 0 && matches!(chars[index - 1], '选' | '抉') {
            continue;
        }
        let suffix = chars[index + 1..].iter().collect::<String>();
        if ACTIONS.iter().any(|action| suffix.starts_with(action)) {
            return true;
        }
    }
    false
}

fn story_field_contains_incomplete_authority_rights_fragment(
    value: &str,
    authority_names: &[&str],
) -> bool {
    let compact = value.replace(char::is_whitespace, "");
    if compact.is_empty() || authority_names.is_empty() {
        return false;
    }
    for name in authority_names {
        let name = name.trim();
        if value_missing(name) {
            continue;
        }
        for object in [
            "控制权",
            "所有权",
            "处置权",
            "管理权",
            "继承权",
            "资格",
            "名额",
            "权限",
        ] {
            if compact.contains(&format!("{name}{object}"))
                || compact.contains(&format!("{name}的{object}"))
            {
                return true;
            }
        }
    }
    false
}

fn story_field_contains_dangling_authority_subject_fragment(
    value: &str,
    authority_names: &[&str],
) -> bool {
    let compact = value.replace(char::is_whitespace, "");
    if compact.is_empty() || authority_names.is_empty() {
        return false;
    }
    for name in authority_names {
        let name = name.trim();
        if value_missing(name) {
            continue;
        }
        if compact
            .split(|ch| matches!(ch, '。' | '；' | ';' | '→'))
            .any(|clause| clause == name)
        {
            return true;
        }
        for leading_punct in ["，", ",", "；", ";"] {
            for trailing_punct in ["，", ",", "；", ";"] {
                let orphaned_subject = format!("{leading_punct}{name}{trailing_punct}");
                let Some((_, after_subject)) = compact.split_once(&orphaned_subject) else {
                    continue;
                };
                if ["但", "而", "却", "并", "仍", "则"]
                    .iter()
                    .any(|connector| after_subject.starts_with(connector))
                {
                    return true;
                }
            }
        }
        for marker in [
            "然后", "最终", "于是", "随后", "并且", "同时", "或者", "或是", "或",
        ] {
            if compact.ends_with(&format!("{marker}{name}")) {
                return true;
            }
            for punct in ["，", ",", "；", ";", "、"] {
                if compact.contains(&format!("{marker}{name}{punct}")) {
                    return true;
                }
            }
        }
    }
    false
}

fn story_field_authority_name_glued_to_ascii_entity(
    value: &str,
    authority_names: &[&str],
) -> Option<String> {
    let compact = value.replace(char::is_whitespace, "");
    if compact.is_empty() || authority_names.is_empty() {
        return None;
    }
    authority_names.iter().find_map(|name| {
        let name = name.trim();
        if value_missing(name) || !name.chars().any(surface_gate::is_cjk_unified) {
            return None;
        }
        compact.match_indices(name).find_map(|(index, _)| {
            let tail = &compact[index + name.len()..];
            if !tail
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_alphanumeric())
            {
                return None;
            }
            let entity = tail
                .chars()
                .take_while(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
                .take(24)
                .collect::<String>();
            Some(format!("{name}{entity}"))
        })
    })
}

fn contains_truncated_power_loss_fragment(compact: &str) -> bool {
    let chars = compact.chars().collect::<Vec<_>>();
    for index in 0..chars.len().saturating_sub(2) {
        if chars[index] != '力' || chars[index + 1] != '受' || chars[index + 2] != '损' {
            continue;
        }
        let previous = index.checked_sub(1).and_then(|pos| chars.get(pos)).copied();
        if previous.is_some_and(|ch| {
            matches!(
                ch,
                '势' | '能' | '体' | '法' | '灵' | '财' | '权' | '战' | '实' | '精'
            )
        }) {
            continue;
        }
        return true;
    }
    false
}

pub(crate) fn world_rule_looks_truncated_or_not_actionable(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() {
        return false;
    }
    let compact = value.replace(char::is_whitespace, "");
    if compact.ends_with('-') || compact.ends_with('—') || compact.ends_with('…') {
        return true;
    }
    if compact.contains("->") || compact.contains("然后") {
        return true;
    }
    if world_rule_uses_generic_placeholder(&compact) {
        return true;
    }
    if world_rule_clause_depends_on_previous(&compact) {
        return true;
    }
    for conditional in ["一旦", "如果", "若"] {
        let Some((_, consequence)) = compact.split_once(conditional) else {
            continue;
        };
        let separator = consequence
            .char_indices()
            .find(|(_, ch)| matches!(ch, '，' | ',' | '；' | ';'));
        let consequence_clause = separator
            .and_then(|(index, ch)| consequence.get(index + ch.len_utf8()..))
            .unwrap_or(consequence);
        let consequence_markers: &[&str] = if separator.is_some() {
            &[
                "就", "便", "会", "将", "必须", "只能", "需要", "需", "导致", "触发", "失去",
                "无法", "不能",
            ]
        } else {
            &["就", "便", "会", "将", "必须", "只能", "需要", "需"]
        };
        if consequence_markers
            .iter()
            .any(|marker| consequence_clause.contains(marker))
        {
            continue;
        }
        return true;
    }
    let has_rule_signal = [
        "会",
        "必须",
        "不能",
        "只能",
        "只有",
        "唯一",
        "需要",
        "需",
        "取决于",
        "决定",
        "绑定",
        "遵循",
        "依赖",
        "分配",
        "优先",
        "淘汰",
        "资格",
        "门槛",
        "代价",
        "限制",
        "失败",
        "后果",
        "消耗",
        "稀缺",
        "规则",
        "法则",
        "交易",
        "契约",
        "触发",
        "记录",
        "抽取",
        "反噬",
        "惩罚",
        "条件",
        "若",
        "如果",
        "一旦",
        "否则",
        "将",
        "越",
    ]
    .iter()
    .any(|marker| compact.contains(marker));
    if !has_rule_signal {
        return true;
    }
    false
}

pub(crate) fn world_rule_clause_depends_on_previous(value: &str) -> bool {
    let compact = value.replace(char::is_whitespace, "");
    [
        "否则", "不然", "但是", "然而", "而且", "并且", "同时", "从而", "因此", "所以",
    ]
    .iter()
    .any(|prefix| compact.starts_with(prefix))
        || [
            "但会",
            "但必须",
            "但只能",
            "但不能",
            "但不得",
            "但需要",
            "但需",
        ]
        .iter()
        .any(|prefix| compact.starts_with(prefix))
        || ["代价是", "代价为", "限制是", "限制为", "后果是", "后果为"]
            .iter()
            .any(|prefix| compact.starts_with(prefix))
        || [
            "则会",
            "则必须",
            "则只能",
            "则不能",
            "则不得",
            "则需要",
            "则需",
            "则触发",
            "则导致",
            "则失去",
            "则无法",
        ]
        .iter()
        .any(|prefix| compact.starts_with(prefix))
        || [
            "越会",
            "越需",
            "越需要",
            "越容易",
            "越难",
            "越无法",
            "越不能",
            "越可能",
            "越快",
            "越强",
            "越弱",
        ]
        .iter()
        .any(|prefix| compact.starts_with(prefix))
        || [
            "将会",
            "将被",
            "将导致",
            "将触发",
            "将失去",
            "将无法",
            "将不能",
            "将不得",
            "将永久",
        ]
        .iter()
        .any(|prefix| compact.starts_with(prefix))
}

pub(crate) fn world_rule_clause_completes_pending(value: &str) -> bool {
    let compact = value.replace(char::is_whitespace, "");
    world_rule_clause_depends_on_previous(&compact)
        || [
            "就", "便", "会", "必须", "只能", "需要", "需", "导致", "触发", "失去", "无法", "不能",
        ]
        .iter()
        .any(|prefix| compact.starts_with(prefix))
}

fn world_rule_uses_generic_placeholder(value: &str) -> bool {
    let compact = value.replace(char::is_whitespace, "");
    if compact.is_empty() {
        return false;
    }
    surface_sanitizer::contains_generic_contract_placeholder_residue(&compact)
        || [
            "世界规则",
            "规则1",
            "规则2",
            "规则3",
            "核心设定",
            "能力规则",
            "资源规则",
            "制度规则",
            "世界如何运行",
            "可执行规则",
        ]
        .iter()
        .any(|marker| placeholder_surface_matches(&compact, marker))
}

fn placeholder_surface_matches(value: &str, marker: &str) -> bool {
    if value == marker || value.contains(&format!("{marker}：")) {
        return true;
    }
    let Some(suffix) = value.strip_prefix(marker) else {
        return false;
    };
    !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit())
}

fn contract_list_is_only_repeating_anchor(values: &[String], anchor: &str) -> bool {
    let meaningful = values
        .iter()
        .filter(|value| !value_missing(value))
        .collect::<Vec<_>>();
    !meaningful.is_empty()
        && meaningful
            .iter()
            .all(|value| normalized_contract_text(value) == normalized_contract_text(anchor))
}

fn normalized_contract_text(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_whitespace() && !matches!(ch, '，' | ',' | '。' | ';' | '；' | '、'))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn cjk_title_surface_allows_common_reader_hook_punctuation() {
        let mut contract = NovelCreationContract::default();
        contract.language = "zh-CN".to_string();
        contract.title.canonical_title = "逆袭！我靠拆穿黑幕上位".to_string();
        contract.premise = "普通青年发现黑幕证据并逆袭上位。".to_string();

        assert!(
            !surface_gate::contract_title_surface_is_invalid_for_language(
                &contract.title.canonical_title,
                &contract
            )
        );
    }

    #[test]
    fn plot_surface_gate_blocks_internal_size_parameters_in_near_chapters() {
        let mut contract = NovelCreationContract::default();
        contract.language = "zh-CN".to_string();
        contract.outline.near_chapters.push(
            crate::tool::writing::creation_contract_model::ChapterSeedContract {
                number: Some(3),
                goal: "沈启遥寻求梁谨岚帮助进入图书馆".to_string(),
                expected_turn: "两人结盟但发现更大的阴谋。target_units=100000chapter_unit_target=2500expected_chapters=40".to_string(),
            },
        );
        let mut issues =
            ContractIssueList::new("contract.surface", ContractIssueKind::Skeleton, "contract");

        surface_gate::validate_creative_contract_field_pollution(&contract, &mut issues);

        assert!(issues.iter().any(|issue| {
            issue.kind == ContractIssueKind::Plot
                && issue.code == "contract.outline.surface"
                && issue.contains("近期章节包")
                && issue.contains("用户请求参数")
        }));
    }

    #[test]
    fn world_rule_allows_character_references_when_actionable() {
        assert!(!world_rule_looks_truncated_or_not_actionable(
            "主角每次借用残卷开门都必须抵押一段真实记忆，失败会引来宗门追踪。"
        ));
        assert!(!world_rule_looks_truncated_or_not_actionable(
            "对手只能通过灵脉账册冻结凡人入道名额，公开账册会触发宗门反噬。"
        ));
    }

    #[test]
    fn world_rule_allows_institutional_and_resource_rules() {
        assert!(!world_rule_looks_truncated_or_not_actionable(
            "学术头衔与行政权力深度绑定，副教授是获取独立实验室与招生资格的唯一门槛。"
        ));
        assert!(!world_rule_looks_truncated_or_not_actionable(
            "经费分配遵循马太效应，头部资源向拥有行政职务或资本背书的教授倾斜。"
        ));
    }

    #[test]
    fn world_rule_allows_conditional_consequence_rules() {
        assert!(!world_rule_looks_truncated_or_not_actionable(
            "若燃料耗尽，列车将永久停滞，所有乘客化为磷火尘埃。"
        ));
        assert!(!world_rule_looks_truncated_or_not_actionable(
            "若强行抵达但内心仍逃避，终点站将关闭，列车被迫进入无限循环的荒原轨道。"
        ));
        assert!(world_rule_looks_truncated_or_not_actionable(
            "开发商若隐瞒地基缺陷"
        ));
        assert!(world_rule_looks_truncated_or_not_actionable(
            "如果社区书店失去公益资格"
        ));
    }

    #[test]
    fn world_rule_allows_detailed_cost_and_consequence_rules() {
        assert!(!world_rule_looks_truncated_or_not_actionable(
            "进入非官方管道必须支付压力债，即向垄断公会缴纳双倍蒸汽费或提供等值机械零件，否则核心会锁定该区域并排出所有非授权人员。"
        ));
        assert!(!world_rule_looks_truncated_or_not_actionable(
            "特定版面可作为加密通讯信道，截获信号需匹配对应版面。"
        ));
        assert!(!world_rule_looks_truncated_or_not_actionable(
            "海雾越过警戒线后，岛上航道会交换真实方位与虚假方位。"
        ));
        assert!(world_rule_looks_truncated_or_not_actionable("越需寒玉压制"));
        assert!(!world_rule_looks_truncated_or_not_actionable(
            "越级挑战必须登记并支付十枚灵石。"
        ));
        assert!(!world_rule_looks_truncated_or_not_actionable(
            "将军不得私自调动守城军。"
        ));
        assert!(world_rule_looks_truncated_or_not_actionable(
            "将永久失去大型竞标资格"
        ));
        assert!(world_rule_looks_truncated_or_not_actionable(
            "代价是寿元减半"
        ));
        assert!(world_rule_looks_truncated_or_not_actionable(
            "后果为永久失去资格"
        ));
    }

    #[test]
    fn world_rule_still_blocks_generic_placeholders() {
        assert!(world_rule_looks_truncated_or_not_actionable("世界规则1"));
        assert!(world_rule_looks_truncated_or_not_actionable("核心设定："));
    }

    #[test]
    fn character_anchor_blocks_malformed_idiom_fragment() {
        assert!(character_anchor_looks_like_storyline_or_truncated_surface(
            "自己不为瓦全"
        ));
        assert!(!character_anchor_looks_like_storyline_or_truncated_surface(
            "宁为玉碎，不为瓦全"
        ));
        assert!(character_anchor_looks_like_storyline_or_truncated_surface(
            "唯利图的能源投机者"
        ));
        assert!(!character_anchor_looks_like_storyline_or_truncated_surface(
            "唯利是图的能源投机者"
        ));
    }

    #[test]
    fn character_anchor_blocks_dangling_clause_tail() {
        assert!(character_anchor_looks_like_storyline_or_truncated_surface(
            "不为逃避遗忘自我而背叛修复沉钟以找回失落的"
        ));
        assert!(character_anchor_looks_like_storyline_or_truncated_surface(
            "即使局势失控也要守住关键证据而"
        ));
        assert!(!character_anchor_looks_like_storyline_or_truncated_surface(
            "不为逃避遗忘自我而背叛沉钟修复"
        ));
        assert!(character_anchor_looks_like_storyline_or_truncated_surface(
            "随着老街消失"
        ));
        assert!(!character_anchor_looks_like_storyline_or_truncated_surface(
            "随着老街消失而失去与女儿和解的机会"
        ));
    }

    #[test]
    fn concise_character_arc_can_use_two_commas_without_becoming_storyline() {
        assert!(!character_anchor_looks_like_storyline_or_truncated_surface(
            "在谈判中被废黜，失去特权根基，沦为新秩序下的旁观者"
        ));
    }

    #[test]
    fn character_bottom_line_blocks_name_prefixed_fragment() {
        assert!(character_bottom_line_lacks_boundary_action("秦知棠不破"));
        assert!(character_bottom_line_lacks_boundary_action("证据不能丢"));
        assert!(character_bottom_line_lacks_boundary_action(
            "无论权贵还是贱民"
        ));
        assert!(character_bottom_line_lacks_boundary_action(
            "为达目的可牺牲棋子"
        ));
        assert!(character_bottom_line_lacks_boundary_action(
            "不惜代价保住平台垄断地位"
        ));
        assert!(character_bottom_line_lacks_boundary_action(
            "愿意销毁原始记录换取安全"
        ));
        assert!(!character_bottom_line_lacks_boundary_action(
            "为了大局可以牺牲亲人但不可牺牲民心"
        ));
        assert!(!character_bottom_line_lacks_boundary_action("不牺牲无辜"));
        assert!(!character_bottom_line_lacks_boundary_action(
            "秦知棠不能牺牲同伴换取胜利"
        ));
        assert!(!character_bottom_line_lacks_boundary_action(
            "只与项目负责人共享未公开的原始数据"
        ));
        assert!(character_bottom_line_lacks_boundary_action("只与沈砚"));
        assert!(!character_bottom_line_lacks_boundary_action(
            "只与沈砚共享已核验的案卷"
        ));
        assert!(!character_bottom_line_lacks_boundary_action(
            "绝不切断与核心数据库中原始记忆碎片的神经链接"
        ));
        assert!(!character_bottom_line_lacks_boundary_action(
            "坚守样本采集的无菌原则"
        ));
        assert!(!character_bottom_line_lacks_boundary_action(
            "维持遗迹结构的完整性"
        ));
        assert!(!character_bottom_line_lacks_boundary_action(
            "维护尚未公开的原始证据"
        ));
        assert!(!character_bottom_line_lacks_boundary_action(
            "守护祖父阿旺老人的安宁直至终老"
        ));
        assert!(!character_bottom_line_lacks_boundary_action(
            "绝不使用非认证供应商的次级原料，宁可承担违约赔偿也不牺牲质量信誉"
        ));
    }

    #[test]
    fn story_field_blocks_truncated_power_loss_fragment() {
        assert!(story_field_contains_truncated_role_action(
            "唐庭澜在董事会上公开证据，扳倒内鬼，唐庭澜力受损，主角重返核心圈"
        ));
        assert!(!story_field_contains_truncated_role_action(
            "唐庭澜在董事会上公开证据，扳倒内鬼，唐棠晚势力受损，主角重返核心圈"
        ));
    }

    #[test]
    fn story_field_blocks_orphaned_choice_suffix_before_action() {
        assert!(story_field_contains_truncated_role_action(
            "秦栖原择激活K-7协议，公开事故真相"
        ));
        assert!(!story_field_contains_truncated_role_action(
            "秦栖原选择激活K-7协议，公开事故真相"
        ));
        assert!(!story_field_contains_truncated_role_action(
            "秦栖原的抉择激活了团队内部的新冲突"
        ));
    }

    #[test]
    fn story_field_blocks_dangling_authority_subject_after_connector() {
        assert!(story_field_contains_dangling_authority_subject_fragment(
            "一桩活人诈尸案引出契书，然后岑曜珩，终结诅咒",
            &["岑曜珩"]
        ));
        assert!(!story_field_contains_dangling_authority_subject_fragment(
            "一桩活人诈尸案引出契书，然后岑曜珩终结诅咒",
            &["岑曜珩"]
        ));
        assert!(story_field_contains_dangling_authority_subject_fragment(
            "残卷引出旧案，然后辛闻序",
            &["辛闻序"]
        ));
        assert!(story_field_contains_dangling_authority_subject_fragment(
            "天灵根离体瞬间，岑屿安，但皮肤开始隐隐发烫",
            &["岑屿安"]
        ));
        assert!(!story_field_contains_dangling_authority_subject_fragment(
            "岑屿安侧身避开，但皮肤仍被灵火灼伤",
            &["岑屿安"]
        ));
        assert!(!story_field_contains_dangling_authority_subject_fragment(
            "岑屿安、祝栖桥与韩照澜联手破阵",
            &["岑屿安", "祝栖桥", "韩照澜"]
        ));
        assert!(!story_field_contains_dangling_authority_subject_fragment(
            "岑屿安，祝栖桥与韩照澜联手破阵",
            &["岑屿安", "祝栖桥", "韩照澜"]
        ));
        assert!(story_field_contains_dangling_authority_subject_fragment(
            "若诊所供电不稳或陶云弦，芯片会引发高烧幻听甚至脑死亡",
            &["陶云弦"]
        ));
        assert!(!story_field_contains_dangling_authority_subject_fragment(
            "若诊所供电不稳或陶云弦体温过高，芯片会引发高烧幻听甚至脑死亡",
            &["陶云弦"]
        ));
    }

    #[test]
    fn story_field_blocks_chinese_authority_name_glued_to_ascii_entity() {
        assert_eq!(
            story_field_authority_name_glued_to_ascii_entity(
                "林远K-7并非故障，而是观测站的异常协议",
                &["林远"]
            )
            .as_deref(),
            Some("林远K-7")
        );
        assert_eq!(
            story_field_authority_name_glued_to_ascii_entity(
                "林远发现K-7并非故障，而是观测站的异常协议",
                &["林远"]
            ),
            None
        );
        assert_eq!(
            story_field_authority_name_glued_to_ascii_entity(
                "K-7并非故障，而是观测站的异常协议",
                &["林远"]
            ),
            None
        );
    }

    #[test]
    fn typed_gate_blocks_character_anchor_referencing_unknown_person_name() {
        let mut contract: NovelCreationContract = serde_json::from_value(json!({
            "title": {
                "canonical_title": "旧城灵契",
                "rationale": "旧城来自主角生活入口，灵契来自终局重写城市契约。"
            },
            "language": "zh-CN",
            "genre": "都市言情",
            "brief": "都市言情，每章2500字，至少5万字。",
            "target_units": 50000,
            "chapter_unit_target": 2500,
            "premise": "顶级投行女总裁被迫签下职场契约，并在追查合同真相时面对关系选择。",
            "ending": {
                "desired_resolution": "主角公开合同黑箱，保住事业独立并完成情感选择。"
            },
            "protagonist_arc": "从只相信控制，到学会在信任中保留自我。",
            "world_imagery": "玻璃幕墙、深夜茶水间、被隐藏的合同条款。",
            "main_causal_spine": "合同威胁→职场反击→秘密揭露→关系选择→公开黑箱",
            "characters": [
                {
                    "canonical_name": "晏闻宁",
                    "role": "主角",
                    "desire": "保住事业独立",
                    "fear": "被情感牵制导致事业失败",
                    "bottom_line": "不以牺牲无辜者换取胜利"
                },
                {
                    "canonical_name": "谢澈舟",
                    "role": "关键关系角色",
                    "desire": "协助关键角色：钟栖舟，突破自我设限",
                    "fear": "无法保护重要的人",
                    "bottom_line": "不伪造证据"
                }
            ],
            "themes": ["事业和情感不能互相吞没"],
            "world_rules": ["合同条款会转化为职场资源、信任和声誉代价。"],
            "style_rules": ["第三人称有限视角，紧贴主角选择。"],
            "must_avoid": ["不要改名。"],
            "outline": {
                "raw_outline": "主角从合同威胁入局，最终公开黑箱并完成情感选择。",
                "volumes": [{"title":"合同入局","objective":"确认合同威胁","ending_change":"主角无法再置身事外"}],
                "near_chapters": [{"number":1,"goal":"主角看见第一条异常合同规则","expected_turn":"确认合同会改变身份"}]
            }
        }))
        .expect("contract");
        contract.normalize();

        let report = validate_novel_creation_contract(&contract);
        let issues = report.issues.join("；");
        assert!(issues.contains("权威表外角色 `钟栖舟`"), "{issues}");
    }

    #[test]
    fn typed_gate_does_not_treat_relationship_state_words_as_external_characters() {
        let mut contract: NovelCreationContract = serde_json::from_value(json!({
            "title": {
                "canonical_title": "夺账本翻盘",
                "rationale": "账本是主角发现资源黑幕的关键物件，翻盘对应终局公开黑账后改写晋级规则。"
            },
            "language": "zh-CN",
            "genre": "都市玄幻",
            "brief": "都市玄幻，每章2500字，至少5万字。",
            "target_units": 50000,
            "chapter_unit_target": 2500,
            "premise": "旧城区青年发现灵网晋级系统会转卖失败者资源。",
            "ending": {
                "desired_resolution": "主角公开灵网账本，击败垄断资源的财团，建立透明晋级规则。"
            },
            "protagonist_arc": "从只相信自己，到学会信任盟友，并在利己和利他之间找到平衡。",
            "world_imagery": "旧城灵网、资源账本、夜校考场。",
            "main_causal_spine": "拿到账本线索→进入夜校考场→追查资源黑幕→公开证据→改写晋级规则",
            "characters": [
                {
                    "canonical_name": "钟望宁",
                    "role": "主角",
                    "desire": "拿回被夺走的晋级资格",
                    "fear": "再次被制度吞掉成果",
                    "bottom_line": "不牺牲同伴换晋级",
                    "arc_start": "只相信自己",
                    "arc_end": "信任盟友并找到平衡"
                },
                {
                    "canonical_name": "许砚安",
                    "role": "关键同伴",
                    "desire": "查清旧城账本真相",
                    "fear": "证据再次被买断",
                    "bottom_line": "不伪造证据"
                }
            ],
            "themes": ["公平晋级不能建立在资源剥削上"],
            "world_rules": ["灵网会把失败者资源转卖给上层。", "夜校考场记录每次资源流向。"],
            "style_rules": ["第三人称有限视角，紧贴主角选择。"],
            "must_avoid": ["不要角色改名。"],
            "outline": {
                "raw_outline": "主角从旧城夜校入场，发现灵网资源黑幕，终局公开账本改写规则。",
                "volumes": [{"title":"旧城账本","objective":"确认灵网资源黑幕","ending_change":"主角无法再回到普通考生身份"}],
                "near_chapters": [{"number":1,"goal":"主角进入夜校考场并发现账本编号","expected_turn":"他确认晋级失败不是偶然"}]
            },
            "structured": {
                "relationship_ledger": [{
                    "characters": ["钟望宁", "许砚安"],
                    "relationship_type": "证据同盟",
                    "current_state": "从互相试探到学会信任盟友",
                    "desired_end_state": "在利己和利他之间找到平衡"
                }],
                "emotional_state_ledger": [{
                    "character": "钟望宁",
                    "current_emotion": "不甘但开始信任盟友",
                    "expected_next_shift": "从利己转向利他并找到平衡"
                }]
            }
        }))
        .expect("contract");
        contract.normalize();

        let report = validate_novel_creation_contract(&contract);
        let issues = report.issues.join("；");
        assert!(
            !issues.contains("任盟友") && !issues.contains("平衡"),
            "relationship state words must not be reported as external characters: {issues}"
        );
    }

    #[test]
    fn typed_gate_blocks_structured_fields_that_only_repeat_world_imagery() {
        let imagery = "霓虹灯下隐藏的古老符文，摩天大楼里的秘境空间，瞳孔中闪烁的法则纹路";
        let value = json!({
            "title": {
                "canonical_title": "旧城灵契",
                "rationale": "旧城来自主角生活入口，灵契来自最终以契约重塑都市法则的结局。"
            },
            "language": "zh-CN",
            "genre": "都市玄幻",
            "brief": "都市玄幻，每章2500字，至少5万字起",
            "target_units": 50000,
            "chapter_unit_target": 2500,
            "max_chapters_per_turn": 1,
            "premise": "旧城区旁听生发现城市灵契考试会吞掉失败者记忆。",
            "ending": {
                "desired_resolution": "主角公开灵契账本，击败垄断考试的财团，建立透明晋级规则。"
            },
            "protagonist_arc": "从被排挤的旁听生到公开规则漏洞的城市守约人。",
            "world_imagery": imagery,
            "main_causal_spine": "旁听入场→发现记忆账本→赢下灵契考试→公开财团黑账→重写晋级规则",
            "characters": [
                {
                    "canonical_name": "许闻",
                    "role": "主角",
                    "desire": "拿回被夺走的考试资格",
                    "fear": "再次被制度抹去",
                    "bottom_line": "不牺牲同伴",
                    "arc_start": "被旧城区排挤的旁听生",
                    "arc_end": "公开规则漏洞的城市守约人"
                },
                {
                    "canonical_name": "沈青萝",
                    "role": "关键同伴",
                    "desire": "证明父亲案卷被篡改",
                    "fear": "真相再次被买断",
                    "bottom_line": "不伪造证据"
                }
            ],
            "themes": ["公平晋级不能建立在记忆剥削上"],
            "world_rules": [imagery],
            "style_rules": ["第三人称有限视角，紧贴主角选择和关系压力"],
            "must_avoid": ["不要改名", "不要把工具日志写进正文"],
            "outline": {
                "raw_outline": "许闻从旧城区旁听名额入场，发现灵契考试吞噬失败者记忆，最终公开财团账本并改写晋级规则。",
                "volumes": [
                    {
                        "title": "旧城入场",
                        "objective": "拿到旁听资格并发现灵契考试异常。",
                        "ending_change": "许闻确认记忆账本存在。"
                    }
                ],
                "near_chapters": [
                    {
                        "number": 1,
                        "goal": "许闻进入旧城区考场，发现评分日志异常。",
                        "expected_turn": "他拿到第一条被删改的记忆账本编号。"
                    }
                ]
            },
            "structured": {
                "field_requirements": {
                    "power_progression": "required",
                    "resource_economy": "required",
                    "social_order": "required",
                    "time_model": "required",
                    "relationship_ledger": "required",
                    "emotional_contract": "required",
                    "payoff_matrix": "required",
                    "narration_contract": "required",
                    "scene_type_mix": "required",
                    "character_voice_ledger": "required",
                    "reader_promise": "required",
                    "conflict_pressure_curve": "required",
                    "motif_ledger": "required",
                    "reveal_schedule": "required",
                    "relationship_interaction_quotas": "required"
                },
                "emotional_contract": {
                    "primary_emotion": "不甘",
                    "emotional_promise": "从被排挤到公开规则漏洞",
                    "emotional_beats": ["被夺名额", "找到证据", "公开黑账"],
                    "ending_emotional_state": "不再需要隐忍"
                },
                "relationship_ledger": [{
                    "characters": ["许闻", "沈青萝"],
                    "relationship_type": "证据同盟",
                    "start_state": "互不信任",
                    "desired_end_state": "共同公开账本"
                }],
                "payoff_matrix": [{
                    "promise": "记忆账本编号",
                    "payoff_target": "公开账本",
                    "status": "planned"
                }],
                "power_progression": {
                    "system_name": imagery,
                    "levels": [],
                    "advancement_costs": [],
                    "bottlenecks": [],
                    "failure_consequences": [],
                    "anti_power_creep_rules": []
                },
                "resource_economy": {
                    "value_scale": imagery,
                    "currency": "",
                    "resource_types": []
                },
                "social_order": {
                    "rank_system": imagery,
                    "institutions": []
                },
                "time_model": {
                    "story_start_time": "第一章考前夜"
                },
                "narration_contract": {
                    "pov": "第三人称有限视角",
                    "narrative_distance": "贴近主角选择",
                    "dialogue_style": "用对白推进证据和关系"
                }
            }
        });
        let contract: NovelCreationContract = serde_json::from_value(value).expect("contract");

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(joined.contains("世界规则只是重复世界观意象"), "{joined}");
        assert!(joined.contains("成长体系只是重复世界观意象"), "{joined}");
        assert!(joined.contains("资源体系只是重复世界观意象"), "{joined}");
        assert!(joined.contains("社会秩序只是重复世界观意象"), "{joined}");
    }

    #[test]
    fn typed_gate_blocks_legal_contract_noise_in_genre() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract.genre = "都市爽文2.小说字数：不少于50,000字3.章节数量：按每章约2500字计算，共约20章第三条交付方式与时间1.乙方需在合同签订后日内完成初稿"
            .to_string();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(
            joined.contains("题材含有合同条款或交付协议残片"),
            "{joined}"
        );
    }

    #[test]
    fn typed_gate_blocks_truncated_book_title_embedded_in_story_phrase() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract.title.canonical_title = "改写现实的古代数".to_string();
        contract.title.rationale =
            "改写现实的古代数来自合同证据“主角挖掘出能改写现实的古代数据矿脉”，并连接终局选择。"
                .to_string();
        contract.brief = "主角挖掘出能改写现实的古代数据矿脉，并在终局守住新文明。".to_string();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(joined.contains("半截短语"), "{joined}");
    }

    #[test]
    fn typed_gate_blocks_external_name_inside_character_anchor_example() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        let antagonist = contract
            .characters
            .iter_mut()
            .find(|character| character.role.contains("对手"))
            .expect("antagonist");
        antagonist.fear = "被外部变量（如对手：林远，使用的探测仪）干扰".to_string();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(joined.contains("权威表外角色 `林远`"), "{joined}");
    }

    #[test]
    fn typed_gate_blocks_external_names_in_skeleton_and_outline_fields() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract.ending.desired_resolution =
            "盟友：顾寒舟，辞官并与主角共同守住医馆和公开证据。".to_string();
        contract.main_causal_spine =
            "医馆遇袭→盟友：顾寒舟，介入旧案→主角取得证据→公开真相".to_string();
        contract.outline.raw_outline =
            "主角追查旧案，盟友：顾寒舟，调查失踪卷宗，终局共同公开证据。".to_string();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(
            joined.contains("终局方向引用了角色权威表外角色 `顾寒舟`")
                || joined.contains("总主线因果链引用了角色权威表外角色 `顾寒舟`")
                || joined.contains("大纲引用了角色权威表外角色 `顾寒舟`"),
            "{joined}"
        );
    }

    #[test]
    fn typed_gate_blocks_standalone_superseded_codename_in_story_and_governance() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        let antagonist = contract
            .characters
            .iter_mut()
            .find(|character| character.role.contains("对手"))
            .expect("antagonist");
        antagonist.name_source = "generated_by_writing_tool_policy".to_string();
        antagonist.previous_names.push("K".to_string());
        contract.outline.raw_outline.push_str("，主角继续躲避K追杀");
        contract.structured.antagonist_pressure.primary_pressure = "K持续封锁底层区".to_string();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(joined.contains("已废弃角色名 `K`"), "{joined}");
    }

    #[test]
    fn typed_gate_blocks_meta_placeholder_in_payoff_matrix() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract.structured.payoff_matrix[0].payoff_target = "完成权威终局的不可逆变化".to_string();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(
            joined.contains("兑现矩阵") && joined.contains("规划占位语"),
            "payoff placeholders must not be exposed as a confirmable contract: {joined}"
        );
    }

    #[test]
    fn typed_gate_blocks_incomplete_payoff_matrix_entries() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract.structured.payoff_matrix[0].promise.clear();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(
            joined.contains("兑现矩阵第1项缺少具体承诺或伏笔"),
            "an array entry without a promise must not count as a ready payoff matrix: {joined}"
        );
    }

    #[test]
    fn locked_authority_gate_validates_present_payoff_matrix_entries() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract
            .structured
            .field_requirements
            .insert("payoff_matrix".to_string(), "strong".to_string());
        contract.structured.payoff_matrix[0].promise.clear();

        let report = validate_novel_creation_contract_for_scope(
            &contract,
            ContractReadinessScope::LockedAuthorityContract,
        );
        let joined = report.issues.join("\n");

        assert!(
            joined.contains("兑现矩阵第1项缺少具体承诺或伏笔"),
            "rolling enrichment may be absent before chapter one, but present entries must be valid: {joined}"
        );

        contract.structured.payoff_matrix.clear();
        let report = validate_novel_creation_contract_for_scope(
            &contract,
            ContractReadinessScope::LockedAuthorityContract,
        );
        assert!(
            report.is_ready(),
            "a missing strong rolling field must remain non-blocking before chapter one: {}",
            report.issues.join("\n")
        );
    }

    #[test]
    fn typed_gate_blocks_canonical_name_with_unrecognized_extra_name_character() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        let protagonist = contract
            .characters
            .iter_mut()
            .find(|character| character.role_looks_primary())
            .expect("protagonist");
        protagonist.canonical_name = "叶维棠".to_string();
        contract.outline.raw_outline = "叶维棠秋重生拒婚，逐步查清旧案并改写家族结局。".to_string();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(
            joined.contains("大纲引用了角色权威表外角色 `叶维棠秋`"),
            "{joined}"
        );
    }

    #[test]
    fn typed_gate_blocks_two_character_external_name_after_role_noun() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract.premise =
            "城市支教教师许望棠与对手：陈伯，因教育理念冲突，被迫共同解决辍学危机。".to_string();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(
            joined.contains("故事前提引用了角色权威表外角色 `陈伯`"),
            "{joined}"
        );
    }

    #[test]
    fn typed_gate_blocks_authority_name_plus_rights_object_without_action() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract.main_causal_spine =
            "旁听入场→发现记忆账本→财团许闻控制权→公开黑账→重写晋级规则".to_string();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(joined.contains("截断短语"), "{joined}");
    }

    #[test]
    fn typed_gate_blocks_authority_name_as_standalone_causal_clause() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        let primary = contract.characters[0].canonical_name.clone();
        contract.main_causal_spine =
            format!("发现名册异常；锁定墨迹来源；图稿被毁；{primary}；决口前夜公开原始证据");

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(joined.contains("含有角色名后接逗号的截断主语"), "{joined}");
    }

    #[test]
    fn typed_gate_blocks_user_request_controls_in_creative_contract_fields() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract
            .themes
            .push("异界修仙，每章2500字，至少5万字起".to_string());
        contract.structured.emotional_contract.emotional_promise =
            "异界修仙，每章2500字，至少5万字起".to_string();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(
            joined.contains("核心主题")
                && joined.contains("情感承诺")
                && joined.contains("用户请求参数"),
            "{joined}"
        );
    }

    #[test]
    fn typed_gate_blocks_contract_review_instructions_in_story_fields() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract.brief.push_str(
            "；如果最终用途与既定效果不同，合同必须明确可验证的改写、反转、重定向或代价因果",
        );

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(
            joined.contains("小说合同简述混入用户请求参数或流程说明"),
            "{joined}"
        );
    }

    #[test]
    fn typed_gate_blocks_user_request_controls_in_structured_contract_fields() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract.structured.power_progression.system_name =
            "异界修仙小说2.作品字数：不少于5万字3.每章2500字".to_string();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(
            joined.contains("结构化字段 `成长体系名` 混入用户请求参数或流程说明"),
            "{joined}"
        );
    }

    #[test]
    fn typed_gate_blocks_story_summary_in_compact_structured_slots() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract.premise =
            "凡人少年意外获得上古问道传承，在资源匮乏的边陲之地踏上修仙之路".to_string();
        contract.main_causal_spine =
            "传承觉醒→资源争夺→道心磨砺→秘境历练→法则领悟→仙途登顶".to_string();
        contract.structured.resource_economy.value_scale =
            "主角被迫入局并确认代价：凡人少年意外获得上古问道传承，在资源匮乏的边陲之地踏上修仙之路"
                .to_string();
        contract.structured.social_order.rank_system =
            "传承觉醒→资源争夺→道心磨砺→秘境历练→法则领悟→仙途登顶".to_string();
        if let Some(relation) = contract.structured.relationship_ledger.first_mut() {
            relation.relationship_type =
                "主角与对手围绕“传承觉醒→资源争夺→道心磨砺→秘境历练→法则领悟→仙途登顶”形成的关键压力关系"
                    .to_string();
        }

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(joined.contains("结构化字段 `资源尺度`"), "{joined}");
        assert!(joined.contains("结构化字段 `社会等级/秩序`"), "{joined}");
        assert!(joined.contains("关系类型像把剧情摘要"), "{joined}");
    }

    #[test]
    fn typed_gate_blocks_legal_contract_noise_in_nested_structured_fields() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract.structured.power_progression.system_name =
            "异界言情小说。第三条创作周期1.乙方应于合同签订后[具体天数]日内完成初稿，并提交甲方审阅。"
                .to_string();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(
            joined.contains("结构化字段含有法律合同、交付协议或甲乙方条款残片"),
            "{joined}"
        );
    }

    #[test]
    fn typed_gate_blocks_publish_terms_without_rejecting_generic_theme_language() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract
            .themes
            .push("将本作品或其部分章节在其他平台或渠道发布三".to_string());
        contract.themes.push("内容健康、积极".to_string());

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(
            joined.contains("主题2含有合同条款或交付协议残片"),
            "{joined}"
        );
        assert!(
            !joined.contains("主题3含有合同条款或交付协议残片"),
            "{joined}"
        );
    }

    #[test]
    fn typed_gate_blocks_default_aesthetic_contract_placeholders() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract.structured.scene_type_mix = Default::default();
        contract.structured.scene_type_mix.balance_rule =
            "根据题材在动作、对话、日常、揭示、情感和转折之间轮换，避免连续章节形态单一。"
                .to_string();
        contract.structured.character_voice_ledger.clear();
        contract.structured.reader_promise = Default::default();
        contract.structured.reader_promise.core_hook =
            "用当前题材的主角入口、核心矛盾、关键规则和结局承诺形成持续阅读期待。".to_string();
        contract.structured.conflict_pressure_curve = Default::default();
        contract.structured.motif_ledger.clear();
        contract.structured.reveal_schedule.clear();
        contract.structured.relationship_interaction_quotas.clear();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(joined.contains("缺少具体场景类型配比"), "{joined}");
        assert!(joined.contains("缺少角色声音表"), "{joined}");
        assert!(joined.contains("缺少读者期待/爽点合同"), "{joined}");
        assert!(joined.contains("缺少冲突升降压曲线"), "{joined}");
        assert!(joined.contains("缺少主题母题账本"), "{joined}");
        assert!(joined.contains("缺少信息揭示节奏表"), "{joined}");
        assert!(joined.contains("缺少角色关系互动配额"), "{joined}");
    }

    #[test]
    fn typed_gate_blocks_contract_slot_label_placeholders() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract.ending.desired_resolution = "终局方向".to_string();
        contract.ending.final_state = "终局状态".to_string();
        contract.protagonist_arc = "主角弧线".to_string();
        contract.world_imagery = "世界观意象".to_string();
        contract.main_causal_spine = "总主线因果链".to_string();
        contract.themes = vec!["核心主题".to_string()];
        contract.world_rules = vec!["世界规则".to_string()];
        contract.structured.reader_promise.core_hook =
            "读者追看主角如何让 `总主线因果链` 付出代价。".to_string();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(!report.is_ready(), "{joined}");
        assert!(joined.contains("缺少终局方向"), "{joined}");
        assert!(joined.contains("缺少主角弧线"), "{joined}");
        assert!(joined.contains("缺少世界观意象"), "{joined}");
        assert!(joined.contains("缺少总主线因果链"), "{joined}");
        assert!(joined.contains("合同槽位名占位"), "{joined}");
    }

    #[test]
    fn typed_gate_blocks_missing_governance_authority_fields() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract.themes.clear();
        contract.style_rules.clear();
        contract.must_avoid.clear();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(joined.contains("缺少核心主题"), "{joined}");
        assert!(joined.contains("缺少叙事风格"), "{joined}");
        assert!(joined.contains("缺少必须避免"), "{joined}");
    }

    #[test]
    fn typed_gate_blocks_near_chapter_goal_reused_as_expected_turn() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        let chapter = contract
            .outline
            .near_chapters
            .first_mut()
            .expect("near chapter");
        chapter.expected_turn = chapter.goal.clone();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(joined.contains("目标与预期转折重复"), "{joined}");
    }

    #[test]
    fn typed_gate_does_not_treat_empty_optional_final_state_as_placeholder_pollution() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract.ending.final_state.clear();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(!joined.contains("终局状态仍含角色或内容占位符"), "{joined}");
    }

    #[test]
    fn typed_gate_blocks_character_anchor_referencing_unknown_role_name() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        let antagonist = contract
            .characters
            .iter_mut()
            .find(|character| !character.role_looks_primary())
            .expect("non-primary character");
        antagonist.fear = "主角洛衡舟公开灵契账本".to_string();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(
            joined.contains("权威表外角色 `洛衡舟`"),
            "unknown role-prefixed names in character anchors must be repaired: {joined}"
        );
    }

    #[test]
    fn typed_gate_blocks_unqualified_unknown_person_name_in_character_anchor() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        let support = contract
            .characters
            .iter_mut()
            .find(|character| !character.role_looks_primary())
            .expect("non-primary character");
        support.desire = "帮助盟友：林晚晴，成长".to_string();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(
            joined.contains("权威表外角色 `林晚晴`"),
            "unqualified unknown person names in character anchors must be repaired: {joined}"
        );
    }

    #[test]
    fn typed_gate_blocks_non_primary_character_missing_bottom_line() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        let support = contract
            .characters
            .iter_mut()
            .find(|character| !character.role_looks_primary())
            .expect("non-primary character");
        support.bottom_line.clear();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(
            joined.contains("缺少底线锚点"),
            "non-primary character anchors must not be masked by generic defaults: {joined}"
        );
    }

    #[test]
    fn typed_gate_blocks_generic_non_primary_character_role() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        let support = contract
            .characters
            .iter_mut()
            .find(|character| !character.role_looks_primary())
            .expect("non-primary character");
        support.role = "角色".to_string();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(
            joined.contains("角色定位过于泛化"),
            "generic role labels must not pass as an authority role: {joined}"
        );
    }

    #[test]
    fn display_gate_allows_default_strong_enrichment_but_blocks_required_fields() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract.structured.reveal_schedule.clear();
        contract.structured.relationship_interaction_quotas.clear();
        contract.structured.reader_promise = Default::default();

        let display_report = validate_novel_creation_contract_for_scope(
            &contract,
            ContractReadinessScope::DisplayContract,
        );
        let full_report = validate_novel_creation_contract(&contract);

        assert!(
            display_report.is_ready(),
            "display gate should allow rolling execution details to be completed later: {}",
            display_report.issues.join("\n")
        );
        assert!(
            !full_report.is_ready(),
            "full longform enrichment gate should still require rolling execution details"
        );

        contract
            .structured
            .field_requirements
            .insert("reader_promise".to_string(), "required".to_string());
        let display_report = validate_novel_creation_contract_for_scope(
            &contract,
            ContractReadinessScope::DisplayContract,
        );
        assert!(
            !display_report.is_ready(),
            "display gate must still block missing required reader promise"
        );
    }

    #[test]
    fn locked_authority_gate_allows_strong_rolling_enrichment_fields() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract.structured.reveal_schedule.clear();
        contract.structured.relationship_interaction_quotas.clear();
        contract.structured.reader_promise = Default::default();

        let locked_report = validate_novel_creation_contract_for_scope(
            &contract,
            ContractReadinessScope::LockedAuthorityContract,
        );
        let full_report = validate_novel_creation_contract(&contract);

        assert!(
            locked_report.is_ready(),
            "locked authority should not block first-chapter writing on rolling enrichment fields: {}",
            locked_report.issues.join("\n")
        );
        assert!(
            !full_report.is_ready(),
            "full longform enrichment should still surface missing rolling fields"
        );
    }

    #[test]
    fn locked_authority_gate_requires_authored_structured_governance() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract.structured = Default::default();

        let display_report = validate_novel_creation_contract_for_scope(
            &contract,
            ContractReadinessScope::DisplayContract,
        );
        let locked_report = validate_novel_creation_contract_for_scope(
            &contract,
            ContractReadinessScope::LockedAuthorityContract,
        );

        assert!(
            display_report.is_ready(),
            "an unfinished structured contract may still be rendered for review: {}",
            display_report.issues.join("\n")
        );
        assert!(
            locked_report
                .issues
                .join("\n")
                .contains("缺少可执行的结构化治理内容"),
            "the same contract must not become chapter authority: {}",
            locked_report.issues.join("\n")
        );
        assert!(
            locked_report.issues.iter().any(|issue| {
                issue.kind == ContractIssueKind::Governance
                    && issue.code == "contract.structured_governance"
            }),
            "the existing staged repair coordinator must route the blocker to Governance"
        );
    }

    #[test]
    fn typed_gate_blocks_unregistered_character_names_inside_structured_contract() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract.structured.payoff_matrix[0].payoff_target =
            "对手：林浩然，情报网络瓦解".to_string();
        contract.structured.character_voice_ledger[1]
            .dialogue_rules
            .push("恐惧被对手：林浩然，再次买断证据".to_string());

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(
            joined.contains("权威表外角色 `林浩然`"),
            "structured contract text must not carry stale external character names: {joined}"
        );
    }

    #[test]
    fn typed_gate_keeps_authority_name_separate_from_two_character_predicate() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract.outline.raw_outline =
            "许闻与导师沈青萝解读灵契回执，随后公开财团篡改考试的证据。".to_string();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(
            !joined.contains("权威表外角色 `沈青萝解读`"),
            "a canonical authority name followed by a predicate must not become a new character: {joined}"
        );
    }

    #[test]
    fn typed_gate_keeps_authority_name_separate_from_progressive_action_marker() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract.outline.raw_outline =
            "许闻发现导师沈青萝正在解读灵契回执，随后公开财团篡改考试的证据。".to_string();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(
            !joined.contains("权威表外角色 `沈青萝正`"),
            "a canonical authority name followed by a progressive action marker must not become a new character: {joined}"
        );
    }

    #[test]
    fn typed_gate_keeps_authority_name_separate_from_single_character_predicate() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract.structured.payoff_matrix[0].payoff_target =
            "种子被沈青萝藏在古井底，最终由主角取回。".to_string();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(
            !joined.contains("权威表外角色 `沈青萝藏`"),
            "a canonical authority name followed by a predicate boundary must not become a new character: {joined}"
        );
    }

    #[test]
    fn typed_gate_does_not_treat_abstract_conflict_terms_as_character_names() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract
            .structured
            .conflict_pressure_curve
            .global_curve
            .push(crate::tool::writing::novel_contract_v2::PressureBeat {
                range: "第一卷".to_string(),
                pressure_level: "中".to_string(),
                function: "卷入一场关于记忆与时间的争夺战".to_string(),
            });
        contract.structured.antagonist_pressure.antagonists.push(
            crate::tool::writing::novel_contract_v2::AntagonistRecord {
                name: "秦砚北".to_string(),
                current_move: "继续推进关于记忆与时间的争夺战".to_string(),
                ..Default::default()
            },
        );
        contract
            .structured
            .emotional_contract
            .emotional_beats
            .push("第一次意识到时间的争夺并不是个人恩怨".to_string());

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(
            !joined.contains("权威表外角色 `时间`") && !joined.contains("权威表外角色 `时间的争`"),
            "ordinary abstract conflict terms must not be treated as character names: {joined}"
        );
    }

    #[test]
    fn typed_gate_does_not_treat_reader_promise_objects_as_character_names() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract.structured.reader_promise.core_hook =
            "读者追看主角如何把第一桶金滚成金条堆叠，再用资本证据反杀旧规则。".to_string();
        contract.structured.reader_promise.pleasure_points = vec![
            "金条堆叠".to_string(),
            "资本滚雪球".to_string(),
            "证据反杀".to_string(),
        ];
        contract.structured.reader_promise.curiosity_engine =
            "每次账户跳变都揭开一层旧规则的漏洞。".to_string();
        contract.structured.reader_promise.payoff_style =
            "用账目、回执和公开审计兑现爽点。".to_string();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(
            !joined.contains("权威表外角色 `金条堆叠`"),
            "reader promise object and payoff imagery must not be treated as character drift: {joined}"
        );
        assert!(report.is_ready(), "{joined}");
    }

    #[test]
    fn typed_gate_blocks_structured_field_marking_non_primary_as_primary() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract.structured.emotional_contract.primary_emotion =
            "主角沈青萝被旧城考场吞掉旁听资格，只能先假装服从。".to_string();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(
            joined.contains("把 `沈青萝` 标成主角"),
            "structured fields must not mark a non-primary authority character as protagonist: {joined}"
        );
    }

    #[test]
    fn typed_gate_does_not_scan_relationship_quota_schedule_as_character_names() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract.structured.relationship_interaction_quotas[0].cadence =
            "每2-3章推进一次信任或分歧".to_string();
        contract.structured.relationship_interaction_quotas[0].required_interaction =
            "共同核对一条账本证据，并因此在下一章推进关系变化".to_string();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(
            !joined.contains("章推进"),
            "relationship quota scheduling prose must not be treated as an external character: {joined}"
        );
    }

    #[test]
    fn typed_gate_does_not_treat_motif_or_reveal_imagery_as_external_character() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract.structured.motif_ledger[0].meaning =
            "水如铁浆在每次试炼后凝固，象征灵脉代价从隐性压力变成可见债务。".to_string();
        contract.structured.motif_ledger[0].payoff_target =
            "终局水如铁浆倒流，证明旧秩序可以被改写。".to_string();
        contract.structured.reveal_schedule[0].secret =
            "水如铁浆不是角色，而是灵脉债务显形后的世界规则证据。".to_string();
        contract.structured.reveal_schedule[0].reader_knows =
            "读者先看到水如铁浆凝固，再逐步知道它来自灵脉债务。".to_string();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(
            !joined.contains("权威表外角色 `水如铁浆`"),
            "world imagery in motif/reveal prose must not become a character blocker: {joined}"
        );
    }

    #[test]
    fn typed_gate_blocks_primary_name_mismatch_between_outline_and_character_ledger() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract.premise = "主角林逸意外获得龙族血脉，在都市丛林中崛起。".to_string();
        contract.outline.raw_outline =
            "主角林逸觉醒龙族血脉，最终公开血契账本并重写地下秩序。".to_string();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(
            joined.contains("故事前提中的主角名 `林逸` 与角色权威表主角 `许闻` 不一致"),
            "{joined}"
        );
        assert!(
            joined.contains("大纲中的主角名 `林逸` 与角色权威表主角 `许闻` 不一致"),
            "{joined}"
        );
    }

    #[test]
    fn typed_gate_blocks_authority_name_used_as_company_or_organization() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract.characters[0].canonical_name = "祝澈遥".to_string();
        contract.premise = "祝澈遥入职行业巨头‘祝澈遥’，在数据危机中完成职场逆袭。".to_string();
        contract.main_causal_spine =
            "祝澈遥入职遭遇危机，然后发现数据异常，最终重塑公司创新文化。".to_string();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(
            joined.contains("把角色权威名 `祝澈遥` 用作组织、地点或机构名"),
            "{joined}"
        );
    }

    #[test]
    fn typed_gate_blocks_truncated_concessive_character_anchor() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract.characters[0].bottom_line = "即使妥协".to_string();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(
            joined.contains("底线锚点像全书主线、截断残句或流程说明"),
            "{joined}"
        );
    }

    #[test]
    fn typed_gate_allows_complete_concessive_character_anchor() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract.characters[0].bottom_line = "即使崩塌也不逃".to_string();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(
            !joined.contains("底线锚点像全书主线、截断残句或流程说明"),
            "{joined}"
        );
    }

    #[test]
    fn typed_gate_does_not_treat_primary_pressure_event_as_character_reference() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract.structured.antagonist_pressure.primary_pressure = "王陵崩塌".to_string();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(
            !joined.contains("对手压力引用了角色权威表外角色"),
            "{joined}"
        );
    }

    #[test]
    fn typed_gate_blocks_short_passive_character_anchor_fragment() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract.characters[0].fear = "被自己".to_string();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(
            joined.contains("恐惧锚点像全书主线、截断残句或流程说明")
                || joined.contains("通用兜底动机"),
            "{joined}"
        );
    }

    #[test]
    fn typed_gate_blocks_dangling_temporal_character_anchor() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract.characters[1].fear = "害怕真相暴露后".to_string();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(
            joined.contains("恐惧锚点像全书主线、截断残句或流程说明"),
            "{joined}"
        );
    }

    #[test]
    fn typed_gate_blocks_truncated_role_action_in_story_fields() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract.ending.desired_resolution = "南栖安要竞争对手，接管核心业务部门。".to_string();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(joined.contains("截断动作短语"), "{joined}");
    }

    #[test]
    fn typed_gate_blocks_dangling_authority_subject_in_world_rules() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        let protagonist_name = contract.characters[0].canonical_name.clone();
        contract.world_rules.push(format!(
            "若诊所供电不稳或{protagonist_name}，芯片会引发高烧幻听甚至脑死亡"
        ));

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(
            joined.contains("世界规则") && joined.contains("截断主语"),
            "{joined}"
        );
    }

    #[test]
    fn typed_gate_blocks_long_lowercase_latin_fragment_in_chinese_contract() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract
            .world_rules
            .push("董事会投票权掌握在拥有核心业务利润分成的seniorpartners手中。".to_string());

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(joined.contains("英文残片"), "{joined}");
    }

    #[test]
    fn typed_gate_blocks_long_lowercase_latin_fragment_in_style_rules() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract
            .style_rules
            .push("避免纯视觉化的sterile描写".to_string());

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(joined.contains("叙事风格混入英文残片"), "{joined}");
    }

    #[test]
    fn typed_gate_blocks_title_case_latin_fragment_in_chinese_contract() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract.premise =
            "县剧院面临复演Deadline，技术员必须在验收前找回原始吊装账册。".to_string();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(joined.contains("英文残片"), "{joined}");
    }

    #[test]
    fn typed_gate_does_not_treat_primary_action_phrase_as_name() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract.main_causal_spine = "主角被陷害→觉醒能力→揭露阴谋→逆袭成功".to_string();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(!joined.contains("主角名 `被陷害`"), "{joined}");
    }

    #[test]
    fn typed_gate_trims_primary_name_before_following_clause_connector() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract.characters[0].canonical_name = "辛照野".to_string();
        contract.outline.raw_outline =
            "主角辛照野在异界大陆觉醒修炼天赋，拜入宗门后追查九重天阶真相。".to_string();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(!joined.contains("辛照野在"), "{joined}");
        assert!(
            !joined.contains("与角色权威表主角 `辛照野` 不一致"),
            "{joined}"
        );
    }

    #[test]
    fn typed_gate_blocks_stale_primary_name_inside_character_fields() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract.characters[0].canonical_name = "司衡砺".to_string();
        contract.characters[0].role = "主角".to_string();
        contract.characters[1].fear = "主角陆尘揭开源核真相".to_string();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(
            joined.contains("角色恐惧中的主角名 `陆尘` 与角色权威表主角 `司衡砺` 不一致")
                || joined
                    .contains("角色恐惧中的主角行动名 `陆尘` 与角色权威表主角 `司衡砺` 不一致"),
            "{joined}"
        );
    }

    #[test]
    fn typed_gate_blocks_short_bottom_line_without_boundary_action() {
        let value = base_ready_contract_json();
        let mut contract: NovelCreationContract = serde_json::from_value(value).expect("contract");
        contract.characters[1].bottom_line = "无论多忙".to_string();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(
            joined.contains("底线锚点缺少明确边界"),
            "short dangling bottom-line fragments must not pass as character anchors: {joined}"
        );
    }

    #[test]
    fn typed_gate_blocks_companion_as_antagonist_pressure_actor() {
        let mut value = base_ready_contract_json();
        value["structured"]["antagonist_pressure"] = json!({
            "primary_pressure": "财团考试垄断持续压制旧城考生",
            "antagonists": [
                {
                    "name": "沈青萝",
                    "goal": "封锁账本证据",
                    "resources": ["财团席位"],
                    "knowledge_state": "知道账本可以被篡改",
                    "current_move": "阻止主角公开证据",
                    "defeat_condition": "账本公开后失去垄断权"
                }
            ]
        });
        let contract: NovelCreationContract = serde_json::from_value(value).expect("contract");

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(
            joined.contains("对手压力把 `沈青萝`"),
            "antagonist pressure must not silently reuse a companion or relationship actor: {joined}"
        );
    }

    fn base_ready_contract_json() -> serde_json::Value {
        let mut value = json!({
            "title": {
                "canonical_title": "旧城灵契",
                "rationale": "旧城来自主角生活入口，灵契来自最终以契约重塑都市法则的结局。"
            },
            "language": "zh-CN",
            "genre": "都市玄幻",
            "brief": "旧城区旁听生卷入记忆抵押考试，追查财团账本并改写城市晋级规则。",
            "target_units": 50000,
            "chapter_unit_target": 2500,
            "max_chapters_per_turn": 1,
            "premise": "旧城区旁听生发现城市灵契考试会吞掉失败者记忆。",
            "ending": {
                "desired_resolution": "许闻公开灵契账本，击败垄断考试的财团，建立透明晋级规则。",
                "final_state": "旧城区考生不再被记忆账本剥削。"
            },
            "protagonist_arc": "许闻从被排挤的旁听生到公开规则漏洞的城市守约人。",
            "world_imagery": "霓虹灯下隐藏的古老符文，摩天大楼里的秘境空间，瞳孔中闪烁的法则纹路",
            "main_causal_spine": "旁听入场→发现记忆账本→赢下灵契考试→公开财团黑账→重写晋级规则",
            "characters": [
                {
                    "canonical_name": "许闻",
                    "role": "主角",
                    "desire": "拿回被夺走的考试资格",
                    "fear": "再次被制度抹去",
                    "bottom_line": "不牺牲同伴",
                    "arc_start": "被旧城区排挤的旁听生",
                    "arc_end": "公开规则漏洞的城市守约人"
                },
                {
                    "canonical_name": "沈青萝",
                    "role": "关键同伴",
                    "desire": "证明父亲案卷被篡改",
                    "fear": "真相再次被买断",
                    "bottom_line": "不伪造证据",
                    "arc_start": "只相信纸面证据",
                    "arc_end": "愿意公开承担风险"
                },
                {
                    "canonical_name": "周砚城",
                    "role": "关键对手",
                    "desire": "保住财团考试垄断",
                    "fear": "记忆账本公开",
                    "bottom_line": "不允许旧城考生越级",
                    "arc_start": "规则幕后掌控者",
                    "arc_end": "被迫面对公开审判"
                }
            ],
            "themes": ["公平晋级不能建立在记忆剥削上"],
            "world_rules": [
                "灵契考试每轮只能使用一次记忆抵押，抵押失败会遗失对应事件",
                "财团账本能篡改考生评分，但每次篡改都会留下灵纹回执",
                "公开账本必须集齐三枚考场印记，否则证据会被系统抹除"
            ],
            "style_rules": ["第三人称有限视角，紧贴主角选择和关系压力"],
            "must_avoid": ["不要改名", "不要把工具日志写进正文"],
            "outline": {
                "raw_outline": "许闻从旧城区旁听名额入场，发现灵契考试吞噬失败者记忆，最终公开财团账本并改写晋级规则。",
                "volumes": [
                    {
                        "title": "旧城入场",
                        "objective": "拿到旁听资格并发现灵契考试异常。",
                        "ending_change": "许闻确认记忆账本存在。"
                    }
                ],
                "near_chapters": [
                    {
                        "number": 1,
                        "goal": "许闻进入旧城区考场，发现评分日志异常。",
                        "expected_turn": "他拿到第一条被删改的记忆账本编号。"
                    }
                ]
            }
        });
        value["structured"] = json!({
                "field_requirements": {
                    "power_progression": "required",
                    "resource_economy": "required",
                    "social_order": "required",
                    "time_model": "required",
                    "relationship_ledger": "required",
                    "emotional_contract": "required",
                    "payoff_matrix": "required",
                    "narration_contract": "required"
                },
                "emotional_contract": {
                    "primary_emotion": "不甘",
                    "emotional_promise": "从被排挤到公开规则漏洞",
                    "emotional_beats": ["被夺名额", "找到证据", "公开黑账"],
                    "ending_emotional_state": "不再需要隐忍"
                },
                "relationship_ledger": [{
                    "characters": ["许闻", "沈青萝"],
                    "relationship_type": "证据同盟",
                    "start_state": "互不信任",
                    "desired_end_state": "共同公开账本"
                }],
                "payoff_matrix": [{
                    "promise": "记忆账本编号",
                    "payoff_target": "公开账本",
                    "status": "planned"
                }],
                "power_progression": {
                    "system_name": "灵契晋级",
                    "levels": ["旁听", "入场", "守约", "改约"],
                    "advancement_costs": ["抵押记忆", "公开证据"],
                    "bottlenecks": ["三枚考场印记"],
                    "failure_consequences": ["记忆缺失"],
                    "anti_power_creep_rules": ["每次越级必须付出可见代价"]
                },
                "resource_economy": {
                    "value_scale": "记忆抵押和灵纹回执构成核心资源",
                    "currency": "灵纹回执",
                    "resource_types": ["记忆抵押", "考场印记", "账本编号"]
                },
                "social_order": {
                    "rank_system": "财团考试委员会控制晋级",
                    "institutions": ["考试委员会", "旧城考场"]
                },
                "time_model": {
                    "story_start_time": "第一章考前夜"
                },
                "narration_contract": {
                    "pov": "第三人称有限视角",
                    "narrative_distance": "贴近主角选择",
                    "dialogue_style": "用对白推进证据和关系"
                },
                "scene_type_mix": {
                    "action": "考场冲突和追证行动承担主要推进",
                    "dialogue": "同伴互证和对手施压时使用对话推进信息",
                    "everyday": "旧城区生活细节作为高压后的短缓冲",
                    "reveal": "每卷至少揭开一层账本规则",
                    "emotional": "关键选择后给角色关系一次情绪落点",
                    "turning_point": "章尾用证据或规则代价造成不可逆变化",
                    "balance_rule": "动作、揭示、情感和日常缓冲轮换，避免连续两章同形态"
                },
                "character_voice_ledger": [
                    {
                        "character": "许闻",
                        "voice_style": "克制、短句多，先问证据再表态",
                        "dialogue_rules": ["压力下仍围绕证据和选择代价说话"]
                    },
                    {
                        "character": "沈青萝",
                        "voice_style": "冷静尖锐，常用案卷和账目比喻",
                        "dialogue_rules": ["不轻易承诺，但会指出事实漏洞"]
                    }
                ],
                "reader_promise": {
                    "core_hook": "底层旁听生用被篡改的记忆账本反杀财团考试垄断",
                    "pleasure_points": ["越级查账", "公开黑幕", "同伴互证", "规则反杀"],
                    "curiosity_engine": "每枚考场印记都揭开一次记忆账本的真实用途",
                    "payoff_style": "用证据链和制度漏洞兑现逆袭，而不是无代价开挂"
                },
                "conflict_pressure_curve": {
                    "global_curve": [
                        {"range": "第1-5章", "pressure_level": "低到中", "function": "旁听资格被夺并发现记忆账本"},
                        {"range": "第6-15章", "pressure_level": "中到高", "function": "财团封口并逼迫同盟公开选择"},
                        {"range": "第16-20章", "pressure_level": "高峰", "function": "公开账本反杀考试垄断"}
                    ],
                    "release_strategy": "每次高压后用旧城日常或同伴互怼短暂缓冲",
                    "peak_policy": "卷尾压力必须带来证据、关系或身份的不可逆变化"
                },
                "motif_ledger": [
                    {
                        "motif": "灵纹回执",
                        "meaning": "被系统记录却被财团篡改的底层证据",
                        "evolution": ["零散线索", "互证凭据", "公开黑账的证据链"],
                        "payoff_target": "终局公开账本时成为推翻垄断的核心凭据"
                    }
                ],
                "reveal_schedule": [
                    {
                        "secret": "灵契考试会吞噬失败者记忆",
                        "reader_knows": "第一卷中段知道代价存在",
                        "protagonist_knows": "第一章末拿到第一条账本编号",
                        "antagonist_knows": "周砚城从开局就知道账本可被篡改",
                        "reveal_window": "第1-5章逐步揭示",
                        "status": "planned"
                    }
                ],
                "relationship_interaction_quotas": [
                    {
                        "relationship": "证据同盟",
                        "characters": ["许闻", "沈青萝"],
                        "cadence": "每2-3章至少推进一次信任或分歧",
                        "next_due": "第2章",
                        "required_interaction": "共同核对一条账本证据，并因此产生信任变化"
                    }
                ]
        });
        value
    }

    #[test]
    fn typed_gate_blocks_duplicate_character_ids_and_relationship_self_edges() {
        let mut contract: NovelCreationContract =
            serde_json::from_value(base_ready_contract_json()).expect("contract");
        contract.characters[0].character_id = "character-1".to_string();
        contract.characters[1].character_id = "character-1".to_string();
        contract.structured.relationship_ledger[0].characters = vec![
            contract.characters[0].canonical_name.clone(),
            contract.characters[0].canonical_name.clone(),
        ];
        contract.structured.relationship_ledger[0].character_ids =
            vec!["character-1".to_string(), "character-1".to_string()];

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(
            joined.contains("重复 character_id `character-1`"),
            "{joined}"
        );
        assert!(joined.contains("角色与自身的关系边"), "{joined}");
        assert!(joined.contains("不能形成自环"), "{joined}");
    }

    #[test]
    fn typed_gate_blocks_relationship_ids_that_disagree_with_authority() {
        let mut contract: NovelCreationContract =
            serde_json::from_value(base_ready_contract_json()).expect("contract");
        contract.characters[0].character_id = "character-1".to_string();
        contract.characters[1].character_id = "character-2".to_string();
        contract.structured.relationship_ledger[0].characters = vec![
            contract.characters[0].canonical_name.clone(),
            contract.characters[1].canonical_name.clone(),
        ];
        contract.structured.relationship_ledger[0].character_ids =
            vec!["character-2".to_string(), "missing-character".to_string()];

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(
            joined.contains("未知 character_id `missing-character`"),
            "{joined}"
        );
        assert!(
            joined.contains("character_id 与角色权威表不一致"),
            "{joined}"
        );
    }

    #[test]
    fn typed_gate_blocks_intimate_relationship_destination_that_conflicts_with_role_authority() {
        let mut contract: NovelCreationContract =
            serde_json::from_value(base_ready_contract_json()).expect("contract");
        let related_name = contract.characters[1].canonical_name.clone();
        contract.characters[1].role = "关键对手".to_string();
        contract.structured.relationship_ledger[0].characters = vec![
            contract.characters[0].canonical_name.clone(),
            related_name.clone(),
        ];
        contract.structured.relationship_ledger[0].relationship_type =
            "从相互试探到相爱".to_string();
        contract.structured.relationship_ledger[0].desired_end_state = "结为伴侣".to_string();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(
            joined.contains(&format!(
                "关系账本把 `{related_name}` 的终局明确写成恋爱/伴侣关系"
            )),
            "{joined}"
        );
    }

    #[test]
    fn typed_gate_allows_dual_antagonist_and_relationship_role_authority() {
        let mut contract: NovelCreationContract =
            serde_json::from_value(base_ready_contract_json()).expect("contract");
        let related_name = contract.characters[1].canonical_name.clone();
        contract.characters[1].role = "关键对手兼关键关系对象".to_string();
        contract.structured.relationship_ledger[0].characters = vec![
            contract.characters[0].canonical_name.clone(),
            related_name.clone(),
        ];
        contract.structured.relationship_ledger[0].relationship_type =
            "从竞争对手到相爱".to_string();
        contract.structured.relationship_ledger[0].desired_end_state = "结为伴侣".to_string();

        let report = validate_novel_creation_contract(&contract);
        let joined = report.issues.join("\n");

        assert!(
            !joined.contains(&format!(
                "关系账本把 `{related_name}` 的终局明确写成恋爱/伴侣关系"
            )),
            "{joined}"
        );
    }
}
