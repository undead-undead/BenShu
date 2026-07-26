use super::*;

pub(crate) fn fiction_contract_character_name_is_valid(name: &str) -> bool {
    let name = name.trim();
    if contract_name_scalar_is_malformed(name) {
        return false;
    }
    if contract_story_overlap_token_is_noise(name) {
        return false;
    }
    let language = if name.chars().any(is_cjk_unified) {
        "zh-CN"
    } else {
        "en"
    };
    naming::audit_character_name_candidate(name, language).accepted
}

pub(crate) fn fiction_contract_character_name_is_replaceable_source(name: &str) -> bool {
    let name = name.trim();
    if value_missing(name) || contract_name_scalar_is_malformed(name) {
        return false;
    }
    let char_count = name.chars().count();
    let has_cjk = name.chars().any(is_cjk_unified);
    if has_cjk && !(2..=4).contains(&char_count) {
        return false;
    }
    if !has_cjk && !(2..=32).contains(&char_count) {
        return false;
    }
    if !name
        .chars()
        .all(|ch| is_cjk_unified(ch) || ch.is_ascii_alphabetic())
    {
        return false;
    }
    !matches!(
        name,
        "主角"
            | "反派"
            | "配角"
            | "角色"
            | "人物"
            | "少年"
            | "少女"
            | "青年"
            | "男子"
            | "女子"
            | "男人"
            | "女人"
            | "自己"
            | "自身"
            | "自我"
    )
}

pub(crate) fn fiction_title_is_temporary_placeholder(title: &str) -> bool {
    let trimmed = title.trim().to_ascii_lowercase();
    trimmed.starts_with("未命名小说-")
        || trimmed.starts_with("untitled-fiction-")
        || trimmed == "untitled"
}

pub(crate) fn fiction_character_line_has_placeholder_name(line: &str) -> bool {
    let Some(name) = character_name_from_contract_line(line) else {
        return false;
    };
    let lowered = name.to_ascii_lowercase();
    name.contains("未命名")
        || name.contains("待命名")
        || lowered.contains("unnamed")
        || lowered.contains("placeholder")
}

pub(crate) fn fiction_primary_character_count(lines: &[String]) -> usize {
    lines
        .iter()
        .filter(|line| {
            let lowered = line.to_ascii_lowercase();
            line.contains("role: 主角")
                || line.contains("role：主角")
                || lowered.contains("role: protagonist")
                || lowered.contains("role：protagonist")
        })
        .count()
}
