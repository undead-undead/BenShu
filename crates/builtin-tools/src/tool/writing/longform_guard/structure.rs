use regex::Regex;
use std::sync::LazyLock;

use benshu_brain::runtime::continuous_task::ContinuousStepRequest;
use benshu_compression::ellipsize;

use super::super::text_sanitizer;
use super::core::LongformArtifactGuard;

static NUMBERED_TAIL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\d+[.)、．]\s*").expect("numbered tail regex"));
static CONTENT_HEADING_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^\s*#{1,6}\s*(?:第[一二三四五六七八九十百千万零〇\d]+[章节步]|chapter\s+\d+|section\s+\d+)",
    )
    .expect("content heading regex")
});
static CONTENT_ORDINAL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?:第([一二三四五六七八九十百千万零〇\d]+)[章节步])|(?:(?:chapter|section)\s+(\d{1,6}))",
    )
    .expect("content ordinal regex")
});
static TITLE_HEADING_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*#{1,3}\s*《([^》\n]{1,80})》\s*$").expect("title heading regex")
});
static LABELED_TITLE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?im)^\s*(?:#{1,6}\s*)?(?:[-*+]\s*)?(?:(?:\d+|[一二三四五六七八九十]+)[.)、．]\s*)?(?:\*\*)?\s*(?:标题|书名|名称|作品名|作品名称|文档标题|作品标题|title|document title|name|document name)(?:\s*[（(][^）)\n]{0,80}[）)])?\s*(?:\*\*)?\s*[:：]\s*[\"'“”《]?([^\"'“”》\n]{1,80})[\"'“”》]?\s*$"#)
        .expect("labeled title regex")
});
static PROGRESS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?im)^\s*[*_`]*(?:当前进度|进度|current\s+progress)[*_`]*\s*[:：]\s*(?:第)?\s*(\d{1,6})\s*/\s*[*_`]*(\d{1,6})[*_`]*\s*(?:步|章|章节|steps?)?")
        .expect("progress regex")
});
static PROGRESS_LINE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?im)^(\s*[*_`]*(?:当前进度|进度|current\s+progress)[*_`]*\s*[:：]\s*(?:第)?\s*)(\d{1,6})(\s*/\s*[*_`]*)(\d{1,6})([*_`]*\s*(?:步|章|章节|steps?)?\s*)$")
        .expect("progress line regex")
});

impl LongformArtifactGuard {
    pub(crate) fn should_enforce_body_minimum(request: &ContinuousStepRequest) -> bool {
        request.contract.as_ref().is_some_and(|contract| {
            contract
                .anchors
                .iter()
                .any(|anchor| anchor.name == "planned_total_steps")
        })
    }

    pub(crate) fn generated_body_char_count(output: &str) -> usize {
        let mut total = 0usize;
        for line in output.lines() {
            let trimmed = line.trim();
            if Self::is_continuity_tail_label(trimmed) {
                break;
            }
            if trimmed.is_empty()
                || Self::looks_like_content_heading(trimmed)
                || Self::looks_like_standalone_document_title_heading(trimmed)
                || Self::is_document_identity_marker(trimmed)
                || Self::is_document_identity_label_line(trimmed)
                || NUMBERED_TAIL_RE.is_match(trimmed)
            {
                continue;
            }
            total += trimmed.chars().count();
        }
        total
    }

    pub(crate) fn has_continuity_tail(output: &str) -> bool {
        let lower = output.to_ascii_lowercase();
        let continuity = output.contains("连续性记录")
            || output.contains("连续性说明")
            || lower.contains("continuity notes");
        let hook = output.contains("下一步钩子")
            || output.contains("下一章钩子")
            || output.contains("后续钩子")
            || lower.contains("next hook");
        continuity && hook
    }

    pub(crate) fn body_before_continuity_tail(output: &str) -> String {
        output
            .lines()
            .take_while(|line| !Self::is_continuity_tail_label(line.trim()))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub(crate) fn extract_next_hook_text(output: &str) -> Option<String> {
        let mut collecting = false;
        let mut parts = Vec::new();
        for line in output.lines() {
            let trimmed = line.trim();
            if collecting {
                if trimmed.is_empty()
                    || trimmed.starts_with('#')
                    || trimmed.starts_with("连续性记录")
                    || trimmed.starts_with("连续性说明")
                {
                    break;
                }
                parts.push(trimmed.trim_start_matches(['-', '*', '•']).trim());
                continue;
            }
            let Some((label, value)) = trimmed.split_once('：').or_else(|| trimmed.split_once(':'))
            else {
                continue;
            };
            let label = label.trim().to_ascii_lowercase();
            if label.contains("下一步钩子")
                || label.contains("下一章钩子")
                || label.contains("后续钩子")
                || label.contains("next hook")
            {
                collecting = true;
                if !value.trim().is_empty() {
                    parts.push(value.trim());
                }
            }
        }
        let value = Self::normalize_next_hook_fragment(&parts.join(" "));
        (!value.is_empty()).then_some(value)
    }

    pub(crate) fn normalize_next_hook_fragment(value: &str) -> String {
        value
            .trim()
            .trim_matches(|ch: char| {
                ch.is_whitespace()
                    || matches!(
                        ch,
                        '【' | '】' | '[' | ']' | '(' | ')' | '（' | '）' | '-' | '*' | '•'
                    )
            })
            .trim()
            .to_string()
    }

    pub(crate) fn public_checkpoint_summary(output: &str, fallback_label: &str) -> String {
        let sanitized = text_sanitizer::sanitize_common_surface_report(
            output,
            text_sanitizer::WritingSanitizeStage::StreamProgress,
        )
        .text;
        Self::body_before_continuity_tail(&sanitized)
            .lines()
            .find_map(|line| {
                let line = line.trim();
                (!line.is_empty()).then(|| ellipsize(line, 180))
            })
            .unwrap_or_else(|| fallback_label.to_string())
    }

    pub(crate) fn extract_document_title(text: &str) -> Option<String> {
        for line in text.lines().take(24) {
            if let Some(captures) = TITLE_HEADING_RE.captures(line) {
                if let Some(title) = captures
                    .get(1)
                    .and_then(|m| Self::normalize_title(m.as_str()))
                {
                    return Some(title);
                }
            }
        }
        LABELED_TITLE_RE
            .captures(text)
            .and_then(|captures| captures.get(1))
            .and_then(|value| Self::normalize_title(value.as_str()))
    }

    pub(crate) fn normalize_title(raw: &str) -> Option<String> {
        let value = raw
            .trim()
            .trim_matches(|ch: char| {
                ch.is_whitespace()
                    || matches!(
                        ch,
                        '#' | '*' | '`' | '"' | '\'' | '“' | '”' | '《' | '》' | ':' | '：'
                    )
            })
            .trim();
        (!value.is_empty()).then(|| value.to_string())
    }

    pub(crate) fn title_is_placeholder(title: &str) -> bool {
        let compact = title
            .chars()
            .filter(|ch| !ch.is_whitespace() && !matches!(ch, '-' | '_' | ':' | '：'))
            .collect::<String>();
        matches!(
            compact.as_str(),
            "" | "无标题" | "待定" | "标题" | "文档标题" | "作品标题" | "书名"
        ) || compact.eq_ignore_ascii_case("untitled")
    }

    pub(crate) fn extract_labeled_primary_anchor(text: &str) -> Option<String> {
        for line in text.lines().take(32) {
            let Some((label, value)) = line.split_once('：').or_else(|| line.split_once(':'))
            else {
                continue;
            };
            let label = Self::normalize_document_identity_label(label);
            if [
                "主角",
                "主人公",
                "主线人物",
                "核心人物",
                "主体",
                "核心对象",
                "研究对象",
                "核心命题",
                "protagonist",
                "primary subject",
                "main subject",
                "core subject",
                "core thesis",
            ]
            .iter()
            .any(|candidate| label == *candidate)
            {
                if let Some(anchor) = Self::normalize_primary_anchor(value) {
                    return Some(anchor);
                }
            }
        }
        None
    }

    pub(crate) fn normalize_primary_anchor(raw: &str) -> Option<String> {
        let value = raw
            .trim()
            .trim_matches(|ch: char| {
                ch.is_whitespace()
                    || matches!(
                        ch,
                        '#' | '*' | '`' | '"' | '\'' | '“' | '”' | '《' | '》' | ':' | '：'
                    )
            })
            .split(['，', ',', '、', '/', '；', ';', '。', '.'])
            .next()
            .unwrap_or("")
            .trim();
        let value = value
            .split(['（', '(', '[', '【'])
            .next()
            .unwrap_or("")
            .trim();
        let len = value.chars().count();
        (len >= 2 && len <= 80).then(|| value.to_string())
    }

    pub(crate) fn strip_nonfirst_document_identity_blocks(output: &str) -> String {
        let lines = output.lines().collect::<Vec<_>>();
        let mut index = 0usize;
        let mut removed = 0usize;
        while index < lines.len() && index < 20 {
            let trimmed = lines[index].trim();
            if trimmed.is_empty()
                || Self::is_document_identity_marker(trimmed)
                || Self::is_document_identity_label_line(trimmed)
                || Self::looks_like_standalone_document_title_heading(trimmed)
            {
                if !trimmed.is_empty() {
                    removed += 1;
                }
                index += 1;
                continue;
            }
            break;
        }
        if removed < 2 {
            return output.to_string();
        }
        lines[index..].join("\n")
    }

    pub(crate) fn is_document_identity_marker(line: &str) -> bool {
        let compact = line
            .trim()
            .trim_matches(|ch: char| {
                matches!(
                    ch,
                    '[' | ']' | '【' | '】' | '#' | '*' | '-' | ':' | '：' | ' '
                )
            })
            .to_ascii_lowercase();
        matches!(
            compact.as_str(),
            "document metadata"
                | "metadata"
                | "artifact metadata"
                | "文档元数据"
                | "产物元数据"
                | "作品元数据"
                | "元数据"
        )
    }

    pub(crate) fn is_document_identity_label_line(line: &str) -> bool {
        let Some((label, value)) = line.split_once('：').or_else(|| line.split_once(':')) else {
            return false;
        };
        if value.trim().is_empty() {
            return false;
        }
        let label = Self::normalize_document_identity_label(label);
        [
            "标题",
            "书名",
            "名称",
            "作品名",
            "作品名称",
            "文档标题",
            "作品标题",
            "产物类型",
            "类型",
            "素材来源",
            "来源",
            "来源使用边界",
            "目标规模",
            "目标字数",
            "连续性规则",
            "当前进度",
            "进度",
            "主角",
            "主人公",
            "主角/主体/核心对象",
            "主体",
            "核心对象",
            "研究对象",
            "title",
            "document title",
            "name",
            "document name",
            "type",
            "source",
            "target size",
            "target length",
            "continuity rules",
            "current progress",
            "progress",
            "protagonist",
            "primary subject",
            "main subject",
            "core subject",
        ]
        .iter()
        .any(|candidate| label == *candidate)
    }

    fn normalize_document_identity_label(label: &str) -> String {
        let value = label
            .trim()
            .trim_matches(|ch: char| {
                ch.is_whitespace()
                    || matches!(
                        ch,
                        '-' | '*' | '#' | '`' | '[' | ']' | '【' | '】' | '"' | '\'' | '“' | '”'
                    )
            })
            .trim();
        value.to_ascii_lowercase()
    }

    pub(crate) fn looks_like_content_heading(line: &str) -> bool {
        CONTENT_HEADING_RE.is_match(line)
    }

    pub(crate) fn looks_like_standalone_document_title_heading(line: &str) -> bool {
        TITLE_HEADING_RE.is_match(line)
    }

    pub(crate) fn content_heading_ordinals(output: &str) -> Vec<(usize, String)> {
        output
            .lines()
            .filter_map(|line| {
                let captures = CONTENT_ORDINAL_RE.captures(line.trim())?;
                let ordinal = captures
                    .get(1)
                    .or_else(|| captures.get(2))
                    .and_then(|value| Self::parse_step_ordinal(value.as_str()))?;
                Some((ordinal, line.trim().to_string()))
            })
            .collect()
    }

    pub(crate) fn content_heading_mentions_ordinal(heading: &str, expected: usize) -> bool {
        CONTENT_ORDINAL_RE.captures_iter(heading).any(|captures| {
            captures
                .get(1)
                .or_else(|| captures.get(2))
                .and_then(|value| Self::parse_step_ordinal(value.as_str()))
                == Some(expected)
        })
    }

    pub(crate) fn parse_step_ordinal(raw: &str) -> Option<usize> {
        let raw = raw.trim();
        if raw.chars().all(|ch| ch.is_ascii_digit()) {
            return raw.parse().ok();
        }
        Self::parse_chinese_ordinal(raw)
    }

    fn parse_chinese_ordinal(text: &str) -> Option<usize> {
        let mut total = 0usize;
        let mut section = 0usize;
        let mut digit = 0usize;
        for ch in text.chars() {
            match ch {
                '零' | '〇' => digit = 0,
                '一' => digit = 1,
                '二' | '两' => digit = 2,
                '三' => digit = 3,
                '四' => digit = 4,
                '五' => digit = 5,
                '六' => digit = 6,
                '七' => digit = 7,
                '八' => digit = 8,
                '九' => digit = 9,
                '十' => {
                    section += digit.max(1) * 10;
                    digit = 0;
                }
                '百' => {
                    section += digit.max(1) * 100;
                    digit = 0;
                }
                '千' => {
                    section += digit.max(1) * 1000;
                    digit = 0;
                }
                '万' => {
                    total += (section + digit).max(1) * 10_000;
                    section = 0;
                    digit = 0;
                }
                _ => return None,
            }
        }
        let value = total + section + digit;
        (value > 0).then_some(value)
    }

    pub(crate) fn extract_declared_progress_total(text: &str) -> Option<usize> {
        PROGRESS_RE
            .captures(text)
            .and_then(|captures| captures.get(2))
            .and_then(|value| value.as_str().parse().ok())
    }

    pub(crate) fn extract_declared_progress_current(text: &str) -> Option<usize> {
        PROGRESS_RE
            .captures(text)
            .and_then(|captures| captures.get(1))
            .and_then(|value| value.as_str().parse().ok())
    }

    pub(crate) fn repair_declared_progress_total(text: &str, total: usize) -> String {
        PROGRESS_LINE_RE
            .replace_all(text, |captures: &regex::Captures<'_>| {
                format!(
                    "{}{}{}{}{}",
                    &captures[1], &captures[2], &captures[3], total, &captures[5]
                )
            })
            .into_owned()
    }

    pub(crate) fn downgrade_stray_document_title_to_step_heading(
        output: &str,
        title: &str,
        step_index: usize,
    ) -> String {
        let mut changed = false;
        let lines = output
            .lines()
            .enumerate()
            .map(|(index, line)| {
                if index <= 8
                    && TITLE_HEADING_RE
                        .captures(line)
                        .and_then(|captures| captures.get(1))
                        .and_then(|value| Self::normalize_title(value.as_str()))
                        .as_deref()
                        == Some(title)
                {
                    changed = true;
                    format!("### 第{step_index}步：{title}")
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<_>>();
        if changed {
            lines.join("\n")
        } else {
            output.to_string()
        }
    }

    fn is_continuity_tail_label(line: &str) -> bool {
        let lower = line.to_ascii_lowercase();
        line.starts_with("连续性记录")
            || line.starts_with("连续性说明")
            || line.starts_with("下一步钩子")
            || line.starts_with("下一章钩子")
            || line.starts_with("后续钩子")
            || lower.starts_with("continuity notes")
            || lower.starts_with("next hook")
    }
}
