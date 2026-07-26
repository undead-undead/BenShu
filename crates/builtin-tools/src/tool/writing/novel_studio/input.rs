use serde_json::json;

use super::{preview_chars, tool_schema, NovelStudioArgs};

pub(super) fn normalize_novel_studio_arguments(mut value: serde_json::Value) -> serde_json::Value {
    let Some(map) = value.as_object_mut() else {
        return value;
    };
    let nested = match map.get("input").cloned() {
        Some(serde_json::Value::String(raw)) => {
            match serde_json::from_str::<serde_json::Value>(&raw) {
                Ok(value) => Some(value),
                Err(_) => {
                    if !raw.trim().is_empty()
                        && [
                            "content", "plan", "outline", "summary", "notes", "brief", "feedback",
                        ]
                        .iter()
                        .all(|key| map.get(*key).map(argument_value_is_empty).unwrap_or(true))
                    {
                        map.insert("content".to_string(), serde_json::Value::String(raw));
                        map.remove("input");
                    }
                    None
                }
            }
        }
        Some(serde_json::Value::Object(_)) => map.get("input").cloned(),
        _ => None,
    };
    let Some(serde_json::Value::Object(nested)) = nested else {
        normalize_novel_studio_argument_shapes(map);
        return value;
    };
    for (key, nested_value) in nested {
        let should_insert = map.get(&key).map(argument_value_is_empty).unwrap_or(true);
        if should_insert {
            map.insert(key, nested_value);
        }
    }
    map.remove("input");
    normalize_novel_studio_argument_shapes(map);
    value
}

pub(super) fn missing_novel_action_result() -> serde_json::Value {
    json!({
        "success": false,
        "error": "missing required action",
        "recoverable": true,
        "available_actions": tool_schema::PUBLIC_ACTIONS,
        "next_step_hint": "Call novel_studio again with an explicit action and the required project or chapter fields."
    })
}

pub(super) fn wrong_novel_studio_action_result(action: &str) -> serde_json::Value {
    let known_external_tool = matches!(
        action,
        "fetch_document"
            | "knowledge_search"
            | "tiered_search"
            | "knowledge_import_url"
            | "knowledge_manage_document"
            | "read_file"
            | "write_file"
            | "edit_file"
            | "list_dir"
    );
    let internal_hint = tool_schema::internal_compat_action_hint(action);
    json!({
        "success": false,
        "recoverable": true,
        "error_kind": "wrong_tool_action",
        "error": if internal_hint.is_some() {
            format!("`{action}` is an internal compatibility action, not part of the public novel_studio tool surface")
        } else {
            format!("unknown novel_studio action: {action}")
        },
        "attempted_action": action,
        "available_actions": tool_schema::PUBLIC_ACTIONS,
        "internal_compat_action": internal_hint.is_some(),
        "canonical_action_hint": internal_hint,
        "external_tool_hint": if known_external_tool {
            Some(format!("`{action}` is a separate equipped tool name, not a novel_studio action. Call `{action}` directly, then call novel_studio again with the resulting content or path."))
        } else {
            None
        },
        "next_step_hint": "Use a listed novel_studio action, or call the separate equipped tool directly if the attempted action is a different tool name. Do not nest other tool names inside novel_studio.action."
    })
}

pub(super) fn missing_required_content_result(args: &NovelStudioArgs) -> Option<serde_json::Value> {
    let action = args.action.trim();
    let metadata_revision = matches!(action, "revise_draft" | "revise_chapter")
        && (!args.summary.trim().is_empty()
            || !args.key_facts.is_empty()
            || !args.continuity_updates.is_empty()
            || !args.chapter_title.trim().is_empty()
            || !args.status.trim().is_empty()
            || !args.revision_notes.trim().is_empty()
            || !args.feedback.trim().is_empty());
    if !matches!(
        action,
        "add_source"
            | "import_chapters"
            | "update_style"
            | "write_draft"
            | "add_chapter"
            | "revise_draft"
            | "revise_chapter"
            | "update_truth"
    ) || !args.content.trim().is_empty()
        || metadata_revision
    {
        return None;
    }

    let mut required_fields = vec!["content"];
    if matches!(
        action,
        "write_draft" | "add_chapter" | "revise_draft" | "revise_chapter"
    ) {
        required_fields.push("project_path");
        required_fields.push("chapter_number or chapter_title");
    }

    Some(json!({
        "success": false,
        "recoverable": true,
        "error_kind": "missing_required_content",
        "action": action,
        "required_fields": required_fields,
        "next_step_hint": "Generate the actual body text first, or retrieve source/body material when the action is material intake, then call this action again with content containing the complete text to persist. If the text is too long for a tool-call JSON, return only the body text in the next assistant message so the runtime can attach it to this pending content-required action. If the source is represented by a URL, knowledge collection/path, or local path, call the equipped retrieval/read tool directly and pass the resulting material here. Do not write a retrieval command as file content. For revise_chapter, metadata-only revisions are also valid when summary, key_facts, continuity_updates, chapter_title, status, revision_notes, or feedback are supplied. Reading or composing context alone does not satisfy a write request.",
        "example_shape": {
            "action": action,
            "project_path": args.project_path,
            "chapter_number": args.chapter_number,
            "chapter_title": args.chapter_title,
            "content": "<full text to save>"
        }
    }))
}

pub(super) fn invalid_source_content_result(args: &NovelStudioArgs) -> Option<serde_json::Value> {
    if args.action.trim() != "add_source" {
        return None;
    }
    let content = args.content.trim();
    if !source_content_is_locator_only(content, args.source_url.trim()) {
        return None;
    }
    Some(json!({
        "success": false,
        "recoverable": true,
        "error_kind": "invalid_source_content",
        "action": "add_source",
        "invalid_fields": ["content"],
        "received_content_preview": preview_chars(content, 240),
        "next_step_hint": "The content field must contain reusable source material such as extracted text, a summary/evidence packet, or a user-provided excerpt. A URL, file path, identifier, or placeholder alone is not source material. Call an equipped retrieval/search/read tool first if needed, then call add_source again with the resulting text.",
        "example_shape": {
            "action": "add_source",
            "project_path": args.project_path,
            "source_title": args.source_title,
            "source_url": args.source_url,
            "content": "<extracted text, summarized evidence packet, or user-provided source excerpt>"
        }
    }))
}

fn normalize_novel_studio_argument_shapes(map: &mut serde_json::Map<String, serde_json::Value>) {
    for field in NOVEL_STUDIO_STRING_FIELDS {
        if let Some(value) = map.get_mut(*field) {
            normalize_string_argument(value);
        }
    }
    for field in NOVEL_STUDIO_STRING_LIST_FIELDS {
        if let Some(value) = map.get_mut(*field) {
            normalize_string_list_argument(value);
        }
    }
}

const NOVEL_STUDIO_STRING_FIELDS: &[&str] = &[
    "action",
    "project_path",
    "output_root",
    "source_project_path",
    "snapshot_id",
    "title",
    "language",
    "genre",
    "brief",
    "source_title",
    "source_url",
    "notes",
    "content",
    "split_pattern",
    "premise",
    "outline",
    "plan",
    "chapter_title",
    "summary",
    "feedback",
    "verdict",
    "section",
    "revision_notes",
    "status",
    "format",
    "output",
];

const NOVEL_STUDIO_STRING_LIST_FIELDS: &[&str] = &[
    "themes",
    "characters",
    "world_rules",
    "style_rules",
    "must_avoid",
    "key_facts",
    "continuity_updates",
    "issues",
    "advisories",
];

fn normalize_string_argument(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Null => {
            *value = serde_json::Value::String(String::new());
        }
        serde_json::Value::Bool(boolean) => {
            *value = serde_json::Value::String(boolean.to_string());
        }
        serde_json::Value::Number(number) => {
            *value = serde_json::Value::String(number.to_string());
        }
        serde_json::Value::String(_)
        | serde_json::Value::Array(_)
        | serde_json::Value::Object(_) => {}
    }
}

fn normalize_string_list_argument(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Null => {
            *value = serde_json::Value::Array(Vec::new());
        }
        serde_json::Value::String(raw) => {
            let trimmed = raw.trim();
            *value = if trimmed.is_empty() {
                serde_json::Value::Array(Vec::new())
            } else {
                serde_json::Value::Array(vec![serde_json::Value::String(trimmed.to_string())])
            };
        }
        serde_json::Value::Array(items) => {
            let normalized = items
                .iter()
                .filter_map(normalize_string_list_item)
                .collect::<Vec<_>>();
            *value = serde_json::Value::Array(normalized);
        }
        serde_json::Value::Bool(boolean) => {
            *value = serde_json::Value::Array(vec![serde_json::Value::String(boolean.to_string())]);
        }
        serde_json::Value::Number(number) => {
            *value = serde_json::Value::Array(vec![serde_json::Value::String(number.to_string())]);
        }
        serde_json::Value::Object(_) => {}
    }
}

fn normalize_string_list_item(value: &serde_json::Value) -> Option<serde_json::Value> {
    let text = match value {
        serde_json::Value::Null => return None,
        serde_json::Value::String(value) => value.trim().to_string(),
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Number(value) => value.to_string(),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => return None,
    };
    (!text.is_empty()).then(|| serde_json::Value::String(text))
}

fn argument_value_is_empty(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null => true,
        serde_json::Value::String(value) => value.trim().is_empty(),
        serde_json::Value::Array(value) => value.is_empty(),
        serde_json::Value::Object(value) => value.is_empty(),
        serde_json::Value::Bool(_) | serde_json::Value::Number(_) => false,
    }
}

fn source_content_is_locator_only(content: &str, source_url: &str) -> bool {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lowered = trimmed.to_ascii_lowercase();
    if !source_url.trim().is_empty() && trimmed == source_url.trim() {
        return true;
    }
    if looks_like_url(trimmed) {
        return true;
    }
    if lowered.starts_with("url:")
        || lowered.starts_with("source:")
        || lowered.starts_with("path:")
        || lowered.starts_with("file:")
    {
        let value = trimmed
            .split_once(':')
            .map(|(_, value)| value.trim())
            .unwrap_or_default();
        return value.is_empty()
            || looks_like_url(value)
            || looks_like_local_path(value)
            || (!source_url.trim().is_empty() && value == source_url.trim());
    }
    looks_like_local_path(trimmed) && trimmed.split_whitespace().count() <= 2
}

fn looks_like_url(value: &str) -> bool {
    let lowered = value.to_ascii_lowercase();
    (lowered.starts_with("http://") || lowered.starts_with("https://"))
        && !value.chars().any(char::is_whitespace)
}

fn looks_like_local_path(value: &str) -> bool {
    if value.chars().any(char::is_whitespace) {
        return false;
    }
    value.starts_with('/')
        || value.starts_with("~/")
        || value.starts_with("./")
        || value.starts_with("../")
        || value.starts_with("\\\\")
        || value.chars().nth(1).is_some_and(|ch| {
            ch == ':'
                && value
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic())
        })
}
