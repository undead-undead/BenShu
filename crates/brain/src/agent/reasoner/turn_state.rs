use crate::agent::message::{Content, ContentPart, Message, Role};

pub(crate) fn latest_loop_guard_abort_for_tool(messages: &[Message], tool_name: &str) -> bool {
    messages.iter().rev().take(4).any(|message| {
        matches!(message.role, Role::Tool)
            && message
                .metadata
                .get("tool_name")
                .is_some_and(|name| name == tool_name)
            && message
                .metadata
                .get("tool_error")
                .is_some_and(|value| value == "true")
            && message.text().contains("Loop prevention triggered")
    })
}

pub(crate) fn latest_loop_guard_reuse_for_tool(messages: &[Message], tool_name: &str) -> bool {
    current_turn_messages(messages)
        .iter()
        .rev()
        .take(4)
        .any(|message| {
            matches!(message.role, Role::Tool)
                && message
                    .metadata
                    .get("tool_name")
                    .is_some_and(|name| name == tool_name)
                && message
                    .metadata
                    .get("loop_guard_reused_previous")
                    .is_some_and(|value| value == "true")
        })
}

pub(crate) fn latest_loop_guard_reuse_tool_name(messages: &[Message]) -> Option<String> {
    current_turn_messages(messages)
        .iter()
        .rev()
        .take(4)
        .find_map(|message| {
            if !matches!(message.role, Role::Tool) {
                return None;
            }
            if !message
                .metadata
                .get("loop_guard_reused_previous")
                .is_some_and(|value| value == "true")
            {
                return None;
            }
            message.metadata.get("tool_name").cloned()
        })
}

pub(crate) fn latest_runtime_tool_error_for_tool(
    messages: &[Message],
    tool_name: &str,
) -> Option<String> {
    current_turn_messages(messages)
        .iter()
        .rev()
        .find_map(|message| {
            if !matches!(message.role, Role::Tool) {
                return None;
            }
            if !message
                .metadata
                .get("tool_name")
                .is_some_and(|name| name == tool_name)
            {
                return None;
            }
            if !message
                .metadata
                .get("tool_error")
                .is_some_and(|value| value == "true")
            {
                return None;
            }
            Some(message.text())
        })
}

pub(crate) fn current_turn_messages(messages: &[Message]) -> &[Message] {
    let start = messages
        .iter()
        .rposition(|message| message.role == Role::User)
        .unwrap_or(0);
    &messages[start..]
}

pub(crate) fn latest_successful_tool_result_text(
    messages: &[Message],
    tool_name: &str,
) -> Option<String> {
    current_turn_messages(messages)
        .iter()
        .rev()
        .find_map(|message| {
            if !matches!(message.role, Role::Tool) {
                return None;
            }
            if !message
                .metadata
                .get("tool_name")
                .is_some_and(|name| name == tool_name)
            {
                return None;
            }
            if message
                .metadata
                .get("tool_error")
                .is_some_and(|value| value == "true")
            {
                return None;
            }

            let Content::Parts(parts) = &message.content else {
                return None;
            };

            parts.iter().find_map(|part| {
                let ContentPart::ToolResult { content, .. } = part else {
                    return None;
                };
                let trimmed = content.trim();
                if trimmed.is_empty()
                    || tool_result_content_is_runtime_error(trimmed)
                    || tool_result_is_blocked_or_failed(trimmed)
                {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            })
        })
}

pub(crate) fn latest_blocked_tool_result(messages: &[Message]) -> Option<(String, String)> {
    current_turn_messages(messages)
        .iter()
        .rev()
        .find_map(|message| {
            if !matches!(message.role, Role::Tool) {
                return None;
            }
            if message
                .metadata
                .get("tool_error")
                .is_some_and(|value| value == "true")
            {
                return None;
            }
            let metadata_tool_name = message.metadata.get("tool_name").cloned();

            let Content::Parts(parts) = &message.content else {
                return None;
            };

            parts.iter().find_map(|part| {
                let ContentPart::ToolResult { name, content, .. } = part else {
                    return None;
                };
                let trimmed = content.trim();
                if trimmed.is_empty() || !tool_result_is_blocked_or_failed(trimmed) {
                    return None;
                }
                let tool_name = metadata_tool_name
                    .clone()
                    .or_else(|| name.clone())
                    .unwrap_or_else(|| "tool".to_string());
                Some((tool_name, trimmed.to_string()))
            })
        })
}

pub(crate) fn tool_result_content_is_runtime_error(content: &str) -> bool {
    let lowered = content.to_ascii_lowercase();
    lowered.contains("error executing tool")
        || lowered.contains("runtime tool error")
        || lowered.contains("tool execution error")
        || lowered.contains("execution timed out before a usable result")
        || lowered.contains("provider error")
        || lowered.contains("http error")
        || structured_tool_contract_error(&lowered)
}

fn structured_tool_contract_error(lowered: &str) -> bool {
    if structured_tool_observation_not_found(lowered) {
        return false;
    }
    lowered.contains("\"success\":false")
        || lowered.contains("\"success\": false")
        || lowered.contains("missing_required")
        || lowered.contains("missing required")
        || lowered.contains(" is required")
        || lowered.contains(" required for ")
        || (lowered.contains("next_step_hint") && lowered.contains("example_shape"))
}

fn structured_tool_observation_not_found(lowered: &str) -> bool {
    (lowered.contains("\"error_kind\":\"not_found\"")
        || lowered.contains("\"error_kind\": \"not_found\"")
        || lowered.contains("\"error_kind\":\"chapter_not_found\"")
        || lowered.contains("\"error_kind\": \"chapter_not_found\""))
        && !lowered.contains("missing_required")
        && !lowered.contains("missing required")
        && !lowered.contains(" is required")
        && !lowered.contains(" required for ")
}

pub(crate) fn tool_result_is_blocked(result: &str) -> bool {
    let lowered = result.to_ascii_lowercase();
    lowered.contains("status: blocked") || lowered.contains("status: blocker")
}

fn tool_result_is_blocked_or_failed(result: &str) -> bool {
    let lowered = result.to_ascii_lowercase();
    let compact = lowered.replace([' ', '\n', '\r', '\t'], "");
    lowered.contains("status: blocked")
        || lowered.contains("status: blocker")
        || lowered.contains("status: failed")
        || lowered.contains("status: needs_confirmation")
        || compact.contains("\"status\":\"blocker\"")
        || lowered.contains("\"status\":\"blocked\"")
        || lowered.contains("\"status\": \"blocked\"")
        || lowered.contains("\"status\":\"failed\"")
        || lowered.contains("\"status\": \"failed\"")
        || lowered.contains("\"status\":\"needs_confirmation\"")
        || lowered.contains("\"status\": \"needs_confirmation\"")
}

pub(crate) fn latest_successful_tool_name(messages: &[Message]) -> Option<String> {
    current_turn_messages(messages)
        .iter()
        .rev()
        .find_map(|message| {
            if !matches!(message.role, Role::Tool) {
                return None;
            }
            if message
                .metadata
                .get("tool_error")
                .is_some_and(|value| value == "true")
            {
                return None;
            }
            message.metadata.get("tool_name").cloned()
        })
}

pub(crate) fn latest_successful_tool_result_for_names(
    messages: &[Message],
    tool_names: &[&str],
) -> Option<(String, String)> {
    current_turn_messages(messages)
        .iter()
        .rev()
        .find_map(|message| {
            if !matches!(message.role, Role::Tool) {
                return None;
            }
            if message
                .metadata
                .get("tool_error")
                .is_some_and(|value| value == "true")
            {
                return None;
            }

            let tool_name = message.metadata.get("tool_name")?;
            if !tool_names.iter().any(|candidate| tool_name == candidate) {
                return None;
            }

            let text = message.text();
            let trimmed = text.trim();
            if trimmed.is_empty() || tool_result_is_blocked_or_failed(trimmed) {
                None
            } else {
                Some((tool_name.clone(), trimmed.to_string()))
            }
        })
}

pub(crate) fn latest_successful_durable_effect_tool_result(
    messages: &[Message],
) -> Option<(String, String)> {
    current_turn_messages(messages)
        .iter()
        .rev()
        .find_map(|message| {
            if !matches!(message.role, Role::Tool) {
                return None;
            }
            if message
                .metadata
                .get("tool_error")
                .is_some_and(|value| value == "true")
            {
                return None;
            }

            let tool_name = message.metadata.get("tool_name")?.clone();
            let text = message.text();
            let trimmed = text.trim();
            if trimmed.is_empty()
                || tool_result_content_is_runtime_error(trimmed)
                || tool_result_is_blocked_or_failed(trimmed)
            {
                return None;
            }

            if tool_result_has_durable_effect(&tool_name, trimmed) {
                Some((tool_name, trimmed.to_string()))
            } else {
                None
            }
        })
}

fn tool_result_has_durable_effect(tool_name: &str, result: &str) -> bool {
    let lowered = result.to_ascii_lowercase();
    if tool_result_is_process_artifact_only(&lowered) {
        return false;
    }

    lowered.contains("runtime_effect: artifact.")
        || lowered.contains("runtime_effects: artifact.")
        || lowered.contains("\"runtime_effect\":\"artifact.")
        || lowered.contains("\"runtime_effect\": \"artifact.")
        || (lowered.contains("\"runtime_effects\"") && lowered.contains("artifact."))
        || lowered.contains("runtime_effect: knowledge.")
        || lowered.contains("runtime_effects: knowledge.")
        || lowered.contains("\"runtime_effect\":\"knowledge.")
        || lowered.contains("\"runtime_effect\": \"knowledge.")
        || (lowered.contains("\"runtime_effects\"") && lowered.contains("knowledge."))
        || (tool_name == "write_file" && lowered.contains("successfully wrote"))
        || (tool_name == "knowledge_import_url"
            && lowered.contains("imported web knowledge into collection"))
        || (tool_name == "knowledge_manage_document"
            && (lowered.contains("knowledge document created")
                || lowered.contains("knowledge document updated")
                || lowered.contains("knowledge document physically deleted")))
}

fn tool_result_is_process_artifact_only(lowered: &str) -> bool {
    let compact = lowered.replace([' ', '\n', '\r', '\t'], "");
    let process_path_markers = [
        "/status_report.",
        "\\status_report.",
        "/progress_report.",
        "\\progress_report.",
        "/task_status.",
        "\\task_status.",
        "/heartbeat.",
        "\\heartbeat.",
        "/checkpoint_report.",
        "\\checkpoint_report.",
        "/execution_log.",
        "\\execution_log.",
        "/recovery_notes.",
        "\\recovery_notes.",
        "/error",
        "\\error",
        "/err",
        "\\err",
        "/blocker",
        "\\blocker",
        "/blocked",
        "\\blocked",
    ];
    if process_path_markers
        .iter()
        .any(|marker| lowered.contains(marker))
    {
        return true;
    }

    let status_or_progress_note = compact.contains("statusreport")
        || compact.contains("status_report")
        || compact.contains("progressreport")
        || compact.contains("progress_report")
        || compact.contains("taskstatus")
        || compact.contains("task_status")
        || compact.contains("executionlog")
        || compact.contains("execution_log")
        || compact.contains("checkpoint_report")
        || compact.contains("checkpointreport")
        || compact.contains("recoverynotes")
        || compact.contains("recovery_notes")
        || lowered.contains("状态报告")
        || lowered.contains("进展报告")
        || lowered.contains("执行日志")
        || lowered.contains("恢复记录")
        || lowered.contains("progress report")
        || lowered.contains("task status")
        || lowered.contains("execution log");
    let reports_internal_progress = lowered.contains("completion_scope")
        || lowered.contains("initial stage")
        || lowered.contains("file discovery")
        || lowered.contains("路径验证")
        || lowered.contains("blockers:")
        || lowered.contains("blocked:")
        || lowered.contains("status: processing")
        || lowered.contains("need to fetch")
        || lowered.contains("needs retrieval")
        || lowered.contains("需要先获取")
        || lowered.contains("需要从知识库")
        || lowered.contains("下一步");
    status_or_progress_note && reports_internal_progress
}

pub(crate) fn latest_tool_error_result(messages: &[Message]) -> Option<(String, String)> {
    current_turn_messages(messages)
        .iter()
        .rev()
        .find_map(|message| {
            if !matches!(message.role, Role::Tool) {
                return None;
            }

            let tool_name = message
                .metadata
                .get("tool_name")
                .cloned()
                .unwrap_or_else(|| "unknown_tool".to_string());
            let text = message.text();
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return None;
            }

            let metadata_error = message
                .metadata
                .get("tool_error")
                .is_some_and(|value| value == "true");
            if metadata_error || tool_result_content_is_runtime_error(trimmed) {
                Some((tool_name, trimmed.to_string()))
            } else {
                // A later successful tool result supersedes older runtime errors in the
                // same turn. Recovery logic should react to the current tool boundary,
                // not keep re-finalizing from a stale failed call after progress resumed.
                Some((String::new(), String::new()))
            }
        })
        .and_then(|(tool_name, error)| {
            if tool_name.is_empty() && error.is_empty() {
                None
            } else {
                Some((tool_name, error))
            }
        })
}

pub(crate) fn latest_delegate_role(messages: &[Message]) -> Option<String> {
    messages.iter().rev().find_map(|message| {
        if !matches!(message.role, Role::Assistant) {
            return None;
        }
        let Content::Parts(parts) = &message.content else {
            return None;
        };
        parts.iter().rev().find_map(|part| {
            let ContentPart::ToolCall {
                name, arguments, ..
            } = part
            else {
                return None;
            };
            if name != "delegate" {
                return None;
            }
            arguments
                .get("role")
                .and_then(|value| value.as_str())
                .map(|role| role.trim().to_lowercase())
        })
    })
}

pub(crate) fn has_system_marker(messages: &[Message], marker: &str) -> bool {
    messages
        .iter()
        .any(|message| matches!(message.role, Role::System) && message.text().contains(marker))
}

pub(crate) fn has_system_marker_after_latest_user(messages: &[Message], marker: &str) -> bool {
    for message in messages.iter().rev() {
        if matches!(message.role, Role::User) {
            return false;
        }
        if matches!(message.role, Role::System) && message.text().contains(marker) {
            return true;
        }
    }
    false
}
