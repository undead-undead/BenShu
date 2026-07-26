use super::*;

pub fn sanitize_generated_contract_surface(
    draft: &SessionCreationDraftState,
    contract_text: &str,
) -> String {
    let language = draft.language.to_ascii_lowercase();
    let expects_chinese = language.starts_with("zh") || draft.language.contains("中文");
    if !expects_chinese {
        return contract_text.to_string();
    }
    let without_script_noise = contract_text
        .chars()
        .filter(|ch| !is_unexpected_non_cjk_script(*ch))
        .collect::<String>();
    surface_sanitizer::strip_generation_markup_noise(&without_script_noise)
        .replace("世界观意意象", "世界观意象")
        .replace("：本章注：本章目标：", "：本章目标：")
        .replace(":本章注:本章目标:", ":本章目标:")
        .replace("：本章注：本章目标", "：本章目标")
        .replace(":本章注:本章目标", ":本章目标")
        .lines()
        .map(sanitize_contract_line_prefix_noise)
        .filter(|line| !contract_line_is_assistant_surface_noise(line))
        .map(|line| normalize_chapter_plan_goal_label(&line))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn contract_line_is_assistant_surface_noise(line: &str) -> bool {
    surface_sanitizer::line_is_assistant_surface_noise(line)
}

pub(crate) fn sanitize_contract_line_prefix_noise(line: &str) -> String {
    let trimmed_start = line.trim_start();
    if matches!(trimmed_start, "来看" | "如下" | "以下" | "以上") {
        return String::new();
    }
    let indent_len = line.len().saturating_sub(trimmed_start.len());
    let indent = &line[..indent_len];
    for prefix in ["I*", "l*", "|*", "1*"] {
        if let Some(tail) = trimmed_start.strip_prefix(prefix) {
            if tail.starts_with(' ') || tail.starts_with('\t') {
                return format!("{indent}*{}", tail);
            }
        }
    }
    if let Some(index) = trimmed_start.find('第') {
        if (1..=8).contains(&index) {
            let tail = &trimmed_start[index..];
            if line_looks_like_explicit_chapter_plan(tail)
                || (tail.contains('章') && tail.contains("本章目标"))
            {
                return format!("{indent}{tail}");
            }
        }
    }
    line.to_string()
}

pub(crate) fn normalize_chapter_plan_goal_label(line: &str) -> String {
    let line = if line_looks_like_explicit_chapter_plan(line) {
        line.replace("本章法：", "本章目标：")
            .replace("本章法:", "本章目标:")
            .replace("本章目：", "本章目标：")
            .replace("本章目:", "本章目标:")
    } else {
        line.to_string()
    };
    if !line_looks_like_explicit_chapter_plan(&line)
        || line.contains("本章目标")
        || line.contains("章节目标")
    {
        return line;
    }

    let Some(goal_index) = line.find("目标") else {
        return line.to_string();
    };
    let prefix = &line[..goal_index];
    let suffix = &line[goal_index + "目标".len()..];
    if suffix
        .trim_start_matches(|ch| matches!(ch, '：' | ':' | ' ' | '\t'))
        .is_empty()
    {
        return line.to_string();
    }

    let delimiter = prefix
        .rfind('：')
        .map(|index| (index, '：'.len_utf8()))
        .or_else(|| prefix.rfind(':').map(|index| (index, ':'.len_utf8())));
    let Some((delimiter_index, delimiter_len)) = delimiter else {
        return line.to_string();
    };

    let mut normalized = String::new();
    normalized.push_str(&line[..delimiter_index + delimiter_len]);
    normalized.push_str("本章目标");
    if suffix.starts_with('：') || suffix.starts_with(':') {
        normalized.push_str(suffix);
    } else {
        normalized.push('：');
        normalized.push_str(suffix.trim_start());
    }
    normalized
}

pub(crate) fn unexpected_non_cjk_script_fragment(text: &str) -> Option<String> {
    let mut fragment = String::new();
    for ch in text.chars() {
        if is_unexpected_non_cjk_script(ch) {
            fragment.push(ch);
            if fragment.chars().count() >= 8 {
                break;
            }
        } else if !fragment.is_empty() {
            break;
        }
    }
    if fragment.is_empty() {
        None
    } else {
        Some(fragment)
    }
}

pub(crate) fn is_unexpected_non_cjk_script(ch: char) -> bool {
    matches!(
        ch,
        '\u{3040}'..='\u{30ff}' // Hiragana/Katakana
            | '\u{31f0}'..='\u{31ff}' // Katakana extensions
            | '\u{ac00}'..='\u{d7af}' // Hangul syllables
            | '\u{1100}'..='\u{11ff}' // Hangul jamo
            | '\u{0400}'..='\u{04ff}' // Cyrillic
            | '\u{0370}'..='\u{03ff}' // Greek
    )
}

pub(crate) fn latex_or_escape_residue_fragment(text: &str) -> Option<String> {
    let lowered = text.to_ascii_lowercase();
    for marker in [
        "rightarrow",
        "leftarrow",
        "\\l",
        "\\begin",
        "\\end",
        "\\text",
        "\\frac",
        "\\n",
    ] {
        if let Some(index) = lowered.find(marker) {
            return Some(compact_creation_text(&text[index..], 40));
        }
    }
    if let Some(index) = text.find('$') {
        return Some(compact_creation_text(&text[index..], 40));
    }
    None
}

pub(crate) fn cjk_underscore_fragment(text: &str) -> Option<String> {
    let chars = text.chars().collect::<Vec<_>>();
    for (index, ch) in chars.iter().enumerate() {
        if *ch != '_' {
            continue;
        }
        let prev_cjk = index
            .checked_sub(1)
            .and_then(|idx| chars.get(idx))
            .copied()
            .is_some_and(is_cjk_unified);
        let next_cjk = chars.get(index + 1).copied().is_some_and(is_cjk_unified);
        if prev_cjk || next_cjk {
            let start = index.saturating_sub(6);
            let end = (index + 7).min(chars.len());
            return Some(chars[start..end].iter().collect());
        }
    }
    None
}

pub(crate) fn malformed_contract_bullet_prefix_fragment(text: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim_start();
        if ["I*", "l*", "|*", "1*"].iter().any(|prefix| {
            trimmed
                .strip_prefix(prefix)
                .is_some_and(|tail| tail.starts_with(' ') || tail.starts_with('\t'))
        }) {
            return Some(compact_creation_text(trimmed, 80));
        }
        if trimmed
            .chars()
            .next()
            .is_some_and(|ch| is_cjk_unified(ch) && !matches!(ch, '*' | '-' | '+' | '#'))
            && trimmed.contains("**")
        {
            return Some(compact_creation_text(trimmed, 80));
        }
    }
    None
}

pub(crate) fn degenerate_repetition_fragment(text: &str) -> Option<String> {
    let chars = text
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<Vec<_>>();
    if chars.len() < 24 {
        return None;
    }

    for width in 1..=4 {
        let threshold = if width == 1 { 12 } else { 8 };
        let mut index = 0;
        while index + width * threshold <= chars.len() {
            let pattern = &chars[index..index + width];
            if pattern
                .iter()
                .all(|ch| matches!(ch, '，' | '。' | '、' | '：' | ':' | '-' | '*'))
            {
                index += 1;
                continue;
            }
            let mut repeats = 1;
            while index + width * (repeats + 1) <= chars.len()
                && chars[index + width * repeats..index + width * (repeats + 1)] == *pattern
            {
                repeats += 1;
            }
            if repeats >= threshold {
                let end = (index + width * repeats).min(chars.len());
                let fragment = chars[index..end].iter().collect::<String>();
                return Some(compact_creation_text(&fragment, 80));
            }
            index += width * repeats.max(1);
        }
    }
    None
}
