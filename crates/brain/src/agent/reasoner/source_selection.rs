pub(super) fn text_contains_url(text: &str) -> bool {
    text.contains("http://") || text.contains("https://")
}

pub(super) fn first_url(text: &str) -> Option<String> {
    text.split_whitespace()
        .map(|part| {
            part.trim_matches(|ch: char| {
                matches!(
                    ch,
                    '"' | '\'' | ',' | '.' | ')' | ']' | '}' | '>' | '。' | '，' | '）' | '】'
                )
            })
        })
        .find(|part| part.starts_with("http://") || part.starts_with("https://"))
        .map(str::to_string)
}

pub(super) fn explicit_source_url_in_result(result: &str) -> Option<String> {
    result.lines().find_map(|line| {
        line.trim()
            .strip_prefix("source_url:")
            .map(str::trim)
            .filter(|value| value.starts_with("http://") || value.starts_with("https://"))
            .filter(|value| !known_search_garbage_url(value))
            .map(str::to_string)
    })
}

pub(super) fn tool_result_json_blob(result: &str) -> Option<&str> {
    let trimmed = result.trim();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return first_balanced_json_value(trimmed).or(Some(trimmed));
    }

    let candidate = trimmed
        .find("\n{")
        .map(|idx| trimmed[idx + 1..].trim())
        .or_else(|| trimmed.find('{').map(|idx| trimmed[idx..].trim()))?;
    first_balanced_json_value(candidate).or(Some(candidate))
}

pub(super) fn first_balanced_json_value(candidate: &str) -> Option<&str> {
    let mut chars = candidate.char_indices();
    let (_, first) = chars.next()?;
    let (open, close) = match first {
        '{' => ('{', '}'),
        '[' => ('[', ']'),
        _ => return None,
    };
    let mut depth = 1usize;
    let mut in_string = false;
    let mut escaped = false;

    for (idx, ch) in chars {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        if ch == '"' {
            in_string = true;
        } else if ch == open {
            depth += 1;
        } else if ch == close {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(&candidate[..idx + ch.len_utf8()]);
            }
        }
    }

    None
}

pub(super) fn known_search_garbage_url(url: &str) -> bool {
    let lowered = url.to_ascii_lowercase();
    lowered.contains("support.google.com/")
        || lowered.contains("bing.com/ck/")
        || lowered.contains("bing.com/search")
        || lowered.contains("google.com/search")
        || lowered.contains("duckduckgo.com/?q=")
        || lowered.contains("search.yahoo.com/search")
}

pub(super) fn source_matches_query_focus(
    query: &str,
    url: &str,
    title: &str,
    snippet: &str,
) -> bool {
    if known_search_garbage_url(url) {
        return false;
    }

    let focus_terms = query_focus_terms(query);
    if focus_terms.is_empty() {
        return true;
    }
    let haystack = format!("{url} {title} {snippet}").to_lowercase();
    focus_terms.iter().any(|term| haystack.contains(term))
}

pub(super) fn candidate_source_score(query: &str, url: &str, title: &str, snippet: &str) -> i32 {
    let url_lower = url.to_ascii_lowercase();
    let title_lower = title.to_ascii_lowercase();
    let snippet_lower = snippet.to_ascii_lowercase();

    let mut score = 0;

    for term in query_focus_terms(query) {
        if title_lower.contains(&term) {
            score += 20;
        } else if snippet_lower.contains(&term) {
            score += 10;
        } else if url_lower.contains(&term) {
            score += 6;
        }
    }

    if !title.trim().is_empty() {
        score += 3;
    }
    if !snippet.trim().is_empty() {
        score += 2;
    }
    if url_lower.matches('/').count() > 3 {
        score += 4;
    }
    if looks_like_root_or_search_page(&url_lower) {
        score -= 25;
    }
    if url_lower.contains("login")
        || url_lower.contains("captcha")
        || url_lower.contains("verify")
        || url_lower.contains("403")
    {
        score -= 20;
    }

    score
}

fn query_focus_terms(query: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let lowered = query.to_lowercase();
    for token in lowered
        .split(|ch: char| !ch.is_alphanumeric() && ch != '-' && ch != '_')
        .map(str::trim)
        .filter(|token| token.chars().count() >= 3)
    {
        if !generic_query_stopword(token) {
            push_unique(&mut terms, token.to_string());
        }
    }
    for token in query
        .split(|ch: char| {
            ch.is_ascii()
                || ch.is_whitespace()
                || matches!(
                    ch,
                    '，' | '。' | '、' | '；' | '：' | '！' | '？' | '（' | '）' | '《' | '》'
                )
        })
        .map(str::trim)
        .filter(|token| token.chars().count() >= 2)
    {
        push_cjk_focus_terms(&mut terms, token);
    }
    terms
}

fn push_cjk_focus_terms(terms: &mut Vec<String>, token: &str) {
    let chars = token.chars().collect::<Vec<_>>();
    if chars.len() <= 4 {
        if !generic_cjk_query_stopword(token) {
            push_unique(terms, token.to_lowercase());
        }
        return;
    }

    for width in [4usize, 3, 2] {
        if chars.len() < width {
            continue;
        }
        for window in chars.windows(width) {
            let value = window.iter().collect::<String>();
            if !generic_cjk_query_stopword(&value) {
                push_unique(terms, value.to_lowercase());
            }
        }
    }
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}

fn generic_query_stopword(token: &str) -> bool {
    matches!(
        token,
        "the"
            | "and"
            | "for"
            | "with"
            | "from"
            | "about"
            | "latest"
            | "recent"
            | "search"
            | "find"
            | "look"
            | "lookup"
            | "query"
            | "news"
            | "best"
            | "top"
            | "free"
            | "download"
            | "save"
            | "write"
            | "generate"
            | "create"
            | "knowledge"
    )
}

fn generic_cjk_query_stopword(token: &str) -> bool {
    matches!(
        token,
        "搜索"
            | "查找"
            | "检索"
            | "寻找"
            | "获取"
            | "保存"
            | "存入"
            | "导入"
            | "写入"
            | "知识"
            | "知识库"
            | "数据库"
            | "最新"
            | "最近"
            | "今天"
            | "现在"
            | "当前"
            | "关于"
            | "根据"
            | "然后"
            | "并且"
            | "以及"
    )
}

fn looks_like_root_or_search_page(url_lower: &str) -> bool {
    let trimmed = url_lower.trim_end_matches('/');
    let after_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(trimmed);
    !after_scheme.contains('/') || after_scheme.contains("/search") || after_scheme.contains("?q=")
}

pub(super) fn best_lookup_source_url_for_query(query: &str, result: &str) -> Option<String> {
    let json_blob = tool_result_json_blob(result)?;
    let payload: serde_json::Value = serde_json::from_str(json_blob).ok()?;
    if let Some(results) = payload.get("results").and_then(|value| value.as_array()) {
        return results
            .iter()
            .filter_map(|entry| {
                let url = entry.get("url")?.as_str()?.trim();
                let title = entry
                    .get("title")
                    .and_then(|value| value.as_str())
                    .unwrap_or("")
                    .trim();
                let snippet = entry
                    .get("snippet")
                    .and_then(|value| value.as_str())
                    .unwrap_or("")
                    .trim();
                source_matches_query_focus(query, url, title, snippet).then_some((
                    candidate_source_score(query, url, title, snippet),
                    url.to_string(),
                ))
            })
            .max_by_key(|(score, _)| *score)
            .map(|(_, url)| url);
    }

    if let Some(url) = payload.get("url").and_then(|value| value.as_str()) {
        let trimmed = url.trim();
        if !trimmed.is_empty() && !known_search_garbage_url(trimmed) {
            return Some(trimmed.to_string());
        }
    }

    payload
        .get("verification_preview")
        .and_then(|value| value.get("sources"))
        .and_then(|value| value.as_array())
        .and_then(|sources| {
            sources.iter().find_map(|source| {
                source
                    .get("uri")
                    .and_then(|value| value.as_str())
                    .map(str::trim)
                    .filter(|uri| !uri.is_empty() && !known_search_garbage_url(uri))
                    .map(str::to_string)
            })
        })
        .or_else(|| explicit_source_url_in_result(result))
        .or_else(|| {
            first_url(result).and_then(|url| (!known_search_garbage_url(&url)).then_some(url))
        })
}

pub(super) fn followup_execution_source_url(query: &str, result: &str) -> Option<String> {
    best_lookup_source_url_for_query(query, result)
        .or_else(|| explicit_source_url_in_result(result))
}
