//! Generic chat/task DTO sanitization for the panel boundary.
//!
//! This module knows nothing about writing tools. It only prevents local
//! filesystem locations from leaking through user-visible task surfaces.

use super::{task_artifact_workspace_paths_from_text, ChatArtifactRef};
use benshu_state::{TaskArtifactRef, TaskCheckpoint, TaskContract};
use serde_json::Value;
use std::collections::HashMap;

pub(super) fn redact_internal_paths_for_chat(text: &str) -> String {
    let mut redacted = text.to_string();
    let mut paths = task_artifact_workspace_paths_from_text(text);
    paths.sort_by_key(|path| std::cmp::Reverse(path.len()));
    for path in paths {
        redacted = redacted.replace(&path, "[文件]");
    }
    redacted
}

pub(super) fn hide_local_artifact_paths_for_chat(artifacts: &mut [ChatArtifactRef]) {
    for artifact in artifacts {
        if artifact_uri_looks_local(&artifact.uri) {
            artifact.uri = format!("artifact:{}", artifact.artifact_id);
        }
    }
}

pub(super) fn artifact_uri_looks_local(uri: &str) -> bool {
    uri.starts_with('/')
        || uri.starts_with("data/generated/")
        || uri.contains(":\\")
        || uri.starts_with("~/")
}

pub(super) fn sanitize_task_result_for_panel(result: Option<Value>) -> Option<Value> {
    result.map(sanitize_panel_task_value)
}

pub(super) fn hide_local_task_artifact_paths_for_panel(artifacts: &mut [TaskArtifactRef]) {
    for artifact in artifacts {
        artifact.kind = sanitize_panel_task_text(&artifact.kind);
        artifact.media_type = artifact.media_type.as_deref().map(sanitize_panel_task_text);
        if artifact_uri_looks_local(&artifact.uri) {
            artifact.uri = format!("artifact:{}", artifact.artifact_id);
        } else {
            artifact.uri = sanitize_panel_task_text(&artifact.uri);
        }
    }
}

pub(super) fn sanitize_task_checkpoints_for_panel(checkpoints: &mut [TaskCheckpoint]) {
    for checkpoint in checkpoints {
        checkpoint.label = sanitize_panel_task_text(&checkpoint.label);
        checkpoint.summary = checkpoint.summary.as_deref().map(sanitize_panel_task_text);
    }
}

pub(super) fn sanitize_task_contract_for_panel(mut contract: TaskContract) -> TaskContract {
    contract.intent = contract.intent.as_deref().map(sanitize_panel_task_text);
    contract.response_language = contract
        .response_language
        .as_deref()
        .map(sanitize_panel_task_text);
    contract.artifact_language = contract
        .artifact_language
        .as_deref()
        .map(sanitize_panel_task_text);
    contract.decisions = contract
        .decisions
        .into_iter()
        .map(|value| sanitize_panel_task_text(&value))
        .collect();
    for boundary in &mut contract.boundaries {
        boundary.scope = sanitize_panel_task_text(&boundary.scope);
        boundary.rule = sanitize_panel_task_text(&boundary.rule);
        boundary.reason = boundary.reason.as_deref().map(sanitize_panel_task_text);
    }
    contract.completion_criteria = contract
        .completion_criteria
        .into_iter()
        .map(|value| sanitize_panel_task_text(&value))
        .collect();
    contract.required_events = contract
        .required_events
        .into_iter()
        .map(|value| sanitize_panel_task_text(&value))
        .collect();
    for requirement in &mut contract.evidence_requirements {
        requirement.topic = sanitize_panel_task_text(&requirement.topic);
        requirement.description = requirement
            .description
            .as_deref()
            .map(sanitize_panel_task_text);
    }
    contract.lint_warnings = contract
        .lint_warnings
        .into_iter()
        .map(|value| sanitize_panel_task_text(&value))
        .collect();
    contract
}

pub(super) fn sanitize_task_evidence_for_panel(
    evidence: HashMap<String, Value>,
) -> HashMap<String, Value> {
    evidence
        .into_iter()
        .map(|(key, value)| {
            (
                sanitize_panel_task_text(&key),
                sanitize_panel_task_value(value),
            )
        })
        .collect()
}

pub(super) fn sanitize_panel_task_value(value: Value) -> Value {
    match value {
        Value::String(text) => Value::String(sanitize_panel_task_text(&text)),
        Value::Array(items) => {
            Value::Array(items.into_iter().map(sanitize_panel_task_value).collect())
        }
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| {
                    (
                        sanitize_panel_task_text(&key),
                        sanitize_panel_task_value(value),
                    )
                })
                .collect(),
        ),
        other => other,
    }
}

pub(super) fn sanitize_panel_task_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while !rest.is_empty() {
        let Some((index, marker)) = next_local_path_marker(rest) else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..index]);
        out.push_str(if marker == "data/generated/" {
            "[artifact path hidden]"
        } else {
            "[internal path hidden]"
        });
        let tail = &rest[index + marker.len()..];
        let consumed = tail
            .char_indices()
            .find(|(_, ch)| local_path_delimiter(*ch))
            .map(|(offset, _)| offset)
            .unwrap_or(tail.len());
        rest = &tail[consumed..];
    }
    out
}

fn next_local_path_marker(text: &str) -> Option<(usize, &'static str)> {
    [
        "/home/",
        "/mnt/",
        "/tmp/",
        "/var/",
        "/Users/",
        "data/generated/",
    ]
    .into_iter()
    .filter_map(|marker| text.find(marker).map(|index| (index, marker)))
    .min_by_key(|(index, _)| *index)
    .or_else(|| windows_drive_path_marker(text))
}

fn windows_drive_path_marker(text: &str) -> Option<(usize, &'static str)> {
    text.char_indices()
        .find(|(index, ch)| {
            ch.is_ascii_alphabetic()
                && text
                    .get(index + ch.len_utf8()..)
                    .is_some_and(|tail| tail.starts_with(":\\") || tail.starts_with(":/"))
        })
        .map(|(index, _)| (index, "C:\\"))
}

fn local_path_delimiter(ch: char) -> bool {
    ch.is_whitespace()
        || matches!(
            ch,
            '"' | '\'' | '`' | '<' | '>' | ')' | ']' | '}' | ',' | '，' | ';' | '；' | '\n' | '\r'
        )
}
