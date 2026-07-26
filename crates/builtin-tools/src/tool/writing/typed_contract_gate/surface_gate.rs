use super::*;
use crate::tool::writing::creation_contract::issue::{ContractIssueKind, ContractIssueList};
use crate::tool::writing::surface_sanitizer;
use serde_json::Value;

pub(super) fn validate_creative_contract_field_pollution(
    contract: &NovelCreationContract,
    issues: &mut ContractIssueList,
) {
    let expects_chinese = contract.language.to_ascii_lowercase().starts_with("zh")
        || contract.language.contains("中文")
        || contract.story_basis_text().chars().any(is_cjk_unified);
    for (code, kind, field, label, value) in [
        (
            "contract.skeleton.surface",
            ContractIssueKind::Skeleton,
            "brief",
            "简述",
            contract.brief.as_str(),
        ),
        (
            "contract.skeleton.surface",
            ContractIssueKind::Skeleton,
            "premise",
            "故事前提",
            contract.premise.as_str(),
        ),
        (
            "contract.skeleton.surface",
            ContractIssueKind::Skeleton,
            "ending",
            "终局方向",
            contract.ending.desired_resolution.as_str(),
        ),
        (
            "contract.skeleton.surface",
            ContractIssueKind::Skeleton,
            "ending",
            "终局状态",
            contract.ending.final_state.as_str(),
        ),
        (
            "contract.characters.surface",
            ContractIssueKind::Characters,
            "protagonist_arc",
            "主角弧线",
            contract.protagonist_arc.as_str(),
        ),
        (
            "contract.skeleton.surface",
            ContractIssueKind::Skeleton,
            "world_imagery",
            "世界观意象",
            contract.world_imagery.as_str(),
        ),
        (
            "contract.skeleton.surface",
            ContractIssueKind::Skeleton,
            "main_causal_spine",
            "总主线因果链",
            contract.main_causal_spine.as_str(),
        ),
        (
            "contract.structured_governance.surface",
            ContractIssueKind::Governance,
            "structured.emotional_contract",
            "情感承诺",
            contract
                .structured
                .emotional_contract
                .emotional_promise
                .as_str(),
        ),
        (
            "contract.structured_governance.surface",
            ContractIssueKind::Governance,
            "structured.reader_promise",
            "读者期待",
            contract.structured.reader_promise.core_hook.as_str(),
        ),
        (
            "contract.outline.surface",
            ContractIssueKind::Plot,
            "outline.raw_outline",
            "大纲",
            contract.outline.raw_outline.as_str(),
        ),
    ] {
        issues.set_scope(code, kind, field);
        if creative_field_contains_user_request_controls(value) {
            issues.push(format!(
                "ContractBlocker: 小说合同{label}混入用户请求参数或流程说明，不能作为创作内容"
            ));
        }
        if !value_missing(value)
            && surface_sanitizer::contains_generic_contract_placeholder_residue(value)
        {
            issues.push(format!(
                "ContractBlocker: 小说合同{label}仍含角色或内容占位符"
            ));
        }
        if expects_chinese && contains_long_latin_fragment(value) {
            issues.push(format!("ContractBlocker: 中文小说合同{label}混入英文残片"));
        }
    }
    for (field, label, values) in [
        ("themes", "核心主题", contract.themes.as_slice()),
        ("world_rules", "世界规则", contract.world_rules.as_slice()),
        ("style_rules", "叙事风格", contract.style_rules.as_slice()),
        ("must_avoid", "必须避免", contract.must_avoid.as_slice()),
    ] {
        issues.set_scope(
            "contract.governance.surface",
            ContractIssueKind::Governance,
            field,
        );
        if values
            .iter()
            .any(|value| creative_field_contains_user_request_controls(value))
        {
            issues.push(format!(
                "ContractBlocker: 小说合同{label}混入用户请求参数或流程说明，不能作为创作内容"
            ));
        }
        if values
            .iter()
            .any(|value| super::outline_gate::outline_plan_text_is_placeholder(value))
        {
            issues.push(format!(
                "ContractBlocker: 小说合同{label}仍使用阶段证据、主线债务、终局变化等规划占位语，必须改写为具名角色执行具体行动并产生具体结果"
            ));
        }
        if expects_chinese
            && values
                .iter()
                .any(|value| contains_long_latin_fragment(value))
        {
            issues.push(format!("ContractBlocker: 中文小说合同{label}混入英文残片"));
        }
    }
    for (field, label, values) in [
        (
            "outline.volumes",
            "分卷规划",
            contract
                .outline
                .volumes
                .iter()
                .flat_map(|volume| [volume.objective.as_str(), volume.ending_change.as_str()])
                .collect::<Vec<_>>(),
        ),
        (
            "outline.near_chapters",
            "近期章节包",
            contract
                .outline
                .near_chapters
                .iter()
                .flat_map(|chapter| [chapter.goal.as_str(), chapter.expected_turn.as_str()])
                .collect::<Vec<_>>(),
        ),
        (
            "payoff_matrix",
            "兑现矩阵",
            contract
                .structured
                .payoff_matrix
                .iter()
                .flat_map(|entry| [entry.promise.as_str(), entry.payoff_target.as_str()])
                .collect::<Vec<_>>(),
        ),
    ] {
        issues.set_scope("contract.outline.surface", ContractIssueKind::Plot, field);
        if values
            .iter()
            .any(|value| creative_field_contains_user_request_controls(value))
        {
            issues.push(format!(
                "ContractBlocker: 小说合同{label}混入用户请求参数或流程说明，不能作为创作内容"
            ));
        }
        if field == "payoff_matrix"
            && values
                .iter()
                .any(|value| super::outline_gate::outline_plan_text_is_placeholder(value))
        {
            issues.push(
                "ContractBlocker: 小说合同兑现矩阵仍使用阶段证据、主线债务或权威终局等规划占位语，必须写成具体伏笔与具体兑现事件"
                    .to_string(),
            );
        }
        if field == "payoff_matrix"
            && values
                .iter()
                .any(|value| super::outline_gate::outline_text_is_polluted(value))
        {
            issues.push(
                "ContractBlocker: 小说合同兑现矩阵含有结构污染或合同栏目标题残留".to_string(),
            );
        }
        if expects_chinese
            && values
                .iter()
                .any(|value| contains_long_latin_fragment(value))
        {
            issues.push(format!("ContractBlocker: 中文小说合同{label}混入英文残片"));
        }
    }
}

pub(super) fn validate_primary_role_label_residue(
    contract: &NovelCreationContract,
    authority_names: &[&str],
    issues: &mut ContractIssueList,
) {
    if authority_names.is_empty() {
        return;
    }
    for (field, text) in [
        ("brief", contract.brief.as_str()),
        ("premise", contract.premise.as_str()),
        (
            "ending.desired_resolution",
            contract.ending.desired_resolution.as_str(),
        ),
        ("ending.final_state", contract.ending.final_state.as_str()),
        ("protagonist_arc", contract.protagonist_arc.as_str()),
        ("world_imagery", contract.world_imagery.as_str()),
        ("main_causal_spine", contract.main_causal_spine.as_str()),
    ] {
        if string_contains_primary_role_label_residue(text, authority_names) {
            issues.set_scope(
                "contract.skeleton.primary_role_label",
                ContractIssueKind::Skeleton,
                field,
            );
            issues.push(format!(
                "ContractBlocker: 小说合同字段仍残留“主角/主人公+角色名”的说明标签，必须归一为角色名本身：contract.{field}"
            ));
        }
    }
    if string_contains_primary_role_label_residue(&contract.title.rationale, authority_names) {
        issues.set_scope(
            "contract.title.primary_role_label",
            ContractIssueKind::Skeleton,
            "title.rationale",
        );
        issues.push(
            "ContractBlocker: 小说合同字段仍残留“主角/主人公+角色名”的说明标签，必须归一为角色名本身：contract.title.rationale"
                .to_string(),
        );
    }
    if string_contains_primary_role_label_residue(&contract.outline.raw_outline, authority_names) {
        issues.set_scope(
            "contract.outline.primary_role_label",
            ContractIssueKind::Plot,
            "outline.raw_outline",
        );
        issues.push(
            "ContractBlocker: 小说合同字段仍残留“主角/主人公+角色名”的说明标签，必须归一为角色名本身：contract.outline.raw_outline"
                .to_string(),
        );
    }
    for (index, value) in contract.world_rules.iter().enumerate() {
        if string_contains_primary_role_label_residue(value, authority_names) {
            issues.set_scope(
                "contract.governance.primary_role_label",
                ContractIssueKind::Governance,
                "world_rules",
            );
            issues.push(format!(
                "ContractBlocker: 小说合同字段仍残留“主角/主人公+角色名”的说明标签，必须归一为角色名本身：contract.world_rules[{index}]"
            ));
        }
    }
    for (index, character) in contract.characters.iter().enumerate() {
        for (field, value) in [
            ("desire", character.desire.as_str()),
            ("fear", character.fear.as_str()),
            ("bottom_line", character.bottom_line.as_str()),
            ("arc_start", character.arc_start.as_str()),
            ("arc_end", character.arc_end.as_str()),
        ] {
            if string_contains_primary_role_label_residue(value, authority_names) {
                issues.set_scope(
                    "contract.characters.primary_role_label",
                    ContractIssueKind::Characters,
                    "characters",
                );
                issues.push(format!(
                    "ContractBlocker: 小说合同字段仍残留“主角/主人公+角色名”的说明标签，必须归一为角色名本身：contract.characters[{index}].{field}"
                ));
            }
        }
    }
    for (index, volume) in contract.outline.volumes.iter().enumerate() {
        for (field, value) in [
            ("objective", volume.objective.as_str()),
            ("ending_change", volume.ending_change.as_str()),
        ] {
            if string_contains_primary_role_label_residue(value, authority_names) {
                issues.set_scope(
                    "contract.outline.primary_role_label",
                    ContractIssueKind::Plot,
                    "outline.volumes",
                );
                issues.push(format!(
                    "ContractBlocker: 小说合同字段仍残留“主角/主人公+角色名”的说明标签，必须归一为角色名本身：contract.outline.volumes[{index}].{field}"
                ));
            }
        }
    }
    for (index, chapter) in contract.outline.near_chapters.iter().enumerate() {
        for (field, value) in [
            ("goal", chapter.goal.as_str()),
            ("expected_turn", chapter.expected_turn.as_str()),
        ] {
            if string_contains_primary_role_label_residue(value, authority_names) {
                issues.set_scope(
                    "contract.outline.primary_role_label",
                    ContractIssueKind::Plot,
                    "outline.near_chapters",
                );
                issues.push(format!(
                    "ContractBlocker: 小说合同字段仍残留“主角/主人公+角色名”的说明标签，必须归一为角色名本身：contract.outline.near_chapters[{index}].{field}"
                ));
            }
        }
    }
}

fn string_contains_primary_role_label_residue(text: &str, authority_names: &[&str]) -> bool {
    let compact = text.replace(char::is_whitespace, "");
    if compact.is_empty() {
        return false;
    }
    authority_names.iter().any(|name| {
        let name = name.trim();
        !name.is_empty()
            && ["主角", "主人公", "男主", "女主"]
                .iter()
                .any(|marker| compact.contains(&format!("{marker}{name}")))
    })
}

fn creative_field_contains_user_request_controls(value: &str) -> bool {
    surface_sanitizer::contains_creation_request_control_residue(value)
        || ([
            "合同",
            "草案",
            "质量门",
            "可确认",
            "自动流程",
            "contract",
            "draft",
        ]
        .iter()
        .any(|marker| value.contains(marker))
            && crate::tool::writing::creation_contract::creation_planning_note_is_quality_feedback(
                value,
            ))
}

pub(super) fn contract_title_surface_is_invalid_for_language(
    title: &str,
    contract: &NovelCreationContract,
) -> bool {
    let len = title.chars().count();
    if !(2..=16).contains(&len) {
        return true;
    }
    let expects_chinese = contract.language.to_ascii_lowercase().starts_with("zh")
        || contract.language.contains("中文")
        || contract.story_basis_text().chars().any(is_cjk_unified);
    if expects_chinese
        && !title
            .chars()
            .all(|ch| is_cjk_unified(ch) || title_char_is_allowed_cjk_punctuation(ch))
    {
        return true;
    }
    false
}

fn title_char_is_allowed_cjk_punctuation(ch: char) -> bool {
    matches!(
        ch,
        '·' | '：' | ':' | '《' | '》' | '！' | '？' | '!' | '?' | '、'
    )
}

pub(super) fn is_cjk_unified(ch: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&ch)
        || ('\u{3400}'..='\u{4dbf}').contains(&ch)
        || ('\u{20000}'..='\u{2a6df}').contains(&ch)
}

pub(super) fn structured_contract_contains_legal_residue(
    structured: &super::super::novel_contract_v2::NovelContractV2,
) -> bool {
    let Ok(value) = serde_json::to_value(structured) else {
        return false;
    };
    value_contains_legal_residue(&value)
}

pub(super) fn structured_contract_surface_noise_paths(
    structured: &super::super::novel_contract_v2::NovelContractV2,
) -> Vec<String> {
    let Ok(value) = serde_json::to_value(structured) else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    collect_surface_noise_paths(&value, "structured", &mut paths);
    paths
}

fn value_contains_legal_residue(value: &Value) -> bool {
    match value {
        Value::String(text) => surface_sanitizer::contains_legal_contract_residue(text),
        Value::Array(items) => items.iter().any(value_contains_legal_residue),
        Value::Object(map) => map.values().any(value_contains_legal_residue),
        _ => false,
    }
}

fn collect_surface_noise_paths(value: &Value, path: &str, out: &mut Vec<String>) {
    match value {
        Value::String(text) => {
            if surface_sanitizer::contains_excessive_repeated_cjk_surface_noise(text) {
                out.push(path.to_string());
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                collect_surface_noise_paths(item, &format!("{path}[{index}]"), out);
            }
        }
        Value::Object(map) => {
            for (key, value) in map {
                collect_surface_noise_paths(value, &format!("{path}.{key}"), out);
            }
        }
        _ => {}
    }
}

pub(super) fn contains_latin_word(value: &str) -> bool {
    value
        .split(|ch: char| !ch.is_ascii_alphabetic())
        .any(|part| part.chars().filter(|ch| ch.is_ascii_alphabetic()).count() >= 3)
}

fn contains_long_latin_fragment(value: &str) -> bool {
    value
        .split(|ch: char| !ch.is_ascii_alphabetic())
        .any(|part| {
            let letters = part
                .chars()
                .filter(|ch| ch.is_ascii_alphabetic())
                .collect::<String>();
            letters.len() >= 6 && letters.chars().any(|ch| ch.is_ascii_lowercase())
        })
}
