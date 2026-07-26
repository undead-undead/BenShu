use super::source_selection;

pub(super) fn image_output_path_from_tool_result(content: &str) -> Option<String> {
    let marker = "saved to:";
    let lower = content.to_ascii_lowercase();
    let start = lower.find(marker)? + marker.len();
    let path = content[start..].trim();
    if path.is_empty() {
        None
    } else {
        Some(path.to_string())
    }
}

pub(super) fn first_retrieval_snippet(content: &str) -> Option<String> {
    retrieval_snippet_for_query("", content)
}

pub(super) fn retrieval_snippet_for_query(query: &str, content: &str) -> Option<String> {
    let requested_count = requested_search_result_count(query);
    if let Some(summary) = search_result_summary_from_json(content, requested_count) {
        return Some(summary);
    }
    if let Some(summary) = web_fetch_content_summary_from_json(query, content, requested_count) {
        return Some(summary);
    }
    if let Some(summary) = search_result_summary_from_text(content, requested_count) {
        return Some(summary);
    }

    for line in content.lines() {
        let trimmed = line.trim();
        let snippet = trimmed
            .strip_prefix("*Snippet*:")
            .or_else(|| trimmed.strip_prefix("Content Snippet:"))
            .map(str::trim);
        if let Some(snippet) = snippet {
            let cleaned = snippet.trim_end_matches("...").trim().to_string();
            if !cleaned.is_empty() {
                return Some(cleaned);
            }
        }
    }
    None
}

fn web_fetch_content_summary_from_json(query: &str, content: &str, limit: usize) -> Option<String> {
    let json_blob = source_selection::tool_result_json_blob(content)?;
    let payload: serde_json::Value = serde_json::from_str(json_blob).ok()?;
    let body = payload
        .get("content")
        .and_then(|value| value.as_str())
        .or_else(|| {
            payload
                .pointer("/fetched_result/content")
                .and_then(|value| value.as_str())
        })?;
    let mut candidates = Vec::new();
    let focus_terms = summary_focus_terms(query);
    for (index, raw) in body.lines().enumerate() {
        let trimmed = raw.trim();
        let Some(score) = web_fetch_summary_line_score(trimmed, &focus_terms) else {
            continue;
        };
        if candidates
            .iter()
            .any(|(_, _, line): &(usize, i32, String)| line == trimmed)
        {
            continue;
        }
        candidates.push((index, score, trimmed.to_string()));
    }
    candidates.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    let mut selected = candidates
        .into_iter()
        .take(limit.max(1).min(6))
        .collect::<Vec<_>>();
    selected.sort_by_key(|(index, _, _)| *index);
    let lines = selected
        .into_iter()
        .map(|(_, _, line)| line)
        .collect::<Vec<_>>();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

fn summary_focus_terms(query: &str) -> Vec<String> {
    query
        .split(|ch: char| {
            ch.is_whitespace()
                || matches!(
                    ch,
                    ',' | '.'
                        | ';'
                        | ':'
                        | '?'
                        | '!'
                        | '，'
                        | '。'
                        | '、'
                        | '；'
                        | '：'
                        | '？'
                        | '！'
                        | '('
                        | ')'
                        | '（'
                        | '）'
                )
        })
        .map(str::trim)
        .filter(|token| token.chars().count() >= 2)
        .filter(|token| !summary_focus_stopword(token))
        .map(|token| token.to_lowercase())
        .collect()
}

fn summary_focus_stopword(token: &str) -> bool {
    let lowered = token.to_ascii_lowercase();
    matches!(
        lowered.as_str(),
        "search"
            | "find"
            | "lookup"
            | "provide"
            | "include"
            | "source"
            | "sources"
            | "answer"
            | "current"
            | "latest"
            | "today"
            | "result"
            | "results"
    ) || matches!(
        token,
        "搜索"
            | "查找"
            | "查询"
            | "回答"
            | "来源"
            | "结果"
            | "现在"
            | "今天"
            | "最新"
            | "给出"
            | "中文"
    )
}

fn web_fetch_summary_line_score(line: &str, focus_terms: &[String]) -> Option<i32> {
    if line.len() < 3 || line.len() > 220 {
        return None;
    }
    let lowered = line.to_ascii_lowercase();
    let noisy_exact = [
        "skip to main content",
        "log in",
        "sign up",
        "support",
        "menu",
        "search",
        "home",
        "about",
        "contact",
        "privacy",
        "terms",
        "subscribe",
        "loading chart...",
    ];
    if noisy_exact.iter().any(|noise| lowered == *noise) {
        return None;
    }
    if line == "•"
        || line
            .chars()
            .all(|ch| ch == '-' || ch == '•' || ch.is_whitespace())
    {
        return None;
    }
    let mut score = 0;
    let has_signal_char = line.chars().any(|ch| {
        ch.is_ascii_digit() || matches!(ch, '$' | '€' | '£' | '¥' | '%' | '°' | '℃' | '℉' | '#')
    });
    if has_signal_char {
        score += 40;
    }
    if line.contains('$') || line.contains('%') || line.contains('°') || line.contains('℃') {
        score += 30;
    }
    if line.starts_with('#') {
        score += 18;
    }
    if line.ends_with('。') || line.ends_with('.') {
        score += 8;
    }
    if focus_terms
        .iter()
        .any(|term| lowered.contains(term) || line.to_lowercase().contains(term))
    {
        score += 24;
    }
    let has_letters_or_cjk = line
        .chars()
        .any(|ch| ch.is_ascii_alphabetic() || ('\u{4e00}'..='\u{9fff}').contains(&ch));
    if !has_letters_or_cjk && !has_signal_char {
        return None;
    }
    (score > 0).then_some(score)
}

pub(super) fn requested_search_result_count(query: &str) -> usize {
    let lowered = query.to_ascii_lowercase();
    if lowered.contains("two candidates")
        || lowered.contains("two sources")
        || lowered.contains("two results")
        || lowered.contains("2个")
        || lowered.contains("两个")
        || lowered.contains("2 个")
    {
        return 2;
    }
    if lowered.contains("three candidates")
        || lowered.contains("three sources")
        || lowered.contains("three results")
        || lowered.contains("3个")
        || lowered.contains("三个")
        || lowered.contains("3 个")
    {
        return 3;
    }

    lowered
        .split_whitespace()
        .collect::<Vec<_>>()
        .windows(2)
        .find_map(|pair| {
            let count = pair[0].parse::<usize>().ok()?;
            let noun = pair[1];
            (matches!(
                noun,
                "candidate" | "candidates" | "source" | "sources" | "result" | "results"
            ) && (1..=5).contains(&count))
            .then_some(count)
        })
        .unwrap_or_else(|| {
            if lowered.contains("candidates")
                || lowered.contains("sources")
                || lowered.contains("results")
                || lowered.contains("候选")
                || lowered.contains("来源")
                || lowered.contains("结果")
            {
                2
            } else {
                1
            }
        })
}

pub(super) fn delegate_result_summary_block(content: &str) -> Option<String> {
    let marker = "result_summary:";
    let lower = content.to_ascii_lowercase();
    let start = lower.find(marker)? + marker.len();
    let tail = content[start..].trim_start();
    let mut lines = Vec::new();
    for line in tail.lines() {
        let trimmed = line.trim_end();
        let lower = trimmed.to_ascii_lowercase();
        if lower.starts_with("---")
            || lower.starts_with("### notice")
            || lower.starts_with("#### official")
        {
            break;
        }
        if !trimmed.trim().is_empty() {
            lines.push(trimmed.to_string());
        }
    }
    let summary = lines.join("\n").trim().to_string();
    (!summary.is_empty()).then_some(summary)
}

pub(super) fn search_result_summary_from_json(content: &str, limit: usize) -> Option<String> {
    let json_blob = source_selection::tool_result_json_blob(content)?;
    let payload: serde_json::Value = serde_json::from_str(json_blob).ok()?;

    let entries = payload
        .get("results")
        .and_then(|value| value.as_array())
        .filter(|results| !results.is_empty())
        .or_else(|| {
            payload
                .get("candidates")
                .and_then(|value| value.as_array())
                .filter(|candidates| !candidates.is_empty())
        })
        .or_else(|| {
            payload
                .get("evidence_bundle")
                .and_then(|value| value.get("candidates"))
                .and_then(|value| value.as_array())
                .filter(|candidates| !candidates.is_empty())
        });

    let entries = entries?;

    let effective_limit = if limit <= 1 && entries.len() > 1 {
        2
    } else {
        limit
    };
    let summaries = entries
        .iter()
        .take(effective_limit.max(1).min(5))
        .filter_map(search_result_entry_summary)
        .collect::<Vec<_>>();

    if summaries.is_empty() {
        None
    } else if summaries.len() == 1 {
        summaries.into_iter().next()
    } else {
        Some(
            summaries
                .into_iter()
                .enumerate()
                .map(|(index, summary)| format!("{}. {}", index + 1, summary))
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }
}

pub(super) fn search_result_entry_summary(entry: &serde_json::Value) -> Option<String> {
    let title = entry
        .get("title")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim();
    let url = entry
        .get("url")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim();
    let snippet = entry
        .get("snippet")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim();

    if title.is_empty() && url.is_empty() && snippet.is_empty() {
        return None;
    }

    let mut parts = Vec::new();
    if !title.is_empty() {
        parts.push(format!("{}。", title));
    }
    if !url.is_empty() {
        parts.push(format!("来源：{}。", url));
    }
    if !snippet.is_empty() {
        parts.push(format!("摘要：{}", snippet));
    }
    Some(parts.join(" "))
}

pub(super) fn search_result_summary_from_text(content: &str, limit: usize) -> Option<String> {
    let effective_limit = if limit <= 1 { 2 } else { limit }.min(5);
    let titles = json_like_string_fields(content, "title", effective_limit);
    let urls = json_like_string_fields(content, "url", effective_limit);
    let snippets = json_like_string_fields(content, "snippet", effective_limit);
    let count = titles.len().max(urls.len()).max(snippets.len());
    if count == 0 {
        return None;
    }

    let summaries = (0..count.min(effective_limit))
        .filter_map(|index| {
            let title = titles.get(index).cloned().unwrap_or_default();
            let url = urls.get(index).cloned().unwrap_or_default();
            let snippet = snippets.get(index).cloned().unwrap_or_default();
            if title.is_empty() && url.is_empty() && snippet.is_empty() {
                return None;
            }
            let mut parts = Vec::new();
            if !title.is_empty() {
                parts.push(format!("{}。", title));
            }
            if !url.is_empty() {
                parts.push(format!("来源：{}。", url));
            }
            if !snippet.is_empty() {
                parts.push(format!("摘要：{}", snippet));
            }
            Some(parts.join(" "))
        })
        .collect::<Vec<_>>();

    if summaries.len() <= 1 {
        summaries.into_iter().next()
    } else {
        Some(
            summaries
                .into_iter()
                .enumerate()
                .map(|(index, summary)| format!("{}. {}", index + 1, summary))
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }
}

pub(super) fn json_like_string_fields(content: &str, field: &str, limit: usize) -> Vec<String> {
    let marker = format!("\"{field}\"");
    let mut values = Vec::new();
    let mut offset = 0;
    while values.len() < limit.max(1) {
        let Some(relative_start) = content[offset..].find(&marker) else {
            break;
        };
        let start = offset + relative_start;
        let after_marker = &content[start + marker.len()..];
        let Some(colon) = after_marker.find(':') else {
            break;
        };
        let mut chars = after_marker[colon + 1..].trim_start().chars();
        if chars.next() != Some('"') {
            offset = start + marker.len();
            continue;
        }

        let mut value = String::new();
        let mut escaped = false;
        for ch in chars {
            if escaped {
                match ch {
                    'n' => value.push(' '),
                    'r' => {}
                    't' => value.push(' '),
                    '"' => value.push('"'),
                    '\\' => value.push('\\'),
                    other => value.push(other),
                }
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == '"' {
                break;
            }
            value.push(ch);
        }

        let cleaned = value.split_whitespace().collect::<Vec<_>>().join(" ");
        if !cleaned.is_empty() {
            values.push(cleaned);
        }
        offset = start + marker.len();
    }
    values
}

pub(super) fn strip_tool_runtime_notices(content: &str) -> String {
    let markers = [
        "\n---\n### NOTICE: First use of skill",
        "\n### NOTICE: First use of skill",
        " --- ### NOTICE: First use of skill",
        "--- ### NOTICE: First use of skill",
        "### NOTICE: First use of skill",
    ];
    for marker in markers {
        if let Some(idx) = content.find(marker) {
            return content[..idx].trim().to_string();
        }
    }
    content.trim().to_string()
}

pub(super) fn extract_direct_retrieval_answer(query: &str, snippet: &str) -> Option<String> {
    let normalized_query = query.to_lowercase();
    let asks_direct_answer = query.contains("是什么")
        || query.contains("答案")
        || query.contains("对应")
        || normalized_query.contains("what is")
        || normalized_query.contains("answer");
    if !asks_direct_answer {
        return None;
    }

    let cleaned = snippet.trim().trim_end_matches("...").trim();
    for marker in [
        "特殊验证答案是：",
        "特殊验证答案是:",
        "验证答案是：",
        "验证答案是:",
        "答案是：",
        "答案是:",
        "The special verification answer is:",
        "The verification answer is:",
        "The answer is:",
    ] {
        if let Some((_, answer)) = cleaned.split_once(marker) {
            let answer = answer
                .split(['。', '.', '\n'])
                .next()
                .unwrap_or("")
                .trim()
                .trim_matches('`')
                .trim();
            if !answer.is_empty() {
                return Some(answer.to_string());
            }
        }
    }

    None
}

pub(super) fn summarize_search_history_delivery(
    query: &str,
    content: &str,
    prefers_chinese: bool,
) -> String {
    let lowered = query.to_ascii_lowercase();
    let wants_marker_only =
        lowered.contains("marker") || query.contains("标记") || query.contains("只回答");
    if wants_marker_only {
        if let Some(marker) = first_marker_like_token(content) {
            return marker;
        }
    }

    if let Some(line) = first_memory_result_line(content) {
        if query.contains("只回答") {
            if let Some(answer) = direct_memory_answer_from_line(&line) {
                return answer;
            }
        }
        return if prefers_chinese {
            format!("我记得：{}", line)
        } else {
            format!("I remember: {}", line)
        };
    }

    if prefers_chinese {
        "我没有找到相关记忆。".to_string()
    } else {
        "I did not find a relevant memory.".to_string()
    }
}

pub(super) fn summarize_remember_this_delivery(prefers_chinese: bool) -> String {
    if prefers_chinese {
        "我已经记住了。".to_string()
    } else {
        "I have saved that to memory.".to_string()
    }
}

fn first_memory_result_line(content: &str) -> Option<String> {
    let body = content
        .split_once("Search matches (via Managed Memory Pipeline):")
        .map(|(_, body)| body)
        .unwrap_or(content);

    if let Some(content_line) = body.lines().map(str::trim).find_map(|line| {
        line.strip_prefix("Content:")
            .or_else(|| line.strip_prefix("content:"))
            .map(str::trim)
            .filter(|line| !line.is_empty())
    }) {
        return Some(
            content_line
                .trim_end_matches(['。', '.', ','])
                .trim()
                .to_string(),
        );
    }

    body.lines()
        .map(str::trim)
        .find(|line| {
            !line.is_empty()
                && !line.starts_with("---")
                && !line.starts_with("Fact ID:")
                && !line.starts_with("Category:")
        })
        .map(|line| line.trim_end_matches(['。', '.', ',']).trim().to_string())
        .filter(|line| !line.is_empty())
}

fn direct_memory_answer_from_line(line: &str) -> Option<String> {
    let line = line.trim();
    for left in ['「', '“', '"', '\''] {
        let right = match left {
            '「' => '」',
            '“' => '”',
            '"' => '"',
            '\'' => '\'',
            _ => left,
        };
        if let Some((_, rest)) = line.split_once(left) {
            if let Some((answer, _)) = rest.split_once(right) {
                let answer = answer.trim();
                if !answer.is_empty() {
                    return Some(answer.to_string());
                }
            }
        }
    }

    for marker in ["验证码是", "答案是", "标记是", "is"] {
        if let Some((_, answer)) = line.split_once(marker) {
            let answer = answer
                .trim()
                .trim_start_matches([':', '：'])
                .trim()
                .trim_end_matches(['。', '.', ',']);
            if !answer.is_empty() {
                return Some(answer.to_string());
            }
        }
    }

    for separator in ["：", ":"] {
        if let Some((label, answer)) = line.split_once(separator) {
            let label = label.trim();
            if label.contains("验证码") || label.contains("答案") || label.contains("标记") {
                let answer = answer.trim().trim_end_matches(['。', '.', ',']);
                if !answer.is_empty() {
                    return Some(answer.to_string());
                }
            }
        }
    }

    None
}

fn first_marker_like_token(content: &str) -> Option<String> {
    content
        .split(|ch: char| {
            ch.is_whitespace()
                || matches!(
                    ch,
                    '。' | '，'
                        | ','
                        | '.'
                        | '；'
                        | ';'
                        | '：'
                        | ':'
                        | '"'
                        | '\''
                        | '`'
                        | '）'
                        | ')'
                        | '（'
                        | '('
                )
        })
        .map(str::trim)
        .find(|token| token.to_ascii_lowercase().contains("marker"))
        .map(str::to_string)
}
