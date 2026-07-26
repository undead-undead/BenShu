use super::jsonish::{extract_json, jsonish_string_array_field, jsonish_string_field};
use super::model::{is_chinese_language, DraftOutput};

pub(crate) fn parse_draft_output(raw: &str, chapter_number: usize, language: &str) -> DraftOutput {
    if let Some(value) = parse_draft_stream_protocol(raw) {
        return normalize_draft(value, chapter_number, language);
    }
    if let Some(value) =
        extract_json(raw).and_then(|json| serde_json::from_str::<DraftOutput>(&json).ok())
    {
        return normalize_draft(value, chapter_number, language);
    }
    if let Some(value) = parse_jsonish_draft_output(raw) {
        return normalize_draft(value, chapter_number, language);
    }
    let title = fallback_title(raw, chapter_number, language);
    let content = fallback_content(raw);
    DraftOutput {
        title,
        summary: first_sentence(&content, 120),
        key_facts: Vec::new(),
        continuity_updates: Vec::new(),
        content,
        degraded: true,
        degraded_reason: "model output was not valid DraftOutput JSON; parsed freeform fallback"
            .to_string(),
    }
}

pub(crate) fn parse_draft_stream_protocol(raw: &str) -> Option<DraftOutput> {
    let normalized = raw.replace("\r\n", "\n");
    let (header, body_and_tail) = normalized.split_once("---BODY---")?;
    let title = header.lines().find_map(|line| {
        let trimmed = line.trim();
        ["TITLE:", "TITLE：", "标题:", "标题："]
            .iter()
            .find_map(|prefix| trimmed.strip_prefix(prefix))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })?;
    let (content, complete) = find_stream_end_marker(body_and_tail)
        .map(|index| (&body_and_tail[..index], true))
        .unwrap_or((body_and_tail, false));
    let content = content.trim().trim_end_matches("```").trim().to_string();
    if content.is_empty() {
        return None;
    }
    Some(DraftOutput {
        title,
        summary: String::new(),
        key_facts: Vec::new(),
        continuity_updates: Vec::new(),
        content,
        degraded: !complete,
        degraded_reason: if complete {
            String::new()
        } else {
            "stream-safe chapter protocol ended before ---END BODY---; candidate is truncated"
                .to_string()
        },
    })
}

fn find_stream_end_marker(body: &str) -> Option<usize> {
    body.match_indices("---END").find_map(|(index, _)| {
        let after_end = &body[index + "---END".len()..];
        after_end
            .trim_start_matches(char::is_whitespace)
            .starts_with("BODY---")
            .then_some(index)
    })
}

fn parse_jsonish_draft_output(raw: &str) -> Option<DraftOutput> {
    let title = jsonish_string_field(raw, "title", &["content", "summary", "key_facts"])?;
    let content = jsonish_string_field(
        raw,
        "content",
        &["summary", "key_facts", "continuity_updates"],
    )?;
    let summary = jsonish_string_field(raw, "summary", &["key_facts", "continuity_updates"])
        .unwrap_or_default();
    let key_facts = jsonish_string_array_field(raw, "key_facts");
    let continuity_updates = jsonish_string_array_field(raw, "continuity_updates");
    Some(DraftOutput {
        title,
        content,
        summary,
        key_facts,
        continuity_updates,
        degraded: false,
        degraded_reason: String::new(),
    })
}

fn normalize_draft(mut value: DraftOutput, chapter_number: usize, language: &str) -> DraftOutput {
    if value.title.trim().is_empty() || title_violates_language_contract(&value.title, language) {
        value.title = fallback_default_title(chapter_number, language);
    }
    value.title = value.title.trim().to_string();
    value.content = value.content.trim().to_string();
    value.summary = value.summary.trim().to_string();
    if value.summary.is_empty()
        || (is_chinese_language(language)
            && !contains_cjk(&value.summary)
            && contains_cjk(&value.content))
    {
        value.summary = first_sentence(&value.content, 120);
    }
    value.key_facts.retain(|item| {
        !item.trim().is_empty() && (!is_chinese_language(language) || contains_cjk(item))
    });
    value.continuity_updates.retain(|item| {
        !item.trim().is_empty() && (!is_chinese_language(language) || contains_cjk(item))
    });
    value
}

fn title_violates_language_contract(title: &str, language: &str) -> bool {
    if !is_chinese_language(language) {
        return false;
    }
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return true;
    }
    let lowered = trimmed.to_ascii_lowercase();
    !contains_cjk(trimmed)
        || lowered.starts_with("chapter")
        || lowered.contains("the ")
        || lowered.contains("contract")
        || lowered.contains("workflow")
        || lowered.contains("continuity")
}

fn contains_cjk(value: &str) -> bool {
    value
        .chars()
        .any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch))
}

fn fallback_title(raw: &str, chapter_number: usize, language: &str) -> String {
    raw.lines()
        .find_map(|line| {
            let trimmed = line.trim();
            parse_freeform_title_line(trimmed, chapter_number)
                .map(|(title, _)| title)
                .or_else(|| {
                    trimmed
                        .strip_prefix("# ")
                        .or_else(|| trimmed.strip_prefix("章节标题："))
                        .or_else(|| trimmed.strip_prefix("章节标题:"))
                        .or_else(|| trimmed.strip_prefix("CHAPTER_TITLE:"))
                        .map(|value| value.trim().to_string())
                })
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| fallback_default_title(chapter_number, language))
}

fn fallback_default_title(chapter_number: usize, language: &str) -> String {
    if is_chinese_language(language) {
        format!("第{chapter_number}章")
    } else {
        format!("Chapter {chapter_number}")
    }
}

fn fallback_content(raw: &str) -> String {
    raw.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("```")
                || trimmed.starts_with("CHAPTER_TITLE")
                || trimmed.starts_with("章节标题")
            {
                return None;
            }
            if let Some((_, remainder)) = parse_freeform_title_line(trimmed, 0) {
                return (!remainder.is_empty()).then_some(remainder);
            }
            Some(line.to_string())
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn parse_freeform_title_line(line: &str, chapter_number: usize) -> Option<(String, String)> {
    let trimmed = line.trim();
    let lowered = trimmed.to_ascii_lowercase();
    let prefix_len = ["title:", "title："]
        .iter()
        .find_map(|prefix| lowered.starts_with(prefix).then_some(prefix.len()))?;
    let mut rest = trimmed[prefix_len..].trim_start();
    rest = rest.trim_start_matches(['"', '\'', '“', '‘']);
    rest = strip_freeform_chapter_ordinal(rest, chapter_number);

    let (title, remainder) = rest
        .char_indices()
        .find(|(_, ch)| matches!(ch, ':' | '：'))
        .map(|(index, delimiter)| {
            let remainder_start = index + delimiter.len_utf8();
            (&rest[..index], &rest[remainder_start..])
        })
        .unwrap_or((rest, ""));
    let title = title
        .trim()
        .trim_matches(['"', '\'', '“', '”', '‘', '’', '《', '》'])
        .to_string();
    if title.is_empty() || title.chars().count() > 32 {
        return None;
    }
    let remainder = remainder
        .trim_start()
        .trim_start_matches(['"', '\'', '“', '‘'])
        .to_string();
    Some((title, remainder))
}

fn strip_freeform_chapter_ordinal(value: &str, chapter_number: usize) -> &str {
    let value = value.trim_start();
    let Some(after_di) = value.strip_prefix('第') else {
        return value;
    };
    let Some(chapter_index) = after_di.find('章') else {
        return value;
    };
    let ordinal = &after_di[..chapter_index];
    if ordinal.is_empty()
        || !ordinal.chars().all(|ch| {
            ch.is_ascii_digit()
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
                )
        })
    {
        return value;
    }
    if chapter_number > 0
        && ordinal.chars().all(|ch| ch.is_ascii_digit())
        && ordinal.parse::<usize>().ok() != Some(chapter_number)
    {
        return value;
    }
    after_di[chapter_index + '章'.len_utf8()..].trim_start_matches([':', '：', ' ', '\t'])
}

fn first_sentence(content: &str, max_chars: usize) -> String {
    let trimmed = content.trim();
    let sentence_end = trimmed
        .char_indices()
        .find_map(|(idx, ch)| {
            matches!(ch, '。' | '！' | '？' | '.' | '!' | '?').then_some(idx + ch.len_utf8())
        })
        .unwrap_or_else(|| trimmed.len().min(max_chars));
    trimmed[..sentence_end.min(trimmed.len())]
        .chars()
        .take(max_chars)
        .collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::parse_draft_output;

    #[test]
    fn stream_protocol_keeps_long_body_outside_json() {
        let raw = "TITLE: 盐引与铁锈\n---BODY---\n阮岑川翻开父亲留下的旧账。\n\n雨水敲在窗上。\n---END BODY---";
        let draft = parse_draft_output(raw, 1, "zh-CN");

        assert_eq!(draft.title, "盐引与铁锈");
        assert!(draft.content.contains("雨水敲在窗上"));
        assert!(!draft.degraded);
        assert!(draft.key_facts.is_empty());
    }

    #[test]
    fn unterminated_stream_protocol_preserves_truncated_candidate() {
        let raw = "标题：符文醒来\n---BODY---\n辛砚遥看见掌心的金色纹路逐渐亮起。";
        let draft = parse_draft_output(raw, 1, "zh-CN");

        assert_eq!(draft.title, "符文醒来");
        assert!(draft.content.contains("金色纹路"));
        assert!(draft.degraded);
        assert!(draft.degraded_reason.contains("truncated"));
    }

    #[test]
    fn stream_protocol_accepts_compact_layout_and_optional_end_marker_whitespace() {
        let raw = "TITLE:锈蚀芯片---BODY---陶屿遥在酸雨中捡起暗红芯片。---ENDBODY---";
        let draft = parse_draft_output(raw, 1, "zh-CN");

        assert_eq!(draft.title, "锈蚀芯片");
        assert_eq!(draft.content, "陶屿遥在酸雨中捡起暗红芯片。");
        assert!(!draft.degraded);
    }

    #[test]
    fn freeform_title_field_is_extracted_without_polluting_body() {
        let raw = "title:第一章：盐引与铁锈:“现银为尊”的铁律写在青石县每间铺子的账簿上。\n阮岑川翻开父亲留下的旧账。";
        let draft = parse_draft_output(raw, 1, "zh-CN");

        assert_eq!(draft.title, "盐引与铁锈");
        assert!(draft.content.starts_with("现银为尊"), "{}", draft.content);
        assert!(!draft.content.contains("title:"), "{}", draft.content);
    }

    #[test]
    fn malformed_freeform_title_with_fullwidth_delimiter_does_not_split_utf8_codepoint() {
        let raw = "title:锈蚀芯片---BODY---新九龙城的雨像城市腐烂的血管里渗出的血：陶屿遥拉高了防风衣的领口。";
        let draft = parse_draft_output(raw, 1, "zh-CN");

        assert_eq!(draft.title, "第1章");
        assert!(draft.content.contains("陶屿遥"), "{}", draft.content);
    }
}
