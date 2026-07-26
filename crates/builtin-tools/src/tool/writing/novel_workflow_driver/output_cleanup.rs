use super::*;
pub(super) use crate::tool::writing::surface_sanitizer::{
    clean_cjk_markup_residue_line as clean_chinese_markup_residue_line,
    is_cjk_noise_boundary as is_chinese_noise_boundary, line_is_standalone_markup_residue,
    strip_short_escape_residue_near_cjk_line,
};
use crate::tool::writing::text_sanitizer;

pub(super) fn clean_model_output(raw: &str) -> String {
    clean_model_output_report(raw).text
}

pub(super) fn clean_model_output_report(raw: &str) -> text_sanitizer::SanitizeReport {
    let provider_report = text_sanitizer::sanitize_common_surface_report(
        raw,
        text_sanitizer::WritingSanitizeStage::ModelOutput,
    );
    let mut removed_lines = 0usize;
    let without_channel_tags = provider_report
        .text
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return Some(String::new());
            }
            if text_sanitizer::line_starts_with_provider_protocol_marker(trimmed) {
                removed_lines += 1;
                return None;
            }
            Some(line.to_string())
        })
        .collect::<Vec<_>>()
        .join("\n");
    let cleaned = unwrap_outer_fence(without_channel_tags.trim())
        .trim()
        .to_string();
    text_sanitizer::SanitizeReport::from_text(raw, cleaned)
        .merge(provider_report)
        .with_removed_lines(removed_lines)
}

pub(super) fn clean_provider_prompt(raw: &str) -> String {
    clean_provider_prompt_report(raw).text
}

pub(super) fn clean_provider_prompt_report(raw: &str) -> text_sanitizer::SanitizeReport {
    let provider_report = text_sanitizer::sanitize_common_surface_report(
        raw,
        text_sanitizer::WritingSanitizeStage::ProviderPrompt,
    );
    let mut removed_lines = 0usize;
    let cleaned = provider_report
        .text
        .lines()
        .filter_map(|line| {
            if text_sanitizer::line_starts_with_provider_protocol_marker(line) {
                removed_lines += 1;
                return None;
            }
            Some(line.to_string())
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    text_sanitizer::SanitizeReport::from_text(raw, cleaned)
        .merge(provider_report)
        .with_removed_lines(removed_lines)
}

pub(super) fn clean_stream_progress_text(raw: &str) -> String {
    clean_stream_progress_text_report(raw).text
}

pub(super) fn clean_stream_progress_text_report(raw: &str) -> text_sanitizer::SanitizeReport {
    let provider_report = text_sanitizer::sanitize_common_surface_report(
        raw,
        text_sanitizer::WritingSanitizeStage::StreamProgress,
    );
    let mut removed_lines = 0usize;
    let cleaned = provider_report
        .text
        .lines()
        .filter_map(|line| {
            if text_sanitizer::line_starts_with_provider_protocol_marker(line) {
                removed_lines += 1;
                return None;
            }
            Some(line.to_string())
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    text_sanitizer::SanitizeReport::from_text(raw, cleaned)
        .merge(provider_report)
        .with_removed_lines(removed_lines)
}

pub(super) fn sanitize_chapter_body(content: &str, title: &str, language: &str) -> String {
    sanitize_chapter_body_report(content, title, language).text
}

pub(super) fn sanitize_chapter_body_report(
    content: &str,
    title: &str,
    language: &str,
) -> text_sanitizer::SanitizeReport {
    let provider_input = content
        .replace("\\r\\n", "\n")
        .replace("\\n", "\n")
        .replace("\\r", "\n");
    let provider_report = text_sanitizer::sanitize_common_surface_report(
        &provider_input,
        text_sanitizer::WritingSanitizeStage::ChapterBody,
    );
    let content = surface_sanitizer::strip_json_string_line_wrappers(&provider_report.text);
    let content = strip_json_field_residue(&content).to_string();
    let content = strip_trailing_structured_metadata_block(&content);
    let content = strip_leading_json_content_field_wrapper(&content);
    let mut lines = Vec::new();
    let mut removed_lines = 0usize;
    for line in content.replace("\r\n", "\n").replace('\r', "\n").lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            lines.push(String::new());
            continue;
        }
        if trimmed
            .chars()
            .all(|ch| ch.is_ascii_whitespace() || matches!(ch, '\\' | '^' | '{' | '}' | '_' | '-'))
        {
            removed_lines += 1;
            continue;
        }
        if line_contains_placeholder_or_omission_marker(trimmed)
            || line_looks_like_generation_meta_note(trimmed)
            || crate::tool::writing::surface_sanitizer::line_looks_like_story_planning_meta(trimmed)
            || line_looks_like_json_artifact_residue(trimmed)
            || line_contains_provider_protocol_marker(trimmed)
            || line_looks_like_standalone_generation_residue(trimmed)
        {
            removed_lines += 1;
            continue;
        }
        let mut cleaned = line
            .replace("\\ ^{}", "")
            .replace("\\^{}", "")
            .replace("\\ {}", "")
            .trim_end()
            .to_string();
        if novel_runner::is_chinese_language(language) {
            let Some(next) = clean_chinese_markup_residue_line(&cleaned) else {
                removed_lines += 1;
                continue;
            };
            cleaned = next;
        }
        lines.push(cleaned);
    }
    let mut body = lines.join("\n");
    if novel_runner::is_chinese_language(language) {
        body = strip_embedded_model_chapter_heading_residue(&body, title);
    }
    let mut body = collapse_duplicate_leading_heading(&body, title);
    if novel_runner::is_chinese_language(language) {
        body = strip_standalone_chapter_end_markers(&body);
        body = surface_sanitizer::strip_inline_cjk_markup_noise(&body);
        body = strip_inline_markdown_emphasis_markers(&body);
        body = strip_spurious_escape_markers_near_cjk(&body);
        body = strip_unmatched_ascii_closing_bracket_in_cjk_lines(&body);
        body = strip_short_escape_residue_near_cjk(&body);
        body = strip_inline_json_key_prefix_residue_near_cjk(&body);
    }
    let cleaned = body.trim().to_string();
    text_sanitizer::SanitizeReport::from_text(&provider_input, cleaned)
        .merge(provider_report)
        .with_removed_lines(removed_lines)
}

pub(super) fn line_contains_inline_markdown_emphasis_residue(line: &str) -> bool {
    line_has_removable_inline_markdown_emphasis(line, "**")
        || line_has_removable_inline_markdown_emphasis(line, "__")
}

fn strip_inline_markdown_emphasis_markers(content: &str) -> String {
    if !content.contains("**") && !content.contains("__") {
        return content.to_string();
    }
    content
        .lines()
        .map(|line| {
            let line = strip_inline_markdown_emphasis_marker(line, "**");
            strip_inline_markdown_emphasis_marker(&line, "__")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn strip_inline_markdown_emphasis_marker(line: &str, marker: &str) -> String {
    if !line_has_removable_inline_markdown_emphasis(line, marker) {
        return line.to_string();
    }
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(start) = rest.find(marker) {
        out.push_str(&rest[..start]);
        let inner_start = start + marker.len();
        let after_start = &rest[inner_start..];
        let Some(end) = after_start.find(marker) else {
            out.push_str(&rest[start..]);
            return out;
        };
        let inner = &after_start[..end];
        if markdown_emphasis_inner_is_prose_token(inner) {
            out.push_str(inner);
            rest = &after_start[end + marker.len()..];
        } else {
            out.push_str(marker);
            rest = &rest[inner_start..];
        }
    }
    out.push_str(rest);
    out
}

fn line_has_removable_inline_markdown_emphasis(line: &str, marker: &str) -> bool {
    let mut rest = line;
    while let Some(start) = rest.find(marker) {
        let after_start = &rest[start + marker.len()..];
        let Some(end) = after_start.find(marker) else {
            return false;
        };
        if markdown_emphasis_inner_is_prose_token(&after_start[..end]) {
            return true;
        }
        rest = &after_start[end + marker.len()..];
    }
    false
}

fn markdown_emphasis_inner_is_prose_token(inner: &str) -> bool {
    let trimmed = inner.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 80 || trimmed.contains('\n') {
        return false;
    }
    trimmed
        .chars()
        .any(|ch| is_cjk_char(ch) || ch.is_ascii_alphanumeric())
}

pub(super) fn strip_standalone_chapter_end_markers(content: &str) -> String {
    content
        .lines()
        .filter(|line| !line_is_standalone_chapter_end_marker(line.trim()))
        .collect::<Vec<_>>()
        .join("\n")
}
pub(super) fn strip_unmatched_ascii_closing_bracket_in_cjk_lines(content: &str) -> String {
    content
        .lines()
        .map(|line| {
            let trimmed = line.trim_end();
            if !trimmed.ends_with(']') || trimmed.contains('[') || !trimmed.chars().any(is_cjk_char)
            {
                return line.to_string();
            }
            let mut cleaned = trimmed.to_string();
            while cleaned.ends_with(']') && !cleaned.contains('[') {
                cleaned.pop();
            }
            let trailing = &line[trimmed.len()..];
            format!("{cleaned}{trailing}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn line_is_standalone_chapter_end_marker(line: &str) -> bool {
    let compact = line
        .chars()
        .filter(|ch| !ch.is_whitespace() && !matches!(ch, '，' | ',' | '。' | '.'))
        .collect::<String>();
    if compact.is_empty() {
        return false;
    }
    if compact == "完" || compact == "本章完" || compact == "本章结束" {
        return true;
    }
    if compact.ends_with('完') {
        let prefix = compact.trim_end_matches('完');
        return cjk_chapter_heading_prefix(prefix);
    }
    if compact.ends_with("结束") {
        let prefix = compact.trim_end_matches("结束");
        return cjk_chapter_heading_prefix(prefix);
    }
    false
}

fn cjk_chapter_heading_prefix(value: &str) -> bool {
    let Some(rest) = value.strip_prefix('第') else {
        return false;
    };
    let Some(number_part) = rest.strip_suffix('章') else {
        return false;
    };
    !number_part.is_empty()
        && number_part.chars().all(|ch| {
            ch.is_ascii_digit()
                || matches!(
                    ch,
                    '零' | '一'
                        | '二'
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
                )
        })
}
pub(super) fn line_looks_like_standalone_generation_residue(trimmed: &str) -> bool {
    let compact = trimmed.trim();
    if compact.is_empty() || compact.chars().count() > 6 {
        return false;
    }
    if compact.chars().all(|ch| {
        ch.is_ascii_digit()
            || matches!(
                ch,
                '.' | ')' | '(' | '、' | '-' | '_' | '*' | '#' | ' ' | '\t'
            )
    }) {
        return true;
    }
    let lowered = compact.to_ascii_lowercase();
    matches!(
        lowered.as_str(),
        "i" | "ii" | "iii" | "iv" | "v" | "vi" | "vii" | "viii" | "ix" | "x"
    )
}
pub(super) fn line_contains_placeholder_or_omission_marker(line: &str) -> bool {
    let lowered = line.to_ascii_lowercase();
    const MARKERS: &[&str] = &[
        "此处省略",
        "此处应为",
        "省略后续",
        "以下省略",
        "略去",
        "待补充",
        "占位",
        "具体正文",
        "修订后的完整章节内容",
        "已剔除所有",
        "确保全文",
        "内容重点描述",
        "内容详细描述",
        "后续剧情",
        "未完待续",
        "omitted",
        "placeholder",
        "todo",
        "to be continued",
        "specific body text",
        "due to the character limit",
        "full body is truncated",
        "truncated here",
        "not shown in full",
        "cannot provide the full",
        "完整内容受限",
        "篇幅限制",
        "无法完整展示",
    ];
    MARKERS
        .iter()
        .any(|marker| line.contains(marker) || lowered.contains(&marker.to_ascii_lowercase()))
}

pub(super) fn strip_json_field_residue(content: &str) -> &str {
    let markers = [
        "\"addition\"",
        "\",\"addition\"",
        "\", \"addition\"",
        "\"content\"",
        "\",\"content\"",
        "\", \"content\"",
        "\",\"summary_delta\"",
        "\", \"summary_delta\"",
        "\"summary_delta\"",
        "\",\"summary\"",
        "\", \"summary\"",
        "\"key_facts\"",
        "\"key_facts",
        "key_facts:",
        "key_facts：",
        "\"continuity_updates\"",
        "\"continuity_updates",
        "continuity_updates:",
        "continuity_updates：",
        "chapter_end_state:",
        "chapter_end_state：",
    ];
    markers
        .iter()
        .filter_map(|marker| content.find(marker))
        .min()
        .map(|idx| &content[..idx])
        .unwrap_or(content)
}

pub(super) fn strip_leading_json_content_field_wrapper(content: &str) -> String {
    let leading_ws_len = content.len().saturating_sub(content.trim_start().len());
    let leading_ws = &content[..leading_ws_len];
    let trimmed = content.trim_start();
    let lowered = trimmed.to_ascii_lowercase();
    let Some(prefix_len) = bare_content_field_prefix_len(trimmed, &lowered) else {
        return content.to_string();
    };
    let mut body = trimmed[prefix_len..].trim_start();
    let quoted = body.starts_with('"') || body.starts_with('“') || body.starts_with('\'');
    if quoted {
        body = body
            .trim_start_matches(|ch| matches!(ch, '"' | '“' | '\'' | '‘'))
            .trim_start();
    }
    let mut cleaned = body.trim_end().to_string();
    if quoted {
        cleaned = cleaned
            .trim_end_matches(|ch| matches!(ch, '"' | '”' | '\'' | '’' | ',' | '，'))
            .trim_end()
            .to_string();
    }
    format!("{leading_ws}{cleaned}")
}

fn bare_content_field_prefix_len(trimmed: &str, lowered: &str) -> Option<usize> {
    for prefix in ["content", "正文"] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            let delimiter_len = rest
                .chars()
                .next()
                .filter(|ch| matches!(ch, ':' | '：'))
                .map(char::len_utf8)?;
            return Some(prefix.len() + delimiter_len);
        }
    }
    if lowered.starts_with("content:")
        || lowered.starts_with("content：")
        || lowered.starts_with("\"content\":")
        || lowered.starts_with("\"content\"：")
    {
        return trimmed.find([':', '：']).map(|idx| {
            idx + trimmed[idx..]
                .chars()
                .next()
                .map(char::len_utf8)
                .unwrap_or(1)
        });
    }
    None
}

pub(super) fn line_looks_like_generation_meta_note(line: &str) -> bool {
    let trimmed = line.trim_matches(|ch: char| {
        ch.is_whitespace()
            || matches!(
                ch,
                '(' | ')' | '（' | '）' | '[' | ']' | '【' | '】' | '"' | '“' | '”'
            )
    });
    if trimmed.is_empty() {
        return false;
    }
    let lowered = trimmed.to_ascii_lowercase();
    (trimmed.starts_with("此处") || lowered.starts_with("this "))
        && (trimmed.contains("完整章节")
            || trimmed.contains("正文")
            || trimmed.contains("内容")
            || trimmed.contains("修订")
            || lowered.contains("complete chapter")
            || lowered.contains("revised chapter")
            || lowered.contains("actual prose"))
}

pub(super) fn line_looks_like_json_artifact_residue(line: &str) -> bool {
    let trimmed = line.trim();
    let lowered = trimmed.to_ascii_lowercase();
    (trimmed.starts_with('"') || trimmed.starts_with(',') || trimmed.ends_with("\","))
        && (lowered.contains("summary_delta")
            || lowered.contains("addition")
            || lowered.contains("content")
            || lowered.contains("key_facts")
            || lowered.contains("continuity_updates")
            || lowered.contains("revision_notes"))
}

fn strip_trailing_structured_metadata_block(content: &str) -> String {
    let markers = [
        "**SummaryDelta:**",
        "**Summary Delta:**",
        "**KeyFacts:**",
        "**Key Facts:**",
        "**ContinuityUpdates:**",
        "**Continuity Updates:**",
        "SummaryDelta:",
        "Summary Delta:",
        "KeyFacts:",
        "Key Facts:",
        "ContinuityUpdates:",
        "Continuity Updates:",
        "summary_delta:",
        "key_facts:",
        "continuity_updates:",
        "chapter_end_state:",
        "摘要增量：",
        "摘要增量:",
        "关键事实：",
        "关键事实:",
        "连续性更新：",
        "连续性更新:",
        "章节结束状态：",
        "章节结束状态:",
        "章末状态记录：",
        "章末状态记录:",
        "章节状态记录：",
        "章节状态记录:",
    ];
    let Some(cut_at) = markers
        .iter()
        .filter_map(|marker| trailing_structured_metadata_marker_position(content, marker))
        .min()
    else {
        return content.to_string();
    };
    let before = content[..cut_at]
        .trim_end_matches(|ch: char| ch.is_whitespace() || matches!(ch, '*' | '_' | '#'))
        .trim_end();
    if before
        .chars()
        .any(|ch| ch.is_alphanumeric() || is_cjk_char(ch))
    {
        before.to_string()
    } else {
        content.to_string()
    }
}

fn trailing_structured_metadata_marker_position(content: &str, marker: &str) -> Option<usize> {
    content.match_indices(marker).find_map(|(index, _)| {
        let before = &content[..index];
        let after = &content[index + marker.len()..];
        if after.chars().count() > 2_000 {
            return None;
        }
        let line_prefix = before
            .rsplit_once('\n')
            .map(|(_, suffix)| suffix)
            .unwrap_or(before);
        let standalone_boundary = line_prefix
            .trim_matches(|ch: char| ch.is_whitespace() || matches!(ch, '*' | '_' | '#'))
            .is_empty();
        let prose_before_decoration =
            before.trim_end_matches(|ch: char| ch.is_whitespace() || matches!(ch, '*' | '_' | '#'));
        let decoration = &before[prose_before_decoration.len()..];
        let decorated_after_sentence = !decoration.is_empty()
            && prose_before_decoration
                .chars()
                .next_back()
                .is_some_and(|ch| matches!(ch, '。' | '！' | '？' | '.' | '!' | '?'));
        if !standalone_boundary && !decorated_after_sentence {
            return None;
        }
        let chinese_record_marker = marker.starts_with("章末状态记录")
            || marker.starts_with("章节状态记录")
            || marker.starts_with("章节结束状态")
            || marker.starts_with("摘要增量")
            || marker.starts_with("关键事实")
            || marker.starts_with("连续性更新");
        if chinese_record_marker
            && ![
                "地点",
                "人物",
                "角色",
                "状态",
                "下一章",
                "关键事实",
                "连续性",
                "摘要",
                "*",
                "-",
            ]
            .iter()
            .any(|signal| after.contains(signal))
        {
            return None;
        }
        Some(index)
    })
}

pub(super) fn line_contains_provider_protocol_marker(line: &str) -> bool {
    const MARKERS: &[&str] = &[
        "<|channel>",
        "<|channel|>",
        "<channel|>",
        "<|eot_id|>",
        "<|start_header_id|>",
        "<|end_header_id|>",
        "<|im_start|>",
        "<|im_end|>",
    ];
    MARKERS.iter().any(|marker| line.contains(marker))
}

pub(super) fn collapse_duplicate_leading_heading(content: &str, title: &str) -> String {
    let lines = content.lines().collect::<Vec<_>>();
    if lines.len() < 2 {
        return content.to_string();
    }
    let first = lines[0].trim();
    let mut second_index = 1usize;
    while second_index < lines.len() && lines[second_index].trim().is_empty() {
        second_index += 1;
    }
    if second_index >= lines.len() {
        return content.to_string();
    }
    let second = lines[second_index].trim();
    let heading = format!("# {}", title.trim());
    if !title.trim().is_empty() && first == heading && second == heading {
        let search_start = lines[0].len();
        if let Some(relative_pos) = content[search_start..].find(second) {
            let pos = search_start + relative_pos;
            return content[pos..].to_string();
        }
    }
    if first == heading && second.starts_with('#') {
        let mut body_start = second_index;
        while body_start < lines.len() {
            let trimmed = lines[body_start].trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                body_start += 1;
                continue;
            }
            break;
        }
        if body_start < lines.len() {
            let mut repaired = Vec::with_capacity(lines.len() - body_start + 1);
            repaired.push(first);
            repaired.push("");
            repaired.extend_from_slice(&lines[body_start..]);
            return repaired.join("\n");
        }
    }
    content.to_string()
}

fn strip_embedded_model_chapter_heading_residue(content: &str, title: &str) -> String {
    let mut changed = false;
    let mut cleaned = Vec::new();
    for line in content.lines() {
        if let Some(repaired) = strip_model_chapter_heading_residue_line(line, title) {
            changed = true;
            if !repaired.trim().is_empty() {
                cleaned.push(repaired);
            }
        } else {
            cleaned.push(line.to_string());
        }
    }
    if changed {
        cleaned.join("\n")
    } else {
        content.to_string()
    }
}

fn strip_model_chapter_heading_residue_line(line: &str, title: &str) -> Option<String> {
    let leading_ws_len = line.len() - line.trim_start().len();
    let leading_ws = &line[..leading_ws_len];
    let trimmed = line.trim_start();
    if trimmed.starts_with("# ") || trimmed.starts_with("#\t") {
        return None;
    }
    let after_hash = trimmed
        .strip_prefix('#')
        .map(str::trim_start)
        .unwrap_or(trimmed);
    let after_marker = strip_cjk_chapter_marker_prefix(after_hash)?;
    let rest = after_marker
        .trim_start_matches(|ch: char| {
            ch.is_whitespace() || matches!(ch, ':' | '：' | '-' | '—' | '《' | '》')
        })
        .trim_start();
    if rest.is_empty() {
        return Some(String::new());
    }
    if !title.trim().is_empty() {
        let title = title.trim();
        if let Some(after_title) = rest.strip_prefix(title) {
            return Some(format!("{leading_ws}{}", after_title.trim_start()));
        }
    }
    if let Some(prose) = split_glued_cjk_heading_from_prose(rest) {
        return Some(format!("{leading_ws}{}", prose.trim_start()));
    }
    let cjk_count = rest.chars().filter(|ch| is_cjk_char(*ch)).count();
    let has_sentence_boundary = rest
        .chars()
        .take(24)
        .any(|ch| matches!(ch, '。' | '！' | '？' | '；' | ';' | '.' | '!' | '?'));
    if cjk_count <= 16 && !has_sentence_boundary {
        return Some(String::new());
    }
    Some(format!("{leading_ws}{rest}"))
}

fn strip_cjk_chapter_marker_prefix(value: &str) -> Option<&str> {
    let value = value.trim_start();
    let after_di = value.strip_prefix('第')?;
    let mut end = 0usize;
    let mut saw_number = false;
    for (idx, ch) in after_di.char_indices() {
        if ch.is_ascii_digit()
            || matches!(
                ch,
                '零' | '〇'
                    | '一'
                    | '二'
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
                    | '两'
            )
        {
            saw_number = true;
            end = idx + ch.len_utf8();
            continue;
        }
        break;
    }
    if !saw_number {
        return None;
    }
    let rest = &after_di[end..];
    rest.strip_prefix('章')
        .or_else(|| rest.strip_prefix('回'))
        .or_else(|| rest.strip_prefix('节'))
}

fn split_glued_cjk_heading_from_prose(rest: &str) -> Option<&str> {
    let cjk_count = rest.chars().filter(|ch| is_cjk_char(*ch)).count();
    if cjk_count < 18 {
        return None;
    }
    let first_boundary = rest
        .char_indices()
        .take(20)
        .find_map(|(idx, ch)| matches!(ch, '。' | '！' | '？' | '；').then_some(idx));
    if first_boundary.is_some() {
        return None;
    }
    const PROSE_STARTERS: &[&str] = &[
        "清晨", "黎明", "黄昏", "深夜", "夜色", "天亮", "雨", "风", "雪", "阳光", "月光", "空气",
        "钟声", "脚步", "街道", "工坊", "房间", "门外", "窗外", "人群", "他", "她", "我", "他们",
        "她们",
    ];
    for (idx, _) in rest.char_indices().skip(2).take(14) {
        let suffix = &rest[idx..];
        if PROSE_STARTERS
            .iter()
            .any(|starter| suffix.starts_with(starter))
        {
            return Some(suffix);
        }
    }
    None
}
pub(super) fn strip_spurious_escape_markers_near_cjk(content: &str) -> String {
    let chars = content.chars().collect::<Vec<_>>();
    let mut out = String::new();
    for (index, ch) in chars.iter().enumerate() {
        if *ch == '\\' {
            let prev = index.checked_sub(1).and_then(|idx| chars.get(idx)).copied();
            let next = chars.get(index + 1).copied();
            if prev.is_none_or(is_chinese_noise_boundary)
                && next.is_some_and(|next| is_cjk_char(next) || is_chinese_noise_boundary(next))
            {
                continue;
            }
        }
        out.push(*ch);
    }
    out
}

pub(super) fn strip_short_escape_residue_near_cjk(content: &str) -> String {
    surface_sanitizer::strip_short_escape_residue_near_cjk(content)
}

pub(super) fn strip_inline_json_key_prefix_residue_near_cjk(content: &str) -> String {
    let chars = content.chars().collect::<Vec<_>>();
    let mut out = String::with_capacity(content.len());
    let mut index = 0usize;
    while index < chars.len() {
        if !json_boundary_char(chars[index]) {
            out.push(chars[index]);
            index += 1;
            continue;
        }
        let start = index;
        let previous = previous_non_whitespace_char(&chars, start);
        while index < chars.len() && json_boundary_char(chars[index]) {
            index += 1;
        }
        let key_start = index;
        while index < chars.len() && (chars[index].is_ascii_alphabetic() || chars[index] == '_') {
            index += 1;
        }
        let key = chars[key_start..index].iter().collect::<String>();
        if !json_field_key_prefix_fragment(&key) {
            out.extend(chars[start..index].iter());
            continue;
        }
        while index < chars.len() && json_boundary_char(chars[index]) {
            index += 1;
        }
        let next = chars.get(index).copied();
        if next.is_some_and(is_cjk_char)
            && previous.is_some_and(|ch| is_cjk_char(ch) || is_chinese_noise_boundary(ch))
        {
            continue;
        }
        out.extend(chars[start..index].iter());
    }
    out
}

fn json_boundary_char(ch: char) -> bool {
    matches!(
        ch,
        '"' | '\'' | ',' | '，' | ':' | '：' | '{' | '}' | '[' | ']' | '\\' | ' ' | '\t'
    )
}

fn json_field_key_prefix_fragment(value: &str) -> bool {
    let key = value.trim_start_matches('_').to_ascii_lowercase();
    if key.len() < 2 || key.len() > "continuity_updates".len() {
        return false;
    }
    const STRUCTURED_KEYS: &[&str] = &[
        "addition",
        "content",
        "text",
        "title",
        "summary",
        "summary_delta",
        "key_facts",
        "continuity_updates",
        "chapter_end_state",
        "revision_notes",
    ];
    STRUCTURED_KEYS
        .iter()
        .any(|candidate| candidate.starts_with(&key))
}

fn previous_non_whitespace_char(chars: &[char], before: usize) -> Option<char> {
    chars
        .get(..before)?
        .iter()
        .rev()
        .copied()
        .find(|ch| !ch.is_whitespace())
}
pub(super) fn is_cjk_char(ch: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&ch)
}
pub(super) fn unwrap_outer_fence(raw: &str) -> String {
    let trimmed = raw.trim();
    let Some(after_ticks) = trimmed.strip_prefix("```") else {
        return trimmed.to_string();
    };
    let Some(first_newline) = after_ticks.find('\n') else {
        return trimmed.to_string();
    };
    let body = &after_ticks[first_newline + 1..];
    let Some(end) = body.rfind("```") else {
        return trimmed.to_string();
    };
    if body[end + 3..].trim().is_empty() {
        body[..end].trim().to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chapter_body_sanitizer_removes_inline_json_key_prefix_residue() {
        let raw = "怀表的滴答声，在他耳边，从未停歇。\",\"su的滴答声在雨后的静谧中显得格外清晰。";

        let cleaned = sanitize_chapter_body(raw, "", "zh-CN");

        assert_eq!(
            cleaned,
            "怀表的滴答声，在他耳边，从未停歇。的滴答声在雨后的静谧中显得格外清晰。"
        );
        assert!(!cleaned.contains("\",\"su"));
    }

    #[test]
    fn chapter_body_sanitizer_removes_full_inline_json_key_residue() {
        let raw = "他推开门。\",\"summary_delta\":\"辛岑序确认了新线索。";

        let cleaned = sanitize_chapter_body(raw, "", "zh-CN");

        assert_eq!(cleaned, "他推开门。");
        assert!(!cleaned.contains("summary_delta"));
        assert!(!cleaned.contains("辛岑序确认了新线索"));
    }

    #[test]
    fn chapter_body_sanitizer_extracts_bare_content_field_package() {
        let raw = "content:\"他推开门，雨声落在门槛上。\"key_facts:-他推开门。continuity_updates:-位置变化。";

        let cleaned = sanitize_chapter_body(raw, "", "zh-CN");

        assert_eq!(cleaned, "他推开门，雨声落在门槛上。");
        assert!(!cleaned.contains("content"));
        assert!(!cleaned.contains("key_facts"));
        assert!(!cleaned.contains("continuity_updates"));
    }

    #[test]
    fn chapter_body_sanitizer_removes_trailing_chapter_end_state() {
        let raw = "他将最后一支样本放入恒温箱，转身走进雨幕。\nchapter_end_state:顾知桥确认了新线索，准备继续追查。";

        let cleaned = sanitize_chapter_body(raw, "", "zh-CN");

        assert_eq!(cleaned, "他将最后一支样本放入恒温箱，转身走进雨幕。");
        assert!(!cleaned.contains("chapter_end_state"));
    }

    #[test]
    fn chapter_body_sanitizer_removes_chinese_chapter_state_record() {
        let raw = "她吹熄烛火，窗外的风仍拍打着窗棂。***章末状态记录：*地点：旧驿庄。*人物：商清衡。*下一章入口：清理水渠。";

        let cleaned = sanitize_chapter_body(raw, "", "zh-CN");

        assert_eq!(cleaned, "她吹熄烛火，窗外的风仍拍打着窗棂。");
        assert!(!cleaned.contains("章末状态记录"));
        assert!(!cleaned.contains("下一章入口"));
    }

    #[test]
    fn chapter_body_sanitizer_preserves_in_world_state_record_wording() {
        let raw = "她翻到档案中间，看到“章末状态记录：门锁完好”这行旧字，却没有停下脚步。\n\
关键事实：这不是工具元数据，而是她在庭审中朗读的证物标题。";

        let cleaned = sanitize_chapter_body(raw, "", "zh-CN");

        assert_eq!(cleaned, raw);
    }

    #[test]
    fn chapter_body_sanitizer_preserves_normal_acronyms() {
        let raw = "他看见屏幕上的 AI 指标亮起，CBD 的灯光映在玻璃上。";

        let cleaned = sanitize_chapter_body(raw, "", "zh-CN");

        assert!(cleaned.contains("AI"));
        assert!(cleaned.contains("CBD"));
    }

    #[test]
    fn chapter_body_sanitizer_preserves_mid_chapter_closure_prose_for_review() {
        let raw = "他握紧操纵杆，向矿坑深处走去。\n\
天工之眼的光芒在他身后拉长，像是一把利剑，刺破荒凉。从这一刻起，他不再是底层打工人。\n\
“你的垄断链条，就从这里开始断裂。”\n\
他轻声说道，声音淹没在雷声中。\n\
矿坑深处，传来钻机启动的轰鸣声，与暴雨交织，奏响了第一卷的序曲。\n\
钻头咬入岩层的瞬间，震动顺着金属杆传到掌心，他稳住呼吸继续推进。\n\
仪表盘上的指针跳入红区，他切断采集程序，重新校准功率。\n\
岩壁裂开一道缝隙，温热蒸汽喷出，他终于确认地热脉的位置。";

        let cleaned = sanitize_chapter_body(raw, "", "zh-CN");

        assert!(cleaned.contains("他握紧操纵杆"));
        assert!(cleaned.contains("钻头咬入岩层"));
        assert!(cleaned.contains("地热脉的位置"));
        assert!(cleaned.contains("从这一刻起"));
        assert!(cleaned.contains("开始断裂"));
        assert!(cleaned.contains("他轻声说道"));
        assert!(cleaned.contains("第一卷的序曲"));
    }

    #[test]
    fn chapter_body_sanitizer_preserves_dialogue_attribution_for_review() {
        let raw = "他握紧钻机，走向矿坑深处。\n\
天工之眼的光芒在雨幕中拉长，像一把刺破腐朽的利剑。\n\
他轻声说道，声音淹没在雷声中，却带着不容置疑的决绝。\n\
钻头咬入岩层的瞬间，震动顺着金属杆传到掌心。\n\
仪表盘上的指针跳入红区，他切断采集程序，重新校准功率。";

        let cleaned = sanitize_chapter_body(raw, "", "zh-CN");

        assert!(cleaned.contains("他握紧钻机"));
        assert!(cleaned.contains("钻头咬入岩层"));
        assert!(cleaned.contains("他轻声说道"));
    }

    #[test]
    fn chapter_body_sanitizer_preserves_actual_final_closure() {
        let raw = "他握紧操纵杆，向矿坑深处走去。\n\
钻头咬入岩层的瞬间，震动顺着金属杆传到掌心，他稳住呼吸继续推进。\n\
仪表盘上的指针跳入红区，他切断采集程序，重新校准功率。\n\
岩壁裂开一道缝隙，温热蒸汽喷出，他终于确认地热脉的位置。\n\
矿坑深处，传来钻机启动的轰鸣声，与暴雨交织，奏响了第一卷的序曲。";

        let cleaned = sanitize_chapter_body(raw, "", "zh-CN");

        assert!(cleaned.contains("第一卷的序曲"));
    }

    #[test]
    fn chapter_body_sanitizer_preserves_dialogue_that_mentions_story_beginning() {
        let raw = "他推开会议室的门，雨水从袖口滴到地毯上。\n\
“故事才刚刚开始。”她说，语气平静得像早已看见结局。\n\
桌上的终端忽然亮起，新的投票记录滚过屏幕。\n\
他看见被隐藏的签名，终于明白真正的对手还在楼上。\n\
电梯门合拢前，他把证据备份进私人密钥。";

        let cleaned = sanitize_chapter_body(raw, "", "zh-CN");

        assert!(cleaned.contains("“故事才刚刚开始。”她说"));
        assert!(cleaned.contains("真正的对手还在楼上"));
    }

    #[test]
    fn chapter_body_sanitizer_removes_glued_model_chapter_heading_prefix() {
        let raw = "# 耐热合金\n\n#第2章逆熵的余温清晨的底层区弥漫着一种特有的酸涩气味，那是冷却后的蒸汽与生锈的铁皮混合的味道。\n艾拉推开那扇摇摇欲坠的铁门。";

        let cleaned = sanitize_chapter_body(raw, "耐热合金", "zh-CN");

        assert!(cleaned.starts_with("# 耐热合金\n\n清晨的底层区"));
        assert!(!cleaned.contains("#第2章"));
        assert!(!cleaned.contains("逆熵的余温清晨"));
        assert!(cleaned.contains("艾拉推开"));
    }

    #[test]
    fn chapter_body_sanitizer_removes_appended_story_planning_commentary() {
        let raw = "林汐踏入静默区，身后的潮声忽然消失了。\n\n本章以林汐踏入静默区为结，悬念落在未知声音的呼唤上，为下一章的身份揭秘埋下伏笔。";
        let cleaned = sanitize_chapter_body(raw, "", "zh-CN");

        assert_eq!(cleaned, "林汐踏入静默区，身后的潮声忽然消失了。");
    }
}
