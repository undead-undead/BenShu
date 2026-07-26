use crate::model::{ComplexityScore, MediaKind, MessageSnapshot};

pub fn strip_frontstage_media_injection(text: &str) -> String {
    let user_visible_prefix = text.split("\n[Parsed ").next().unwrap_or(text);
    user_visible_prefix
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.starts_with("[Image Attachment")
                && !trimmed.starts_with("[Image Content]")
                && !trimmed.starts_with("[Parsed ")
                && !trimmed.starts_with("[Audio Attachment")
                && !trimmed.starts_with("[Video Attachment")
                && !trimmed.starts_with("source:")
                && !trimmed.starts_with("parser_mode:")
                && !trimmed.starts_with("file://")
                && !trimmed.starts_with("http://")
                && !trimmed.starts_with("https://")
        })
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

pub fn is_simple_media_understanding_turn(
    last_user_message: &MessageSnapshot,
    complexity: &ComplexityScore,
) -> bool {
    let media_detected = !last_user_message.media.is_empty()
        || complexity
            .metadata
            .get("media_types")
            .and_then(|value| value.as_array())
            .is_some_and(|types| !types.is_empty());

    let user_text = strip_frontstage_media_injection(&last_user_message.text).to_lowercase();
    let short_frontstage_prompt = user_text.chars().count() <= 160;
    let simple_understanding_intent = [
        "描述",
        "看看",
        "图里",
        "图片",
        "风格",
        "像什么",
        "什么样",
        "what is in",
        "what's in",
        "what is this",
        "describe",
        "style",
        "look like",
        "photo",
        "image",
    ]
    .iter()
    .any(|marker| user_text.contains(marker));
    let complex_execution_intent = [
        "分析",
        "规划",
        "重构",
        "同时",
        "并行",
        "execute",
        "implement",
        "refactor",
        "parallel",
    ]
    .iter()
    .any(|marker| user_text.contains(marker));

    media_detected
        && short_frontstage_prompt
        && !complex_execution_intent
        && (simple_understanding_intent || complexity.predicted_output_tokens <= 2200)
}

pub fn is_frontstage_single_image_turn(
    last_user_message: &MessageSnapshot,
    complexity: &ComplexityScore,
    steps: usize,
    total_chars: usize,
) -> bool {
    if steps > 1 || total_chars > 1200 {
        return false;
    }

    let direct_image_count = last_user_message
        .media
        .iter()
        .filter(|media| matches!(media, MediaKind::Image))
        .count();

    if direct_image_count == 1 {
        return true;
    }

    complexity
        .metadata
        .get("media_types")
        .and_then(|value| value.as_array())
        .map(|types| {
            types.len() == 1
                && types
                    .first()
                    .and_then(|value| value.as_str())
                    .is_some_and(|media| media == "image")
        })
        .unwrap_or(false)
}
