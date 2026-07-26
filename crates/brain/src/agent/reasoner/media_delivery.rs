pub(super) fn has_repetitive_pattern(response: &str) -> bool {
    let non_empty_lines = response
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if non_empty_lines.len() >= 4 {
        let mut normalized = std::collections::HashMap::<String, usize>::new();
        for line in &non_empty_lines {
            let key = line.split_whitespace().collect::<String>();
            *normalized.entry(key).or_insert(0) += 1;
        }
        if normalized.values().any(|count| *count >= 3) {
            return true;
        }
    }

    let condensed = response.split_whitespace().collect::<String>();
    if condensed.chars().count() < 24 {
        return false;
    }

    for window in [6usize, 8, 10, 12] {
        let prefix = condensed.chars().take(window).collect::<String>();
        if prefix.chars().count() < window {
            continue;
        }
        let repeats = condensed.matches(&prefix).count();
        if repeats >= 4 {
            return true;
        }
    }

    false
}

pub(super) fn has_gibberish_pattern(response: &str) -> bool {
    let trimmed = response.trim();
    let total_chars = trimmed.chars().count();
    if total_chars < 16 {
        return false;
    }

    let suspicious_chars = trimmed
        .chars()
        .filter(|ch| {
            !(ch.is_ascii_alphanumeric()
                || ch.is_ascii_whitespace()
                || ('\u{4E00}'..='\u{9FFF}').contains(ch)
                || matches!(
                    ch,
                    '，' | '。'
                        | '、'
                        | '：'
                        | '；'
                        | '！'
                        | '？'
                        | '（'
                        | '）'
                        | '《'
                        | '》'
                        | '“'
                        | '”'
                        | '‘'
                        | '’'
                        | ' '
                        | ','
                        | '.'
                        | ':'
                        | ';'
                        | '!'
                        | '?'
                        | '-'
                        | '_'
                        | '\''
                        | '"'
                        | '('
                        | ')'
                ))
        })
        .count();

    suspicious_chars * 5 >= total_chars
}

pub(super) fn answer_needs_text_enrichment(response: &str) -> bool {
    let trimmed = response.trim();
    if trimmed.is_empty() {
        return false;
    }

    let lowered = trimmed.to_lowercase();
    let mentions_text = lowered.contains("文字")
        || lowered.contains("文本")
        || lowered.contains("字样")
        || lowered.contains("english text")
        || lowered.contains("text");
    if !mentions_text {
        return false;
    }

    let has_specific_excerpt = trimmed.contains('“')
        || trimmed.contains('"')
        || trimmed.contains('\'')
        || trimmed.contains('：')
        || trimmed.contains(':');

    !has_specific_excerpt
}

pub(super) fn is_low_value_answer(query: &str, response: &str) -> bool {
    let trimmed = response.trim();
    if trimmed.is_empty() {
        return true;
    }
    if query_requests_structured_output(query) && response_looks_like_structured_output(trimmed) {
        return false;
    }
    let lowered = trimmed.to_lowercase();
    let refusal_patterns = [
        "无法直接描述图片",
        "无法查看图片",
        "无法看到图片",
        "没有直接访问图片",
        "不能查看图片",
        "无法访问图片",
        "无法读取图片",
        "没有提供图片",
        "未提供图片",
        "请您提供图片",
        "请提供图片",
        "请稍候",
        "请稍等",
        "稍后再试",
        "我将为您提供",
        "我将为你提供",
        "我将从我的知识库",
        "我将从知识库",
        "我已收到您的请求",
        "我已收到你的请求",
        "我已收到请求",
        "cannot see the image",
        "cannot view the image",
        "can't see the image",
        "can't view the image",
        "no direct access to images",
        "cannot access images",
        "多模态交付没有稳定落成",
        "请再试一次",
        "please wait",
        "please hold on",
        "i will provide",
        "i will retrieve",
        "i have received your request",
        "multimodal turn did not settle",
        "please try again",
        "你是一个有用的助手",
        "请用中文简洁描述用户提供的图片内容",
        "you are a helpful ai assistant",
        "describe the image",
        "回答图片里有什么；如果看不清，就回答",
        "此回答回答图片里有什么",
        "直接回答图片里有什么",
        "如果看不看得到，就回答",
        "如果确实无法判断，再回答“不确定”",
    ];
    if refusal_patterns
        .iter()
        .any(|pattern| lowered.contains(pattern))
    {
        return true;
    }
    if has_repetitive_pattern(trimmed) || has_gibberish_pattern(trimmed) {
        return true;
    }
    let response_len = trimmed.chars().count();
    if query_requests_concise_answer(query) && response_len <= 80 {
        return false;
    }
    if prefers_chinese(query) && !prefers_chinese(trimmed) {
        return response_len < 24;
    }
    response_len < 8
}

pub(super) fn query_requests_concise_answer(query: &str) -> bool {
    let lowered = query.to_lowercase();
    [
        "只回答",
        "只说",
        "只输出",
        "仅回答",
        "仅输出",
        "菜单名",
        "名称",
        "按钮名",
        "字段名",
        "label",
        "name",
        "one word",
        "only answer",
        "just answer",
    ]
    .iter()
    .any(|pattern| lowered.contains(pattern))
}

pub(super) fn query_requests_structured_output(query: &str) -> bool {
    let lowered = query.to_lowercase();
    lowered.contains("json")
        || lowered.contains("结构化")
        || lowered.contains("字段")
        || lowered.contains("只返回一个 json 对象")
        || lowered.contains("只返回json")
        || lowered.contains("只输出json")
}

pub(super) fn response_looks_like_structured_output(response: &str) -> bool {
    normalized_structured_output(response).is_some()
}

pub(super) fn normalized_structured_output(response: &str) -> Option<String> {
    let trimmed = response.trim();
    let mut candidates = Vec::new();

    candidates.push(trimmed.to_string());

    if trimmed.starts_with("```") {
        let mut lines = trimmed.lines();
        let _opening = lines.next();
        let inner = lines.collect::<Vec<_>>().join("\n");
        let unfenced = inner
            .strip_suffix("```")
            .map(str::trim)
            .unwrap_or_else(|| inner.trim())
            .to_string();
        if !unfenced.is_empty() {
            candidates.push(unfenced);
        }
    }

    for (open, close) in [('{', '}'), ('[', ']')] {
        if let (Some(start), Some(end)) = (trimmed.find(open), trimmed.rfind(close)) {
            if end > start {
                let slice = trimmed[start..=end].trim();
                if !slice.is_empty() {
                    candidates.push(slice.to_string());
                }
            }
        }
    }

    candidates.into_iter().find_map(|candidate| {
        serde_json::from_str::<serde_json::Value>(&candidate)
            .ok()
            .and_then(|value| {
                if value.is_object() || value.is_array() {
                    serde_json::to_string_pretty(&value).ok()
                } else {
                    None
                }
            })
    })
}

pub(super) fn understanding_failure_text(query: &str) -> String {
    if prefers_chinese(query) {
        "我收到了这张图片，但本地视觉模型这次没有产出可用的自然语言描述。".to_string()
    } else {
        "I received the image, but the local vision model did not produce a usable natural-language description this time.".to_string()
    }
}

fn prefers_chinese(text: &str) -> bool {
    text.chars()
        .any(|ch| ('\u{4E00}'..='\u{9FFF}').contains(&ch))
}
