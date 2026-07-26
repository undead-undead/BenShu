use crate::agent::truth_verification_policy::TruthVerificationPolicyEngine;
use crate::hooks::{FnHook, HookEngine, HookResult, HookTiming, RuntimeHookCapture};
use crate::skills::tool::{
    classify_query_verification_plan, query_requests_followup_execution_after_lookup,
    VerificationRequirement, VerificationSource,
};
use parking_lot::RwLock;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
struct VerificationPreviewObservation {
    tool_name: String,
    domain: String,
    requirement: String,
    mode: String,
    outcome: String,
    truth_status: String,
    source_posture: String,
    source_count: usize,
    execution_evidence_count: usize,
    state_evidence_count: usize,
    note_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct VerificationFollowupObservation {
    answer_readiness: String,
    next_tools: String,
    cite_required: bool,
    note: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct VerificationOrchestrationObservation {
    route_reason: String,
    continuation: String,
    termination: String,
    requires_followup: bool,
    can_finalize_answer: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ToolErrorObservation {
    tool_name: String,
    error: String,
}

fn extract_json_string_field(payload: Option<&String>, field: &str) -> Option<String> {
    payload
        .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())
        .and_then(|json| {
            json.get(field)
                .and_then(|value| value.as_str())
                .map(str::to_string)
        })
}

fn extract_json_bool_field(payload: Option<&String>, field: &str) -> Option<bool> {
    payload
        .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())
        .and_then(|json| json.get(field).and_then(|value| value.as_bool()))
}

fn extract_verification_preview_observation(
    tool_name: &str,
    payload: Option<&String>,
) -> Option<VerificationPreviewObservation> {
    let json = payload.and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())?;
    let preview = json.get("verification_preview")?;

    let extract = |field: &str| {
        preview
            .get(field)
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };

    let domain = extract("domain")?;
    let requirement = extract("requirement")?;
    let mode = extract("mode")?;
    let outcome = extract("outcome")?;
    let truth_status = extract("truth_status")?;
    let source_posture = extract("source_posture")?;

    let count_array = |field: &str| {
        preview
            .get(field)
            .and_then(|value| value.as_array())
            .map(|items| items.len())
            .unwrap_or(0)
    };

    Some(VerificationPreviewObservation {
        tool_name: tool_name.to_string(),
        domain,
        requirement,
        mode,
        outcome,
        truth_status,
        source_posture,
        source_count: count_array("sources"),
        execution_evidence_count: count_array("execution_evidence"),
        state_evidence_count: count_array("state_evidence"),
        note_count: count_array("notes"),
    })
}

fn extract_verification_followup_observation(
    payload: Option<&String>,
) -> Option<VerificationFollowupObservation> {
    let json = payload.and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())?;
    let followup = json.get("verification_followup")?;
    let answer_readiness = followup
        .get("answer_readiness")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)?;
    let next_tools = followup
        .get("next_tools")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default();
    let cite_required = followup
        .get("cite_required")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let note = followup
        .get("note")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    Some(VerificationFollowupObservation {
        answer_readiness,
        next_tools,
        cite_required,
        note,
    })
}

fn extract_verification_orchestration_observation(
    payload: Option<&String>,
) -> Option<VerificationOrchestrationObservation> {
    let json = payload.and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())?;
    let route_reason = json
        .get("route_reason")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)?;
    let decision = json.get("orchestration_decision")?;
    let continuation = decision
        .get("continuation")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)?;
    let termination = decision
        .get("termination")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)?;
    let requires_followup = decision
        .get("requires_followup")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let can_finalize_answer = decision
        .get("can_finalize_answer")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    Some(VerificationOrchestrationObservation {
        route_reason,
        continuation,
        termination,
        requires_followup,
        can_finalize_answer,
    })
}

fn extract_verification_sources(payload: Option<&String>) -> Vec<VerificationSource> {
    let Some(json) =
        payload.and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())
    else {
        return Vec::new();
    };
    let Some(preview) = json.get("verification_preview") else {
        return Vec::new();
    };
    let Some(sources) = preview.get("sources") else {
        return Vec::new();
    };
    serde_json::from_value::<Vec<VerificationSource>>(sources.clone()).unwrap_or_default()
}

fn extract_verification_string_evidence(payload: Option<&String>, field: &str) -> Vec<String> {
    let Some(json) =
        payload.and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())
    else {
        return Vec::new();
    };
    let Some(preview) = json.get("verification_preview") else {
        return Vec::new();
    };
    let Some(evidence) = preview.get(field) else {
        return Vec::new();
    };
    serde_json::from_value::<Vec<String>>(evidence.clone()).unwrap_or_default()
}

fn render_verification_preview_summary(observation: &VerificationPreviewObservation) -> String {
    format!(
        "after_tool:verification_preview:tool={}:domain={}:requirement={}:mode={}:outcome={}:truth_status={}:source_posture={}:source_count={}:execution_evidence_count={}:state_evidence_count={}:note_count={}",
        observation.tool_name,
        observation.domain,
        observation.requirement,
        observation.mode,
        observation.outcome,
        observation.truth_status,
        observation.source_posture,
        observation.source_count,
        observation.execution_evidence_count,
        observation.state_evidence_count,
        observation.note_count
    )
}

fn render_verification_followup_summary(observation: &VerificationFollowupObservation) -> String {
    format!(
        "after_tool:verification_followup:answer_readiness={}:next_tools={}:cite_required={}:note={}",
        observation.answer_readiness,
        observation.next_tools,
        observation.cite_required,
        observation.note.replace(':', ";")
    )
}

fn render_verification_orchestration_summary(
    observation: &VerificationOrchestrationObservation,
) -> String {
    format!(
        "after_tool:verification_orchestration:route_reason={}:continuation={}:termination={}:requires_followup={}:can_finalize_answer={}",
        observation.route_reason,
        observation.continuation,
        observation.termination,
        observation.requires_followup,
        observation.can_finalize_answer
    )
}

fn render_verification_sources_json_note(sources: &[VerificationSource]) -> Option<String> {
    if sources.is_empty() {
        return None;
    }
    serde_json::to_string(sources)
        .ok()
        .map(|value| format!("after_tool:verification_sources_json:{value}"))
}

fn render_verification_string_array_note(prefix: &str, values: &[String]) -> Option<String> {
    if values.is_empty() {
        return None;
    }
    serde_json::to_string(values)
        .ok()
        .map(|value| format!("{prefix}{value}"))
}

fn parse_key_value_runtime_note_fields(value: &str) -> std::collections::BTreeMap<String, String> {
    let mut fields = std::collections::BTreeMap::new();
    for part in value.split(':') {
        if let Some((key, field_value)) = part.split_once('=') {
            if !key.trim().is_empty() && !field_value.trim().is_empty() {
                fields.insert(key.trim().to_string(), field_value.trim().to_string());
            }
        }
    }
    fields
}

fn latest_verification_preview_fields(
    capture: &RuntimeHookCapture,
) -> Option<std::collections::BTreeMap<String, String>> {
    capture
        .notes
        .iter()
        .filter_map(|note| note.strip_prefix("after_tool:verification_preview:"))
        .map(parse_key_value_runtime_note_fields)
        .last()
}

fn latest_verification_followup_fields(
    capture: &RuntimeHookCapture,
) -> Option<std::collections::BTreeMap<String, String>> {
    capture
        .notes
        .iter()
        .filter_map(|note| note.strip_prefix("after_tool:verification_followup:"))
        .map(parse_key_value_runtime_note_fields)
        .last()
}

fn latest_verification_orchestration_fields(
    capture: &RuntimeHookCapture,
) -> Option<std::collections::BTreeMap<String, String>> {
    capture
        .notes
        .iter()
        .filter_map(|note| note.strip_prefix("after_tool:verification_orchestration:"))
        .map(parse_key_value_runtime_note_fields)
        .last()
}

fn latest_verification_sources_json(capture: &RuntimeHookCapture) -> Option<String> {
    latest_verification_json_payload(capture, "after_tool:verification_sources_json:")
}

fn latest_verification_sources(capture: &RuntimeHookCapture) -> Vec<VerificationSource> {
    latest_verification_sources_json(capture)
        .and_then(|value| serde_json::from_str::<Vec<VerificationSource>>(&value).ok())
        .unwrap_or_default()
}

fn latest_verification_json_payload(capture: &RuntimeHookCapture, prefix: &str) -> Option<String> {
    capture
        .notes
        .iter()
        .filter_map(|note| note.strip_prefix(prefix))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .last()
}

fn latest_verification_string_array(capture: &RuntimeHookCapture, prefix: &str) -> Vec<String> {
    latest_verification_json_payload(capture, prefix)
        .and_then(|value| serde_json::from_str::<Vec<String>>(&value).ok())
        .unwrap_or_default()
}

fn latest_tool_error_observation(capture: &RuntimeHookCapture) -> Option<ToolErrorObservation> {
    capture
        .notes
        .iter()
        .filter_map(|note| note.strip_prefix("tool_error:"))
        .filter_map(|value| {
            let (tool_name, error) = value.split_once(':')?;
            let tool_name = tool_name.trim();
            let error = error.trim();
            if tool_name.is_empty() || error.is_empty() {
                return None;
            }
            Some(ToolErrorObservation {
                tool_name: tool_name.to_string(),
                error: error.to_string(),
            })
        })
        .last()
}

fn text_prefers_chinese(text: Option<&str>) -> bool {
    text.unwrap_or_default()
        .chars()
        .any(|ch| ('\u{4E00}'..='\u{9FFF}').contains(&ch))
}

fn build_dangling_tool_call_repair_notice(
    dangling_count: usize,
    dangling_ids: &str,
) -> Option<String> {
    if dangling_count == 0 {
        return None;
    }
    Some(format!(
        "Runtime notice: {dangling_count} planned tool call(s) did not produce a matching tool result in this turn ({dangling_ids}). Treat any tool-based conclusion as incomplete and retry or clarify before relying on it."
    ))
}

fn build_tool_error_downgrade_notice(observation: &ToolErrorObservation) -> String {
    format!(
        "Verification notice: the required tool `{}` failed before a usable result was observed ({}). Treat the answer below as tentative until verification is completed.",
        observation.tool_name,
        observation.error
    )
}

fn build_tool_error_downgrade_notice_for_language(
    observation: &ToolErrorObservation,
    prefers_chinese: bool,
) -> String {
    if prefers_chinese {
        format!(
            "验证提示：所需工具 `{}` 在拿到可用结果前失败（{}）。在完成验证前，请把下面的回答视为暂定结果。",
            observation.tool_name, observation.error
        )
    } else {
        build_tool_error_downgrade_notice(observation)
    }
}

fn combine_downgrade_notices(primary: Option<String>, secondary: Option<String>) -> Option<String> {
    match (primary, secondary) {
        (Some(left), Some(right)) => Some(format!("{left}\n\n{right}")),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

fn apply_response_downgrade_notices(
    response: Option<&String>,
    downgrade_notice: Option<String>,
    dangling_notice: Option<String>,
) -> Option<String> {
    let mut notices = Vec::new();
    if let Some(value) = downgrade_notice {
        notices.push(value);
    }
    if let Some(value) = dangling_notice {
        notices.push(value);
    }
    if notices.is_empty() {
        return None;
    }

    let mut sections = Vec::new();
    if let Some(response) = response
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        sections.push(response.to_string());
    }
    sections.extend(notices);
    Some(sections.join("\n\n"))
}

fn apply_response_source_appendix(
    response: Option<&String>,
    source_appendix: Option<String>,
) -> Option<String> {
    let appendix = source_appendix?;
    let response = response
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())?;
    Some(format!("{response}\n\n{appendix}"))
}

fn media_tool_kind(tool_name: &str) -> Option<&'static str> {
    match tool_name {
        "probe_media" => Some("media"),
        "extract_video_frames" | "render_video_thumbnail" => Some("video"),
        "extract_audio_track" | "normalize_audio" => Some("audio"),
        _ => None,
    }
}

fn media_tool_engine(tool_name: &str) -> Option<&'static str> {
    match tool_name {
        "probe_media" => Some("ffprobe"),
        "extract_video_frames"
        | "extract_audio_track"
        | "normalize_audio"
        | "render_video_thumbnail" => Some("ffmpeg"),
        _ => None,
    }
}

fn extract_skill_surface_field(content: &str, field: &str) -> Option<String> {
    let prefix = format!("- {field}:");
    content.lines().find_map(|line| {
        let trimmed = line.trim();
        trimmed
            .strip_prefix(&prefix)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeMiddlewareSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub timings: &'static [HookTiming],
    pub priority: u32,
}

const BEFORE_LLM_TIMINGS: &[HookTiming] = &[HookTiming::BeforeLlm];
const AFTER_LLM_TIMINGS: &[HookTiming] = &[HookTiming::AfterLlm];
const BEFORE_TOOL_TIMINGS: &[HookTiming] = &[HookTiming::BeforeToolCall];
const AFTER_TOOL_TIMINGS: &[HookTiming] = &[HookTiming::AfterToolCall];
const ERROR_TIMINGS: &[HookTiming] = &[HookTiming::OnError];
const AFTER_TOOL_AND_ERROR_TIMINGS: &[HookTiming] =
    &[HookTiming::AfterToolCall, HookTiming::OnError];
const BEFORE_RESPONSE_TIMINGS: &[HookTiming] = &[HookTiming::BeforeResponse];

fn normalize_runtime_tool_error_message(tool: &str, error: &str) -> String {
    let lowered = error.to_ascii_lowercase();
    let carries_detailed_worker_failure = lowered.contains("returned a runtime failure")
        || lowered.contains("status: blocked")
        || lowered.contains("status: failed")
        || lowered.contains("runtime failure instead of completed delegated output");
    if lowered.contains("timed out") && !carries_detailed_worker_failure {
        format!("Runtime tool error in `{tool}`: execution timed out before a usable result was returned.")
    } else if lowered.contains("security violation") {
        format!("Runtime tool error in `{tool}`: blocked by security policy before execution could complete.")
    } else {
        format!("Runtime tool error in `{tool}`: {error}")
    }
}

fn classify_skill_asset_execution_surface(tool_name: &str) -> &'static str {
    match tool_name {
        "runtime_surface" | "command_exec" | "shell" | "bash" | "powershell" | "cmd" | "uv"
        | "pixi" | "bun" | "gcc" | "quickjs" => "runtime",
        "delegate" | "handover" | "swarm" | "swarm_broadcast" | "decomposition"
        | "multi_agent_audit" => "worker",
        _ => "tool",
    }
}

fn repair_delegate_args_with_full_followup_request(
    user_input: &str,
    tool_args: &str,
) -> Option<String> {
    if !query_requests_followup_execution_after_lookup(user_input) {
        return None;
    }

    let mut payload = serde_json::from_str::<serde_json::Value>(tool_args).ok()?;
    let task = payload
        .get("task")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if task.is_empty() || task.contains(user_input) {
        return None;
    }

    let Some(object) = payload.as_object_mut() else {
        return None;
    };
    object.insert(
        "task".to_string(),
        serde_json::Value::String(format!(
            "{task}\n\n完整用户请求（必须保留查找之后的后续阶段，不能只完成查找片段）：{user_input}"
        )),
    );
    object.insert(
        "full_user_request".to_string(),
        serde_json::Value::String(user_input.to_string()),
    );
    serde_json::to_string(&payload).ok()
}

pub fn default_runtime_middleware_specs() -> Vec<RuntimeMiddlewareSpec> {
    vec![
        RuntimeMiddlewareSpec {
            name: "runtime_pre_llm_surface",
            description: "Captures route, skill-manual, and provider context before the LLM call.",
            timings: BEFORE_LLM_TIMINGS,
            priority: 5,
        },
        RuntimeMiddlewareSpec {
            name: "runtime_memory_owner_surface",
            description: "Captures visible/memory/approval ownership metadata before the LLM call.",
            timings: BEFORE_LLM_TIMINGS,
            priority: 7,
        },
        RuntimeMiddlewareSpec {
            name: "runtime_loop_guard",
            description: "Aborts repeated or looping tool calls before execution.",
            timings: BEFORE_TOOL_TIMINGS,
            priority: 10,
        },
        RuntimeMiddlewareSpec {
            name: "runtime_delegate_followup_preserver",
            description: "Preserves the full user request when a delegated lookup task also requires downstream execution such as knowledge import or synthesis.",
            timings: BEFORE_TOOL_TIMINGS,
            priority: 11,
        },
        RuntimeMiddlewareSpec {
            name: "runtime_deferred_tool_filter_surface",
            description: "Notes when long-tail tools are deferred from the active tool contract and nudges tool_search-first behavior.",
            timings: BEFORE_LLM_TIMINGS,
            priority: 12,
        },
        RuntimeMiddlewareSpec {
            name: "runtime_post_llm_surface",
            description: "Captures finish reason and provider telemetry after LLM output is assembled.",
            timings: AFTER_LLM_TIMINGS,
            priority: 15,
        },
        RuntimeMiddlewareSpec {
            name: "runtime_skill_manual_surface",
            description: "Records when a skill manual was actually read through the runtime tool path.",
            timings: AFTER_TOOL_TIMINGS,
            priority: 18,
        },
        RuntimeMiddlewareSpec {
            name: "runtime_media_preprocess_surface",
            description: "Captures structured media preprocessing outputs so local media runtime tools enter trace and witness instead of staying ad hoc.",
            timings: AFTER_TOOL_TIMINGS,
            priority: 18,
        },
        RuntimeMiddlewareSpec {
            name: "runtime_forge_surface",
            description: "Captures structured forge registration, smoke-test, and session-scope results after forge_skill succeeds.",
            timings: AFTER_TOOL_TIMINGS,
            priority: 19,
        },
        RuntimeMiddlewareSpec {
            name: "runtime_tool_verification_capture",
            description: "Captures structured verification previews from tool outputs so the runtime can distinguish verified facts from guesses.",
            timings: AFTER_TOOL_TIMINGS,
            priority: 19,
        },
        RuntimeMiddlewareSpec {
            name: "runtime_tool_degradation_surface",
            description: "Surfaces degraded tool execution such as truncation or fallback behavior.",
            timings: AFTER_TOOL_AND_ERROR_TIMINGS,
            priority: 20,
        },
        RuntimeMiddlewareSpec {
            name: "runtime_tool_error_surface",
            description: "Captures tool execution errors as structured runtime notes for trace and witness.",
            timings: ERROR_TIMINGS,
            priority: 25,
        },
        RuntimeMiddlewareSpec {
            name: "runtime_clarification_surface",
            description: "Captures clarification session-contract state before the response is finalized.",
            timings: BEFORE_RESPONSE_TIMINGS,
            priority: 26,
        },
        RuntimeMiddlewareSpec {
            name: "runtime_subagent_budget_surface",
            description: "Captures delegation/handover state together with configured subagent and execution budgets before the response is finalized.",
            timings: BEFORE_RESPONSE_TIMINGS,
            priority: 27,
        },
        RuntimeMiddlewareSpec {
            name: "runtime_title_surface",
            description: "Captures the active session title signal before the response is finalized.",
            timings: BEFORE_RESPONSE_TIMINGS,
            priority: 29,
        },
        RuntimeMiddlewareSpec {
            name: "runtime_engram_windows_native_surface",
            description: "Captures Engram embedding and rerank Windows-native runtime results before the response is finalized.",
            timings: BEFORE_RESPONSE_TIMINGS,
            priority: 30,
        },
        RuntimeMiddlewareSpec {
            name: "runtime_truth_verification_surface",
            description: "Projects the latest verification state into before-response runtime notes for final response and trace contracts.",
            timings: BEFORE_RESPONSE_TIMINGS,
            priority: 30,
        },
        RuntimeMiddlewareSpec {
            name: "runtime_dangling_tool_call_repair",
            description: "Surfaces assistant tool calls that never produced a matching tool result before the response is finalized.",
            timings: BEFORE_RESPONSE_TIMINGS,
            priority: 28,
        },
        RuntimeMiddlewareSpec {
            name: "runtime_post_run_evaluation_tap",
            description: "Records post-run evaluation and response summary metadata.",
            timings: BEFORE_RESPONSE_TIMINGS,
            priority: 30,
        },
    ]
}

pub fn install_default_runtime_middlewares(
    engine: &mut HookEngine,
    runtime_hook_capture: Arc<RwLock<RuntimeHookCapture>>,
) -> Vec<RuntimeMiddlewareSpec> {
    let specs = default_runtime_middleware_specs();

    let pre_llm_capture = runtime_hook_capture.clone();
    engine.register(Arc::new(
        FnHook::new(
            "runtime_pre_llm_surface",
            vec![HookTiming::BeforeLlm],
            move |event| {
                let mut capture = pre_llm_capture.write();
                capture.pre_llm_tap_count = capture.pre_llm_tap_count.saturating_add(1);
                if let Some(route) = event.metadata.get("capability_route") {
                    capture.notes.push(format!("before_llm:route:{route}"));
                }
                if let Some(chat_route) = event.metadata.get("chat_route") {
                    capture
                        .notes
                        .push(format!("before_llm:chat_route:{chat_route}"));
                }
                if let Some(tool_surface_mode) = event.metadata.get("tool_surface_mode") {
                    capture
                        .notes
                        .push(format!("before_llm:tool_surface_mode:{tool_surface_mode}"));
                }
                if let Some(skill_name) = event.metadata.get("matched_skill_manual") {
                    capture
                        .notes
                        .push(format!("before_llm:skill_manual:{skill_name}"));
                }
                if let Some(asset_path) = event.metadata.get("matched_skill_asset_path") {
                    capture
                        .notes
                        .push(format!("before_llm:skill_asset:{asset_path}"));
                }
                if let Some(tool_names) = event.metadata.get("forge_followup_tool_names") {
                    capture
                        .notes
                        .push(format!("before_llm:forge_followup_tools:{tool_names}"));
                }
                if let Some(strategies) = event.metadata.get("media_followup_strategies") {
                    capture
                        .notes
                        .push(format!("before_llm:media_followup_strategies:{strategies}"));
                }
                if let Some(route) = event.metadata.get("media_followup_capability_route") {
                    capture.notes.push(format!(
                        "before_llm:media_followup_capability_route:{route}"
                    ));
                }
                if let Some(surface) = event.metadata.get("media_followup_execution_surface") {
                    capture.notes.push(format!(
                        "before_llm:media_followup_execution_surface:{surface}"
                    ));
                }
                if event
                    .metadata
                    .get("skill_manual_gate_active")
                    .is_some_and(|value| value == "true")
                {
                    capture
                        .notes
                        .push("before_llm:skill_manual_gate_active".to_string());
                }
                if event
                    .metadata
                    .get("skill_asset_gate_active")
                    .is_some_and(|value| value == "true")
                {
                    capture
                        .notes
                        .push("before_llm:skill_asset_gate_active".to_string());
                }
                if event
                    .metadata
                    .get("forge_followup_gate_active")
                    .is_some_and(|value| value == "true")
                {
                    capture
                        .notes
                        .push("before_llm:forge_followup_gate_active".to_string());
                }
                if event
                    .metadata
                    .get("media_followup_guidance_active")
                    .is_some_and(|value| value == "true")
                {
                    capture
                        .notes
                        .push("before_llm:media_followup_guidance_active".to_string());
                }
                if event
                    .metadata
                    .get("truth_verification_guidance_active")
                    .is_some_and(|value| value == "true")
                {
                    capture
                        .notes
                        .push("before_llm:truth_verification_guidance_active".to_string());
                }
                if let Some(provider_name) = event.metadata.get("provider_name") {
                    capture
                        .notes
                        .push(format!("before_llm:provider:{provider_name}"));
                }
                for (metadata_key, note_prefix) in [
                    (
                        "runtime_continuation_user_session_id",
                        "before_llm:runtime_continuation_user_session_id:",
                    ),
                    (
                        "runtime_continuation_turn_id",
                        "before_llm:runtime_continuation_turn_id:",
                    ),
                    (
                        "runtime_continuation_worker_run_id",
                        "before_llm:runtime_continuation_worker_run_id:",
                    ),
                    (
                        "runtime_continuation_frontier_id",
                        "before_llm:runtime_continuation_frontier_id:",
                    ),
                    (
                        "runtime_continuation_visible_prompt_fingerprint",
                        "before_llm:runtime_continuation_visible_prompt_fingerprint:",
                    ),
                ] {
                    if let Some(value) = event.metadata.get(metadata_key) {
                        capture.notes.push(format!("{note_prefix}{value}"));
                    }
                }
                HookResult::Continue
            },
        )
        .with_priority(5),
    ));

    let memory_capture = runtime_hook_capture.clone();
    engine.register(Arc::new(
        FnHook::new(
            "runtime_memory_owner_surface",
            vec![HookTiming::BeforeLlm],
            move |event| {
                let visible_owner = event
                    .metadata
                    .get("visible_owner")
                    .cloned()
                    .unwrap_or_else(|| "unknown".to_string());
                let memory_owner = event
                    .metadata
                    .get("memory_owner")
                    .cloned()
                    .unwrap_or_else(|| "unknown".to_string());
                let approval_owner = event
                    .metadata
                    .get("approval_owner")
                    .cloned()
                    .unwrap_or_else(|| "unknown".to_string());

                let mut capture = memory_capture.write();
                capture.memory_surface_count = capture.memory_surface_count.saturating_add(1);
                capture.notes.push(format!(
                    "before_llm:ownership:visible={visible_owner}:memory={memory_owner}:approval={approval_owner}"
                ));
                HookResult::Continue
            },
        )
        .with_priority(7),
    ));

    let loop_capture = runtime_hook_capture.clone();
    engine.register(Arc::new(
        FnHook::new(
            "runtime_loop_guard",
            vec![HookTiming::BeforeToolCall],
            move |event| {
                if let Some(warning) = event.metadata.get("loop_warning") {
                    let action = event
                        .metadata
                        .get("loop_guard_action")
                        .map(String::as_str)
                        .unwrap_or("block");
                    let tool = event
                        .tool_name
                        .as_deref()
                        .unwrap_or("unknown_tool")
                        .to_string();
                    let mut capture = loop_capture.write();
                    capture.notes.push(format!("loop_guard:{tool}:{warning}"));
                    if action == "block" {
                        capture.loop_abort_count = capture.loop_abort_count.saturating_add(1);
                        return HookResult::Abort(format!("Loop prevention triggered. {warning}"));
                    }
                }
                HookResult::Continue
            },
        )
        .with_priority(10),
    ));

    let delegate_followup_capture = runtime_hook_capture.clone();
    engine.register(Arc::new(
        FnHook::new(
            "runtime_delegate_followup_preserver",
            vec![HookTiming::BeforeToolCall],
            move |event| {
                if event.tool_name.as_deref() != Some("delegate") {
                    return HookResult::Continue;
                }
                let Some(user_input) = event.user_input.as_deref() else {
                    return HookResult::Continue;
                };
                let Some(tool_args) = event.tool_args.as_deref() else {
                    return HookResult::Continue;
                };
                let Some(repaired) =
                    repair_delegate_args_with_full_followup_request(user_input, tool_args)
                else {
                    return HookResult::Continue;
                };

                let mut capture = delegate_followup_capture.write();
                capture
                    .notes
                    .push("before_tool:delegate_followup_preserved_full_user_request".to_string());
                HookResult::Modify(repaired)
            },
        )
        .with_priority(11),
    ));

    let deferred_capture = runtime_hook_capture.clone();
    engine.register(Arc::new(
        FnHook::new(
            "runtime_deferred_tool_filter_surface",
            vec![HookTiming::BeforeLlm],
            move |event| {
                let deferred_count = event
                    .metadata
                    .get("deferred_tool_count")
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or(0);
                if deferred_count == 0 {
                    return HookResult::Continue;
                }

                let visible_count = event
                    .metadata
                    .get("requested_tool_count")
                    .cloned()
                    .unwrap_or_else(|| "unknown".to_string());
                let total_count = event
                    .metadata
                    .get("total_tool_count")
                    .cloned()
                    .unwrap_or_else(|| "unknown".to_string());

                let mut capture = deferred_capture.write();
                capture.notes.push(format!(
                    "before_llm:deferred_tool_filter:{visible_count}/{total_count}:deferred={deferred_count}"
                ));
                HookResult::Modify(format!(
                    "Long-tail tools remain deferred from the active tool contract for this turn ({visible_count} visible / {total_count} total). If you need a niche or non-core tool, call `tool_search` first instead of guessing."
                ))
            },
        )
        .with_priority(12),
    ));

    let post_llm_capture = runtime_hook_capture.clone();
    engine.register(Arc::new(
        FnHook::new(
            "runtime_post_llm_surface",
            vec![HookTiming::AfterLlm],
            move |event| {
                let mut capture = post_llm_capture.write();
                capture.post_llm_tap_count = capture.post_llm_tap_count.saturating_add(1);
                if let Some(reason) = event.metadata.get("finish_reason") {
                    capture.notes.push(format!("after_llm:finish:{reason}"));
                }
                if let Some(provider_name) = event.metadata.get("provider_name") {
                    let model = event
                        .metadata
                        .get("provider_model")
                        .cloned()
                        .unwrap_or_else(|| "unknown".to_string());
                    capture
                        .notes
                        .push(format!("after_llm:provider:{provider_name}:{model}"));
                }
                if let Some(latency_ms) = event.metadata.get("provider_latency_ms") {
                    capture
                        .notes
                        .push(format!("after_llm:provider_latency_ms:{latency_ms}"));
                }
                for (metadata_key, note_prefix) in [
                    (
                        "provider_usage_prompt_tokens",
                        "after_llm:provider_prompt_tokens:",
                    ),
                    (
                        "provider_usage_completion_tokens",
                        "after_llm:provider_completion_tokens:",
                    ),
                    (
                        "provider_usage_total_tokens",
                        "after_llm:provider_total_tokens:",
                    ),
                    (
                        "provider_telemetry_finish_reason",
                        "after_llm:provider_finish_reason:",
                    ),
                    (
                        "provider_telemetry_tool_call_count",
                        "after_llm:provider_tool_call_count:",
                    ),
                    (
                        "provider_telemetry_tool_contract_mode",
                        "after_llm:provider_tool_contract_mode:",
                    ),
                    (
                        "provider_telemetry_mainline_stability",
                        "after_llm:provider_mainline_stability:",
                    ),
                    (
                        "provider_continuation_mode",
                        "after_llm:provider_continuation_mode:",
                    ),
                    (
                        "provider_continuation_cache_source",
                        "after_llm:provider_continuation_cache_source:",
                    ),
                    (
                        "provider_continuation_prompt_tokens",
                        "after_llm:provider_continuation_prompt_tokens:",
                    ),
                    (
                        "provider_continuation_prefill_ms",
                        "after_llm:provider_continuation_prefill_ms:",
                    ),
                    (
                        "provider_continuation_decode_ms",
                        "after_llm:provider_continuation_decode_ms:",
                    ),
                    (
                        "provider_continuation_miss_reason",
                        "after_llm:provider_continuation_miss_reason:",
                    ),
                    (
                        "provider_continuation_tool_exact_replay_used",
                        "after_llm:provider_continuation_tool_exact_replay_used:",
                    ),
                    (
                        "provider_continuation_protocol_live_used",
                        "after_llm:provider_continuation_protocol_live_used:",
                    ),
                    (
                        "runtime_continuation_user_session_id",
                        "after_llm:runtime_continuation_user_session_id:",
                    ),
                    (
                        "runtime_continuation_turn_id",
                        "after_llm:runtime_continuation_turn_id:",
                    ),
                    (
                        "runtime_continuation_worker_run_id",
                        "after_llm:runtime_continuation_worker_run_id:",
                    ),
                    (
                        "runtime_continuation_frontier_id",
                        "after_llm:runtime_continuation_frontier_id:",
                    ),
                    (
                        "runtime_continuation_visible_prompt_fingerprint",
                        "after_llm:runtime_continuation_visible_prompt_fingerprint:",
                    ),
                    (
                        "provider_telemetry_media_preprocess_consumed_by",
                        "after_llm:provider_media_preprocess_consumed_by:",
                    ),
                    (
                        "provider_telemetry_media_preprocess_consumption_routes",
                        "after_llm:provider_media_preprocess_consumption_routes:",
                    ),
                    (
                        "provider_telemetry_media_preprocess_outcomes",
                        "after_llm:provider_media_preprocess_outcomes:",
                    ),
                    (
                        "provider_telemetry_media_preprocess_preprocess_failed_routes",
                        "after_llm:provider_media_preprocess_preprocess_failed_routes:",
                    ),
                    (
                        "provider_telemetry_media_preprocess_model_failed_routes",
                        "after_llm:provider_media_preprocess_model_failed_routes:",
                    ),
                    (
                        "provider_telemetry_media_preprocess_result_insufficient_routes",
                        "after_llm:provider_media_preprocess_result_insufficient_routes:",
                    ),
                    (
                        "provider_telemetry_media_preprocess_followup_strategies",
                        "after_llm:provider_media_preprocess_followup_strategies:",
                    ),
                    (
                        "provider_telemetry_media_preprocess_attachment_fallback_routes",
                        "after_llm:provider_media_preprocess_attachment_fallback_routes:",
                    ),
                    (
                        "provider_telemetry_media_preprocess_alternate_model_fallback_routes",
                        "after_llm:provider_media_preprocess_alternate_model_fallback_routes:",
                    ),
                    (
                        "provider_telemetry_media_preprocess_clarification_routes",
                        "after_llm:provider_media_preprocess_clarification_routes:",
                    ),
                    (
                        "provider_telemetry_media_preprocess_strategy_note_complete",
                        "after_llm:provider_media_preprocess_strategy_note_complete:",
                    ),
                    (
                        "provider_telemetry_media_preprocess_strategy_contract_complete",
                        "after_llm:provider_media_preprocess_strategy_contract_complete:",
                    ),
                ] {
                    if let Some(value) = event.metadata.get(metadata_key) {
                        if !value.trim().is_empty() {
                            capture.notes.push(format!("{note_prefix}{value}"));
                        }
                    }
                }
                HookResult::Continue
            },
        )
        .with_priority(15),
    ));

    let degradation_capture = runtime_hook_capture.clone();
    engine.register(Arc::new(
        FnHook::new(
            "runtime_skill_manual_surface",
            vec![HookTiming::AfterToolCall],
            move |event| {
                if !matches!(
                    event.tool_name.as_deref(),
                    Some("read_skill_manual" | "read_skill_asset")
                ) {
                    return HookResult::Continue;
                }

                let mut capture = degradation_capture.write();
                let skill_name = event
                    .metadata
                    .get("skill_name")
                    .cloned()
                    .or_else(|| extract_json_string_field(event.tool_args.as_ref(), "skill_name"))
                    .unwrap_or_else(|| "unknown".to_string());
                match event.tool_name.as_deref() {
                    Some("read_skill_manual") => {
                        capture.skill_manual_read_count =
                            capture.skill_manual_read_count.saturating_add(1);
                        capture
                            .notes
                            .push(format!("skill_manual_read:{skill_name}"));
                        if let Some(tool_result) = event.tool_result.as_deref() {
                            if let Some(classification) =
                                extract_skill_surface_field(tool_result, "classification")
                            {
                                capture.notes.push(format!(
                                    "skill_surface_classification:{skill_name}:{classification}"
                                ));
                            }
                            if let Some(surface) =
                                extract_skill_surface_field(tool_result, "execution_surface")
                            {
                                capture.notes.push(format!(
                                    "skill_surface_execution:{skill_name}:{surface}"
                                ));
                            }
                            if let Some(runtime) =
                                extract_skill_surface_field(tool_result, "runtime")
                            {
                                capture
                                    .notes
                                    .push(format!("skill_surface_runtime:{skill_name}:{runtime}"));
                            }
                            if let Some(kind) = extract_skill_surface_field(tool_result, "kind") {
                                capture
                                    .notes
                                    .push(format!("skill_surface_kind:{skill_name}:{kind}"));
                            }
                        }
                    }
                    Some("read_skill_asset") => {
                        capture.skill_asset_read_count =
                            capture.skill_asset_read_count.saturating_add(1);
                        let asset_path = event
                            .metadata
                            .get("asset_path")
                            .cloned()
                            .or_else(|| {
                                extract_json_string_field(event.tool_args.as_ref(), "asset_path")
                            })
                            .unwrap_or_else(|| "unknown".to_string());
                        capture
                            .notes
                            .push(format!("skill_asset_read:{skill_name}:{asset_path}"));
                    }
                    _ => {}
                }
                HookResult::Continue
            },
        )
        .with_priority(18),
    ));

    let skill_followup_capture = runtime_hook_capture.clone();
    engine.register(Arc::new(
        FnHook::new(
            "runtime_media_preprocess_surface",
            vec![HookTiming::AfterToolCall],
            move |event| {
                let Some(tool_name) = event.tool_name.as_deref() else {
                    return HookResult::Continue;
                };
                let Some(media_kind) = media_tool_kind(tool_name) else {
                    return HookResult::Continue;
                };
                let Some(tool_result) = event.tool_result.as_ref() else {
                    return HookResult::Continue;
                };
                let Ok(payload) = serde_json::from_str::<serde_json::Value>(tool_result) else {
                    return HookResult::Continue;
                };

                let status = payload
                    .get("status")
                    .and_then(|value| value.as_str())
                    .unwrap_or("unknown");
                let mut capture = skill_followup_capture.write();
                capture.media_surface_count = capture.media_surface_count.saturating_add(1);
                capture
                    .notes
                    .push(format!("media_preprocess_tool:{tool_name}"));
                capture
                    .notes
                    .push(format!("media_preprocess_status:{tool_name}:{status}"));
                capture
                    .notes
                    .push(format!("media_preprocess_kind:{tool_name}:{media_kind}"));
                if let Some(engine) = media_tool_engine(tool_name) {
                    capture
                        .notes
                        .push(format!("media_preprocess_engine:{tool_name}:{engine}"));
                }
                if let Some(path) = payload.get("path").and_then(|value| value.as_str()) {
                    if !path.trim().is_empty() {
                        capture
                            .notes
                            .push(format!("media_preprocess_input:{tool_name}:{path}"));
                    }
                }
                if let Some(path) = payload.get("output_path").and_then(|value| value.as_str()) {
                    if !path.trim().is_empty() {
                        capture
                            .notes
                            .push(format!("media_preprocess_output:{tool_name}:file:{path}"));
                    }
                }
                if let Some(path) = payload.get("output_dir").and_then(|value| value.as_str()) {
                    if !path.trim().is_empty() {
                        capture
                            .notes
                            .push(format!("media_preprocess_output:{tool_name}:dir:{path}"));
                    }
                }
                if let Some(count) = payload
                    .get("frame_count_extracted")
                    .and_then(|value| value.as_u64())
                {
                    capture
                        .notes
                        .push(format!("media_preprocess_frames:{tool_name}:{count}"));
                }
                if let Some(cleanup_active) = payload
                    .get("cleanup")
                    .and_then(|value| value.get("active"))
                    .and_then(|value| value.as_bool())
                {
                    capture.notes.push(format!(
                        "media_preprocess_cleanup:{tool_name}:{cleanup_active}"
                    ));
                }
                if let Some(artifact_kind) = payload
                    .get("artifact_kind")
                    .and_then(|value| value.as_str())
                {
                    if !artifact_kind.trim().is_empty() {
                        capture.notes.push(format!(
                            "media_preprocess_artifact_kind:{tool_name}:{artifact_kind}"
                        ));
                    }
                }
                if let Some(registration) = payload.get("artifact_registration") {
                    if let Some(registered) = registration
                        .get("registered")
                        .and_then(|value| value.as_bool())
                    {
                        capture.notes.push(format!(
                            "media_preprocess_artifact_registered:{tool_name}:{registered}"
                        ));
                    }
                    if let Some(source_kind) = registration
                        .get("source_kind")
                        .and_then(|value| value.as_str())
                        .filter(|value| !value.trim().is_empty())
                    {
                        capture.notes.push(format!(
                            "media_preprocess_artifact_source_kind:{tool_name}:{source_kind}"
                        ));
                    }
                    if let Some(uri) = registration
                        .get("uri")
                        .and_then(|value| value.as_str())
                        .filter(|value| !value.trim().is_empty())
                    {
                        capture
                            .notes
                            .push(format!("media_preprocess_artifact_uri:{tool_name}:{uri}"));
                    }
                }
                HookResult::Continue
            },
        )
        .with_priority(18),
    ));

    let media_consumption_capture = runtime_hook_capture.clone();
    engine.register(Arc::new(
        FnHook::new(
            "runtime_media_preprocess_consumption_surface",
            vec![HookTiming::AfterToolCall],
            move |event| {
                if !matches!(
                    event.tool_name.as_deref(),
                    Some("document_understand" | "text_extract" | "pdf_parse")
                ) {
                    return HookResult::Continue;
                }
                let Some(tool_result) = event.tool_result.as_ref() else {
                    return HookResult::Continue;
                };
                let Ok(payload) = serde_json::from_str::<serde_json::Value>(tool_result) else {
                    return HookResult::Continue;
                };
                let preprocess_route = payload
                    .get("media_preprocess_route")
                    .and_then(|value| value.as_str())
                    .filter(|value| !value.trim().is_empty())
                    .map(str::to_string);

                let mut capture = media_consumption_capture.write();
                if let Some(preprocess_route) = preprocess_route.as_deref() {
                    if let Some(outcome) = payload
                        .get("media_pipeline_outcome")
                        .and_then(|value| value.as_str())
                        .filter(|value| !value.trim().is_empty())
                    {
                        capture.notes.push(format!(
                            "media_preprocess_outcome:{preprocess_route}:{outcome}"
                        ));
                        match outcome {
                            "preprocess_failed" => {
                                capture.notes.push(format!(
                                    "media_preprocess_preprocess_failed:{preprocess_route}"
                                ));
                                capture.notes.push(format!(
                                    "media_preprocess_followup_strategy:{preprocess_route}:attachment_fallback"
                                ));
                                capture.notes.push(format!(
                                    "media_preprocess_strategy_attachment_fallback:{preprocess_route}"
                                ));
                            }
                            "model_failed_after_preprocess" => {
                                capture.notes.push(format!(
                                    "media_preprocess_model_failed:{preprocess_route}"
                                ));
                                capture.notes.push(format!(
                                    "media_preprocess_followup_strategy:{preprocess_route}:alternate_model_fallback"
                                ));
                                capture.notes.push(format!(
                                    "media_preprocess_strategy_alternate_model_fallback:{preprocess_route}"
                                ));
                            }
                            "model_result_insufficient" => {
                                capture.notes.push(format!(
                                    "media_preprocess_result_insufficient:{preprocess_route}"
                                ));
                                capture.notes.push(format!(
                                    "media_preprocess_followup_strategy:{preprocess_route}:clarification_or_manual_review"
                                ));
                                capture.notes.push(format!(
                                    "media_preprocess_strategy_clarification:{preprocess_route}"
                                ));
                            }
                            _ => {}
                        }
                    }
                    if let Some(source_kind) = payload
                        .get("media_preprocess_source_kind")
                        .and_then(|value| value.as_str())
                        .filter(|value| !value.trim().is_empty())
                    {
                        capture.notes.push(format!(
                            "media_preprocess_source_kind:{preprocess_route}:{source_kind}"
                        ));
                    }
                    if let Some(source_ref) = payload
                        .get("media_preprocess_source_ref")
                        .and_then(|value| value.as_str())
                        .filter(|value| !value.trim().is_empty())
                    {
                        capture.notes.push(format!(
                            "media_preprocess_source_ref:{preprocess_route}:{source_ref}"
                        ));
                    }
                }
                if let Some(page_routes) = payload.get("page_routes").and_then(|value| value.as_array())
                {
                    for page_route in page_routes {
                        let Some(selected_route) = page_route
                            .get("selected_route")
                            .and_then(|value| value.as_str())
                            .filter(|value| !value.trim().is_empty())
                        else {
                            continue;
                        };
                        let route_key = format!("pdf_parse_tool:{selected_route}");
                        if let Some(source_kind) = page_route
                            .get("source_contract_kind")
                            .and_then(|value| value.as_str())
                            .filter(|value| !value.trim().is_empty())
                        {
                            capture.notes.push(format!(
                                "media_preprocess_source_kind:{route_key}:{source_kind}"
                            ));
                        }
                        if let Some(source_ref) = page_route
                            .get("source_contract_ref")
                            .and_then(|value| value.as_str())
                            .filter(|value| !value.trim().is_empty())
                        {
                            capture.notes.push(format!(
                                "media_preprocess_source_ref:{route_key}:{source_ref}"
                            ));
                        }
                    }
                }
                if let Some(preprocess_route) = preprocess_route.as_deref() {
                    if let Some(frame_contracts) = payload
                        .get("frame_source_contracts")
                        .and_then(|value| value.as_array())
                    {
                        for frame_contract in frame_contracts {
                            if let Some(source_kind) = frame_contract
                                .get("source_contract_kind")
                                .and_then(|value| value.as_str())
                                .filter(|value| !value.trim().is_empty())
                            {
                                capture.notes.push(format!(
                                    "media_preprocess_source_kind:{preprocess_route}:{source_kind}"
                                ));
                            }
                            if let Some(source_ref) = frame_contract
                                .get("source_contract_ref")
                                .and_then(|value| value.as_str())
                                .filter(|value| !value.trim().is_empty())
                            {
                                capture.notes.push(format!(
                                    "media_preprocess_source_ref:{preprocess_route}:{source_ref}"
                                ));
                            }
                        }
                    }
                }
                let status_ok = payload
                    .get("status")
                    .and_then(|value| value.as_str())
                    == Some("ok");
                if !status_ok {
                    return HookResult::Continue;
                }
                let Some(preprocess_route) = preprocess_route.as_deref() else {
                    return HookResult::Continue;
                };
                if payload
                    .get("media_preprocess_consumed")
                    .and_then(|value| value.as_bool())
                    != Some(true)
                {
                    return HookResult::Continue;
                }

                if let Some(consumer) = payload
                    .get("media_preprocess_consumer")
                    .and_then(|value| value.as_str())
                    .filter(|value| !value.trim().is_empty())
                {
                    capture.notes.push(format!(
                        "media_preprocess_consumed_by:{preprocess_route}:{consumer}"
                    ));
                }
                if let Some(route) = payload
                    .get("media_preprocess_consumer_route")
                    .and_then(|value| value.as_str())
                    .filter(|value| !value.trim().is_empty())
                {
                    capture.notes.push(format!(
                        "media_preprocess_consumption_route:{preprocess_route}:{route}"
                    ));
                }
                HookResult::Continue
            },
        )
        .with_priority(18),
    ));

    let skill_followup_capture = runtime_hook_capture.clone();
    engine.register(Arc::new(
        FnHook::new(
            "runtime_skill_asset_followup_surface",
            vec![HookTiming::AfterToolCall],
            move |event| {
                let Some(tool_name) = event.tool_name.as_deref() else {
                    return HookResult::Continue;
                };
                if matches!(tool_name, "read_skill_manual" | "read_skill_asset") {
                    return HookResult::Continue;
                }

                let mut capture = skill_followup_capture.write();
                let last_asset_ref = capture.notes.iter().rev().find_map(|note| {
                    note.strip_prefix("skill_asset_read:")
                        .map(std::string::ToString::to_string)
                });
                if let Some(asset_ref) = last_asset_ref {
                    let surface = classify_skill_asset_execution_surface(tool_name);
                    capture
                        .notes
                        .push(format!("skill_asset_followup:{asset_ref}:{tool_name}"));
                    capture.notes.push(format!(
                        "skill_asset_execution_surface:{asset_ref}:{surface}:{tool_name}"
                    ));
                }
                HookResult::Continue
            },
        )
        .with_priority(19),
    ));

    let forge_capture = runtime_hook_capture.clone();
    engine.register(Arc::new(
        FnHook::new(
            "runtime_forge_surface",
            vec![HookTiming::AfterToolCall],
            move |event| {
                if event.tool_name.as_deref() != Some("forge_skill") {
                    return HookResult::Continue;
                }

                let Some(tool_result) = event.tool_result.as_ref() else {
                    return HookResult::Continue;
                };
                let Ok(payload) = serde_json::from_str::<serde_json::Value>(tool_result) else {
                    return HookResult::Continue;
                };

                let tool_name = payload
                    .get("tool_name")
                    .and_then(|value| value.as_str())
                    .unwrap_or("unknown");
                let source = payload
                    .get("source")
                    .and_then(|value| value.as_str())
                    .unwrap_or("forge");
                let scope = payload
                    .get("scope")
                    .and_then(|value| value.as_str())
                    .unwrap_or("session");
                let execution_surface = payload
                    .get("execution_surface")
                    .and_then(|value| value.as_str())
                    .unwrap_or("runtime");
                let smoke_status = payload
                    .get("smoke_test")
                    .and_then(|value| value.get("status"))
                    .and_then(|value| value.as_str())
                    .unwrap_or("unknown");
                let smoke_latency_ms = payload
                    .get("smoke_test")
                    .and_then(|value| value.get("latency_ms"))
                    .and_then(|value| value.as_u64());
                let cleanup_recorded = payload
                    .get("session_cleanup_recorded")
                    .and_then(|value| value.as_bool())
                    .or_else(|| {
                        extract_json_bool_field(
                            event.tool_result.as_ref(),
                            "session_cleanup_recorded",
                        )
                    })
                    .unwrap_or(false);

                let mut capture = forge_capture.write();
                capture.forge_surface_count = capture.forge_surface_count.saturating_add(1);
                capture.notes.push(format!("forge_registered:{tool_name}"));
                capture.notes.push(format!("forge_source:{source}"));
                capture.notes.push(format!("forge_scope:{scope}"));
                capture.notes.push(format!(
                    "forge_execution_surface:{tool_name}:{execution_surface}"
                ));
                capture
                    .notes
                    .push(format!("forge_smoke_status:{tool_name}:{smoke_status}"));
                capture.notes.push(format!(
                    "forge_cleanup_recorded:{tool_name}:{cleanup_recorded}"
                ));
                if let Some(capability_domain) = payload
                    .get("capability_domain")
                    .and_then(|value| value.as_str())
                {
                    if !capability_domain.is_empty() {
                        capture.notes.push(format!(
                            "forge_capability_domain:{tool_name}:{capability_domain}"
                        ));
                    }
                }
                if let Some(latency_ms) = smoke_latency_ms {
                    capture
                        .notes
                        .push(format!("forge_smoke_latency_ms:{tool_name}:{latency_ms}"));
                }
                HookResult::Continue
            },
        )
        .with_priority(19),
    ));

    let verification_capture = runtime_hook_capture.clone();
    engine.register(Arc::new(
        FnHook::new(
            "runtime_tool_verification_capture",
            vec![HookTiming::AfterToolCall],
            move |event| {
                let Some(tool_name) = event.tool_name.as_deref() else {
                    return HookResult::Continue;
                };
                let Some(observation) =
                    extract_verification_preview_observation(tool_name, event.tool_result.as_ref())
                else {
                    return HookResult::Continue;
                };
                let sources = extract_verification_sources(event.tool_result.as_ref());
                let execution_evidence = extract_verification_string_evidence(
                    event.tool_result.as_ref(),
                    "execution_evidence",
                );
                let state_evidence = extract_verification_string_evidence(
                    event.tool_result.as_ref(),
                    "state_evidence",
                );

                let mut capture = verification_capture.write();
                capture
                    .notes
                    .push(render_verification_preview_summary(&observation));
                if let Some(note) = render_verification_sources_json_note(&sources) {
                    capture.notes.push(note);
                }
                if let Some(note) = render_verification_string_array_note(
                    "after_tool:verification_execution_evidence_json:",
                    &execution_evidence,
                ) {
                    capture.notes.push(note);
                }
                if let Some(note) = render_verification_string_array_note(
                    "after_tool:verification_state_evidence_json:",
                    &state_evidence,
                ) {
                    capture.notes.push(note);
                }
                if let Some(followup) =
                    extract_verification_followup_observation(event.tool_result.as_ref())
                {
                    capture
                        .notes
                        .push(render_verification_followup_summary(&followup));
                }
                if let Some(orchestration) =
                    extract_verification_orchestration_observation(event.tool_result.as_ref())
                {
                    capture
                        .notes
                        .push(render_verification_orchestration_summary(&orchestration));
                }
                HookResult::Continue
            },
        )
        .with_priority(19),
    ));

    let degradation_capture = runtime_hook_capture.clone();
    engine.register(Arc::new(
        FnHook::new(
            "runtime_tool_degradation_surface",
            vec![HookTiming::AfterToolCall, HookTiming::OnError],
            move |event| {
                let mut reasons = Vec::new();
                if event
                    .metadata
                    .get("tool_output_truncated")
                    .is_some_and(|value| value == "true")
                {
                    reasons.push("tool_output_truncated".to_string());
                }
                if let Some(reason) = event.metadata.get("degradation_reason") {
                    reasons.push(reason.clone());
                }
                if reasons.is_empty() {
                    return HookResult::Continue;
                }

                let tool = event
                    .tool_name
                    .as_deref()
                    .unwrap_or("unknown_tool")
                    .to_string();
                let mut capture = degradation_capture.write();
                capture.degraded_tool_call_count =
                    capture.degraded_tool_call_count.saturating_add(1);
                for reason in reasons {
                    capture
                        .notes
                        .push(format!("tool_degradation:{tool}:{reason}"));
                }
                HookResult::Continue
            },
        )
        .with_priority(20),
    ));

    let error_capture = runtime_hook_capture.clone();
    engine.register(Arc::new(
        FnHook::new(
            "runtime_tool_error_surface",
            vec![HookTiming::OnError],
            move |event| {
                let Some(error) = event.error.as_ref() else {
                    return HookResult::Continue;
                };

                let tool = event
                    .tool_name
                    .as_deref()
                    .unwrap_or("unknown_tool")
                    .to_string();
                let mut capture = error_capture.write();
                capture.tool_error_count = capture.tool_error_count.saturating_add(1);
                capture.notes.push(format!("tool_error:{tool}:{error}"));
                capture.notes.push(format!("tool_error_surface:{tool}"));
                HookResult::Modify(normalize_runtime_tool_error_message(&tool, error))
            },
        )
        .with_priority(25),
    ));

    let clarification_capture = runtime_hook_capture.clone();
    engine.register(Arc::new(
        FnHook::new(
            "runtime_clarification_surface",
            vec![HookTiming::BeforeResponse],
            move |event| {
                let Some(status_kind) = event
                    .metadata
                    .get("clarification_status_kind")
                    .cloned()
                    .or_else(|| event.metadata.get("session_status").cloned())
                else {
                    return HookResult::Continue;
                };

                let event_name = event
                    .metadata
                    .get("clarification_event")
                    .cloned()
                    .unwrap_or_else(|| "unknown".to_string());
                let prompt_present = event.metadata.contains_key("clarification_prompt");
                let original_present = event
                    .metadata
                    .contains_key("clarification_original_request");
                let json_valid = event
                    .metadata
                    .get("clarification_session_status_json_valid")
                    .cloned()
                    .unwrap_or_else(|| "false".to_string());

                let mut capture = clarification_capture.write();
                capture.clarification_surface_count =
                    capture.clarification_surface_count.saturating_add(1);
                capture.notes.push(format!(
                    "before_response:clarification_surface:status={status_kind}:event={event_name}:prompt_present={prompt_present}:original_present={original_present}:json_valid={json_valid}"
                ));
                HookResult::Continue
            },
        )
        .with_priority(26),
    ));

    let subagent_capture = runtime_hook_capture.clone();
    engine.register(Arc::new(
        FnHook::new(
            "runtime_subagent_budget_surface",
            vec![HookTiming::BeforeResponse],
            move |event| {
                let delegation_present = event
                    .metadata
                    .get("delegation_present")
                    .is_some_and(|value| value == "true");
                let handover_present = event
                    .metadata
                    .get("handover_present")
                    .is_some_and(|value| value == "true");
                let max_parallel_tools = event
                    .metadata
                    .get("max_parallel_tools")
                    .cloned()
                    .unwrap_or_else(|| "unknown".to_string());

                let mut capture = subagent_capture.write();
                capture.subagent_surface_count =
                    capture.subagent_surface_count.saturating_add(1);
                capture.notes.push(format!(
                    "before_response:subagent_budget:delegation={delegation_present}:handover={handover_present}:parallel_tools={max_parallel_tools}"
                ));
                HookResult::Continue
            },
        )
        .with_priority(27),
    ));

    let title_capture = runtime_hook_capture.clone();
    engine.register(Arc::new(
        FnHook::new(
            "runtime_title_surface",
            vec![HookTiming::BeforeResponse],
            move |event| {
                let title_present = event
                    .metadata
                    .get("session_title_present")
                    .is_some_and(|value| value == "true");
                let title_source = event
                    .metadata
                    .get("session_title_source")
                    .cloned()
                    .unwrap_or_else(|| "missing".to_string());
                let title_value = event
                    .metadata
                    .get("session_title")
                    .cloned()
                    .unwrap_or_else(|| "(untitled)".to_string());

                let mut capture = title_capture.write();
                capture.title_surface_count = capture.title_surface_count.saturating_add(1);
                capture.notes.push(format!(
                    "before_response:title:present={title_present}:source={title_source}:value={title_value}"
                ));
                HookResult::Continue
            },
        )
        .with_priority(29),
    ));

    let memory_session_capture = runtime_hook_capture.clone();
    engine.register(Arc::new(
        FnHook::new(
            "runtime_memory_session_surface_note",
            vec![HookTiming::BeforeResponse],
            move |event| {
                let visible_owner = event
                    .metadata
                    .get("visible_owner")
                    .cloned()
                    .unwrap_or_else(|| "unknown".to_string());
                let memory_owner = event
                    .metadata
                    .get("memory_owner")
                    .cloned()
                    .unwrap_or_else(|| "unknown".to_string());
                let approval_owner = event
                    .metadata
                    .get("approval_owner")
                    .cloned()
                    .unwrap_or_else(|| "unknown".to_string());
                let title_present = event
                    .metadata
                    .get("session_title_present")
                    .cloned()
                    .unwrap_or_else(|| "false".to_string());
                let title_source = event
                    .metadata
                    .get("session_title_source")
                    .cloned()
                    .unwrap_or_else(|| "missing".to_string());
                let summary_present = event
                    .metadata
                    .get("post_run_summary")
                    .map(|value| (!value.trim().is_empty()).to_string())
                    .unwrap_or_else(|| "false".to_string());

                let mut capture = memory_session_capture.write();
                capture.notes.push(format!(
                    "before_response:memory_session_surface:visible={visible_owner}:memory={memory_owner}:approval={approval_owner}:title_present={title_present}:title_source={title_source}:summary_present={summary_present}"
                ));
                HookResult::Continue
            },
        )
        .with_priority(30),
    ));

    let engram_windows_native_capture = runtime_hook_capture.clone();
    engine.register(Arc::new(
        FnHook::new(
            "runtime_engram_windows_native_surface",
            vec![HookTiming::BeforeResponse],
            move |event| {
                let embed_outcome = event
                    .metadata
                    .get("engram_windows_native_embed_outcome")
                    .cloned();
                let embed_class = event
                    .metadata
                    .get("engram_windows_native_embed_class")
                    .cloned();
                let embed_strategy = event
                    .metadata
                    .get("engram_windows_native_embed_strategy")
                    .cloned();
                let embed_note = event
                    .metadata
                    .get("engram_windows_native_embed_note")
                    .cloned();
                let rerank_outcome = event
                    .metadata
                    .get("engram_windows_native_rerank_outcome")
                    .cloned();
                let rerank_class = event
                    .metadata
                    .get("engram_windows_native_rerank_class")
                    .cloned();
                let rerank_strategy = event
                    .metadata
                    .get("engram_windows_native_rerank_strategy")
                    .cloned();
                let rerank_note = event
                    .metadata
                    .get("engram_windows_native_rerank_note")
                    .cloned();

                let embed_present = embed_outcome.is_some()
                    || embed_class.is_some()
                    || embed_strategy.is_some()
                    || embed_note.is_some();
                let rerank_present = rerank_outcome.is_some()
                    || rerank_class.is_some()
                    || rerank_strategy.is_some()
                    || rerank_note.is_some();
                if !embed_present && !rerank_present {
                    return HookResult::Continue;
                }

                let mut capture = engram_windows_native_capture.write();
                if let Some(value) = embed_outcome {
                    capture.notes.push(format!(
                        "before_response:engram_windows_native_embed_outcome:{value}"
                    ));
                }
                if let Some(value) = embed_class {
                    capture.notes.push(format!(
                        "before_response:engram_windows_native_embed_class:{value}"
                    ));
                }
                if let Some(value) = embed_strategy {
                    capture.notes.push(format!(
                        "before_response:engram_windows_native_embed_strategy:{value}"
                    ));
                }
                if let Some(value) = embed_note {
                    capture.notes.push(format!(
                        "before_response:engram_windows_native_embed_note:{value}"
                    ));
                }
                if let Some(value) = rerank_outcome {
                    capture.notes.push(format!(
                        "before_response:engram_windows_native_rerank_outcome:{value}"
                    ));
                }
                if let Some(value) = rerank_class {
                    capture.notes.push(format!(
                        "before_response:engram_windows_native_rerank_class:{value}"
                    ));
                }
                if let Some(value) = rerank_strategy {
                    capture.notes.push(format!(
                        "before_response:engram_windows_native_rerank_strategy:{value}"
                    ));
                }
                if let Some(value) = rerank_note {
                    capture.notes.push(format!(
                        "before_response:engram_windows_native_rerank_note:{value}"
                    ));
                }
                capture.notes.push(format!(
                    "before_response:engram_windows_native_surface:embed_present={embed_present}:rerank_present={rerank_present}"
                ));
                HookResult::Continue
            },
        )
        .with_priority(30),
    ));

    let truth_verification_capture = runtime_hook_capture.clone();
    engine.register(Arc::new(
        FnHook::new(
            "runtime_local_context_truth_surface",
            vec![HookTiming::BeforeResponse],
            move |event| {
                let policy = TruthVerificationPolicyEngine::default();
                let Some(user_input) = event.user_input.as_deref() else {
                    return HookResult::Continue;
                };
                let Some(plan) = classify_query_verification_plan(user_input) else {
                    return HookResult::Continue;
                };
                if plan.requirement != VerificationRequirement::LocalContextAllowed {
                    return HookResult::Continue;
                }
                let source_required = policy.query_requests_explicit_sources(user_input);

                let mut capture = truth_verification_capture.write();
                let has_tool_verification = capture
                    .notes
                    .iter()
                    .any(|note| note.starts_with("after_tool:verification_preview:"));
                let already_emitted = capture
                    .notes
                    .iter()
                    .any(|note| note == "before_response:verification_mode:LocalContextOnly");
                if has_tool_verification || already_emitted {
                    return HookResult::Continue;
                }

                capture
                    .notes
                    .push("before_response:truth_status:Unverified".to_string());
                capture
                    .notes
                    .push("before_response:verification_domain:KnowledgeFact".to_string());
                capture.notes.push(
                    "before_response:verification_requirement:LocalContextAllowed".to_string(),
                );
                capture
                    .notes
                    .push("before_response:verification_mode:LocalContextOnly".to_string());
                capture.notes.push(
                    "before_response:verification_outcome:VerificationNotRequired".to_string(),
                );
                capture.notes.push(format!(
                    "before_response:source_posture:{}",
                    if source_required {
                        "SourcesRequiredButMissing"
                    } else {
                        "NoSourcesRequired"
                    }
                ));
                capture.notes.push(
                    "before_response:verification_answer_readiness:local_context_only".to_string(),
                );
                if source_required {
                    capture
                        .notes
                        .push("before_response:verification_cite_required:true".to_string());
                }
                capture.notes.push(
                    "before_response:verification_followup_note:local_context_only".to_string(),
                );

                match event.llm_response.as_ref() {
                    Some(response) if source_required && !response.trim().is_empty() => {
                        HookResult::Modify(format!(
                            "{}\n\n{}",
                            policy.build_local_context_only_notice(source_required),
                            response
                        ))
                    }
                    _ => HookResult::Continue,
                }
            },
        )
        .with_priority(30),
    ));

    let truth_verification_capture = runtime_hook_capture.clone();
    engine.register(Arc::new(
        FnHook::new(
            "runtime_truth_verification_surface",
            vec![HookTiming::BeforeResponse],
            move |event| {
                let policy = TruthVerificationPolicyEngine::default();
                let summaries = {
                    let capture = truth_verification_capture.read();
                    capture
                        .notes
                        .iter()
                        .filter_map(|note| {
                            note.strip_prefix("after_tool:verification_preview:")
                                .map(parse_key_value_runtime_note_fields)
                        })
                        .collect::<Vec<_>>()
                };
                if summaries.is_empty() {
                    return HookResult::Continue;
                }

                let latest = summaries
                    .last()
                    .expect("verification summaries should not be empty");
                let latest_followup = {
                    let capture = truth_verification_capture.read();
                    latest_verification_followup_fields(&capture)
                };
                let latest_orchestration = {
                    let capture = truth_verification_capture.read();
                    latest_verification_orchestration_fields(&capture)
                };
                let latest_sources_json = {
                    let capture = truth_verification_capture.read();
                    latest_verification_sources_json(&capture)
                };
                let latest_execution_evidence_json = {
                    let capture = truth_verification_capture.read();
                    latest_verification_json_payload(
                        &capture,
                        "after_tool:verification_execution_evidence_json:",
                    )
                };
                let latest_state_evidence_json = {
                    let capture = truth_verification_capture.read();
                    latest_verification_json_payload(
                        &capture,
                        "after_tool:verification_state_evidence_json:",
                    )
                };
                let tool_names = summaries
                    .iter()
                    .filter_map(|summary| summary.get("tool").cloned())
                    .collect::<std::collections::BTreeSet<_>>();
                let tools_csv = tool_names.iter().cloned().collect::<Vec<_>>().join(",");
                let latest_tool = latest
                    .get("tool")
                    .cloned()
                    .unwrap_or_else(|| "unknown".to_string());
                let source_request_missing = policy.explicit_source_request_still_missing(
                    event.user_input.as_deref(),
                    latest,
                    latest_followup.as_ref(),
                );
                let complete = [
                    "domain",
                    "requirement",
                    "mode",
                    "outcome",
                    "truth_status",
                    "source_posture",
                ]
                .iter()
                .all(|key| latest.contains_key(*key));

                let mut capture = truth_verification_capture.write();
                if let Some(value) = latest.get("truth_status") {
                    capture
                        .notes
                        .push(format!("before_response:truth_status:{value}"));
                }
                if let Some(value) = latest.get("domain") {
                    capture
                        .notes
                        .push(format!("before_response:verification_domain:{value}"));
                }
                if let Some(value) = latest.get("requirement") {
                    capture.notes.push(format!(
                        "before_response:verification_requirement:{value}"
                    ));
                }
                if let Some(value) = latest.get("mode") {
                    capture
                        .notes
                        .push(format!("before_response:verification_mode:{value}"));
                }
                if let Some(value) = latest.get("outcome") {
                    capture.notes.push(format!(
                        "before_response:verification_outcome:{value}"
                    ));
                }
                if let Some(value) = latest.get("source_posture") {
                    let projected_posture = if source_request_missing {
                        "SourcesRequiredButMissing"
                    } else {
                        value
                    };
                    capture.notes.push(format!(
                        "before_response:source_posture:{projected_posture}"
                    ));
                }
                if let Some(followup) = latest_followup.as_ref() {
                    if let Some(value) = followup.get("answer_readiness") {
                        capture.notes.push(format!(
                            "before_response:verification_answer_readiness:{value}"
                        ));
                    }
                    if let Some(value) = followup.get("next_tools") {
                        if !value.trim().is_empty() {
                            capture
                                .notes
                                .push(format!("before_response:verification_next_tools:{value}"));
                        }
                    }
                    if let Some(value) = followup.get("cite_required") {
                        capture.notes.push(format!(
                            "before_response:verification_cite_required:{value}"
                        ));
                    }
                    if let Some(value) = followup.get("note") {
                        if !value.trim().is_empty() {
                            capture.notes.push(format!(
                                "before_response:verification_followup_note:{value}"
                            ));
                        }
                    }
                }
                if let Some(orchestration) = latest_orchestration.as_ref() {
                    if let Some(value) = orchestration.get("route_reason") {
                        capture.notes.push(format!(
                            "before_response:verification_route_reason:{value}"
                        ));
                    }
                    if let Some(value) = orchestration.get("continuation") {
                        capture.notes.push(format!(
                            "before_response:verification_continuation:{value}"
                        ));
                    }
                    if let Some(value) = orchestration.get("termination") {
                        capture.notes.push(format!(
                            "before_response:verification_termination:{value}"
                        ));
                    }
                    if let Some(value) = orchestration.get("requires_followup") {
                        capture.notes.push(format!(
                            "before_response:verification_requires_followup:{value}"
                        ));
                    }
                    if let Some(value) = orchestration.get("can_finalize_answer") {
                        capture.notes.push(format!(
                            "before_response:verification_can_finalize_answer:{value}"
                        ));
                    }
                }
                if let Some(value) = latest_sources_json.as_ref() {
                    capture.notes.push(format!(
                        "before_response:verification_sources_json:{value}"
                    ));
                }
                if let Some(value) = latest_execution_evidence_json.as_ref() {
                    capture.notes.push(format!(
                        "before_response:verification_execution_evidence_json:{value}"
                    ));
                }
                if let Some(value) = latest_state_evidence_json.as_ref() {
                    capture.notes.push(format!(
                        "before_response:verification_state_evidence_json:{value}"
                    ));
                }
                if source_request_missing {
                    capture
                        .notes
                        .push("before_response:verification_cite_required:true".to_string());
                    capture.notes.push(
                        "before_response:verification_followup_note:user_requested_source_missing"
                            .to_string(),
                    );
                }
                let compound_lookup_followup_requested = event
                    .user_input
                    .as_deref()
                    .is_some_and(query_requests_followup_execution_after_lookup);
                let requires_followup = latest_orchestration
                    .as_ref()
                    .and_then(|orchestration| orchestration.get("requires_followup"))
                    .is_some_and(|value| value == "true");
                if compound_lookup_followup_requested && requires_followup {
                    capture.notes.push(
                        "before_response:verification_followup_note:compound_lookup_followup_preserve_full_request"
                            .to_string(),
                    );
                    capture.notes.push(
                        "before_response:verification_followup_note:If the user asked to search and then do something with the result, preserve the full original request in any later tool_search or delegate step. Do not narrow the task down to only the lookup fragment."
                            .to_string(),
                    );
                }
                capture.notes.push(format!(
                    "before_response:verification_last_tool:{latest_tool}"
                ));
                if !tools_csv.is_empty() {
                    capture
                        .notes
                        .push(format!("before_response:verification_tools:{tools_csv}"));
                }
                for key in [
                    "source_count",
                    "execution_evidence_count",
                    "state_evidence_count",
                    "note_count",
                ] {
                    if let Some(value) = latest.get(key) {
                        capture.notes.push(format!(
                            "before_response:verification_{key}:{value}"
                        ));
                    }
                }
                capture.notes.push(format!(
                    "before_response:verification_surface:tools={}:count={}:latest_tool={latest_tool}:complete={complete}",
                    tools_csv,
                    tool_names.len()
                ));
                HookResult::Continue
            },
        )
        .with_priority(30),
    ));

    let dangling_capture = runtime_hook_capture.clone();
    engine.register(Arc::new(
        FnHook::new(
            "runtime_dangling_tool_call_repair",
            vec![HookTiming::BeforeResponse],
            move |event| {
                let dangling_count = event
                    .metadata
                    .get("dangling_tool_call_count")
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or(0);
                if dangling_count == 0 {
                    return HookResult::Continue;
                }

                let dangling_ids = event
                    .metadata
                    .get("dangling_tool_call_ids")
                    .cloned()
                    .unwrap_or_else(|| "unknown".to_string());

                let mut capture = dangling_capture.write();
                capture.dangling_tool_call_count = capture
                    .dangling_tool_call_count
                    .saturating_add(dangling_count as u32);
                capture.notes.push(format!(
                    "dangling_tool_call_repair:count={dangling_count}:ids={dangling_ids}"
                ));

                let repair_notice =
                    build_dangling_tool_call_repair_notice(dangling_count, &dangling_ids)
                        .expect("dangling notice should exist when count > 0");
                match apply_response_downgrade_notices(
                    event.llm_response.as_ref(),
                    None,
                    Some(repair_notice),
                ) {
                    Some(modified) => HookResult::Modify(modified),
                    None => HookResult::Continue,
                }
            },
        )
        .with_priority(28),
    ));

    let truth_response_capture = runtime_hook_capture.clone();
    engine.register(Arc::new(
        FnHook::new(
            "runtime_truth_verification_response_downgrade",
            vec![HookTiming::BeforeResponse],
            move |event| {
                let policy = TruthVerificationPolicyEngine::default();
                let latest = {
                    let capture = truth_response_capture.read();
                    latest_verification_preview_fields(&capture)
                };
                let latest_followup = {
                    let capture = truth_response_capture.read();
                    latest_verification_followup_fields(&capture)
                };
                let latest_sources = {
                    let capture = truth_response_capture.read();
                    latest_verification_sources(&capture)
                };
                let latest_execution_evidence = {
                    let capture = truth_response_capture.read();
                    latest_verification_string_array(
                        &capture,
                        "after_tool:verification_execution_evidence_json:",
                    )
                };
                let latest_state_evidence = {
                    let capture = truth_response_capture.read();
                    latest_verification_string_array(
                        &capture,
                        "after_tool:verification_state_evidence_json:",
                    )
                };
                let latest_tool_error = {
                    let capture = truth_response_capture.read();
                    latest_tool_error_observation(&capture)
                };

                if latest.is_none() && latest_tool_error.is_none() {
                    return HookResult::Continue;
                }

                let source_request_missing = latest.as_ref().is_some_and(|latest| {
                    policy.explicit_source_request_still_missing(
                        event.user_input.as_deref(),
                        latest,
                        latest_followup.as_ref(),
                    )
                });
                let prefers_chinese = text_prefers_chinese(event.user_input.as_deref());
                let downgrade_notice = latest.as_ref().and_then(|latest| {
                    let base = policy.build_downgrade_notice_for_language(
                        latest,
                        latest_followup.as_ref(),
                        prefers_chinese,
                    );
                    let requested_source_notice = source_request_missing
                        .then(|| policy.build_requested_source_missing_notice());
                    combine_downgrade_notices(base, requested_source_notice)
                });
                let tool_error_notice = latest_tool_error.as_ref().map(|observation| {
                    build_tool_error_downgrade_notice_for_language(observation, prefers_chinese)
                });
                let combined_notice =
                    combine_downgrade_notices(downgrade_notice, tool_error_notice);
                let dangling_count = event
                    .metadata
                    .get("dangling_tool_call_count")
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or(0);
                let dangling_ids = event
                    .metadata
                    .get("dangling_tool_call_ids")
                    .cloned()
                    .unwrap_or_else(|| "unknown".to_string());
                let dangling_notice =
                    build_dangling_tool_call_repair_notice(dangling_count, &dangling_ids);
                let downgraded_response = apply_response_downgrade_notices(
                    event.llm_response.as_ref(),
                    combined_notice,
                    dangling_notice,
                );
                let source_appendix = latest.as_ref().and_then(|latest| {
                    policy.build_evidence_attachment_appendix(
                        latest,
                        latest_followup.as_ref(),
                        &latest_sources,
                        &latest_execution_evidence,
                        &latest_state_evidence,
                    )
                });

                match apply_response_source_appendix(
                    downgraded_response.as_ref().or(event.llm_response.as_ref()),
                    source_appendix,
                ) {
                    Some(modified) => HookResult::Modify(modified),
                    None => match downgraded_response {
                        Some(modified) => HookResult::Modify(modified),
                        None => HookResult::Continue,
                    },
                }
            },
        )
        .with_priority(29),
    ));

    let evaluation_capture = runtime_hook_capture.clone();
    engine.register(Arc::new(
        FnHook::new(
            "runtime_post_run_evaluation_tap",
            vec![HookTiming::BeforeResponse],
            move |event| {
                if let Some(summary) = event.metadata.get("post_run_summary") {
                    let mut capture = evaluation_capture.write();
                    capture.post_run_tap_count = capture.post_run_tap_count.saturating_add(1);
                    capture.summarization_surface_count =
                        capture.summarization_surface_count.saturating_add(1);
                    capture.notes.push(format!("post_run_eval:{summary}"));
                }
                HookResult::Continue
            },
        )
        .with_priority(31),
    ));

    specs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::HookEvent;

    #[test]
    fn tool_error_normalization_preserves_detailed_worker_failures() {
        let error = "worker `researcher` returned a runtime failure instead of completed delegated output: browser_browse timed out after a usable diagnostic was captured";

        let normalized = normalize_runtime_tool_error_message("delegate", error);

        assert!(normalized.contains("worker `researcher` returned a runtime failure"));
        assert!(normalized.contains("browser_browse timed out"));
        assert!(!normalized.contains("execution timed out before a usable result"));
    }

    #[tokio::test]
    async fn installs_default_runtime_middlewares_and_records_llm_taps() {
        let mut engine = HookEngine::new();
        let capture = Arc::new(RwLock::new(RuntimeHookCapture::default()));
        let specs = install_default_runtime_middlewares(&mut engine, capture.clone());

        assert!(specs
            .iter()
            .any(|spec| spec.name == "runtime_pre_llm_surface"));
        assert!(specs
            .iter()
            .any(|spec| spec.name == "runtime_memory_owner_surface"));
        assert!(specs
            .iter()
            .any(|spec| spec.name == "runtime_post_llm_surface"));
        assert!(specs
            .iter()
            .any(|spec| spec.name == "runtime_media_preprocess_surface"));
        assert!(specs
            .iter()
            .any(|spec| spec.name == "runtime_clarification_surface"));
        assert!(specs
            .iter()
            .any(|spec| spec.name == "runtime_subagent_budget_surface"));
        assert!(specs
            .iter()
            .any(|spec| spec.name == "runtime_title_surface"));
        assert!(specs
            .iter()
            .any(|spec| spec.name == "runtime_dangling_tool_call_repair"));

        let mut before = HookEvent::new(HookTiming::BeforeLlm);
        before.metadata.insert(
            "matched_skill_manual".to_string(),
            "python_tooling".to_string(),
        );
        before
            .metadata
            .insert("visible_owner".to_string(), "benshu".to_string());
        before
            .metadata
            .insert("memory_owner".to_string(), "benshu".to_string());
        before
            .metadata
            .insert("approval_owner".to_string(), "benshu".to_string());
        before
            .metadata
            .insert("skill_manual_gate_active".to_string(), "true".to_string());
        let _ = engine.fire(&before).await;

        let mut after = HookEvent::new(HookTiming::AfterLlm).with_llm_response("done");
        after
            .metadata
            .insert("finish_reason".to_string(), "stop".to_string());
        let _ = engine.fire(&after).await;

        let snapshot = capture.read().clone();
        assert_eq!(snapshot.pre_llm_tap_count, 1);
        assert_eq!(snapshot.post_llm_tap_count, 1);
        assert!(snapshot
            .notes
            .iter()
            .any(|note| note.contains("before_llm:skill_manual:python_tooling")));
        assert!(snapshot
            .notes
            .iter()
            .any(|note| note == "before_llm:skill_manual_gate_active"));
        assert!(snapshot.notes.iter().any(|note| {
            note.contains("before_llm:ownership:visible=benshu:memory=benshu:approval=benshu")
        }));
        assert!(snapshot
            .notes
            .iter()
            .any(|note| note.contains("after_llm:finish:stop")));
    }

    #[tokio::test]
    async fn default_runtime_middlewares_record_skill_manual_reads_and_tool_errors() {
        let mut engine = HookEngine::new();
        let capture = Arc::new(RwLock::new(RuntimeHookCapture::default()));
        install_default_runtime_middlewares(&mut engine, capture.clone());

        let mut manual_event = HookEvent::new(HookTiming::AfterToolCall)
            .with_tool("read_skill_manual", r#"{"skill_name":"python_tooling"}"#)
            .with_tool_result("manual content");
        manual_event
            .metadata
            .insert("skill_name".to_string(), "python_tooling".to_string());
        let _ = engine.fire(&manual_event).await;

        let error_event = HookEvent::new(HookTiming::OnError)
            .with_tool("web_search", r#"{"query":"btc"}"#)
            .with_error("network timeout");
        let hook_result = engine.fire(&error_event).await;

        let snapshot = capture.read().clone();
        assert_eq!(snapshot.skill_manual_read_count, 1);
        assert_eq!(snapshot.tool_error_count, 1);
        match hook_result {
            HookResult::Modify(value) => {
                assert!(value.contains("Runtime tool error in `web_search`"));
            }
            other => panic!("expected HookResult::Modify, got {other:?}"),
        }
        assert!(snapshot
            .notes
            .iter()
            .any(|note| note.contains("skill_manual_read:python_tooling")));
        assert!(snapshot
            .notes
            .iter()
            .any(|note| note.contains("tool_error:web_search:network timeout")));
        assert!(snapshot
            .notes
            .iter()
            .any(|note| note == "tool_error_surface:web_search"));
    }

    #[tokio::test]
    async fn default_runtime_middlewares_record_skill_surface_contract() {
        let mut engine = HookEngine::new();
        let capture = Arc::new(RwLock::new(RuntimeHookCapture::default()));
        install_default_runtime_middlewares(&mut engine, capture.clone());

        let mut manual_event = HookEvent::new(HookTiming::AfterToolCall)
            .with_tool("read_skill_manual", r#"{"skill_name":"python_tooling"}"#)
            .with_tool_result(
                "# Skill: python_tooling\n\n## Skill Surface Contract\n\n- classification: executable\n- tool_surface: skill_loading\n- execution_surface: runtime\n- runtime: uv\n- kind: tool\n",
            );
        manual_event
            .metadata
            .insert("skill_name".to_string(), "python_tooling".to_string());
        let _ = engine.fire(&manual_event).await;

        let snapshot = capture.read().clone();
        assert!(snapshot
            .notes
            .iter()
            .any(|note| note == "skill_surface_classification:python_tooling:executable"));
        assert!(snapshot
            .notes
            .iter()
            .any(|note| note == "skill_surface_execution:python_tooling:runtime"));
        assert!(snapshot
            .notes
            .iter()
            .any(|note| note == "skill_surface_runtime:python_tooling:uv"));
        assert!(snapshot
            .notes
            .iter()
            .any(|note| note == "skill_surface_kind:python_tooling:tool"));
    }

    #[tokio::test]
    async fn default_runtime_middlewares_record_skill_asset_reads() {
        let mut engine = HookEngine::new();
        let capture = Arc::new(RwLock::new(RuntimeHookCapture::default()));
        install_default_runtime_middlewares(&mut engine, capture.clone());

        let asset_event = HookEvent::new(HookTiming::AfterToolCall)
            .with_tool(
                "read_skill_asset",
                r#"{"skill_name":"python_tooling","asset_path":"references/setup.md"}"#,
            )
            .with_tool_result("asset content");
        let _ = engine.fire(&asset_event).await;

        let snapshot = capture.read().clone();
        assert_eq!(snapshot.skill_asset_read_count, 1);
        assert!(snapshot
            .notes
            .iter()
            .any(|note| note.contains("skill_asset_read:python_tooling:references/setup.md")));
    }

    #[tokio::test]
    async fn default_runtime_middlewares_record_skill_asset_followup_tools() {
        let mut engine = HookEngine::new();
        let capture = Arc::new(RwLock::new(RuntimeHookCapture::default()));
        install_default_runtime_middlewares(&mut engine, capture.clone());

        let asset_event = HookEvent::new(HookTiming::AfterToolCall)
            .with_tool(
                "read_skill_asset",
                r#"{"skill_name":"python_tooling","asset_path":"references/setup.md"}"#,
            )
            .with_tool_result("asset content");
        let _ = engine.fire(&asset_event).await;

        let followup_event = HookEvent::new(HookTiming::AfterToolCall)
            .with_tool("shell", r#"{"command":"python scripts/run.py"}"#)
            .with_tool_result("ok");
        let _ = engine.fire(&followup_event).await;

        let snapshot = capture.read().clone();
        assert!(snapshot.notes.iter().any(|note| {
            note.contains("skill_asset_followup:python_tooling:references/setup.md:shell")
        }));
        assert!(snapshot.notes.iter().any(|note| {
            note.contains(
                "skill_asset_execution_surface:python_tooling:references/setup.md:runtime:shell",
            )
        }));
    }

    #[tokio::test]
    async fn default_runtime_middlewares_record_media_preprocess_outputs() {
        let mut engine = HookEngine::new();
        let capture = Arc::new(RwLock::new(RuntimeHookCapture::default()));
        install_default_runtime_middlewares(&mut engine, capture.clone());

        let media_event = HookEvent::new(HookTiming::AfterToolCall)
            .with_tool(
                "normalize_audio",
                r#"{"path":"/tmp/input.wav","output_path":"/tmp/output.wav"}"#,
            )
            .with_tool_result(
                serde_json::json!({
                    "status": "ok",
                    "tool": "normalize_audio",
                    "path": "/tmp/input.wav",
                    "sample_rate": 16000,
                    "channels": 1,
                    "output_path": "/tmp/output.wav",
                    "cleanup": { "active": false }
                })
                .to_string(),
            );
        let _ = engine.fire(&media_event).await;

        let snapshot = capture.read().clone();
        assert_eq!(snapshot.media_surface_count, 1);
        assert!(snapshot
            .notes
            .iter()
            .any(|note| note == "media_preprocess_tool:normalize_audio"));
        assert!(snapshot
            .notes
            .iter()
            .any(|note| note == "media_preprocess_status:normalize_audio:ok"));
        assert!(snapshot
            .notes
            .iter()
            .any(|note| note == "media_preprocess_kind:normalize_audio:audio"));
        assert!(snapshot
            .notes
            .iter()
            .any(|note| note == "media_preprocess_engine:normalize_audio:ffmpeg"));
        assert!(snapshot
            .notes
            .iter()
            .any(|note| note == "media_preprocess_output:normalize_audio:file:/tmp/output.wav"));
    }

    #[tokio::test]
    async fn default_runtime_middlewares_record_text_extract_media_contract() {
        let mut engine = HookEngine::new();
        let capture = Arc::new(RwLock::new(RuntimeHookCapture::default()));
        install_default_runtime_middlewares(&mut engine, capture.clone());

        let media_event = HookEvent::new(HookTiming::AfterToolCall)
            .with_tool(
                "text_extract",
                r#"{"action":"recognize","path":"/tmp/screenshot.png"}"#,
            )
            .with_tool_result(
                serde_json::json!({
                    "status": "error",
                    "input_kind": "image",
                    "goal": "extract_text",
                    "route": "ocr_backend",
                    "path": "/tmp/screenshot.png",
                    "media_preprocess_route": "image_page_raster",
                    "media_preprocess_source_kind": "direct_image",
                    "media_preprocess_source_ref": "/tmp/screenshot.png",
                    "media_pipeline_outcome": "model_failed_after_preprocess",
                    "backend": "Null OCR",
                    "error": "OCR failed"
                })
                .to_string(),
            );
        let _ = engine.fire(&media_event).await;

        let snapshot = capture.read().clone();
        assert!(snapshot.notes.iter().any(|note| {
            note == "media_preprocess_outcome:image_page_raster:model_failed_after_preprocess"
        }));
        assert!(snapshot
            .notes
            .iter()
            .any(|note| { note == "media_preprocess_model_failed:image_page_raster" }));
        assert!(snapshot
            .notes
            .iter()
            .any(|note| { note == "media_preprocess_source_kind:image_page_raster:direct_image" }));
        assert!(snapshot.notes.iter().any(|note| {
            note == "media_preprocess_source_ref:image_page_raster:/tmp/screenshot.png"
        }));
    }

    #[tokio::test]
    async fn default_runtime_middlewares_record_pdf_parse_page_source_contract() {
        let mut engine = HookEngine::new();
        let capture = Arc::new(RwLock::new(RuntimeHookCapture::default()));
        install_default_runtime_middlewares(&mut engine, capture.clone());

        let media_event = HookEvent::new(HookTiming::AfterToolCall)
            .with_tool("pdf_parse", r#"{"path":"/tmp/doc.pdf","mode":"auto"}"#)
            .with_tool_result(
                serde_json::json!({
                    "status": "ok",
                    "route": "pdf_parse_tool",
                    "page_routes": [
                        {
                            "page_number": 3,
                            "selected_route": "page_image_ocr",
                            "source_contract_kind": "pdf_page_image",
                            "source_contract_ref": "pdf_page:3"
                        }
                    ]
                })
                .to_string(),
            );
        let _ = engine.fire(&media_event).await;

        let snapshot = capture.read().clone();
        assert!(snapshot.notes.iter().any(|note| {
            note == "media_preprocess_source_kind:pdf_parse_tool:page_image_ocr:pdf_page_image"
        }));
        assert!(snapshot.notes.iter().any(|note| {
            note == "media_preprocess_source_ref:pdf_parse_tool:page_image_ocr:pdf_page:3"
        }));
    }

    #[tokio::test]
    async fn default_runtime_middlewares_record_video_frame_source_contract() {
        let mut engine = HookEngine::new();
        let capture = Arc::new(RwLock::new(RuntimeHookCapture::default()));
        install_default_runtime_middlewares(&mut engine, capture.clone());

        let media_event = HookEvent::new(HookTiming::AfterToolCall)
            .with_tool(
                "document_understand",
                r#"{"path":"/tmp/demo.mp4","goal":"extract_text"}"#,
            )
            .with_tool_result(
                serde_json::json!({
                    "status": "error",
                    "input_kind": "video",
                    "goal": "extract_text",
                    "route": "media_runtime_video_frames_ocr",
                    "path": "/tmp/demo.mp4",
                    "frame_count": 2,
                    "frame_source_contracts": [
                        {
                            "source_contract_kind": "video_frame_image",
                            "source_contract_ref": "video_frame:1"
                        },
                        {
                            "source_contract_kind": "video_frame_image",
                            "source_contract_ref": "video_frame:2"
                        }
                    ],
                    "media_preprocess_route": "extract_video_frames",
                    "media_pipeline_outcome": "model_failed_after_preprocess",
                    "error": "OCR failed"
                })
                .to_string(),
            );
        let _ = engine.fire(&media_event).await;

        let snapshot = capture.read().clone();
        assert!(snapshot.notes.iter().any(|note| {
            note == "media_preprocess_source_kind:extract_video_frames:video_frame_image"
        }));
        assert!(snapshot.notes.iter().any(|note| {
            note == "media_preprocess_source_ref:extract_video_frames:video_frame:1"
        }));
        assert!(snapshot.notes.iter().any(|note| {
            note == "media_preprocess_source_ref:extract_video_frames:video_frame:2"
        }));
    }

    #[tokio::test]
    async fn default_runtime_middlewares_surface_deferred_tool_filter() {
        let mut engine = HookEngine::new();
        let capture = Arc::new(RwLock::new(RuntimeHookCapture::default()));
        install_default_runtime_middlewares(&mut engine, capture.clone());

        let mut before = HookEvent::new(HookTiming::BeforeLlm);
        before
            .metadata
            .insert("requested_tool_count".to_string(), "6".to_string());
        before
            .metadata
            .insert("total_tool_count".to_string(), "14".to_string());
        before
            .metadata
            .insert("deferred_tool_count".to_string(), "8".to_string());
        let hook_result = engine.fire(&before).await;

        match hook_result {
            HookResult::Modify(value) => {
                assert!(value.contains("Long-tail tools remain deferred"));
                assert!(value.contains("tool_search"));
            }
            other => panic!("expected HookResult::Modify, got {other:?}"),
        }
        let snapshot = capture.read().clone();
        assert!(snapshot
            .notes
            .iter()
            .any(|note| note.contains("before_llm:deferred_tool_filter:6/14:deferred=8")));
    }

    #[tokio::test]
    async fn default_runtime_middlewares_repair_dangling_tool_calls_before_response() {
        let mut engine = HookEngine::new();
        let capture = Arc::new(RwLock::new(RuntimeHookCapture::default()));
        install_default_runtime_middlewares(&mut engine, capture.clone());

        let mut before = HookEvent::new(HookTiming::BeforeResponse)
            .with_llm_response("Partial answer based on unfinished execution.");
        before
            .metadata
            .insert("dangling_tool_call_count".to_string(), "1".to_string());
        before
            .metadata
            .insert("dangling_tool_call_ids".to_string(), "call_1".to_string());

        let hook_result = engine.fire(&before).await;
        let snapshot = capture.read().clone();
        assert_eq!(snapshot.dangling_tool_call_count, 1);
        assert!(snapshot
            .notes
            .iter()
            .any(|note| note == "dangling_tool_call_repair:count=1:ids=call_1"));
        match hook_result {
            HookResult::Modify(value) => {
                assert!(value.contains("Runtime notice: 1 planned tool call(s)"));
                assert!(value.contains("call_1"));
            }
            other => panic!("expected HookResult::Modify, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn default_runtime_middlewares_surface_subagent_budgets_and_summary() {
        let mut engine = HookEngine::new();
        let capture = Arc::new(RwLock::new(RuntimeHookCapture::default()));
        install_default_runtime_middlewares(&mut engine, capture.clone());

        let mut before = HookEvent::new(HookTiming::BeforeResponse).with_llm_response("done");
        before
            .metadata
            .insert("delegation_present".to_string(), "false".to_string());
        before
            .metadata
            .insert("handover_present".to_string(), "false".to_string());
        before
            .metadata
            .insert("max_parallel_tools".to_string(), "4".to_string());
        before
            .metadata
            .insert("session_title_present".to_string(), "true".to_string());
        before.metadata.insert(
            "session_title_source".to_string(),
            "extra_params.session_title".to_string(),
        );
        before
            .metadata
            .insert("session_title".to_string(), "BTC dashboard".to_string());
        before.metadata.insert(
            "post_run_summary".to_string(),
            "thoughts=1,tool_calls=0".to_string(),
        );

        let _ = engine.fire(&before).await;
        let snapshot = capture.read().clone();
        assert_eq!(snapshot.subagent_surface_count, 1);
        assert_eq!(snapshot.title_surface_count, 1);
        assert_eq!(snapshot.summarization_surface_count, 1);
        assert!(snapshot.notes.iter().any(|note| {
            note.contains(
                "before_response:subagent_budget:delegation=false:handover=false:parallel_tools=4",
            )
        }));
        assert!(snapshot.notes.iter().any(|note| {
            note.contains(
                "before_response:title:present=true:source=extra_params.session_title:value=BTC dashboard",
            )
        }));
        assert!(snapshot.notes.iter().any(|note| {
            note.contains(
                "before_response:memory_session_surface:visible=unknown:memory=unknown:approval=unknown:title_present=true:title_source=extra_params.session_title:summary_present=true",
            )
        }));
        assert!(snapshot
            .notes
            .iter()
            .any(|note| note.contains("post_run_eval:thoughts=1,tool_calls=0")));
    }

    #[tokio::test]
    async fn default_runtime_middlewares_surface_clarification_contract_before_response() {
        let mut engine = HookEngine::new();
        let capture = Arc::new(RwLock::new(RuntimeHookCapture::default()));
        install_default_runtime_middlewares(&mut engine, capture.clone());

        let mut before = HookEvent::new(HookTiming::BeforeResponse).with_llm_response("waiting");
        before.metadata.insert(
            "session_status".to_string(),
            "awaiting_clarification".to_string(),
        );
        before.metadata.insert(
            "clarification_status_kind".to_string(),
            "awaiting_clarification".to_string(),
        );
        before
            .metadata
            .insert("clarification_event".to_string(), "awaiting".to_string());
        before.metadata.insert(
            "clarification_prompt".to_string(),
            "你想查哪个城市的天气？".to_string(),
        );
        before.metadata.insert(
            "clarification_original_request".to_string(),
            "帮我查天气".to_string(),
        );
        before.metadata.insert(
            "clarification_session_status_json_valid".to_string(),
            "true".to_string(),
        );

        let _ = engine.fire(&before).await;
        let snapshot = capture.read().clone();
        assert_eq!(snapshot.clarification_surface_count, 1);
        assert!(snapshot.notes.iter().any(|note| {
            note.contains(
                "before_response:clarification_surface:status=awaiting_clarification:event=awaiting:prompt_present=true:original_present=true:json_valid=true",
            )
        }));
    }

    #[tokio::test]
    async fn default_runtime_middlewares_capture_tool_verification_before_response() {
        let mut engine = HookEngine::new();
        let capture = Arc::new(RwLock::new(RuntimeHookCapture::default()));
        install_default_runtime_middlewares(&mut engine, capture.clone());

        let after_tool = HookEvent::new(HookTiming::AfterToolCall)
            .with_tool(
                "web_search",
                r#"{"query":"latest amd news","structured":true}"#,
            )
            .with_tool_result(
                r#"{
                    "kind":"web_search",
                    "verification_preview":{
                        "domain":"KnowledgeFact",
                        "requirement":"Required",
                        "mode":"WebSearchFetch",
                        "outcome":"VerificationSucceeded",
                        "truth_status":"Verified",
                        "source_posture":"SourcesAttached",
                        "sources":[{"kind":"web","title":"Example","uri":"https://example.com"}],
                        "execution_evidence":[],
                        "state_evidence":[],
                        "notes":["verified through search"]
                    },
                    "verification_followup":{
                        "answer_readiness":"search_results_only",
                        "next_tools":["web_fetch"],
                        "cite_required":true,
                        "note":"Search results were observed, but source pages were not fetched yet."
                    }
                }"#,
            );
        let _ = engine.fire(&after_tool).await;

        let before = HookEvent::new(HookTiming::BeforeResponse).with_llm_response("done");
        let _ = engine.fire(&before).await;

        let snapshot = capture.read().clone();
        assert!(snapshot.notes.iter().any(|note| {
            note.contains(
                "after_tool:verification_preview:tool=web_search:domain=KnowledgeFact:requirement=Required:mode=WebSearchFetch:outcome=VerificationSucceeded:truth_status=Verified:source_posture=SourcesAttached:source_count=1",
            )
        }));
        assert!(snapshot.notes.iter().any(|note| {
            note.contains(
                "after_tool:verification_followup:answer_readiness=search_results_only:next_tools=web_fetch:cite_required=true",
            )
        }));
        assert!(snapshot
            .notes
            .iter()
            .any(|note| note == "before_response:truth_status:Verified"));
        assert!(snapshot
            .notes
            .iter()
            .any(|note| note == "before_response:verification_domain:KnowledgeFact"));
        assert!(snapshot
            .notes
            .iter()
            .any(|note| note == "before_response:verification_mode:WebSearchFetch"));
        assert!(snapshot
            .notes
            .iter()
            .any(|note| note == "before_response:verification_outcome:VerificationSucceeded"));
        assert!(snapshot.notes.iter().any(
            |note| note == "before_response:verification_answer_readiness:search_results_only"
        ));
        assert!(snapshot
            .notes
            .iter()
            .any(|note| note == "before_response:verification_next_tools:web_fetch"));
        assert!(snapshot
            .notes
            .iter()
            .any(|note| note == "before_response:verification_cite_required:true"));
        assert!(snapshot
            .notes
            .iter()
            .any(|note| note == "before_response:source_posture:SourcesAttached"));
        assert!(snapshot
            .notes
            .iter()
            .any(|note| note == "before_response:verification_last_tool:web_search"));
        assert!(snapshot.notes.iter().any(|note| {
            note == "before_response:verification_surface:tools=web_search:count=1:latest_tool=web_search:complete=true"
        }));
    }

    #[tokio::test]
    async fn default_runtime_middlewares_downgrade_unverified_or_execution_missing_response() {
        let mut engine = HookEngine::new();
        let capture = Arc::new(RwLock::new(RuntimeHookCapture::default()));
        install_default_runtime_middlewares(&mut engine, capture);

        let after_tool = HookEvent::new(HookTiming::AfterToolCall)
            .with_tool(
                "runtime_surface",
                r#"{"action":"ensure","runtime":"python","structured":true}"#,
            )
            .with_tool_result(
                r#"{
                    "kind":"runtime_surface",
                    "verification_preview":{
                        "domain":"ExecutionFact",
                        "requirement":"Required",
                        "mode":"ExecutionResultCheck",
                        "outcome":"VerificationExecutionMissing",
                        "truth_status":"Unverified",
                        "source_posture":"SourcesRequiredButMissing",
                        "sources":[],
                        "execution_evidence":[],
                        "state_evidence":[],
                        "notes":["runtime execution still pending"]
                    }
                }"#,
            );
        let _ = engine.fire(&after_tool).await;

        let before = HookEvent::new(HookTiming::BeforeResponse)
            .with_llm_response("Python runtime is ready.")
            .with_user_input("帮我确认 python runtime 是否已经准备好");
        let result = engine.fire(&before).await;

        match result {
            HookResult::Modify(value) => {
                assert!(value.contains("Python runtime is ready."));
                assert!(value.contains("Verification notice:"));
                assert!(value.contains("still unverified"));
                assert!(value.contains("planned execution"));
                assert!(value.contains("required supporting sources are missing"));
            }
            other => panic!("expected modified downgraded response, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn default_runtime_middlewares_downgrade_source_missing_external_fact_response() {
        let mut engine = HookEngine::new();
        let capture = Arc::new(RwLock::new(RuntimeHookCapture::default()));
        install_default_runtime_middlewares(&mut engine, capture);

        let after_tool = HookEvent::new(HookTiming::AfterToolCall)
            .with_tool(
                "web_search",
                r#"{"query":"latest AMD revenue guidance","structured":true}"#,
            )
            .with_tool_result(
                r#"{
                    "kind":"web_search",
                    "verification_preview":{
                        "domain":"KnowledgeFact",
                        "requirement":"Required",
                        "mode":"WebSearchFetch",
                        "outcome":"VerificationSourceInsufficient",
                        "truth_status":"Unverified",
                        "source_posture":"SourcesReferencedButNotAttached",
                        "sources":[],
                        "execution_evidence":[],
                        "state_evidence":[],
                        "notes":["search completed but no attached sources"]
                    }
                }"#,
            );
        let _ = engine.fire(&after_tool).await;

        let before = HookEvent::new(HookTiming::BeforeResponse)
            .with_llm_response("AMD just raised its revenue guidance.")
            .with_user_input("AMD 最新营收指引是什么");
        let result = engine.fire(&before).await;

        match result {
            HookResult::Modify(value) => {
                assert!(value.contains("AMD just raised its revenue guidance."));
                assert!(value.contains("Verification notice:"));
                assert!(value.contains("still unverified"));
                assert!(value.contains("sources were referenced but not actually attached"));
                assert!(value.contains("insufficient to support a confident answer"));
            }
            other => panic!("expected modified downgraded response, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn default_runtime_middlewares_downgrade_latest_info_without_completed_verification() {
        let mut engine = HookEngine::new();
        let capture = Arc::new(RwLock::new(RuntimeHookCapture::default()));
        install_default_runtime_middlewares(&mut engine, capture);

        let after_tool = HookEvent::new(HookTiming::AfterToolCall)
            .with_tool(
                "tool_search",
                r#"{"query":"今天 OpenAI 最新新闻是什么","structured":true}"#,
            )
            .with_tool_result(
                r#"{
                    "kind":"tool_search",
                    "verification_preview":{
                        "domain":"KnowledgeFact",
                        "requirement":"Required",
                        "mode":"RealtimeLookup",
                        "outcome":"VerificationSkippedByPolicyGap",
                        "truth_status":"Uncertain",
                        "source_posture":"SourcesRequiredButMissing",
                        "sources":[],
                        "execution_evidence":[],
                        "state_evidence":[],
                        "notes":["latest info lookup is still pending"]
                    }
                }"#,
            );
        let _ = engine.fire(&after_tool).await;

        let before = HookEvent::new(HookTiming::BeforeResponse)
            .with_llm_response("OpenAI 今天刚发布了新的旗舰模型。")
            .with_user_input("今天 OpenAI 最新新闻是什么");
        let result = engine.fire(&before).await;

        match result {
            HookResult::Modify(value) => {
                assert!(value.contains("OpenAI 今天刚发布了新的旗舰模型。"));
                assert!(value.contains("Verification notice:"));
                assert!(value.contains("required supporting sources are missing"));
                assert!(value.contains("runtime lacks a required verification path"));
                assert!(value.contains("tentative until verification is completed"));
            }
            other => panic!("expected modified downgraded response, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn default_runtime_middlewares_downgrade_tool_unavailable_response() {
        let mut engine = HookEngine::new();
        let capture = Arc::new(RwLock::new(RuntimeHookCapture::default()));
        install_default_runtime_middlewares(&mut engine, capture);

        let after_tool = HookEvent::new(HookTiming::AfterToolCall)
            .with_tool(
                "tool_search",
                r#"{"query":"帮我确认有没有 ffmpeg 这个 cli 工具","structured":true}"#,
            )
            .with_tool_result(
                r#"{
                    "kind":"tool_search",
                    "verification_preview":{
                        "domain":"ToolFact",
                        "requirement":"Required",
                        "mode":"ToolInventoryCheck",
                        "outcome":"VerificationToolUnavailable",
                        "truth_status":"Unverified",
                        "source_posture":"NoSourcesRequired",
                        "sources":[],
                        "execution_evidence":[],
                        "state_evidence":[],
                        "notes":["tool inventory surface is unavailable"]
                    }
                }"#,
            );
        let _ = engine.fire(&after_tool).await;

        let before = HookEvent::new(HookTiming::BeforeResponse)
            .with_llm_response("ffmpeg 已经可以直接调用了。")
            .with_user_input("帮我确认有没有 ffmpeg 这个 cli 工具");
        let result = engine.fire(&before).await;

        match result {
            HookResult::Modify(value) => {
                assert!(value.contains("ffmpeg 已经可以直接调用了。"));
                assert!(value.contains("Verification notice:"));
                assert!(value.contains("still unverified"));
                assert!(value.contains("verification tool was not available"));
            }
            other => panic!("expected modified downgraded response, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn default_runtime_middlewares_downgrade_high_risk_advice_without_completed_verification()
    {
        let mut engine = HookEngine::new();
        let capture = Arc::new(RwLock::new(RuntimeHookCapture::default()));
        install_default_runtime_middlewares(&mut engine, capture);

        let after_tool = HookEvent::new(HookTiming::AfterToolCall)
            .with_tool(
                "tool_search",
                r#"{"query":"我现在胸口疼要不要立刻吃药","structured":true}"#,
            )
            .with_tool_result(
                r#"{
                    "kind":"tool_search",
                    "verification_preview":{
                        "domain":"KnowledgeFact",
                        "requirement":"Required",
                        "mode":"WebSearchFetch",
                        "outcome":"VerificationSkippedByPolicyGap",
                        "truth_status":"Uncertain",
                        "source_posture":"SourcesRequiredButMissing",
                        "sources":[],
                        "execution_evidence":[],
                        "state_evidence":[],
                        "notes":["high-risk advice still requires verification"]
                    }
                }"#,
            );
        let _ = engine.fire(&after_tool).await;

        let before = HookEvent::new(HookTiming::BeforeResponse)
            .with_llm_response("你可以先立刻吃药观察。")
            .with_user_input("我现在胸口疼要不要立刻吃药");
        let result = engine.fire(&before).await;

        match result {
            HookResult::Modify(value) => {
                assert!(value.contains("你可以先立刻吃药观察。"));
                assert!(value.contains("Verification notice:"));
                assert!(value.contains("remains uncertain"));
                assert!(value.contains("required supporting sources are missing"));
                assert!(value.contains("runtime lacks a required verification path"));
            }
            other => panic!("expected modified downgraded response, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn default_runtime_middlewares_downgrade_search_results_only_without_fetch() {
        let mut engine = HookEngine::new();
        let capture = Arc::new(RwLock::new(RuntimeHookCapture::default()));
        install_default_runtime_middlewares(&mut engine, capture);

        let after_tool = HookEvent::new(HookTiming::AfterToolCall)
            .with_tool(
                "web_search",
                r#"{"query":"latest release version","structured":true}"#,
            )
            .with_tool_result(
                r#"{
                    "kind":"web_search",
                    "verification_preview":{
                        "domain":"KnowledgeFact",
                        "requirement":"Required",
                        "mode":"WebSearchFetch",
                        "outcome":"VerificationSucceeded",
                        "truth_status":"Verified",
                        "source_posture":"SourcesAttached",
                        "sources":[{"kind":"web","title":"Example","uri":"https://example.com"}],
                        "execution_evidence":[],
                        "state_evidence":[],
                        "notes":["verified through search"]
                    },
                    "verification_followup":{
                        "answer_readiness":"search_results_only",
                        "next_tools":["web_fetch"],
                        "cite_required":true,
                        "note":"Search results were observed, but source pages were not fetched yet."
                    }
                }"#,
            );
        let _ = engine.fire(&after_tool).await;

        let before = HookEvent::new(HookTiming::BeforeResponse)
            .with_llm_response("The current release is definitely 1.2.3.")
            .with_user_input("latest release version");
        let result = engine.fire(&before).await;

        match result {
            HookResult::Modify(value) => {
                assert!(value.contains("The current release is definitely 1.2.3."));
                assert!(value.contains("Verification notice:"));
                assert!(value.contains("source pages were not fetched yet"));
                assert!(value.contains("tentative until verification is completed"));
                assert!(!value.contains("Sources:"));
            }
            other => panic!("expected modified downgraded response, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn default_runtime_middlewares_mark_source_missing_when_user_requests_source_but_only_search_results_exist(
    ) {
        let mut engine = HookEngine::new();
        let capture = Arc::new(RwLock::new(RuntimeHookCapture::default()));
        install_default_runtime_middlewares(&mut engine, capture.clone());

        let after_tool = HookEvent::new(HookTiming::AfterToolCall)
            .with_tool(
                "web_search",
                r#"{"query":"latest release version with source","structured":true}"#,
            )
            .with_tool_result(
                r#"{
                    "kind":"web_search",
                    "verification_preview":{
                        "domain":"KnowledgeFact",
                        "requirement":"Required",
                        "mode":"WebSearchFetch",
                        "outcome":"VerificationSucceeded",
                        "truth_status":"Verified",
                        "source_posture":"SourcesAttached",
                        "sources":[{"kind":"web","title":"Example","uri":"https://example.com"}],
                        "execution_evidence":[],
                        "state_evidence":[],
                        "notes":["verified through search"]
                    },
                    "verification_followup":{
                        "answer_readiness":"search_results_only",
                        "next_tools":["web_fetch"],
                        "cite_required":true,
                        "note":"Search results were observed, but source pages were not fetched yet."
                    }
                }"#,
            );
        let _ = engine.fire(&after_tool).await;

        let before = HookEvent::new(HookTiming::BeforeResponse)
            .with_llm_response("The current release is 1.2.3.")
            .with_user_input("latest release version, give me the source link");
        let result = engine.fire(&before).await;

        match result {
            HookResult::Modify(value) => {
                assert!(value.contains("The current release is 1.2.3."));
                assert!(value.contains("Verification notice:"));
                assert!(value.contains("requested a source or link"));
                assert!(value.contains("fetch or attach a supporting source"));
            }
            other => panic!("expected modified downgraded response, got {:?}", other),
        }

        let capture = capture.read();
        assert!(capture
            .notes
            .iter()
            .any(|note| note == "before_response:source_posture:SourcesRequiredButMissing"));
        assert!(capture
            .notes
            .iter()
            .any(|note| note == "before_response:verification_cite_required:true"));
    }

    #[tokio::test]
    async fn default_runtime_middlewares_keep_sources_satisfied_when_source_content_was_observed() {
        let mut engine = HookEngine::new();
        let capture = Arc::new(RwLock::new(RuntimeHookCapture::default()));
        install_default_runtime_middlewares(&mut engine, capture.clone());

        let after_tool = HookEvent::new(HookTiming::AfterToolCall)
            .with_tool(
                "web_fetch",
                r#"{"url":"https://example.com/release","structured":true}"#,
            )
            .with_tool_result(
                r#"{
                    "kind":"web_fetch",
                    "verification_preview":{
                        "domain":"KnowledgeFact",
                        "requirement":"Required",
                        "mode":"WebSearchFetch",
                        "outcome":"VerificationSucceeded",
                        "truth_status":"Verified",
                        "source_posture":"SourcesAttached",
                        "sources":[{"kind":"web","title":"Release notes","uri":"https://example.com/release"}],
                        "execution_evidence":[],
                        "state_evidence":[],
                        "notes":["source content observed directly"]
                    },
                    "verification_followup":{
                        "answer_readiness":"source_content_observed",
                        "next_tools":[],
                        "cite_required":true,
                        "note":"Source content has been observed directly."
                    }
                }"#,
            );
        let _ = engine.fire(&after_tool).await;

        let before = HookEvent::new(HookTiming::BeforeResponse)
            .with_llm_response("The current release is 1.2.3.")
            .with_user_input("latest release version, give me the source link");
        let result = engine.fire(&before).await;

        match result {
            HookResult::Modify(value) => {
                assert!(value.contains("The current release is 1.2.3."));
                assert!(value.contains("Sources:"));
                assert!(value.contains("Release notes"));
                assert!(value.contains("https://example.com/release"));
                assert!(!value.contains("requested a source or link"));
            }
            other => panic!("expected modified response with attached sources, got {other:?}"),
        }

        let capture = capture.read();
        assert!(capture
            .notes
            .iter()
            .any(|note| note == "before_response:source_posture:SourcesAttached"));
        assert!(capture.notes.iter().any(|note| {
            note.starts_with("before_response:verification_sources_json:")
                && note.contains("https://example.com/release")
        }));
        assert!(!capture
            .notes
            .iter()
            .any(|note| note == "before_response:source_posture:SourcesRequiredButMissing"));
    }

    #[tokio::test]
    async fn default_runtime_middlewares_attach_execution_state_evidence_when_observed() {
        let mut engine = HookEngine::new();
        let capture = Arc::new(RwLock::new(RuntimeHookCapture::default()));
        install_default_runtime_middlewares(&mut engine, capture.clone());

        let after_tool = HookEvent::new(HookTiming::AfterToolCall)
            .with_tool(
                "runtime_surface",
                r#"{"action":"inspect","runtime":"quickjs"}"#,
            )
            .with_tool_result(
                r#"{
                    "action":"inspect",
                    "verification_preview":{
                        "domain":"StateFact",
                        "requirement":"Required",
                        "mode":"RuntimeStateCheck",
                        "outcome":"VerificationSucceeded",
                        "truth_status":"Verified",
                        "source_posture":"StateEvidenceAttached",
                        "sources":[],
                        "execution_evidence":[],
                        "state_evidence":["runtime=quickjs available=true","source=embedded"],
                        "notes":["runtime surface inspect completed"]
                    },
                    "verification_followup":{
                        "answer_readiness":"execution_or_state_observed",
                        "next_tools":[],
                        "cite_required":false,
                        "note":"Execution or runtime state has been observed directly."
                    }
                }"#,
            );
        let _ = engine.fire(&after_tool).await;

        let before = HookEvent::new(HookTiming::BeforeResponse)
            .with_llm_response("quickjs 已经可用。")
            .with_user_input("帮我确认 quickjs runtime 是否已准备好");
        let result = engine.fire(&before).await;

        match result {
            HookResult::Modify(value) => {
                assert!(value.contains("quickjs 已经可用。"));
                assert!(value.contains("State Evidence:"));
                assert!(value.contains("runtime=quickjs available=true"));
                assert!(value.contains("source=embedded"));
            }
            other => {
                panic!("expected modified response with attached state evidence, got {other:?}")
            }
        }

        let capture = capture.read();
        assert!(capture.notes.iter().any(|note| {
            note.starts_with("before_response:verification_state_evidence_json:")
                && note.contains("runtime=quickjs available=true")
        }));
    }

    #[tokio::test]
    async fn default_runtime_middlewares_attach_sources_after_browser_source_observed() {
        let mut engine = HookEngine::new();
        let capture = Arc::new(RwLock::new(RuntimeHookCapture::default()));
        install_default_runtime_middlewares(&mut engine, capture.clone());

        let after_tool = HookEvent::new(HookTiming::AfterToolCall)
            .with_tool(
                "browser_browse",
                r#"{"action":"navigate","url":"https://example.com/release","structured":true}"#,
            )
            .with_tool_result(
                r#"{
                    "action":"navigate",
                    "url":"https://example.com/release",
                    "verification_preview":{
                        "domain":"KnowledgeFact",
                        "requirement":"Required",
                        "mode":"BrowserValidation",
                        "outcome":"VerificationSucceeded",
                        "truth_status":"Verified",
                        "source_posture":"SourcesAttached",
                        "sources":[{"kind":"browser_page","title":"Release page","uri":"https://example.com/release"}],
                        "execution_evidence":[],
                        "state_evidence":[],
                        "notes":["browser source observed directly"]
                    },
                    "verification_followup":{
                        "answer_readiness":"source_content_observed",
                        "next_tools":[],
                        "cite_required":true,
                        "note":"Source content has been observed directly."
                    }
                }"#,
            );
        let _ = engine.fire(&after_tool).await;

        let before = HookEvent::new(HookTiming::BeforeResponse)
            .with_llm_response("当前页面已经确认版本号为 1.2.3。")
            .with_user_input("打开这个页面帮我确认版本，并给我来源");
        let result = engine.fire(&before).await;

        match result {
            HookResult::Modify(value) => {
                assert!(value.contains("当前页面已经确认版本号为 1.2.3。"));
                assert!(value.contains("Sources:"));
                assert!(value.contains("Release page"));
                assert!(value.contains("https://example.com/release"));
            }
            other => {
                panic!("expected modified response with attached browser sources, got {other:?}")
            }
        }
    }

    #[tokio::test]
    async fn default_runtime_middlewares_downgrade_response_after_tool_error_without_bluffing() {
        let mut engine = HookEngine::new();
        let capture = Arc::new(RwLock::new(RuntimeHookCapture::default()));
        install_default_runtime_middlewares(&mut engine, capture);

        let error_event = HookEvent::new(HookTiming::OnError)
            .with_tool("web_fetch", r#"{"url":"https://example.com/release"}"#)
            .with_error("network timeout");
        let _ = engine.fire(&error_event).await;

        let before = HookEvent::new(HookTiming::BeforeResponse)
            .with_llm_response("我已经确认最新版本就是 1.2.3。")
            .with_user_input("帮我确认最新版本是多少");
        let result = engine.fire(&before).await;

        match result {
            HookResult::Modify(value) => {
                assert!(value.contains("我已经确认最新版本就是 1.2.3。"));
                assert!(value.contains("Verification notice:"));
                assert!(value.contains("required tool `web_fetch` failed"));
                assert!(value.contains("network timeout"));
                assert!(value.contains("tentative until verification is completed"));
            }
            other => panic!("expected modified downgraded response, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn default_runtime_middlewares_track_local_context_answers_without_noisy_notice() {
        let mut engine = HookEngine::new();
        let capture = Arc::new(RwLock::new(RuntimeHookCapture::default()));
        install_default_runtime_middlewares(&mut engine, capture.clone());

        let before = HookEvent::new(HookTiming::BeforeResponse)
            .with_llm_response("OpenAI 是一家人工智能公司。")
            .with_user_input("介绍一下 OpenAI");
        let result = engine.fire(&before).await;

        assert!(matches!(result, HookResult::Continue));

        let capture = capture.read();
        assert!(capture
            .notes
            .iter()
            .any(|note| note == "before_response:truth_status:Unverified"));
        assert!(capture
            .notes
            .iter()
            .any(|note| note == "before_response:verification_requirement:LocalContextAllowed"));
        assert!(capture
            .notes
            .iter()
            .any(|note| note == "before_response:verification_mode:LocalContextOnly"));
        assert!(
            capture
                .notes
                .iter()
                .any(|note| note
                    == "before_response:verification_answer_readiness:local_context_only")
        );
        assert!(capture
            .notes
            .iter()
            .any(|note| note == "before_response:source_posture:NoSourcesRequired"));
    }

    #[tokio::test]
    async fn default_runtime_middlewares_mark_local_context_answers_as_source_missing_when_user_requests_sources(
    ) {
        let mut engine = HookEngine::new();
        let capture = Arc::new(RwLock::new(RuntimeHookCapture::default()));
        install_default_runtime_middlewares(&mut engine, capture.clone());

        let before = HookEvent::new(HookTiming::BeforeResponse)
            .with_llm_response("OpenAI 是一家人工智能公司。")
            .with_user_input("介绍一下 OpenAI，并给来源链接");
        let result = engine.fire(&before).await;

        match result {
            HookResult::Modify(value) => {
                assert!(value.contains("OpenAI 是一家人工智能公司。"));
                assert!(value.contains("Verification notice:"));
                assert!(value.contains("requested source or link is still missing"));
            }
            other => panic!(
                "expected modified local-context source-missing response, got {:?}",
                other
            ),
        }

        let capture = capture.read();
        assert!(capture
            .notes
            .iter()
            .any(|note| note == "before_response:truth_status:Unverified"));
        assert!(capture
            .notes
            .iter()
            .any(|note| { note == "before_response:source_posture:SourcesRequiredButMissing" }));
        assert!(capture
            .notes
            .iter()
            .any(|note| note == "before_response:verification_cite_required:true"));
    }

    #[tokio::test]
    async fn default_runtime_middlewares_preserve_full_followup_request_for_delegate() {
        let mut engine = HookEngine::new();
        let capture = Arc::new(RwLock::new(RuntimeHookCapture::default()));
        install_default_runtime_middlewares(&mut engine, capture.clone());

        let user_input = "搜索起点中文网免费玄幻小说前10部，保存进知识库，然后原创写一部新小说";
        let args = r#"{"role":"researcher","task":"搜索起点中文网免费玄幻小说前10部"}"#;
        let event = HookEvent::new(HookTiming::BeforeToolCall)
            .with_tool("delegate", args)
            .with_user_input(user_input);
        let result = engine.fire(&event).await;

        let HookResult::Modify(value) = result else {
            panic!("expected delegate args to be repaired, got {result:?}");
        };
        let repaired: serde_json::Value =
            serde_json::from_str(&value).expect("repaired delegate args should be valid json");
        let task = repaired
            .get("task")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        assert!(task.contains("搜索起点中文网免费玄幻小说前10部"));
        assert!(task.contains("完整用户请求"));
        assert!(task.contains("保存进知识库"));
        assert!(task.contains("原创写一部新小说"));
        assert_eq!(
            repaired
                .get("full_user_request")
                .and_then(|value| value.as_str()),
            Some(user_input)
        );

        let capture = capture.read();
        assert!(capture
            .notes
            .iter()
            .any(|note| { note == "before_tool:delegate_followup_preserved_full_user_request" }));
    }
}
