use benshu_hardness::{
    is_frontstage_single_image_turn as is_frontstage_single_image_turn_core,
    is_simple_media_understanding_turn as is_simple_media_understanding_turn_core, MediaKind,
    MessageSnapshot,
};
use std::collections::HashSet;

use crate::agent::message::{Content, ContentPart, Message, Role};
use crate::skills::tool::{capability_route_requires_real_tool_call, CapabilityRouteHint};

pub(crate) fn strategy_from_outcome(outcome: &str) -> Option<&'static str> {
    match outcome {
        "preprocess_failed" => Some("attachment_fallback"),
        "model_failed_after_preprocess" => Some("alternate_model_fallback"),
        "model_result_insufficient" => Some("clarification_or_manual_review"),
        _ => None,
    }
}

pub(crate) fn latest_user_message_has_media(messages: &[Message]) -> bool {
    latest_user_message_with_media(messages).is_some()
        || latest_user_message_has_parsed_attachment_context(messages)
}

fn latest_user_message_has_parsed_attachment_context(messages: &[Message]) -> bool {
    messages
        .iter()
        .rev()
        .find(|message| message.role == Role::User)
        .is_some_and(message_has_parsed_attachment_context)
}

fn message_has_parsed_attachment_context(message: &Message) -> bool {
    match &message.content {
        Content::Parts(parts) => parts.iter().any(|part| match part {
            ContentPart::Text { text } => {
                let trimmed = text.trim_start();
                trimmed.starts_with("[Parsed ") || trimmed.contains("\n[Parsed ")
            }
            _ => false,
        }),
        _ => false,
    }
}

fn latest_user_message_snapshot(messages: &[Message]) -> Option<MessageSnapshot> {
    let latest_user = messages
        .iter()
        .rev()
        .find(|message| message.role == Role::User)?;
    let media_source = latest_user_message_with_media(messages).unwrap_or(latest_user);
    let media = match &media_source.content {
        Content::Parts(parts) => parts
            .iter()
            .filter_map(|part| match part {
                ContentPart::Image { .. } => Some(MediaKind::Image),
                ContentPart::Audio { .. } => Some(MediaKind::Audio),
                ContentPart::Video { .. } => Some(MediaKind::Video),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };

    Some(MessageSnapshot {
        text: latest_user.content.as_text(),
        media,
    })
}

fn message_has_media(message: &Message) -> bool {
    matches!(
        &message.content,
        Content::Parts(parts)
            if parts.iter().any(|part| matches!(
                part,
                ContentPart::Image { .. } | ContentPart::Audio { .. } | ContentPart::Video { .. }
            ))
    )
}

pub(crate) fn latest_user_message_with_media(messages: &[Message]) -> Option<&Message> {
    messages
        .iter()
        .rev()
        .find(|message| message.role == Role::User && message_has_media(message))
}

pub(crate) fn latest_turn_simple_media_understanding(
    messages: &[Message],
    complexity: &crate::agent::evolution::complexity::ComplexityScore,
    steps: usize,
    total_chars: usize,
) -> bool {
    latest_user_message_snapshot(messages)
        .as_ref()
        .is_some_and(|snapshot| {
            is_simple_media_understanding_turn_core(snapshot, complexity)
                || is_frontstage_single_image_turn_core(snapshot, complexity, steps, total_chars)
        })
}

pub(crate) fn route_requires_real_tool_call_for_turn(
    route: CapabilityRouteHint,
    has_media_input: bool,
) -> bool {
    if has_media_input
        && matches!(
            route,
            CapabilityRouteHint::DocumentUnderstanding | CapabilityRouteHint::VisualUnderstanding
        )
    {
        return false;
    }

    capability_route_requires_real_tool_call(route)
}

pub(crate) fn should_force_direct_multimodal_answer(
    raw_capability_route: Option<CapabilityRouteHint>,
    has_media_input: bool,
    has_media_followup_contract: bool,
) -> bool {
    has_media_input
        && !has_media_followup_contract
        && matches!(
            raw_capability_route,
            Some(CapabilityRouteHint::DocumentUnderstanding)
                | Some(CapabilityRouteHint::VisualUnderstanding)
                | None
        )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MediaFollowupCapabilityContract {
    pub capability_route: &'static str,
    pub execution_surface: &'static str,
    pub prefer_document_understanding_tools: bool,
}

pub(crate) fn strategies_from_messages(messages: &[Message]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut strategies = Vec::new();

    for message in messages.iter().rev() {
        if message.role == Role::Assistant {
            if let Some(raw) = message
                .metadata
                .get("provider_media_preprocess_followup_strategies")
            {
                for entry in raw
                    .split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    if seen.insert(entry.to_string()) {
                        strategies.push(entry.to_string());
                    }
                }
            }
        }

        if message.role != Role::Tool {
            continue;
        }

        let Content::Parts(parts) = &message.content else {
            continue;
        };

        for part in parts {
            let ContentPart::ToolResult {
                name: Some(name),
                content,
                ..
            } = part
            else {
                continue;
            };

            if !matches!(name.as_str(), "document_understand" | "text_extract") {
                continue;
            }

            let Ok(payload) = serde_json::from_str::<serde_json::Value>(content) else {
                continue;
            };
            let Some(route) = payload
                .get("media_preprocess_route")
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                continue;
            };
            let Some(outcome) = payload
                .get("media_pipeline_outcome")
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                continue;
            };
            let Some(strategy) = strategy_from_outcome(outcome) else {
                continue;
            };

            let entry = format!("{route}:{strategy}");
            if seen.insert(entry.clone()) {
                strategies.push(entry);
            }
        }
    }

    strategies.sort();
    strategies
}

pub(crate) fn render_strategy_prompt(strategies: &[String]) -> Option<String> {
    if strategies.is_empty() {
        return None;
    }

    let mut lines = vec![
        "### MEDIA FOLLOW-UP STRATEGY".to_string(),
        "The previous media understanding step already produced a runtime follow-up strategy. Use it for the next action instead of fabricating understanding.".to_string(),
    ];

    for entry in strategies {
        let Some((route, strategy)) = entry.split_once(':') else {
            continue;
        };
        let instruction = match strategy {
            "attachment_fallback" => {
                "preprocessing failed; prefer attachment fallback or explicitly tell the user a more accessible file/format is needed"
            }
            "alternate_model_fallback" => {
                "preprocessing succeeded but the model path failed; prefer an alternate understanding path instead of pretending the media was understood"
            }
            "clarification_or_manual_review" => {
                "the model result was insufficient; ask for clarification or recommend manual review instead of over-claiming certainty"
            }
            _ => continue,
        };
        lines.push(format!("- `{route}` -> `{strategy}`: {instruction}."));
    }

    Some(lines.join("\n"))
}

pub(crate) fn capability_contract(
    strategies: &[String],
) -> Option<MediaFollowupCapabilityContract> {
    let has_alternate = strategies
        .iter()
        .any(|entry| entry.ends_with(":alternate_model_fallback"));
    if has_alternate {
        return Some(MediaFollowupCapabilityContract {
            capability_route: "document_understanding",
            execution_surface: "document_understanding_alternate_model_fallback",
            prefer_document_understanding_tools: true,
        });
    }

    let has_attachment = strategies
        .iter()
        .any(|entry| entry.ends_with(":attachment_fallback"));
    if has_attachment {
        return Some(MediaFollowupCapabilityContract {
            capability_route: "document_understanding",
            execution_surface: "document_understanding_attachment_fallback",
            prefer_document_understanding_tools: true,
        });
    }

    let has_clarification = strategies
        .iter()
        .any(|entry| entry.ends_with(":clarification_or_manual_review"));
    if has_clarification {
        return Some(MediaFollowupCapabilityContract {
            capability_route: "document_understanding",
            execution_surface: "document_understanding_clarification_or_manual_review",
            prefer_document_understanding_tools: true,
        });
    }

    None
}

pub(crate) fn apply_capability_route(
    extra: &mut serde_json::Value,
    contract: Option<MediaFollowupCapabilityContract>,
) {
    let Some(contract) = contract else {
        return;
    };

    if !extra.is_object() {
        *extra = serde_json::Value::Object(serde_json::Map::new());
    }

    if let serde_json::Value::Object(map) = extra {
        map.entry("capability_route".to_string())
            .or_insert_with(|| serde_json::json!(contract.capability_route));
        map.entry("preferred_capability_domain".to_string())
            .or_insert_with(|| serde_json::json!(contract.capability_route));
        map.entry("media_followup_execution_surface".to_string())
            .or_insert_with(|| serde_json::json!(contract.execution_surface));
    }
}
