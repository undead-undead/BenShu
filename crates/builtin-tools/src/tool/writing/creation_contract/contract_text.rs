#[cfg(test)]
use super::planning_gate::generated_fiction_contract_planning_issues;
use super::*;

mod chapter_plan;
mod field_pack;
mod missing;
mod name_surface;
mod surface_noise;

pub(crate) use chapter_plan::*;
pub(crate) use field_pack::*;
pub(crate) use missing::*;
pub(crate) use name_surface::*;
pub(crate) use surface_noise::*;

#[cfg(test)]
pub(super) fn fiction_contract_mentions_core_identity(text: &str) -> bool {
    let has_protagonist = text.contains("主角") || text.contains("主人公");
    let has_conflict = text.contains("核心矛盾") || text.contains("主要冲突");
    let has_ending = text.contains("结尾") || text.contains("结局") || text.contains("终局");
    has_protagonist && has_conflict && has_ending
}

#[cfg(test)]
pub(super) fn malformed_chapter_plan_fragment(text: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.contains("第第")
            || trimmed.contains("第并")
            || trimmed.contains("第，")
            || trimmed.contains("第,")
            || trimmed.contains("第、")
            || trimmed.contains("第. ")
            || trimmed.contains("第．")
        {
            return Some(preview_text(trimmed, 80));
        }
    }
    None
}

pub(super) fn assistant_surface_noise_fragment(text: &str) -> Option<String> {
    text.lines()
        .find(|line| contract_line_is_assistant_surface_noise(line))
        .map(|line| preview_text(line.trim(), 100))
}

pub(super) fn malformed_contract_name_fragment(text: &str) -> Option<String> {
    for label in [
        "书名",
        "标题",
        "主角",
        "主人公",
        "男主",
        "女主",
        "反派",
        "导师",
        "卷名",
        "Volume",
        "Title",
        "Protagonist",
        "Character",
    ] {
        let Some(value) = generated_contract_field(text, &[label]) else {
            continue;
        };
        let scalar = sanitize_generated_contract_scalar(&value);
        if scalar.is_empty() {
            continue;
        }
        if contract_name_scalar_is_malformed(&scalar) {
            return Some(format!("{label}：{}", preview_text(&scalar, 60)));
        }
    }
    None
}

pub(super) fn contract_name_scalar_is_malformed(value: &str) -> bool {
    let compact = value.trim();
    if compact.is_empty() {
        return true;
    }
    let placeholders = [
        "冲突点",
        "待定",
        "占位",
        "未指定",
        "由系统生成",
        "由 BenShu",
        "由 writer",
        "由 LLM",
        "根据用户",
        "暂无",
    ];
    if placeholders.iter().any(|term| compact.contains(term)) {
        return true;
    }
    if compact.ends_with('吗')
        || compact.ends_with('呢')
        || compact.ends_with('吧')
        || compact.ends_with('么')
        || compact.contains("是否")
    {
        return true;
    }
    false
}

#[cfg(test)]
pub(super) fn generated_title_is_contract_noise(
    _draft: &SessionCreationDraftState,
    contract_text: &str,
) -> Option<String> {
    let title = generated_contract_field(contract_text, &["书名", "标题", "Title"])?;
    let title = sanitize_generated_contract_scalar(&title);
    if title.trim().is_empty() {
        return Some(
            "书名像合同字段或为空；请根据剧情主线、结局、核心规则、关键地点或关键意象重新命名"
                .to_string(),
        );
    }
    if title_surface_is_meta_discussion(&title) {
        return Some(
            "书名像用户追问、合同诊断或元讨论；请根据剧情主线、结局、核心规则、关键地点或关键意象重新命名"
                .to_string(),
        );
    }
    if let Some(issue) = naming::title_formality_issue(&title, "书名") {
        return Some(issue);
    }
    None
}

#[cfg(test)]
pub(super) fn generated_title_reuses_protagonist_name(
    _draft: &SessionCreationDraftState,
    contract_text: &str,
) -> Option<String> {
    let title = generated_contract_field(contract_text, &["书名", "标题", "Title"])?;
    let normalized_title = normalize_cjk_identity(&title);
    if normalized_title.is_empty() {
        return None;
    }
    if generated_contract_character_names(contract_text)
        .iter()
        .map(|candidate| normalize_cjk_identity(candidate))
        .any(|candidate| candidate == normalized_title)
    {
        return Some(
            "书名直接复用了主角名；请根据剧情主线、结局、核心规则、关键地点或关键意象重新命名"
                .to_string(),
        );
    }
    None
}

pub(super) fn contract_story_overlap_token_is_noise(token: &str) -> bool {
    [
        "书名", "命名", "理由", "依据", "合同", "阶段", "章节", "目标", "质量", "导出", "原创",
        "语言", "一致", "主角", "成长", "主题", "象征", "体现", "人设", "命运", "逆袭",
    ]
    .iter()
    .any(|noise| token.contains(noise))
}
