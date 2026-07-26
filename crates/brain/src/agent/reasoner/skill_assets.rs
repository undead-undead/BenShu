use super::reasoner_constants;
use crate::agent::message::{Content, Message, Role};

pub(crate) fn matched_skill_manual_name(extra: &serde_json::Value) -> Option<String> {
    extra
        .as_object()
        .and_then(|map| map.get("matched_skill_manual"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(crate) fn matched_skill_manual_name_from_messages(messages: &[Message]) -> Option<String> {
    const MARKER: &str = "This request matches the skill `";

    messages.iter().rev().find_map(|message| {
        if message.role != Role::System {
            return None;
        }

        let text = message.text();
        let start = text.find(MARKER)? + MARKER.len();
        let rest = &text[start..];
        let end = rest.find('`')?;
        let skill_name = rest[..end].trim();
        if skill_name.is_empty() {
            return None;
        }

        Some(skill_name.to_string())
    })
}

pub(crate) fn approved_forge_request_from_messages(messages: &[Message]) -> bool {
    messages.iter().rev().any(|message| {
        message.role == Role::System
            && message
                .text()
                .contains(reasoner_constants::MARKER_FORGE_APPROVED)
    })
}

pub(crate) fn forged_session_tool_names_from_messages(messages: &[Message]) -> Vec<String> {
    let mut forged = Vec::new();

    for message in messages.iter().rev() {
        if message.role != Role::Tool {
            continue;
        }

        let Content::Parts(parts) = &message.content else {
            continue;
        };

        for part in parts {
            let crate::agent::message::ContentPart::ToolResult {
                name: Some(name),
                content,
                ..
            } = part
            else {
                continue;
            };

            if name != "forge_skill" {
                continue;
            }

            let Ok(payload) = serde_json::from_str::<serde_json::Value>(content) else {
                continue;
            };
            let Some(tool_name) = payload
                .get("tool_name")
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                continue;
            };

            let status = payload
                .get("status")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            let source = payload
                .get("source")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            let scope = payload
                .get("scope")
                .and_then(|value| value.as_str())
                .unwrap_or_default();

            if status == "success"
                && source == "forge"
                && scope == "session"
                && !forged.iter().any(|existing| existing == tool_name)
            {
                forged.push(tool_name.to_string());
            }
        }
    }

    forged
}

pub(crate) fn forged_session_tool_already_executed(messages: &[Message], tool_name: &str) -> bool {
    messages.iter().rev().any(|message| {
        if message.role != Role::Tool {
            return false;
        }

        match &message.content {
            Content::Parts(parts) => parts.iter().any(|part| match part {
                crate::agent::message::ContentPart::ToolResult {
                    name: Some(name), ..
                } => name == tool_name,
                _ => false,
            }),
            _ => false,
        }
    })
}

pub(crate) fn matched_skill_asset_path_from_messages(messages: &[Message]) -> Option<String> {
    messages.iter().rev().find_map(|message| {
        if message.role != Role::User {
            return None;
        }

        message
            .text()
            .split_whitespace()
            .find_map(normalize_explicit_skill_asset_token)
    })
}

pub(crate) fn available_skill_assets_from_messages(
    messages: &[Message],
    skill_name: &str,
) -> Vec<String> {
    messages
        .iter()
        .rev()
        .find_map(|message| {
            let assets = skill_manual_available_assets(message, skill_name);
            if assets.is_empty() {
                None
            } else {
                Some(assets)
            }
        })
        .unwrap_or_default()
}

pub(crate) fn resolve_skill_asset_path_from_messages(
    messages: &[Message],
    skill_name: Option<&str>,
) -> Option<String> {
    if let Some(explicit) = matched_skill_asset_path_from_messages(messages) {
        return Some(explicit);
    }

    let Some(skill_name) = skill_name else {
        return None;
    };
    let latest_user = messages
        .iter()
        .rev()
        .find(|message| message.role == Role::User)
        .map(|message| message.text())?;
    let kind = infer_skill_asset_kind_from_text(&latest_user)?;
    available_skill_assets_from_messages(messages, skill_name)
        .into_iter()
        .find(|asset| asset.starts_with(&format!("{kind}/")))
}

pub(crate) fn runtime_session_title(
    extra: Option<&serde_json::Value>,
) -> Option<(String, &'static str)> {
    let extra = extra.and_then(|value| value.as_object())?;

    if let Some(title) = extra
        .get("session_title")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some((title.to_string(), "extra_params.session_title"));
    }

    if let Some(title) = extra
        .get("title")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some((title.to_string(), "extra_params.title"));
    }

    None
}

pub(crate) fn tool_result_reads_skill_manual(message: &Message, skill_name: &str) -> bool {
    if message.role != Role::Tool {
        return false;
    }

    let expected_prefix = format!("# skill: {}", skill_name.trim().to_ascii_lowercase());
    match &message.content {
        Content::Parts(parts) => parts.iter().any(|part| match part {
            crate::agent::message::ContentPart::ToolResult {
                name: Some(name),
                content,
                ..
            } if name == "read_skill_manual" => content
                .trim_start()
                .to_ascii_lowercase()
                .starts_with(&expected_prefix),
            _ => false,
        }),
        _ => false,
    }
}

pub(crate) fn skill_manual_already_loaded(messages: &[Message], skill_name: &str) -> bool {
    messages
        .iter()
        .rev()
        .any(|message| tool_result_reads_skill_manual(message, skill_name))
}

pub(crate) fn tool_result_reads_skill_asset(message: &Message, asset_path: &str) -> bool {
    if message.role != Role::Tool {
        return false;
    }

    let expected_prefix = format!("# Skill Asset: {}", asset_path.trim().replace('\\', "/"));
    match &message.content {
        Content::Parts(parts) => parts.iter().any(|part| match part {
            crate::agent::message::ContentPart::ToolResult {
                name: Some(name),
                content,
                ..
            } if name == "read_skill_asset" => content.trim_start().starts_with(&expected_prefix),
            _ => false,
        }),
        _ => false,
    }
}

pub(crate) fn skill_asset_already_loaded(messages: &[Message], asset_path: &str) -> bool {
    messages
        .iter()
        .rev()
        .any(|message| tool_result_reads_skill_asset(message, asset_path))
}

fn normalize_explicit_skill_asset_token(token: &str) -> Option<String> {
    let trimmed = token.trim_matches(|ch: char| {
        matches!(
            ch,
            '`' | '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | '.' | ':' | ';'
        )
    });

    let normalized = trimmed.replace('\\', "/");
    if normalized.starts_with("references/")
        || normalized.starts_with("templates/")
        || normalized.starts_with("scripts/")
    {
        return Some(normalized);
    }

    None
}

fn infer_skill_asset_kind_from_text(text: &str) -> Option<&'static str> {
    let lower = format!(" {} ", text.to_ascii_lowercase());
    if lower.contains("references/")
        || lower.contains(" reference ")
        || lower.contains(" references ")
        || text.contains("参考")
        || text.contains("资料")
    {
        return Some("references");
    }
    if lower.contains("templates/")
        || lower.contains(" template ")
        || lower.contains(" templates ")
        || text.contains("模板")
        || text.contains("样板")
    {
        return Some("templates");
    }
    if lower.contains("scripts/")
        || lower.contains(" script ")
        || lower.contains(" scripts ")
        || text.contains("脚本")
    {
        return Some("scripts");
    }
    None
}

fn skill_manual_available_assets(message: &Message, skill_name: &str) -> Vec<String> {
    if message.role != Role::Tool {
        return Vec::new();
    }

    let expected_prefix = format!("# skill: {}", skill_name.trim().to_ascii_lowercase());
    let content = match &message.content {
        Content::Parts(parts) => parts.iter().find_map(|part| match part {
            crate::agent::message::ContentPart::ToolResult {
                name: Some(name),
                content,
                ..
            } if name == "read_skill_manual" => Some(content),
            _ => None,
        }),
        _ => None,
    };

    let Some(content) = content else {
        return Vec::new();
    };
    if !content
        .trim_start()
        .to_ascii_lowercase()
        .starts_with(&expected_prefix)
    {
        return Vec::new();
    }

    let mut assets = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("- `") {
            if let Some((path, _)) = rest.split_once("` (") {
                let normalized = path.trim().replace('\\', "/");
                if normalized.starts_with("references/")
                    || normalized.starts_with("templates/")
                    || normalized.starts_with("scripts/")
                {
                    assets.push(normalized);
                }
            }
        }
    }
    assets
}
