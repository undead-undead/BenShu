use benshu_brain::agent::multi_agent::Coordinator;
use benshu_brain::session::store::SessionStore;
use benshu_brain::skills::tool::{
    capability_route_clarification_message as shared_capability_route_clarification_message,
    capability_route_fetch_required_failure_message as shared_capability_route_fetch_required_failure_message,
    capability_route_should_inject_system_message as shared_capability_route_should_inject_system_message,
    capability_route_system_message as shared_capability_route_system_message,
    capability_route_tool_required_failure_message as shared_capability_route_tool_required_failure_message,
    query_requests_document_understanding, CapabilityRouteHint as SharedCapabilityRouteHint,
    CapabilityRouteRequest, CapabilityRouter,
};
use benshu_builtin_tools::tool::document_understand::DocumentUnderstandTool;
use benshu_builtin_tools::SkillLoader;
use benshu_compression::compact_external_error_message as compact_external_error_message_shared;
use benshu_infra::bus::{InboundMessage, MediaAttachment, MediaType, MessageBus, OutboundMessage};

use async_trait::async_trait;
use benshu_brain::agent::message::Message;
use benshu_protocol_core::{ClarificationSessionEvent, ClarificationSessionState};
use dashmap::DashMap;
use moka::future::Cache;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info, warn};

use crate::api::media::{append_media_context_parts, inbound_media_to_parts};
use benshu_brain::agent::AgentEvent;
use benshu_engram::EngramStore;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

const MAX_HISTORY_MESSAGES: usize = 20; // Sliding window limit
const MAX_REALTIME_HISTORY_MESSAGES: usize = 4;
const PERSISTENCE_RETRIES: u32 = 3;
const IDLE_WORKER_TTL: Duration = Duration::from_secs(300);
const MARKER_FORGE_APPROVED: &str = "### FORGE_APPROVED";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConversationalControlIntent {
    Continue,
    StatusQuery,
    Stop,
    Pause,
    Reprioritize,
    Interject,
}

type CapabilityRoute = SharedCapabilityRouteHint;

#[derive(Debug, Clone, PartialEq, Eq)]
struct SkillRuntimeBias {
    skill_name: String,
    runtime: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct IntentClassification {
    intent: ConversationalControlIntent,
    matched_phrase: Option<&'static str>,
    reason: &'static str,
}

#[derive(Debug, Clone)]
struct PendingForgeConfirmation {
    original_request: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingClarification {
    original_request: String,
    clarification: String,
}

impl PendingClarification {
    fn as_contract_state(&self) -> ClarificationSessionState {
        ClarificationSessionState {
            clarification: self.clarification.clone(),
            original_request: self.original_request.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct DeferredSessionInput {
    followups: Vec<String>,
    queued_request: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PendingForgeResolution {
    Approve { request: String },
    Reject,
    Status,
    Supplement { updated_request: String },
}

fn classify_conversational_control_intent(content: &str) -> IntentClassification {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return IntentClassification {
            intent: ConversationalControlIntent::Continue,
            matched_phrase: None,
            reason: "empty_message",
        };
    }

    let lowered = trimmed.to_lowercase();

    let status_keywords = [
        "继续",
        "继续了吗",
        "还在吗",
        "做好了吗",
        "完成了吗",
        "进度",
        "进展",
        "到哪了",
        "怎么样了",
        "还没好吗",
        "done?",
        "are you still working",
        "still working",
        "status",
        "progress",
        "how is it going",
    ];
    if let Some(matched) = status_keywords
        .iter()
        .copied()
        .find(|keyword| lowered.contains(keyword) || trimmed.contains(keyword))
    {
        return IntentClassification {
            intent: ConversationalControlIntent::StatusQuery,
            matched_phrase: Some(matched),
            reason: "status_query_request",
        };
    }

    let stop_keywords = [
        "!stop",
        "stop",
        "cancel this",
        "abort",
        "停止",
        "取消",
        "别做了",
        "中断",
        "停下",
    ];
    if let Some(matched) = stop_keywords
        .iter()
        .copied()
        .find(|keyword| lowered.contains(keyword) || trimmed.contains(keyword))
    {
        return IntentClassification {
            intent: ConversationalControlIntent::Stop,
            matched_phrase: Some(matched),
            reason: "explicit_stop_request",
        };
    }

    let pause_keywords = [
        "pause",
        "wait",
        "hold on",
        "hang on",
        "等一下",
        "先别",
        "暂停",
        "稍等",
        "先停一下",
    ];
    if let Some(matched) = pause_keywords
        .iter()
        .copied()
        .find(|keyword| lowered.contains(keyword) || trimmed.contains(keyword))
    {
        return IntentClassification {
            intent: ConversationalControlIntent::Pause,
            matched_phrase: Some(matched),
            reason: "temporary_pause_request",
        };
    }

    let reprioritize_keywords = [
        "instead",
        "switch to",
        "new priority",
        "different direction",
        "actually let's",
        "先做",
        "改做",
        "改成",
        "换成",
        "换个方向",
        "我有新想法",
        "我有新指令",
        "先处理",
    ];
    if let Some(matched) = reprioritize_keywords
        .iter()
        .copied()
        .find(|keyword| lowered.contains(keyword) || trimmed.contains(keyword))
    {
        return IntentClassification {
            intent: ConversationalControlIntent::Reprioritize,
            matched_phrase: Some(matched),
            reason: "priority_shift_request",
        };
    }

    IntentClassification {
        intent: ConversationalControlIntent::Interject,
        matched_phrase: None,
        reason: "active_session_new_input",
    }
}

fn is_realtime_connector_channel(channel: &str) -> bool {
    matches!(
        channel,
        "telegram" | "slack" | "discord" | "feishu" | "dingtalk" | "qq"
    )
}

fn compact_external_error_message(channel: &str, error: &str) -> String {
    compact_external_error_message_shared(channel, error, is_realtime_connector_channel)
}

fn sanitize_realtime_outbound_response(channel: &str, response: String) -> String {
    if !is_realtime_connector_channel(channel) {
        return response;
    }

    let mut sanitized = response.trim().to_string();
    for marker in [
        "[CRITIQUE",
        "<|end|>",
        "<|im_end|>",
        "<|assistant|>",
        "<|user|>",
        "<|system|>",
        "Final Answer:",
        "\nAssistant:",
        "\nUser:",
        "\nSystem:",
        "\n---",
    ] {
        if let Some(idx) = sanitized.find(marker) {
            sanitized.truncate(idx);
        }
    }
    sanitized = sanitized.trim().to_string();

    let lowered = sanitized.to_lowercase();
    if lowered.starts_with("i encountered an error:")
        || lowered.contains("reasoning error:")
        || lowered.contains("provider error:")
        || lowered.contains("inference failed")
        || lowered.contains("internal error:")
    {
        return compact_external_error_message(channel, &sanitized);
    }

    if sanitized.is_empty() {
        return "抱歉，我这次没有成功生成可用回复。你可以直接再发一次更短一点的消息。".to_string();
    }

    sanitized
}

fn is_natural_language_continue_confirmation(content: &str) -> bool {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return false;
    }

    let lowered = trimmed.to_lowercase();
    let exact_matches = [
        "继续",
        "继续吧",
        "继续啊",
        "确认",
        "确认继续",
        "可以",
        "可以继续",
        "好",
        "好的",
        "行",
        "行，继续",
        "yes",
        "ok",
        "okay",
        "continue",
        "go ahead",
        "proceed",
    ];

    if exact_matches.iter().any(|candidate| lowered == *candidate) {
        return true;
    }

    trimmed.starts_with("继续") || trimmed.starts_with("确认")
}

fn is_natural_language_reject_confirmation(content: &str) -> bool {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return false;
    }

    let lowered = trimmed.to_lowercase();
    let exact_matches = [
        "取消",
        "不用了",
        "先不用",
        "算了",
        "不继续",
        "不要",
        "no",
        "reject",
        "cancel",
        "stop",
    ];

    exact_matches.iter().any(|candidate| lowered == *candidate)
}

fn merge_pending_forge_request(original_request: &str, followup: &str) -> String {
    let followup = followup.trim();
    if followup.is_empty() {
        return original_request.trim().to_string();
    }

    let original = original_request.trim();
    if original.is_empty() {
        return followup.to_string();
    }

    format!("{original}\n补充要求：{followup}")
}

fn resolve_pending_forge_confirmation(
    pending: &PendingForgeConfirmation,
    content: &str,
    intent: &IntentClassification,
) -> PendingForgeResolution {
    if is_natural_language_continue_confirmation(content) {
        return PendingForgeResolution::Approve {
            request: pending.original_request.clone(),
        };
    }

    if is_natural_language_reject_confirmation(content)
        || intent.intent == ConversationalControlIntent::Stop
    {
        return PendingForgeResolution::Reject;
    }

    if intent.intent == ConversationalControlIntent::StatusQuery {
        return PendingForgeResolution::Status;
    }

    PendingForgeResolution::Supplement {
        updated_request: merge_pending_forge_request(&pending.original_request, content),
    }
}

fn build_forge_approved_system_message(original_request: &str) -> String {
    format!(
        "{MARKER_FORGE_APPROVED}\n\
         The user explicitly approved attempting `forge_skill` for the pending task if the current toolset is insufficient.\n\
         Original request: {original_request}\n\
         Execution rules:\n\
         - This is an execution turn, not a discussion turn.\n\
         - If an existing tool can solve it, use that first.\n\
         - If there is still a real capability gap, call `forge_skill` in this turn.\n\
         - Forge the smallest practical session-scoped tool unless the user explicitly asked for something persistent.\n\
         - Keep the runtime sandbox-friendly and minimize dependencies.\n\
         - The forged tool must be concrete enough to smoke-test in a constrained environment before relying on it.\n\
         - Do not ask for confirmation again in this turn.\n\
         - Do not merely describe what you would do.\n\
         - Return a direct user-facing result after taking the tool step."
    )
}

fn should_force_document_hard_route(user_input: &str) -> bool {
    if looks_like_tool_creation_request(user_input) {
        return false;
    }

    query_requests_document_understanding(user_input)
}

fn runtime_implies_runtime_surface(runtime: &str) -> bool {
    matches!(
        runtime.trim().to_ascii_lowercase().as_str(),
        "bash"
            | "sh"
            | "shell"
            | "powershell"
            | "pwsh"
            | "cmd"
            | "uv"
            | "pixi"
            | "bun"
            | "node"
            | "quickjs"
            | "js"
            | "javascript"
            | "python"
            | "python3"
            | "gcc"
            | "cc"
            | "c"
            | "cpp"
            | "c++"
            | "cargo"
            | "rust"
    )
}

fn detect_skill_runtime_bias(user_input: &str, skills: &SkillLoader) -> Option<SkillRuntimeBias> {
    skills
        .match_manual_reference(user_input)
        .and_then(|matched| {
            let runtime = matched.runtime.as_ref()?;
            if !runtime_implies_runtime_surface(runtime) {
                return None;
            }
            Some(SkillRuntimeBias {
                skill_name: matched.name,
                runtime: runtime.to_string(),
            })
        })
}

fn classify_capability_route(
    user_input: &str,
    media: Option<&[MediaAttachment]>,
    approved_forge_request: bool,
    skills: Option<&SkillLoader>,
) -> Option<CapabilityRoute> {
    let looks_like_tool_creation = looks_like_tool_creation_request(user_input);
    let runtime_surface_bias = skills
        .and_then(|loader| detect_skill_runtime_bias(user_input, loader))
        .is_some();
    let router = CapabilityRouter::new(CapabilityRouteRequest {
        approved_forge_request,
        has_media_input: !looks_like_tool_creation
            && media.is_some_and(|attachments| !attachments.is_empty()),
        force_document_understanding: should_force_document_hard_route(user_input),
        runtime_surface_bias,
        suppress_document_understanding: looks_like_tool_creation,
        suppress_realtime_lookup: looks_like_tool_creation,
    });

    router.classify_query_route(user_input)
}

fn capability_route_system_message(
    user_request: &str,
    route: CapabilityRoute,
    media: Option<&[MediaAttachment]>,
    matched_skill_manual: Option<&str>,
) -> String {
    let media_summary = summarize_media_kinds(media);
    shared_capability_route_system_message(
        user_request,
        route,
        Some(media_summary.as_str()),
        matched_skill_manual,
    )
    .unwrap_or_else(|| {
        shared_capability_route_system_message(
            user_request,
            CapabilityRoute::DocumentUnderstanding,
            Some(media_summary.as_str()),
            matched_skill_manual,
        )
        .unwrap_or_default()
    })
}

fn capability_route_should_inject_system_message(route: CapabilityRoute) -> bool {
    shared_capability_route_should_inject_system_message(route)
}

fn realtime_lookup_has_fetch(tool_calls: &[benshu_brain::agent::protocol::ToolCallData]) -> bool {
    tool_calls.iter().any(|call| {
        let name = call.name.as_str();
        name == "web_fetch"
            || name == "browser_browse"
            || name == "browser_open"
            || name == "browser_extract"
    })
}

fn metadata_list_contains_tool(
    metadata: &std::collections::HashMap<String, String>,
    key: &str,
    tool_name: &str,
) -> bool {
    metadata
        .get(key)
        .map(String::as_str)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .any(|value| value == tool_name)
}

fn run_trace_indicates_failed_forge_attempt(run_trace: &benshu_telemetry::RunTrace) -> bool {
    let attempted_forge = run_trace
        .tools
        .iter()
        .any(|tool| tool.tool_name == "forge_skill");
    if !attempted_forge {
        return false;
    }

    let forge_tool_failed = run_trace.tools.iter().any(|tool| {
        tool.tool_name == "forge_skill"
            && (tool.error.is_some()
                || matches!(
                    tool.status,
                    benshu_telemetry::TraceStatus::Failed
                        | benshu_telemetry::TraceStatus::Cancelled
                        | benshu_telemetry::TraceStatus::Degraded
                ))
    });
    let surfaced_forge_error =
        metadata_list_contains_tool(&run_trace.metadata, "tool_error_tools", "forge_skill")
            || metadata_list_contains_tool(
                &run_trace.metadata,
                "tool_error_surface_tools",
                "forge_skill",
            );
    let registered_forge_tool = run_trace
        .metadata
        .get("forge_surface_present")
        .map(String::as_str)
        == Some("true")
        || run_trace.metadata.contains_key("forge_registered_tools");

    forge_tool_failed || surfaced_forge_error || !registered_forge_tool
}

fn approved_forge_failure_message() -> String {
    "我刚才已经按你的确认尝试用 `forge_skill` 临时构建受控工具了，但这次 forge 没有成功完成验证或注册，所以我不会假装它已经可用。\n\n如果你愿意，我可以下一步直接告诉你 forge 失败点，或者改走别的现成能力/更小的受控实现路径。".to_string()
}

fn summarize_media_kinds(media: Option<&[MediaAttachment]>) -> String {
    let Some(media) = media else {
        return "none".to_string();
    };

    let mut labels = Vec::new();
    for attachment in media {
        let label = match attachment.media_type {
            MediaType::Image => "image",
            MediaType::Voice => "voice",
            MediaType::Video => "video",
            MediaType::Document => "document",
        };
        if !labels.contains(&label) {
            labels.push(label);
        }
    }

    if labels.is_empty() {
        "none".to_string()
    } else {
        labels.join(", ")
    }
}

fn looks_like_tool_creation_request(user_input: &str) -> bool {
    let lowered = user_input.to_lowercase();
    let build_verbs = [
        "造",
        "做",
        "创建",
        "生成",
        "编写",
        "开发",
        "实现",
        "搭一个",
        "写一个",
        "build",
        "create",
        "generate",
        "make",
        "implement",
        "develop",
        "write",
    ];
    let artifact_targets = [
        "工具",
        "skill",
        "脚本",
        "script",
        "插件",
        "plugin",
        "能力",
        "capability",
        "worker",
        "导出器",
        "生成器",
    ];

    let has_build_verb = build_verbs
        .iter()
        .any(|marker| lowered.contains(marker) || user_input.contains(marker));
    let has_artifact_target = artifact_targets
        .iter()
        .any(|marker| lowered.contains(marker) || user_input.contains(marker));

    has_build_verb && has_artifact_target
}

fn looks_like_capability_gap_candidate_request(user_input: &str) -> bool {
    if looks_like_tool_creation_request(user_input) {
        return true;
    }

    let lowered = user_input.to_lowercase();
    let explicit_gap_markers = [
        "没有现成工具",
        "没有这个工具",
        "缺少工具",
        "缺工具",
        "需要新工具",
        "需要一个工具",
        "新增工具",
        "添加工具",
        "扩展能力",
        "新能力",
        "capability gap",
        "missing tool",
        "new tool",
        "custom tool",
        "add a tool",
        "build a tool",
        "create a tool",
        "new capability",
    ];

    explicit_gap_markers
        .iter()
        .any(|marker| lowered.contains(marker) || user_input.contains(marker))
}

fn push_deferred_followup(
    existing: Option<DeferredSessionInput>,
    followup: &str,
) -> DeferredSessionInput {
    let mut deferred = existing.unwrap_or_default();
    let followup = followup.trim();
    if !followup.is_empty() {
        deferred.followups.push(followup.to_string());
    }
    deferred
}

fn queue_deferred_request(
    existing: Option<DeferredSessionInput>,
    request: &str,
) -> DeferredSessionInput {
    let mut deferred = existing.unwrap_or_default();
    let request = request.trim();
    if !request.is_empty() {
        deferred.queued_request = Some(request.to_string());
    }
    deferred
}

fn build_deferred_resume_request(deferred: DeferredSessionInput) -> Option<String> {
    let mut segments = Vec::new();
    if let Some(request) = deferred.queued_request {
        segments.push(request);
    }

    if !deferred.followups.is_empty() {
        let joined = deferred.followups.join("\n");
        if segments.is_empty() {
            segments.push(format!("继续处理刚才那件事。\n补充信息：\n{joined}"));
        } else {
            segments.push(format!("补充信息：\n{joined}"));
        }
    }

    if segments.is_empty() {
        None
    } else {
        Some(segments.join("\n\n"))
    }
}

fn build_deferred_status_message(deferred: &DeferredSessionInput) -> String {
    if let Some(request) = deferred.queued_request.as_ref() {
        if deferred.followups.is_empty() {
            format!(
                "当前没有正在执行的任务，但我已经记下你的下一项需求：{request}。回复“继续”我就开始处理。"
            )
        } else {
            format!(
                "当前没有正在执行的任务。我已经记下下一项需求：{request}，以及 {} 条补充信息。回复“继续”我就开始处理。",
                deferred.followups.len()
            )
        }
    } else if !deferred.followups.is_empty() {
        format!(
            "当前没有正在执行的任务，但我已经记下 {} 条补充信息。回复“继续”我就按这些信息继续上一件事。",
            deferred.followups.len()
        )
    } else {
        "当前没有正在执行的任务。如果你想继续做某件事，直接把新的需求发给我就行。".to_string()
    }
}

fn build_pending_clarification_resume_request(
    pending: &PendingClarification,
    user_reply: &str,
) -> String {
    let reply = user_reply.trim();
    if reply.is_empty() {
        pending.original_request.clone()
    } else {
        format!("{}\n补充信息：\n{}", pending.original_request, reply)
    }
}

fn build_pending_clarification_status_message(pending: &PendingClarification) -> String {
    format!(
        "我现在还在等你补充关键信息，才能继续刚才这件事。\n\n我上一条问题是：{}\n\n你直接回复缺的内容就行；如果不想继续，回复“停止”即可。",
        pending.clarification
    )
}

fn build_pending_clarification_status_message_record(pending: &PendingClarification) -> Message {
    let mut message = Message::system(benshu_brain::agent::message::Content::notification(
        format!(
            "Session is waiting for clarification before continuing: {}",
            pending.clarification
        ),
    ));
    pending.as_contract_state().apply_message_metadata(
        &mut message,
        ClarificationSessionEvent::Awaiting,
        None,
    );
    message
}

fn build_pending_clarification_status_surface_record(pending: &PendingClarification) -> Message {
    let mut message = Message::system(benshu_brain::agent::message::Content::notification(
        format!(
            "Session is still waiting for clarification before continuing: {}",
            pending.clarification
        ),
    ));
    pending.as_contract_state().apply_message_metadata(
        &mut message,
        ClarificationSessionEvent::StatusSurface,
        None,
    );
    message
}

fn build_pending_clarification_resolved_message(pending: &PendingClarification) -> Message {
    let mut message = Message::system(benshu_brain::agent::message::Content::notification(
        format!(
            "Clarification received; resuming the pending request: {}",
            pending.original_request
        ),
    ));
    pending.as_contract_state().apply_message_metadata(
        &mut message,
        ClarificationSessionEvent::Resolved,
        None,
    );
    message
}

fn build_pending_clarification_cancelled_message(
    pending: &PendingClarification,
    reason: &str,
) -> Message {
    let mut message = Message::system(benshu_brain::agent::message::Content::notification(
        format!(
            "Pending clarification was cancelled before the request resumed: {}",
            pending.original_request
        ),
    ));
    pending.as_contract_state().apply_message_metadata(
        &mut message,
        ClarificationSessionEvent::Cancelled,
        Some(reason),
    );
    message
}

fn recover_pending_clarification_from_history(history: &[Message]) -> Option<PendingClarification> {
    ClarificationSessionState::recover_from_history(history).map(|state| PendingClarification {
        original_request: state.original_request,
        clarification: state.clarification,
    })
}

fn should_offer_forge_confirmation(
    user_input: &str,
    response: &str,
    tool_call_count: usize,
) -> bool {
    if tool_call_count > 0 || user_input.contains(MARKER_FORGE_APPROVED) {
        return false;
    }

    let user_trimmed = user_input.trim();
    let response_trimmed = response.trim();
    if user_trimmed.is_empty() || response_trimmed.is_empty() {
        return false;
    }

    let response_lower = response_trimmed.to_lowercase();

    let promise_markers = [
        "我将",
        "我会",
        "我来帮你",
        "请稍候",
        "请稍等",
        "正在为你",
        "开始准备",
        "可以开始",
        "i will",
        "i'll",
        "please wait",
        "let me",
        "i can prepare",
        "i can help",
    ];
    let completion_markers = [
        "已生成",
        "已经生成",
        "已完成",
        "结果如下",
        "搜索结果",
        "文件已保存",
        "saved to",
        "generated successfully",
        "artifact",
        ".pdf",
        "/tmp/",
        "/home/",
    ];

    let looks_like_tool_creation = looks_like_tool_creation_request(user_trimmed);
    let router = CapabilityRouter::new(CapabilityRouteRequest {
        suppress_document_understanding: looks_like_tool_creation,
        suppress_realtime_lookup: looks_like_tool_creation,
        ..Default::default()
    });
    let looks_like_realtime_lookup_request = matches!(
        router.classify_query_route(user_trimmed),
        Some(CapabilityRoute::RealtimeLookup(_))
    );
    let looks_like_capability_gap = looks_like_capability_gap_candidate_request(user_trimmed);
    let route_is_capability_gap = matches!(
        router.classify_query_route(user_trimmed),
        Some(CapabilityRoute::CapabilityGap)
    );
    let response_reports_capability_gap = [
        "没有现成工具",
        "没有可用工具",
        "缺少工具",
        "能力缺口",
        "cannot complete with the current tools",
        "no available tool",
        "missing tool",
        "capability gap",
    ]
    .iter()
    .any(|marker| response_lower.contains(marker) || response_trimmed.contains(marker));
    let sounds_like_promise_only = promise_markers
        .iter()
        .any(|marker| response_lower.contains(marker) || response_trimmed.contains(marker));
    let already_completed = completion_markers
        .iter()
        .any(|marker| response_lower.contains(marker) || response_trimmed.contains(marker));

    if looks_like_realtime_lookup_request && !looks_like_tool_creation {
        return false;
    }

    (looks_like_capability_gap || route_is_capability_gap || response_reports_capability_gap)
        && sounds_like_promise_only
        && !already_completed
}

/// Implementation of SessionStore using Engram-KV backend.
pub struct EngramSessionStore {
    store: Arc<EngramStore>,
}

impl EngramSessionStore {
    pub fn new(store: Arc<EngramStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl SessionStore for EngramSessionStore {
    async fn save(&self, id: &str, messages: &[Message]) -> benshu_brain::error::Result<()> {
        let data = serde_json::to_string(messages).map_err(|e| {
            benshu_brain::error::Error::Internal(format!("Failed to serialize session: {}", e))
        })?;

        self.store.store_session(id, &data).map_err(|e| {
            benshu_brain::error::Error::Internal(format!("Failed to store session: {}", e))
        })?;

        Ok(())
    }

    async fn load(&self, id: &str) -> benshu_brain::error::Result<Option<Vec<Message>>> {
        let data = self.store.get_session(id).map_err(|e| {
            benshu_brain::error::Error::Internal(format!("Failed to load session: {}", e))
        })?;

        if let Some(s) = data {
            let messages = serde_json::from_str(&s).map_err(|e| {
                benshu_brain::error::Error::Internal(format!(
                    "Failed to deserialize session: {}",
                    e
                ))
            })?;
            Ok(Some(messages))
        } else {
            Ok(None)
        }
    }

    async fn delete_stale(&self, max_age_days: u32) -> benshu_brain::error::Result<usize> {
        self.store
            .delete_stale_sessions(max_age_days as i64)
            .map_err(|e| {
                benshu_brain::error::Error::Internal(format!("Failed to cleanup sessions: {}", e))
            })
    }
}

/// Bridges the MessageBus with the Coordinator (Swarm).
///
/// It listens for InboundMessages, routes them through the Swarm,
/// maintains session history (with LRU cache and persistent KV storage),
/// and publishes responses as OutboundMessages.
pub struct AgentBridge {
    coordinator: Arc<Coordinator>,
    skills: Arc<SkillLoader>,
    bus: Arc<MessageBus>,
    channel_observability: Arc<RwLock<HashMap<String, crate::api::state::ChannelObservability>>>,
    document_router: Arc<DocumentUnderstandTool>,
    /// Persistent storage for sessions
    store: Arc<dyn SessionStore>,
    /// In-memory LRU cache with TTL to prevent OOM
    cache: Cache<String, Vec<Message>>,
    pending_forge_confirmations: DashMap<String, PendingForgeConfirmation>,
    deferred_session_inputs: DashMap<String, DeferredSessionInput>,
    session_execution_tracker: SessionExecutionTracker,
    approval_manager: Arc<crate::api::security::ApprovalManager>,
    shutdown_token: CancellationToken,
}

#[derive(Default)]
struct SessionExecutionTracker {
    generations: DashMap<String, u64>,
}

impl SessionExecutionTracker {
    fn start(&self, session_key: &str) -> u64 {
        let next = self
            .generations
            .get(session_key)
            .map(|entry| entry.value().saturating_add(1))
            .unwrap_or(1);
        self.generations.insert(session_key.to_string(), next);
        next
    }

    fn invalidate(&self, session_key: &str) {
        let _ = self.start(session_key);
    }

    fn is_current(&self, session_key: &str, generation: u64) -> bool {
        self.generations
            .get(session_key)
            .map(|entry| *entry.value() == generation)
            .unwrap_or(false)
    }
}

impl AgentBridge {
    pub fn new(
        coordinator: Arc<Coordinator>,
        skills: Arc<SkillLoader>,
        bus: Arc<MessageBus>,
        channel_observability: Arc<
            RwLock<HashMap<String, crate::api::state::ChannelObservability>>,
        >,
        document_router: Arc<DocumentUnderstandTool>,
        store: Arc<dyn SessionStore>,
        approval_manager: Arc<crate::api::security::ApprovalManager>,
    ) -> Self {
        // Cache: Max 1000 active sessions, 1 hour idle TTL
        let cache = Cache::builder()
            .max_capacity(1000)
            .time_to_idle(Duration::from_secs(3600))
            .build();

        Self {
            coordinator,
            skills,
            bus,
            channel_observability,
            document_router,
            store,
            cache,
            pending_forge_confirmations: DashMap::new(),
            deferred_session_inputs: DashMap::new(),
            session_execution_tracker: SessionExecutionTracker::default(),
            approval_manager,
            shutdown_token: CancellationToken::new(),
        }
    }

    fn start_execution_generation(&self, session_key: &str) -> u64 {
        self.session_execution_tracker.start(session_key)
    }

    fn invalidate_execution_generation(&self, session_key: &str) {
        self.session_execution_tracker.invalidate(session_key);
    }

    fn is_current_execution_generation(&self, session_key: &str, generation: u64) -> bool {
        self.session_execution_tracker
            .is_current(session_key, generation)
    }

    async fn load_session_history(&self, session_key: &str) -> anyhow::Result<Vec<Message>> {
        if let Some(history) = self.cache.get(session_key).await {
            Ok(history)
        } else if let Some(history) = self.store.load(session_key).await? {
            Ok(history)
        } else {
            Ok(Vec::new())
        }
    }

    async fn persist_session_history(
        &self,
        session_key: &str,
        history: Vec<Message>,
    ) -> anyhow::Result<()> {
        self.cache
            .insert(session_key.to_string(), history.clone())
            .await;
        self.store.save(session_key, &history).await?;
        Ok(())
    }

    async fn load_pending_clarification(
        &self,
        session_key: &str,
    ) -> anyhow::Result<Option<PendingClarification>> {
        let history = self.load_session_history(session_key).await?;
        Ok(recover_pending_clarification_from_history(&history))
    }

    async fn set_pending_clarification(
        &self,
        session_key: &str,
        pending: PendingClarification,
    ) -> anyhow::Result<()> {
        let mut history = self.load_session_history(session_key).await?;
        history.push(build_pending_clarification_status_message_record(&pending));
        self.persist_session_history(session_key, history).await
    }

    async fn resolve_pending_clarification(
        &self,
        session_key: &str,
        pending: &PendingClarification,
    ) -> anyhow::Result<()> {
        let mut history = self.load_session_history(session_key).await?;
        history.push(build_pending_clarification_resolved_message(pending));
        self.persist_session_history(session_key, history).await
    }

    async fn record_pending_clarification_status_surface(
        &self,
        session_key: &str,
        pending: &PendingClarification,
    ) -> anyhow::Result<()> {
        let mut history = self.load_session_history(session_key).await?;
        history.push(build_pending_clarification_status_surface_record(pending));
        self.persist_session_history(session_key, history).await
    }

    async fn cancel_pending_clarification(
        &self,
        session_key: &str,
        pending: &PendingClarification,
        reason: &str,
    ) -> anyhow::Result<()> {
        let mut history = self.load_session_history(session_key).await?;
        history.push(build_pending_clarification_cancelled_message(
            pending, reason,
        ));
        self.persist_session_history(session_key, history).await
    }

    fn record_channel_inbound(
        &self,
        channel_id: &str,
        session_key: Option<&str>,
        chat_id: Option<&str>,
        thread_id: Option<&str>,
    ) {
        let mut guard = self.channel_observability.write();
        let entry = guard.entry(channel_id.to_string()).or_insert_with(|| {
            crate::api::state::ChannelObservability {
                channel_id: channel_id.to_string(),
                ..Default::default()
            }
        });
        entry.inbound_total += 1;
        entry.last_inbound_session_key = session_key.map(|s| s.to_string());
        entry.last_chat_id = chat_id.map(|s| s.to_string());
        entry.last_thread_id = thread_id.map(|s| s.to_string());
        entry.last_observed_at = Some(chrono::Utc::now());
    }

    fn record_channel_outbound(
        &self,
        channel_id: &str,
        chat_id: Option<&str>,
        thread_id: Option<&str>,
    ) {
        let mut guard = self.channel_observability.write();
        let entry = guard.entry(channel_id.to_string()).or_insert_with(|| {
            crate::api::state::ChannelObservability {
                channel_id: channel_id.to_string(),
                ..Default::default()
            }
        });
        entry.outbound_total += 1;
        entry.last_chat_id = chat_id.map(|s| s.to_string());
        entry.last_thread_id = thread_id.map(|s| s.to_string());
        entry.last_observed_at = Some(chrono::Utc::now());
    }

    fn record_channel_failure(
        &self,
        channel_id: &str,
        kind: impl Into<String>,
        detail: impl Into<String>,
        chat_id: Option<&str>,
    ) {
        let mut guard = self.channel_observability.write();
        let entry = guard.entry(channel_id.to_string()).or_insert_with(|| {
            crate::api::state::ChannelObservability {
                channel_id: channel_id.to_string(),
                ..Default::default()
            }
        });
        entry.last_failure_kind = Some(kind.into());
        entry.last_failure_detail = Some(detail.into());
        if let Some(chat_id) = chat_id {
            entry.last_chat_id = Some(chat_id.to_string());
        }
        entry.last_observed_at = Some(chrono::Utc::now());
    }

    pub fn shutdown_token(&self) -> CancellationToken {
        self.shutdown_token.clone()
    }

    pub async fn start(self: Arc<Self>) {
        info!("Agent Bridge started. Listening for inbound messages...");

        // Phase 5: Global Risk Notification Relay
        // We spawn a task for each known agent role to listen for ApprovalPending events.
        for role in self.coordinator.roles() {
            if let Some(agent) = self.coordinator.get(&role) {
                let bus = self.bus.clone();
                let am = self.approval_manager.clone();
                let mut events_rx = agent.events();
                let shutdown = self.shutdown_token.clone();
                let cache = self.cache.clone();
                tokio::spawn(async move {
                    loop {
                        tokio::select! {
                            _ = shutdown.cancelled() => break,
                            event_res = events_rx.recv() => {
                                match event_res {
                                    Ok(event) => {
                                        if let Err(e) = Self::process_agent_event(event, bus.clone(), am.clone(), cache.clone()).await {
                                            error!("Error processing agent event: {}", e);
                                        }
                                    }
                                    Err(broadcast::error::RecvError::Lagged(n)) => {
                                        warn!("Agent event stream lagged by {} events", n);
                                    }
                                    Err(broadcast::error::RecvError::Closed) => break,
                                }
                            }
                        }
                    }
                });
            }
        }

        let bus = self.bus.clone();
        let shutdown = self.shutdown_token.clone();

        loop {
            tokio::select! {
                _ = shutdown.cancelled() => {
                    info!("Agent Bridge shutting down gracefully...");
                    break;
                }
                msg_step = bus.consume_inbound() => {
                    match msg_step {
                        Ok(msg) => {
                            let self_clone = self.clone();
                            tokio::spawn(async move {
                                if let Err(e) = self_clone.handle_message(msg).await {
                                    error!("Error handling message: {}", e);
                                }
                            });
                        }
                        Err(e) => {
                            error!("Bus error: {}", e);
                            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

                            // Circuit Breaker: If bus is fundamentally broken, exit loop
                            if e.to_string().contains("Connection lost") {
                                error!("Unrecoverable Bus Error. Terminating Bridge loop.");
                                break;
                            }
                        }
                    }
                }
            }
        }
    }

    /// Internal helper for robust event processing
    async fn process_agent_event(
        event: AgentEvent,
        bus: Arc<MessageBus>,
        approval_manager: Arc<crate::api::security::ApprovalManager>,
        _cache: Cache<String, Vec<Message>>,
    ) -> anyhow::Result<()> {
        match event.data {
            benshu_brain::agent::AgentEventData::ApprovalPending {
                tool,
                input,
                safety,
            } => {
                let approval = approval_manager
                    .list_pending()
                    .into_iter()
                    .find(|p| p.tool_name == tool && p.arguments == input);

                let (approval_id, challenge) = match &approval {
                    Some(p) => (p.id.clone(), p.challenge_code.clone()),
                    None => ("".to_string(), "----".to_string()),
                };

                let risk_color = match safety {
                    benshu_brain::skills::tool::SafetyLevel::Green => "🟢 GREEN",
                    benshu_brain::skills::tool::SafetyLevel::Yellow => "🟡 YELLOW",
                    benshu_brain::skills::tool::SafetyLevel::Red => "🔴 RED",
                };

                let msg = format!(
                    "⚠️ *Action Approval Required*\n\n\
                     *Risk:* {}\n\
                     *Tool:* `{}`\n\
                     *Input:* `{}`\n\n\
                     Use the buttons below or reply with: `YES {}`",
                    risk_color, tool, input, challenge
                );

                let mut outbound = OutboundMessage::new("broadcast", "all", msg);

                if !approval_id.is_empty() {
                    use benshu_infra::bus::Button;
                    let buttons = vec![
                        Button::new("✅ Approve", format!("approve:{}", approval_id)),
                        Button::new("❌ Reject", format!("reject:{}", approval_id)),
                    ];
                    outbound = outbound.with_buttons(buttons);
                }

                bus.publish_outbound(outbound).await?;
            }
            benshu_brain::agent::AgentEventData::PartialResponse { content } => {
                debug!("Partial response received: {}", content);
            }
            benshu_brain::agent::AgentEventData::Error { message } => {
                debug!("Agent encountered error: {}", message);
            }
            _ => {}
        }
        Ok(())
    }

    async fn handle_message(&self, mut msg: InboundMessage) -> anyhow::Result<()> {
        self.record_channel_inbound(
            &msg.channel,
            Some(&msg.session_key),
            Some(&msg.chat_id),
            Some(&msg.session_key),
        );
        debug!(
            "Processing message from {}: {} (payload: {:?})",
            msg.channel, msg.content, msg.payload
        );

        // 0. Lightweight conversational control for any inbound channel/bot adapter.
        let trimmed_content_owned = msg.content.trim().to_string();
        let trimmed_content = trimmed_content_owned.as_str();
        let intent = classify_conversational_control_intent(trimmed_content);
        let mut effective_intent = intent.clone();

        let session_key = msg.session_key.clone();
        let mut approved_forge_request: Option<String> = None;
        if let Some(pending) = self
            .pending_forge_confirmations
            .get(&session_key)
            .map(|entry| entry.clone())
        {
            match resolve_pending_forge_confirmation(&pending, trimmed_content, &intent) {
                PendingForgeResolution::Approve { request } => {
                    self.pending_forge_confirmations.remove(&session_key);
                    approved_forge_request = Some(request);
                    effective_intent = IntentClassification {
                        intent: ConversationalControlIntent::Continue,
                        matched_phrase: Some("继续"),
                        reason: "pending_forge_confirmation_approved",
                    };
                }
                PendingForgeResolution::Reject => {
                    self.pending_forge_confirmations.remove(&session_key);
                    let outbound = OutboundMessage::new(
                        msg.channel.clone(),
                        msg.chat_id.clone(),
                        "好的，这次我先不尝试临时造新工具。你可以换个做法，或者之后再让我继续。"
                            .to_string(),
                    );
                    self.record_channel_outbound(
                        &outbound.channel,
                        Some(&outbound.chat_id),
                        Some(&session_key),
                    );
                    self.bus.publish_outbound(outbound).await?;
                    return Ok(());
                }
                PendingForgeResolution::Status => {
                    let outbound = OutboundMessage::new(
                        msg.channel.clone(),
                        msg.chat_id.clone(),
                        "我这边正在等你确认是否继续尝试临时造新工具。回复“继续”就会往下执行；如果你不想继续，回复“取消”即可。"
                            .to_string(),
                    );
                    self.record_channel_outbound(
                        &outbound.channel,
                        Some(&outbound.chat_id),
                        Some(&session_key),
                    );
                    self.bus.publish_outbound(outbound).await?;
                    return Ok(());
                }
                PendingForgeResolution::Supplement { updated_request } => {
                    self.pending_forge_confirmations.insert(
                        session_key.clone(),
                        PendingForgeConfirmation {
                            original_request: updated_request,
                        },
                    );
                    let outbound = OutboundMessage::new(
                        msg.channel.clone(),
                        msg.chat_id.clone(),
                        "我已经记下这些补充要求了。准备好后直接回复“继续”，我就会按更新后的需求往下尝试；如果不想继续，回复“取消”即可。"
                            .to_string(),
                    );
                    self.record_channel_outbound(
                        &outbound.channel,
                        Some(&outbound.chat_id),
                        Some(&session_key),
                    );
                    self.bus.publish_outbound(outbound).await?;
                    return Ok(());
                }
            }
        }

        let pending_clarification = self.load_pending_clarification(&session_key).await?;

        if let Some(pending) = pending_clarification {
            match effective_intent.intent {
                ConversationalControlIntent::StatusQuery
                | ConversationalControlIntent::Continue => {
                    self.record_pending_clarification_status_surface(&session_key, &pending)
                        .await?;
                    let outbound = OutboundMessage::new(
                        msg.channel.clone(),
                        msg.chat_id.clone(),
                        build_pending_clarification_status_message(&pending),
                    );
                    self.record_channel_outbound(
                        &outbound.channel,
                        Some(&outbound.chat_id),
                        Some(&session_key),
                    );
                    self.bus.publish_outbound(outbound).await?;
                    return Ok(());
                }
                ConversationalControlIntent::Stop => {
                    self.cancel_pending_clarification(
                        &session_key,
                        &pending,
                        "clarification_cancelled_by_user",
                    )
                    .await?;
                    let outbound = OutboundMessage::new(
                        msg.channel.clone(),
                        msg.chat_id.clone(),
                        "好的，这条待补充的问题我先取消了。后面如果你想继续，直接重新发需求就行。"
                            .to_string(),
                    );
                    self.record_channel_outbound(
                        &outbound.channel,
                        Some(&outbound.chat_id),
                        Some(&session_key),
                    );
                    self.bus.publish_outbound(outbound).await?;
                    return Ok(());
                }
                _ => {
                    self.resolve_pending_clarification(&session_key, &pending)
                        .await?;
                    msg.content =
                        build_pending_clarification_resume_request(&pending, trimmed_content);
                    effective_intent = IntentClassification {
                        intent: ConversationalControlIntent::Continue,
                        matched_phrase: Some("补充信息"),
                        reason: "pending_clarification_resolved",
                    };
                }
            }
        }

        if let Some((_, role)) = self
            .coordinator
            .active_agents()
            .into_iter()
            .find(|(k, _)| k == &session_key)
        {
            if let Some(agent) = self.coordinator.get(&role) {
                let has_live_foreground_task = agent.has_active_foreground_task();
                if !has_live_foreground_task {
                    debug!(
                        "Session {} has an active agent mapping but no live foreground task; treating inbound message as a normal turn.",
                        session_key
                    );
                    if effective_intent.intent == ConversationalControlIntent::Continue {
                        if let Some((_, deferred)) =
                            self.deferred_session_inputs.remove(&session_key)
                        {
                            if let Some(resume_request) = build_deferred_resume_request(deferred) {
                                msg.content = resume_request.clone();
                                effective_intent = IntentClassification {
                                    intent: ConversationalControlIntent::Continue,
                                    matched_phrase: Some("继续"),
                                    reason: "deferred_session_resumed",
                                };
                            }
                        }
                    } else if effective_intent.intent == ConversationalControlIntent::StatusQuery {
                        let status_msg = self
                            .deferred_session_inputs
                            .get(&session_key)
                            .map(|entry| build_deferred_status_message(entry.value()))
                            .unwrap_or_else(|| {
                                "当前没有正在执行的任务。如果你想继续做某件事，直接把新的需求发给我就行。"
                                    .to_string()
                            });
                        let outbound = OutboundMessage::new(
                            msg.channel.clone(),
                            msg.chat_id.clone(),
                            status_msg,
                        );
                        self.record_channel_outbound(
                            &outbound.channel,
                            Some(&outbound.chat_id),
                            Some(&session_key),
                        );
                        self.bus.publish_outbound(outbound).await?;
                        return Ok(());
                    }
                }

                if !has_live_foreground_task {
                    // Fall through to normal chat handling below.
                } else {
                    match effective_intent.intent {
                        ConversationalControlIntent::Stop => {
                            self.invalidate_execution_generation(&session_key);
                            info!(
                                "Emergency stop triggered for session: {} ({})",
                                session_key, effective_intent.reason
                            );
                            agent.cancel();
                            agent.ensure_active_token();
                            let outbound = OutboundMessage::new(
                                msg.channel,
                                msg.chat_id,
                                "🛑 **任务已中断。**\nAgent 已停止当前执行。你可以稍后继续发新指令，我会从最新意图重新开始。"
                                    .to_string(),
                            );
                            self.record_channel_outbound(
                                &outbound.channel,
                                Some(&outbound.chat_id),
                                Some(&session_key),
                            );
                            self.bus.publish_outbound(outbound).await?;
                            return Ok(());
                        }
                        ConversationalControlIntent::Pause => {
                            let deferred = push_deferred_followup(
                                self.deferred_session_inputs
                                    .remove(&session_key)
                                    .map(|(_, deferred)| deferred),
                                trimmed_content,
                            );
                            self.deferred_session_inputs
                                .insert(session_key.clone(), deferred);
                            info!(
                                "Soft pause requested for active session: {} ({})",
                                session_key, effective_intent.reason
                            );
                            let outbound = OutboundMessage::new(
                                msg.channel,
                                msg.chat_id,
                                "我先记下这条补充了。当前任务会继续运行，不会被打断；等这轮结束后，我会把你的新信息一起带上。如果你想立刻停掉当前任务，请直接回复“停止”。"
                                    .to_string(),
                            );
                            self.record_channel_outbound(
                                &outbound.channel,
                                Some(&outbound.chat_id),
                                Some(&session_key),
                            );
                            self.bus.publish_outbound(outbound).await?;
                            return Ok(());
                        }
                        ConversationalControlIntent::StatusQuery => {
                            let status_msg = if self
                                .pending_forge_confirmations
                                .contains_key(&session_key)
                            {
                                "我这边正在等待你确认是否继续尝试临时造新工具。回复“继续”就会往下执行，回复“取消”就会停止这条路径。"
                                    .to_string()
                            } else {
                                "我还在继续处理刚才那件事，当前任务仍在运行中。完成后我会直接把结果发给你；如果你想中断它，回复“停止”即可。"
                                    .to_string()
                            };
                            let outbound =
                                OutboundMessage::new(msg.channel, msg.chat_id, status_msg);
                            self.record_channel_outbound(
                                &outbound.channel,
                                Some(&outbound.chat_id),
                                Some(&session_key),
                            );
                            self.bus.publish_outbound(outbound).await?;
                            return Ok(());
                        }
                        ConversationalControlIntent::Reprioritize => {
                            let deferred = queue_deferred_request(
                                self.deferred_session_inputs
                                    .remove(&session_key)
                                    .map(|(_, deferred)| deferred),
                                trimmed_content,
                            );
                            self.deferred_session_inputs
                                .insert(session_key.clone(), deferred);
                            debug!(
                                "Queued reprioritized request for active session {} with phrase {:?}",
                                session_key, effective_intent.matched_phrase
                            );
                            let outbound = OutboundMessage::new(
                                msg.channel.clone(),
                                msg.chat_id.clone(),
                                "我已经记下你的新方向了。当前任务会先跑完，结束后我再按这条新需求继续；如果你想立刻切过去，请直接回复“停止”。"
                                    .to_string(),
                            );
                            self.record_channel_outbound(
                                &outbound.channel,
                                Some(&outbound.chat_id),
                                Some(&session_key),
                            );
                            self.bus.publish_outbound(outbound).await?;
                            return Ok(());
                        }
                        ConversationalControlIntent::Interject => {
                            let deferred = push_deferred_followup(
                                self.deferred_session_inputs
                                    .remove(&session_key)
                                    .map(|(_, deferred)| deferred),
                                trimmed_content,
                            );
                            self.deferred_session_inputs
                                .insert(session_key.clone(), deferred);
                            debug!("Queued follow-up for active session: {}", session_key);
                            let outbound = OutboundMessage::new(
                                msg.channel.clone(),
                                msg.chat_id.clone(),
                                "我已经记下这条补充了。当前任务仍在运行，我不会用这条新消息去抢占它；等这轮完成后，我会把这条补充一起接上。"
                                    .to_string(),
                            );
                            self.record_channel_outbound(
                                &outbound.channel,
                                Some(&outbound.chat_id),
                                Some(&session_key),
                            );
                            self.bus.publish_outbound(outbound).await?;
                            return Ok(());
                        }
                        ConversationalControlIntent::Continue => {
                            let outbound = OutboundMessage::new(
                                msg.channel,
                                msg.chat_id,
                                "当前任务还在继续运行中。我不会因为这条“继续”去打断它；完成后会直接把结果发给你。如果你想查状态，回复“还在吗”；如果想中断，回复“停止”。"
                                    .to_string(),
                            );
                            self.record_channel_outbound(
                                &outbound.channel,
                                Some(&outbound.chat_id),
                                Some(&session_key),
                            );
                            self.bus.publish_outbound(outbound).await?;
                            return Ok(());
                        }
                    }
                }
            }
        }

        // 0.1 Bot Commands: /role, /switch, /clear, /help
        if trimmed_content.starts_with('/') {
            let cmd_parts: Vec<&str> = trimmed_content.split_whitespace().collect();
            let cmd = cmd_parts[0].to_lowercase();

            match cmd.as_str() {
                "/role" | "/switch" => {
                    if cmd_parts.len() > 1 {
                        let target_role_name = cmd_parts[1].to_lowercase();
                        let prime_role = self.coordinator.primary_role();
                        if target_role_name == "benshu" {
                            self.coordinator
                                .switch_session_agent(&session_key, prime_role.clone());
                            let outbound = OutboundMessage::new(
                                msg.channel,
                                msg.chat_id,
                                format!(
                                    "✅ **Prime agent locked: {}**\nFuture messages in this chat will continue through the primary BenShu persona.",
                                    prime_role.name()
                                ),
                            );
                            self.bus.publish_outbound(outbound).await?;
                            return Ok(());
                        }

                        let available = self
                            .coordinator
                            .roles()
                            .iter()
                            .map(|r| r.name())
                            .collect::<Vec<_>>()
                            .join(", ");
                        let outbound = OutboundMessage::new(
                            msg.channel,
                            msg.chat_id,
                            format!(
                                "ℹ️ Direct specialist switching is disabled.\nRoute requests through **{}**, which will delegate internally via A2A.\n\nAvailable specialist roles remain configurable in the system: {}",
                                prime_role.name(),
                                available
                            ),
                        );
                        self.bus.publish_outbound(outbound).await?;
                        return Ok(());
                    }
                }
                "/clear" | "/reset" => {
                    self.invalidate_execution_generation(&session_key);
                    let _ = self.coordinator.remove_session(&session_key);
                    let _ = self.cache.remove(&session_key).await;
                    let _ = self.store.save(&session_key, &[]).await;
                    self.deferred_session_inputs.remove(&session_key);
                    self.pending_forge_confirmations.remove(&session_key);
                    let outbound = OutboundMessage::new(
                        msg.channel,
                        msg.chat_id,
                        "扫帚 **Chat session cleared.**\nShort-term memory wiped for this chat."
                            .to_string(),
                    );
                    self.bus.publish_outbound(outbound).await?;
                    return Ok(());
                }
                "/help" => {
                    let roles = self
                        .coordinator
                        .roles()
                        .iter()
                        .map(|r| format!("- `{}`", r.name()))
                        .collect::<Vec<_>>()
                        .join("\n");
                    let help_msg = format!(
                        "🤖 **Agent Swarm Controller**\n\n\
                        Commands:\n\
                        - `/role benshu` : Re-lock this chat to the prime agent persona\n\
                        - `/switch benshu` : Same as /role\n\
                        - `/clear` : Wipe chat history (Short-Term Memory)\n\
                        - `/stop` : Emergency stop current task\n\n\
                        **Available Specialist Agents:**\n{}\n\n\
                        Specialist personas are routed internally through BenShu rather than exposed as direct chat owners.", roles);
                    let outbound = OutboundMessage::new(msg.channel, msg.chat_id, help_msg);
                    self.bus.publish_outbound(outbound).await?;
                    return Ok(());
                }
                _ => {} // Fallthrough to LLM if it's an unknown command
            }
        }

        // 0. Check for Button Callbacks (Roadmap Phase 5.4)
        let mut msg = msg; // Make msg mutable to update content from payload
        if let Some(payload) = &msg.payload {
            if payload.starts_with("approve:") {
                let id = &payload[8..];
                if self.approval_manager.resolve(id, true) {
                    let outbound = OutboundMessage::new(
                        msg.channel.clone(),
                        msg.chat_id.clone(),
                        "✅ Action approved via button interact. Proceeding...",
                    );
                    self.bus.publish_outbound(outbound).await?;
                    return Ok(());
                }
            } else if payload.starts_with("reject:") {
                let id = &payload[7..];
                if self.approval_manager.resolve(id, false) {
                    let outbound = OutboundMessage::new(
                        msg.channel.clone(),
                        msg.chat_id.clone(),
                        "❌ Action rejected. Blocked.",
                    );
                    self.bus.publish_outbound(outbound).await?;
                    return Ok(());
                }
            } else if payload.starts_with('/') {
                // Feature: Buttons can trigger commands directly
                debug!("Button callback triggering command: {}", payload);
                msg.content = payload.clone();
            }
        }

        // 0.5 Check for Universal Challenge Resolution (Roadmap Phase 6.1)
        let trimmed = msg.content.trim().to_uppercase();
        if trimmed.starts_with("YES ") || trimmed.starts_with("APPROVE ") {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 2 {
                let code = parts[1];
                if self.approval_manager.resolve_by_challenge(code, true) {
                    let outbound = OutboundMessage::new(
                        msg.channel,
                        msg.chat_id,
                        format!("✅ Challenge `{}` approved. Agent is proceeding.", code),
                    );
                    self.bus.publish_outbound(outbound).await?;
                    return Ok(());
                }
            }
        } else if trimmed.starts_with("NO ") || trimmed.starts_with("REJECT ") {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 2 {
                let code = parts[1];
                if self.approval_manager.resolve_by_challenge(code, false) {
                    let outbound = OutboundMessage::new(
                        msg.channel,
                        msg.chat_id,
                        format!("❌ Challenge `{}` rejected. Operation cancelled.", code),
                    );
                    self.bus.publish_outbound(outbound).await?;
                    return Ok(());
                }
            }
        }

        let session_key = msg.session_key.clone();
        let routing_content = approved_forge_request
            .as_deref()
            .unwrap_or(msg.content.as_str());
        let capability_route = classify_capability_route(
            routing_content,
            msg.media.as_deref(),
            approved_forge_request.is_some(),
            Some(self.skills.as_ref()),
        );
        if let Some(route) = capability_route {
            if let Some(clarification) =
                shared_capability_route_clarification_message(routing_content, route)
            {
                let pending = PendingClarification {
                    original_request: routing_content.to_string(),
                    clarification: clarification.clone(),
                };
                self.set_pending_clarification(&session_key, pending)
                    .await?;
                let outbound = OutboundMessage::new(msg.channel, msg.chat_id, clarification);
                self.bus.publish_outbound(outbound).await?;
                return Ok(());
            }
        }

        // Gateway bridge no longer runs active SwarmRouter delegation as a front-door path.
        // Runtime mainline routing now stays on CapabilityRouter + lazy worker orchestration.
        if let Some(route) = capability_route {
            debug!(
                "Skipping legacy gateway worker routing for session {} because {} is active.",
                session_key,
                CapabilityRouter::default().route_debug_label(route)
            );
        } else {
            debug!(
                "Skipping legacy gateway worker routing for session {} because runtime mainline routing owns worker selection.",
                session_key
            );
        }

        // 1. Load History (Cache -> Persistent Store -> New)
        let mut history = if let Some(h) = self.cache.get(&session_key).await {
            debug!("Session cache hit: {}", session_key);
            h
        } else if let Some(h) = self.store.load(&session_key).await? {
            debug!("Session retrieved from persistence: {}", session_key);
            h
        } else {
            debug!("New session started: {}", session_key);
            Vec::new()
        };

        // 1.5 Sliding Window Pruning (Phase 18: Resource & Token Guard)
        let effective_history_limit = if is_realtime_connector_channel(&msg.channel) {
            MAX_REALTIME_HISTORY_MESSAGES
        } else {
            MAX_HISTORY_MESSAGES
        };

        if history.len() > effective_history_limit {
            info!(
                "Pruning session history for {}: {} -> {}",
                session_key,
                history.len(),
                effective_history_limit
            );

            // Keep the first message if it's a System Prompt (Essential for personality)
            let mut new_history = Vec::new();
            if let Some(first) = history.first() {
                if first.role == benshu_brain::agent::message::Role::System {
                    new_history.push(first.clone());
                }
            }

            // Take the last N messages
            let to_take = effective_history_limit.saturating_sub(new_history.len());
            let start_idx = history.len().saturating_sub(to_take);
            new_history.extend(history[start_idx..].iter().cloned());
            history = new_history;
        }

        let effective_user_text = approved_forge_request
            .clone()
            .unwrap_or_else(|| msg.content.clone());

        if let Some(original_request) = approved_forge_request.as_ref() {
            history.push(Message::system(build_forge_approved_system_message(
                original_request,
            )));
        }

        if let Some(route) =
            capability_route.filter(|route| capability_route_should_inject_system_message(*route))
        {
            let matched_skill_manual =
                detect_skill_runtime_bias(&effective_user_text, self.skills.as_ref())
                    .map(|bias| bias.skill_name);
            history.push(Message::system(capability_route_system_message(
                &effective_user_text,
                route,
                msg.media.as_deref(),
                matched_skill_manual.as_deref(),
            )));
        }

        // 2. Append User Message (Multimodal Support)
        let mut parts = vec![benshu_brain::agent::message::ContentPart::Text {
            text: effective_user_text.clone(),
        }];
        let media_parts = inbound_media_to_parts(self.document_router.clone(), msg.media).await;
        append_media_context_parts(&mut parts, media_parts);
        history.push(Message::user(benshu_brain::agent::message::Content::parts(
            parts,
        )));

        let execution_generation = self.start_execution_generation(&session_key);

        // 3. Prompt Swarm
        let outcome = match self
            .coordinator
            .chat_session(&session_key, history.clone())
            .await
        {
            Ok(outcome) => Some(outcome),
            Err(e) => {
                error!("Swarm chat error: {}", e);
                self.record_channel_failure(
                    &msg.channel,
                    "bridge_chat_error",
                    e.to_string(),
                    Some(&msg.chat_id),
                );
                None
            }
        };

        let mut full_response = if let Some(outcome) = outcome {
            let response = outcome.response;
            let should_offer_forge = approved_forge_request.is_none()
                && should_offer_forge_confirmation(
                    &effective_user_text,
                    &response,
                    outcome.tool_calls.len(),
                );
            let forge_attempt_failed = approved_forge_request.is_some()
                && outcome
                    .run_trace
                    .as_ref()
                    .is_some_and(run_trace_indicates_failed_forge_attempt);

            if forge_attempt_failed {
                approved_forge_failure_message()
            } else if let Some(route) = capability_route {
                if CapabilityRouter::default().route_requires_real_tool_call(route)
                    && outcome.tool_calls.is_empty()
                {
                    shared_capability_route_tool_required_failure_message(route)
                } else if !realtime_lookup_has_fetch(&outcome.tool_calls) {
                    if let Some(fetch_failure) =
                        shared_capability_route_fetch_required_failure_message(route)
                    {
                        fetch_failure
                    } else if should_offer_forge {
                        self.pending_forge_confirmations.insert(
                            session_key.clone(),
                            PendingForgeConfirmation {
                                original_request: effective_user_text.clone(),
                            },
                        );
                        "当前工具集中还没有直接完成这类任务的现成能力，而且这次也没有成功触发有效工具调用。\n\n如果你愿意，我下一步可以尝试临时使用 `forge_skill` 造一个受控工具来完成它。直接回复“继续”即可；如果你不想这么做，回复“取消”就行。"
                            .to_string()
                    } else {
                        response
                    }
                } else if should_offer_forge {
                    self.pending_forge_confirmations.insert(
                        session_key.clone(),
                        PendingForgeConfirmation {
                            original_request: effective_user_text.clone(),
                        },
                    );
                    "当前工具集中还没有直接完成这类任务的现成能力，而且这次也没有成功触发有效工具调用。\n\n如果你愿意，我下一步可以尝试临时使用 `forge_skill` 造一个受控工具来完成它。直接回复“继续”即可；如果你不想这么做，回复“取消”就行。"
                        .to_string()
                } else {
                    response
                }
            } else if should_offer_forge {
                self.pending_forge_confirmations.insert(
                    session_key.clone(),
                    PendingForgeConfirmation {
                        original_request: effective_user_text.clone(),
                    },
                );
                "当前工具集中还没有直接完成这类任务的现成能力，而且这次也没有成功触发有效工具调用。\n\n如果你愿意，我下一步可以尝试临时使用 `forge_skill` 造一个受控工具来完成它。直接回复“继续”即可；如果你不想这么做，回复“取消”就行。"
                    .to_string()
            } else {
                response
            }
        } else {
            compact_external_error_message(&msg.channel, "bridge_chat_error")
        };

        if full_response.is_empty() {
            full_response = "I'm sorry, I couldn't generate a response.".to_string();
        }

        if !self.is_current_execution_generation(&session_key, execution_generation) {
            warn!(
                "Discarding stale response for session {} at generation {}",
                session_key, execution_generation
            );
            return Ok(());
        }

        full_response = sanitize_realtime_outbound_response(&msg.channel, full_response);

        // 4. Update History
        history.push(Message::assistant(full_response.clone()));

        // 5. Save (Cache + Persistence with Reliability)
        self.cache
            .insert(session_key.clone(), history.clone())
            .await;

        let mut save_success = false;
        for attempt in 1..=PERSISTENCE_RETRIES {
            match self.store.save(&session_key, &history).await {
                Ok(_) => {
                    save_success = true;
                    break;
                }
                Err(e) => {
                    warn!(
                        "Persistence attempt {} failed for session {}: {}",
                        attempt, session_key, e
                    );
                    tokio::time::sleep(tokio::time::Duration::from_millis(100 * attempt as u64))
                        .await;
                }
            }
        }

        if !save_success {
            error!("CRITICAL: Failed to persist session {} after {} retries. Data may be lost on restart.", session_key, PERSISTENCE_RETRIES);
            self.record_channel_failure(
                &msg.channel,
                "persistence_pressure",
                format!(
                    "failed to persist session {} after {} retries",
                    session_key, PERSISTENCE_RETRIES
                ),
                Some(&msg.chat_id),
            );
            // Optionally notify user via bus
            let warning = OutboundMessage::new(
                msg.channel.clone(),
                msg.chat_id.clone(),
                "⚠️ **Warning**: System storage is currently under pressure. Your recent conversation might not be fully persisted if the service restarts.".to_string()
            );
            self.record_channel_outbound(
                &warning.channel,
                Some(&warning.chat_id),
                Some(&session_key),
            );
            let _ = self.bus.publish_outbound(warning).await;
        }

        // 6. Send Response
        let outbound = OutboundMessage::new(msg.channel, msg.chat_id, full_response);
        self.record_channel_outbound(
            &outbound.channel,
            Some(&outbound.chat_id),
            Some(&session_key),
        );
        self.bus.publish_outbound(outbound).await?;

        let reaped = self.coordinator.reap_idle_workers(IDLE_WORKER_TTL);
        if !reaped.is_empty() {
            info!(
                "Reaped idle workers after session {}: {}",
                session_key,
                reaped
                    .into_iter()
                    .map(|role| role.name().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }

        Ok(())
    }

    /// Cleanup stale sessions from persistent storage
    pub async fn cleanup_sessions(&self, max_age_days: u32) -> anyhow::Result<usize> {
        self.store
            .delete_stale(max_age_days)
            .await
            .map_err(|e| anyhow::anyhow!(e))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        approved_forge_failure_message, build_deferred_resume_request,
        build_deferred_status_message, build_forge_approved_system_message,
        build_pending_clarification_cancelled_message,
        build_pending_clarification_resolved_message, build_pending_clarification_resume_request,
        build_pending_clarification_status_message,
        build_pending_clarification_status_message_record,
        build_pending_clarification_status_surface_record, classify_capability_route,
        classify_conversational_control_intent, detect_skill_runtime_bias,
        is_natural_language_continue_confirmation, is_natural_language_reject_confirmation,
        merge_pending_forge_request, push_deferred_followup, queue_deferred_request,
        realtime_lookup_has_fetch, recover_pending_clarification_from_history,
        resolve_pending_forge_confirmation, run_trace_indicates_failed_forge_attempt,
        sanitize_realtime_outbound_response, should_force_document_hard_route,
        should_offer_forge_confirmation, CapabilityRoute, ConversationalControlIntent,
        DeferredSessionInput, IntentClassification, PendingClarification, PendingForgeConfirmation,
        PendingForgeResolution, SessionExecutionTracker,
    };
    use benshu_brain::agent::protocol::ToolCallData;
    use benshu_brain::agent::SessionStatus;
    use benshu_brain::skills::tool::{
        capability_route_clarification_message, capability_route_fetch_required_failure_message,
        capability_route_requires_source_fetch,
        capability_route_system_message as shared_capability_route_system_message,
        capability_route_tool_required_failure_message, resolve_capability_route,
        CapabilityClarificationHint, CapabilityRouteRequest, CapabilityRouter, RealtimeLookupKind,
    };
    use benshu_builtin_tools::SkillLoader;
    use benshu_infra::bus::{MediaAttachment, MediaType};
    use benshu_telemetry::{RunTrace, ToolTrace, TraceStatus};
    use chrono::Utc;
    use tempfile::tempdir;
    use uuid::Uuid;

    fn write_skill(dir: &std::path::Path, name: &str, content: &str) {
        let skill_dir = dir.join(name);
        std::fs::create_dir_all(&skill_dir).expect("create skill dir");
        std::fs::write(skill_dir.join("SKILL.md"), content).expect("write SKILL.md");
    }

    fn classify(input: &str) -> IntentClassification {
        classify_conversational_control_intent(input)
    }

    #[test]
    fn classifies_explicit_stop_requests() {
        let result = classify("停止，别做了");
        assert_eq!(result.intent, ConversationalControlIntent::Stop);
        assert_eq!(result.reason, "explicit_stop_request");
    }

    #[test]
    fn classifies_pause_requests() {
        let result = classify("等一下，我补充一句");
        assert_eq!(result.intent, ConversationalControlIntent::Pause);
        assert_eq!(result.reason, "temporary_pause_request");
    }

    #[test]
    fn classifies_status_queries() {
        let result = classify("做好了吗");
        assert_eq!(result.intent, ConversationalControlIntent::StatusQuery);
        assert_eq!(result.reason, "status_query_request");
    }

    #[test]
    fn classifies_reprioritize_requests() {
        let result = classify("我有新想法，先做数据库迁移");
        assert_eq!(result.intent, ConversationalControlIntent::Reprioritize);
        assert_eq!(result.reason, "priority_shift_request");
    }

    #[test]
    fn treats_other_active_input_as_interjection() {
        let result = classify("这里再补一个路径参数");
        assert_eq!(result.intent, ConversationalControlIntent::Interject);
        assert_eq!(result.reason, "active_session_new_input");
    }

    #[test]
    fn bot_channel_commands_cover_stop_pause_reprioritize_and_interject() {
        let stop = classify("/stop");
        assert_eq!(stop.intent, ConversationalControlIntent::Stop);
        assert_eq!(stop.reason, "explicit_stop_request");

        let pause = classify("hold on, I need to add one more detail");
        assert_eq!(pause.intent, ConversationalControlIntent::Pause);
        assert_eq!(pause.reason, "temporary_pause_request");

        let reprioritize = classify("actually let's switch to the Windows build first");
        assert_eq!(
            reprioritize.intent,
            ConversationalControlIntent::Reprioritize
        );
        assert_eq!(reprioritize.reason, "priority_shift_request");

        let status = classify("还在吗");
        assert_eq!(status.intent, ConversationalControlIntent::StatusQuery);
        assert_eq!(status.reason, "status_query_request");

        let interject = classify("再补一个日志路径参数");
        assert_eq!(interject.intent, ConversationalControlIntent::Interject);
        assert_eq!(interject.reason, "active_session_new_input");
    }

    #[test]
    fn telegram_sanitization_truncates_template_bleed() {
        let sanitized = sanitize_realtime_outbound_response(
            "telegram",
            "I am your general purpose assistant.\n\nUser: What can you do for me?\nAssistant: I can help."
                .to_string(),
        );
        assert_eq!(sanitized, "I am your general purpose assistant.");

        let sanitized = sanitize_realtime_outbound_response(
            "telegram",
            "是你的通用助手，旨在帮助你完成任务。\n\n---\n\nUser: 继续".to_string(),
        );
        assert_eq!(sanitized, "是你的通用助手，旨在帮助你完成任务。");
    }

    #[test]
    fn telegram_sanitization_compacts_internal_errors() {
        let sanitized = sanitize_realtime_outbound_response(
            "telegram",
            "I encountered an error: provider error: inference failed".to_string(),
        );
        assert!(sanitized.contains("抱歉"));
        assert!(!sanitized.contains("provider error"));
    }

    #[test]
    fn non_realtime_channels_keep_original_response() {
        let original = "Final Answer:\nhello".to_string();
        assert_eq!(
            sanitize_realtime_outbound_response("api", original.clone()),
            original
        );
    }

    #[test]
    fn recognizes_natural_language_forge_confirmation() {
        assert!(is_natural_language_continue_confirmation("继续"));
        assert!(is_natural_language_continue_confirmation("确认继续"));
        assert!(is_natural_language_continue_confirmation("go ahead"));
        assert!(!is_natural_language_continue_confirmation(
            "你可以使用pdf吗"
        ));
    }

    #[test]
    fn recognizes_natural_language_forge_rejection() {
        assert!(is_natural_language_reject_confirmation("取消"));
        assert!(is_natural_language_reject_confirmation("不用了"));
        assert!(!is_natural_language_reject_confirmation(
            "取消订单之前先看看"
        ));
    }

    #[test]
    fn offers_forge_confirmation_when_model_only_promises_action() {
        assert!(should_offer_forge_confirmation(
            "帮我生成一个健身计划pdf",
            "好的，我会为你生成一个 PDF 文档，请稍等。",
            0
        ));
        assert!(!should_offer_forge_confirmation(
            "帮我生成一个健身计划pdf",
            "已生成文件并保存到 /tmp/workout_plan.pdf",
            0
        ));
        assert!(!should_offer_forge_confirmation(
            "帮我生成一个健身计划pdf",
            "好的，我会为你生成一个 PDF 文档，请稍等。",
            1
        ));
    }

    #[test]
    fn does_not_offer_forge_confirmation_for_search_requests() {
        assert!(!should_offer_forge_confirmation(
            "帮我搜索网页我要知道btc现在的价格",
            "我来帮你搜索一下比特币价格，请稍等。",
            0
        ));
    }

    #[test]
    fn realtime_lookup_classification_splits_price_fx_weather_latest_and_search() {
        assert_eq!(
            resolve_capability_route(
                "帮我搜索网页我要知道btc现在的价格",
                CapabilityRouteRequest::default(),
            ),
            Some(CapabilityRoute::RealtimeLookup(
                RealtimeLookupKind::PriceLookup
            ))
        );
        assert_eq!(
            resolve_capability_route(
                "帮我查一下美元兑人民币汇率",
                CapabilityRouteRequest::default(),
            ),
            Some(CapabilityRoute::RealtimeLookup(
                RealtimeLookupKind::FxLookup
            ))
        );
        assert_eq!(
            resolve_capability_route("上海明天天气怎么样", CapabilityRouteRequest::default(),),
            Some(CapabilityRoute::RealtimeLookup(
                RealtimeLookupKind::WeatherLookup
            ))
        );
        assert_eq!(
            resolve_capability_route("OpenAI 今天最新消息", CapabilityRouteRequest::default(),),
            Some(CapabilityRoute::RealtimeLookup(
                RealtimeLookupKind::LatestInfoLookup
            ))
        );
        assert_eq!(
            resolve_capability_route(
                "帮我搜索一下好用的 pdf 工具",
                CapabilityRouteRequest::default(),
            ),
            Some(CapabilityRoute::RealtimeLookup(
                RealtimeLookupKind::WebSearch
            ))
        );
        assert_eq!(
            resolve_capability_route(
                "帮我做一个搜索 btc 价格的工具",
                CapabilityRouteRequest {
                    suppress_document_understanding: true,
                    suppress_realtime_lookup: true,
                    ..Default::default()
                },
            ),
            None
        );
    }

    #[test]
    fn capability_route_prioritizes_document_before_realtime_lookup() {
        let media = vec![MediaAttachment {
            media_type: MediaType::Document,
            url: "https://example.com/report.pdf".to_string(),
            caption: None,
        }];

        assert_eq!(
            classify_capability_route("帮我总结这个 PDF 里今天最新信息", Some(&media), false, None),
            Some(CapabilityRoute::DocumentUnderstanding)
        );

        assert_eq!(
            classify_capability_route("帮我查 BTC 现在价格", None, false, None),
            Some(CapabilityRoute::RealtimeLookup(
                RealtimeLookupKind::PriceLookup
            ))
        );

        assert_eq!(
            classify_capability_route("帮我查 BTC 现在价格", None, true, None),
            None
        );
    }

    #[test]
    fn capability_route_classifies_runtime_surface_and_external_cli_tools() {
        assert_eq!(
            classify_capability_route("用 git cli 看当前分支", None, false, None),
            Some(CapabilityRoute::ExternalCliTools)
        );
        assert_eq!(
            classify_capability_route("用 powershell 列出当前目录", None, false, None),
            Some(CapabilityRoute::RuntimeSurface)
        );
    }

    #[tokio::test]
    async fn capability_route_uses_skill_runtime_bias_for_runtime_surface() {
        let temp = tempdir().expect("tempdir");
        write_skill(
            temp.path(),
            "git_helper",
            r#"---
name: git_helper
description: Git helper workflow
runtime: bash
script: run.sh
---
# Git Helper

Use this skill when the user asks for the git_helper workflow.
"#,
        );

        let loader = SkillLoader::new(temp.path());
        loader.load_all().await.expect("load skills");

        let bias =
            detect_skill_runtime_bias("请用 git_helper 帮我处理这个仓库", &loader).expect("bias");
        assert_eq!(bias.skill_name, "git_helper");
        assert_eq!(bias.runtime, "bash");

        assert_eq!(
            classify_capability_route(
                "请用 git_helper 帮我处理这个仓库",
                None,
                false,
                Some(&loader),
            ),
            Some(CapabilityRoute::RuntimeSurface)
        );
    }

    #[test]
    fn realtime_lookup_prompt_requires_real_tool_usage() {
        let prompt = shared_capability_route_system_message(
            "帮我搜索网页我要知道btc现在的价格",
            CapabilityRoute::RealtimeLookup(RealtimeLookupKind::PriceLookup),
            None,
            None,
        )
        .unwrap_or_default();
        assert!(prompt.contains("real-time price / market lookup"));
        assert!(prompt.contains("must use an existing search-capable or lookup-capable tool"));
        assert!(prompt.contains("web_fetch"));
        assert!(prompt.contains("browser_browse"));
        assert!(prompt.contains("Absolute date today"));
        assert!(prompt.contains("Query rewrite hint"));
        assert!(prompt.contains("<asset or symbol> price"));
        assert!(prompt.contains("Do not present remembered or estimated prices"));
        assert!(prompt.contains("Source-priority rules"));
        assert!(prompt.contains("exchange pages"));
        assert!(prompt.contains("lookup tool was not successfully invoked"));
    }

    #[test]
    fn runtime_surface_prompt_requires_real_runtime_tool_usage() {
        let prompt = shared_capability_route_system_message(
            "用 powershell 列出当前目录",
            CapabilityRoute::RuntimeSurface,
            None,
            None,
        )
        .unwrap_or_default();
        assert!(prompt.contains("runtime-surface execution task"));
        assert!(prompt.contains("must use an existing runtime-surface tool"));
        assert!(prompt.contains("tool_search"));
        assert!(prompt.contains("Do not pretend"));
    }

    #[test]
    fn runtime_surface_prompt_prefers_progressive_skill_loading_when_skill_matches() {
        let prompt = shared_capability_route_system_message(
            "按 python_tooling 这个 skill 来处理",
            CapabilityRoute::RuntimeSurface,
            None,
            Some("python_tooling"),
        )
        .unwrap_or_default();
        assert!(prompt.contains("read_skill_manual"));
        assert!(prompt.contains("python_tooling"));
    }

    #[test]
    fn external_cli_tools_prompt_requires_real_cli_tool_usage() {
        let prompt = shared_capability_route_system_message(
            "用 git cli 看当前分支",
            CapabilityRoute::ExternalCliTools,
            None,
            None,
        )
        .unwrap_or_default();
        assert!(prompt.contains("CLI / command execution task"));
        assert!(prompt.contains("external program CLI task"));
        assert!(prompt.contains("must use an existing external-CLI-capable tool"));
        assert!(prompt.contains("tool_search"));
        assert!(prompt.contains("Do not pretend"));
    }

    #[test]
    fn realtime_lookup_query_rewrite_hints_are_specific() {
        let price = shared_capability_route_system_message(
            "帮我查 BTC 现在价格",
            CapabilityRoute::RealtimeLookup(RealtimeLookupKind::PriceLookup),
            None,
            None,
        )
        .unwrap_or_default();
        assert!(price.contains("<asset or symbol> price"));

        let fx = shared_capability_route_system_message(
            "美元兑人民币今天汇率",
            CapabilityRoute::RealtimeLookup(RealtimeLookupKind::FxLookup),
            None,
            None,
        )
        .unwrap_or_default();
        assert!(fx.contains("USD CNY exchange rate"));

        let weather = shared_capability_route_system_message(
            "上海明天天气",
            CapabilityRoute::RealtimeLookup(RealtimeLookupKind::WeatherLookup),
            None,
            None,
        )
        .unwrap_or_default();
        assert!(weather.contains("<location> weather"));

        let latest = shared_capability_route_system_message(
            "OpenAI 最新消息",
            CapabilityRoute::RealtimeLookup(RealtimeLookupKind::LatestInfoLookup),
            None,
            None,
        )
        .unwrap_or_default();
        assert!(latest.contains("latest news"));
    }

    #[test]
    fn realtime_lookup_missing_context_requires_clarification() {
        let router = CapabilityRouter::default();
        assert_eq!(
            router.clarification_hint("帮我查一下价格"),
            Some(CapabilityClarificationHint::MissingPriceTarget)
        );
        assert_eq!(
            router.clarification_hint("帮我查一下汇率"),
            Some(CapabilityClarificationHint::MissingFxPair)
        );
        assert_eq!(
            router.clarification_hint("明天天气怎么样"),
            Some(CapabilityClarificationHint::MissingWeatherLocation)
        );
        assert_eq!(router.clarification_hint("BTC 现在多少钱"), None);
        assert_eq!(router.clarification_hint("美元兑人民币汇率"), None);
        assert_eq!(router.clarification_hint("上海明天天气怎么样"), None);
    }

    #[test]
    fn realtime_lookup_clarification_messages_are_user_facing() {
        let price = capability_route_clarification_message(
            "帮我查一下价格",
            CapabilityRoute::RealtimeLookup(RealtimeLookupKind::PriceLookup),
        )
        .unwrap_or_default();
        assert!(price.contains("实时价格"));

        let fx = capability_route_clarification_message(
            "帮我查一下汇率",
            CapabilityRoute::RealtimeLookup(RealtimeLookupKind::FxLookup),
        )
        .unwrap_or_default();
        assert!(fx.contains("汇率"));

        let weather = capability_route_clarification_message(
            "明天天气怎么样",
            CapabilityRoute::RealtimeLookup(RealtimeLookupKind::WeatherLookup),
        )
        .unwrap_or_default();
        assert!(weather.contains("天气"));

        assert_eq!(
            capability_route_clarification_message(
                "帮我搜索一下",
                CapabilityRoute::RealtimeLookup(RealtimeLookupKind::WebSearch),
            ),
            None
        );
    }

    #[test]
    fn realtime_lookup_failure_messages_refuse_to_guess() {
        let search = capability_route_tool_required_failure_message(
            CapabilityRoute::RealtimeLookup(RealtimeLookupKind::WebSearch),
        );
        assert!(search.contains("没有成功调用搜索工具"));
        assert!(search.contains("不编造结果"));

        let price = capability_route_tool_required_failure_message(
            CapabilityRoute::RealtimeLookup(RealtimeLookupKind::PriceLookup),
        );
        assert!(price.contains("不编造价格或行情数据"));

        let fx = capability_route_tool_required_failure_message(CapabilityRoute::RealtimeLookup(
            RealtimeLookupKind::FxLookup,
        ));
        assert!(fx.contains("不编造汇率数据"));

        let weather = capability_route_tool_required_failure_message(
            CapabilityRoute::RealtimeLookup(RealtimeLookupKind::WeatherLookup),
        );
        assert!(weather.contains("不猜测天气"));

        let latest = capability_route_tool_required_failure_message(
            CapabilityRoute::RealtimeLookup(RealtimeLookupKind::LatestInfoLookup),
        );
        assert!(latest.contains("不编造最新信息"));
    }

    #[test]
    fn runtime_surface_failure_message_refuses_to_guess() {
        let message =
            capability_route_tool_required_failure_message(CapabilityRoute::RuntimeSurface);
        assert!(message.contains("没有成功调用运行时工具"));
        assert!(message.contains("不编造脚本输出"));
    }

    #[test]
    fn external_cli_tools_failure_message_refuses_to_guess() {
        let message =
            capability_route_tool_required_failure_message(CapabilityRoute::ExternalCliTools);
        assert!(message.contains("没有成功调用外部程序的 CLI 工具"));
        assert!(message.contains("不编造分支状态"));
    }

    #[test]
    fn realtime_lookup_fetch_gate_requires_source_page_for_sensitive_queries() {
        assert!(!capability_route_requires_source_fetch(
            CapabilityRoute::RealtimeLookup(RealtimeLookupKind::WebSearch)
        ));
        assert!(capability_route_requires_source_fetch(
            CapabilityRoute::RealtimeLookup(RealtimeLookupKind::PriceLookup)
        ));
        assert!(capability_route_requires_source_fetch(
            CapabilityRoute::RealtimeLookup(RealtimeLookupKind::FxLookup)
        ));
        assert!(capability_route_requires_source_fetch(
            CapabilityRoute::RealtimeLookup(RealtimeLookupKind::WeatherLookup)
        ));
        assert!(capability_route_requires_source_fetch(
            CapabilityRoute::RealtimeLookup(RealtimeLookupKind::LatestInfoLookup)
        ));

        let only_search = vec![ToolCallData {
            receipt_id: None,
            tool_call_id: None,
            name: "web_search".to_string(),
            args: "{}".to_string(),
            result: Some("[]".to_string()),
            backup: None,
            duration_ms: 10,
            timestamp: 0,
            caller_id: None,
            safety_level: Default::default(),
            cpu_pressure: None,
            vram_pressure: None,
            result_truncated: false,
            result_original_chars: None,
            result_omitted_chars: None,
            args_fingerprint: None,
            result_fingerprint: None,
            outcome: None,
            replay: None,
        }];
        assert!(!realtime_lookup_has_fetch(&only_search));

        let with_fetch = vec![ToolCallData {
            receipt_id: None,
            tool_call_id: None,
            name: "web_fetch".to_string(),
            args: "{}".to_string(),
            result: Some("content".to_string()),
            backup: None,
            duration_ms: 10,
            timestamp: 0,
            caller_id: None,
            safety_level: Default::default(),
            cpu_pressure: None,
            vram_pressure: None,
            result_truncated: false,
            result_original_chars: None,
            result_omitted_chars: None,
            args_fingerprint: None,
            result_fingerprint: None,
            outcome: None,
            replay: None,
        }];
        assert!(realtime_lookup_has_fetch(&with_fetch));

        let failure = capability_route_fetch_required_failure_message(
            CapabilityRoute::RealtimeLookup(RealtimeLookupKind::PriceLookup),
        )
        .unwrap_or_default();
        assert!(failure.contains("读取到足够可靠的来源页面"));
    }

    #[test]
    fn document_hard_route_detects_media_and_understanding_requests() {
        let media = vec![MediaAttachment {
            media_type: MediaType::Document,
            url: "https://example.com/report.pdf".to_string(),
            caption: None,
        }];
        assert!(should_force_document_hard_route("帮我总结这个pdf"));
        assert!(should_force_document_hard_route("帮我看一下这张图片"));
        assert!(!should_force_document_hard_route("帮我做一个图片理解工具"));
        assert_eq!(
            classify_capability_route("这个附件讲了什么", Some(&media), false, None),
            Some(CapabilityRoute::DocumentUnderstanding)
        );
        assert_eq!(
            classify_capability_route("帮我做一个图片理解工具", Some(&media), false, None),
            None
        );
    }

    #[test]
    fn document_hard_route_prompt_prioritizes_coordination_before_execution() {
        let _media = vec![
            MediaAttachment {
                media_type: MediaType::Image,
                url: "https://example.com/image.png".to_string(),
                caption: None,
            },
            MediaAttachment {
                media_type: MediaType::Voice,
                url: "https://example.com/audio.ogg".to_string(),
                caption: None,
            },
        ];
        let prompt = shared_capability_route_system_message(
            "帮我看图并转写语音",
            CapabilityRoute::DocumentUnderstanding,
            Some("image, voice"),
            None,
        )
        .unwrap_or_default();
        assert!(prompt.contains("BenShu stays in coordinator posture first"));
        assert!(prompt.contains("prefer `delegate`"));
        assert!(prompt.contains("Detected media: image, voice"));
        assert!(prompt.contains("Do not pretend"));
    }

    #[test]
    fn document_hard_route_failure_message_refuses_to_guess() {
        let message =
            capability_route_tool_required_failure_message(CapabilityRoute::DocumentUnderstanding);
        assert!(message.contains("没有成功调用文档理解工具"));
        assert!(message.contains("不猜测附件内容"));
    }

    #[test]
    fn still_offers_forge_confirmation_for_explicit_tool_creation_requests() {
        assert!(should_offer_forge_confirmation(
            "帮我做一个搜索 btc 价格的工具",
            "好的，我会帮你做一个工具，请稍等。",
            0
        ));
    }

    #[test]
    fn generic_artifact_requests_do_not_offer_forge_confirmation() {
        assert!(!should_offer_forge_confirmation(
            "搜索资料并保存成 pdf",
            "好的，我会开始准备，请稍等。",
            0
        ));
        assert!(!should_offer_forge_confirmation(
            "写一篇小说并保存为 txt",
            "我会继续处理并生成文件。",
            0
        ));
    }

    #[test]
    fn pending_forge_continue_overrides_status_like_wording() {
        let pending = PendingForgeConfirmation {
            original_request: "帮我做一个 pdf 生成工具".to_string(),
        };

        let resolution = resolve_pending_forge_confirmation(&pending, "继续", &classify("继续"));
        assert_eq!(
            resolution,
            PendingForgeResolution::Approve {
                request: "帮我做一个 pdf 生成工具".to_string()
            }
        );
    }

    #[test]
    fn pending_forge_followup_text_updates_original_request() {
        let pending = PendingForgeConfirmation {
            original_request: "帮我做一个 pdf 生成工具".to_string(),
        };

        let resolution = resolve_pending_forge_confirmation(
            &pending,
            "包含图片和文档",
            &classify("包含图片和文档"),
        );
        assert_eq!(
            resolution,
            PendingForgeResolution::Supplement {
                updated_request: "帮我做一个 pdf 生成工具\n补充要求：包含图片和文档".to_string()
            }
        );
    }

    #[test]
    fn merge_pending_forge_request_keeps_existing_request() {
        assert_eq!(
            merge_pending_forge_request("原需求", "补充条件"),
            "原需求\n补充要求：补充条件"
        );
    }

    #[test]
    fn approved_forge_prompt_requires_execution_not_discussion() {
        let prompt = build_forge_approved_system_message("帮我做一个 pdf 生成工具");
        assert!(prompt.contains("call `forge_skill` in this turn"));
        assert!(prompt.contains("Do not merely describe what you would do"));
        assert!(prompt.contains("This is an execution turn"));
        assert!(prompt.contains("session-scoped tool"));
        assert!(prompt.contains("sandbox-friendly"));
        assert!(prompt.contains("smoke-test"));
    }

    #[test]
    fn forge_failure_detection_surfaces_failed_or_unregistered_attempts() {
        let mut run_trace = RunTrace {
            run_id: Uuid::nil(),
            session_id: Uuid::nil(),
            agent_id: "test".to_string(),
            status: TraceStatus::Succeeded,
            started_at: Utc::now(),
            finished_at: Some(Utc::now()),
            task_id: None,
            thread_id: None,
            provider: None,
            model: None,
            prompt_tokens: None,
            completion_tokens: None,
            stages: Vec::new(),
            tools: vec![ToolTrace {
                call_id: "call-1".to_string(),
                tool_name: "forge_skill".to_string(),
                status: TraceStatus::Failed,
                started_at: Utc::now(),
                finished_at: Some(Utc::now()),
                duration_ms: Some(12),
                input: None,
                output: None,
                error: Some("compile failed".to_string()),
                degraded: false,
            }],
            artifacts: Vec::new(),
            degradation_notes: Vec::new(),
            witness: None,
            metadata: std::collections::HashMap::new(),
        };
        assert!(run_trace_indicates_failed_forge_attempt(&run_trace));

        run_trace.tools[0].status = TraceStatus::Succeeded;
        run_trace.tools[0].error = None;
        run_trace.metadata.insert(
            "forge_registered_tools".to_string(),
            "pdf_builder".to_string(),
        );
        run_trace
            .metadata
            .insert("forge_surface_present".to_string(), "true".to_string());
        run_trace.metadata.insert(
            "forge_smoke_statuses".to_string(),
            "pdf_builder:passed".to_string(),
        );
        assert!(!run_trace_indicates_failed_forge_attempt(&run_trace));
    }

    #[test]
    fn approved_forge_failure_message_is_explicitly_user_facing() {
        let message = approved_forge_failure_message();
        assert!(message.contains("已经按你的确认尝试"));
        assert!(message.contains("forge 没有成功完成验证或注册"));
        assert!(message.contains("不会假装它已经可用"));
    }

    #[test]
    fn deferred_followups_build_resume_request() {
        let deferred = push_deferred_followup(None, "再加一个导出按钮");
        assert_eq!(
            build_deferred_resume_request(deferred),
            Some("继续处理刚才那件事。\n补充信息：\n再加一个导出按钮".to_string())
        );
    }

    #[test]
    fn queued_request_keeps_followups_for_later_resume() {
        let deferred = push_deferred_followup(None, "包含图片");
        let deferred = queue_deferred_request(Some(deferred), "改做一个网页抓取器");
        assert_eq!(
            build_deferred_resume_request(deferred),
            Some("改做一个网页抓取器\n\n补充信息：\n包含图片".to_string())
        );
    }

    #[test]
    fn deferred_status_message_mentions_waiting_request() {
        let deferred = DeferredSessionInput {
            followups: vec!["包含图片".to_string(), "包含文档".to_string()],
            queued_request: Some("改做一个 PDF 生成工具".to_string()),
        };
        let status = build_deferred_status_message(&deferred);
        assert!(status.contains("当前没有正在执行的任务"));
        assert!(status.contains("改做一个 PDF 生成工具"));
        assert!(status.contains("2 条补充信息"));
    }

    #[test]
    fn pending_clarification_status_message_is_user_facing() {
        let pending = PendingClarification {
            original_request: "帮我查天气".to_string(),
            clarification: "你想查哪个城市的天气？".to_string(),
        };
        let status = build_pending_clarification_status_message(&pending);
        assert!(status.contains("等你补充关键信息"));
        assert!(status.contains("你想查哪个城市的天气"));
    }

    #[test]
    fn pending_clarification_resume_request_merges_user_reply() {
        let pending = PendingClarification {
            original_request: "帮我查天气".to_string(),
            clarification: "你想查哪个城市的天气？".to_string(),
        };
        assert_eq!(
            build_pending_clarification_resume_request(&pending, "上海明天"),
            "帮我查天气\n补充信息：\n上海明天"
        );
    }

    #[test]
    fn pending_clarification_status_record_uses_shared_session_contract() {
        let pending = PendingClarification {
            original_request: "帮我查天气".to_string(),
            clarification: "你想查哪个城市的天气？".to_string(),
        };
        let record = build_pending_clarification_status_message_record(&pending);
        assert_eq!(
            record.metadata.get("session_status"),
            Some(&"awaiting_clarification".to_string())
        );
        let decoded = record
            .metadata
            .get("session_status_json")
            .and_then(|value| serde_json::from_str::<SessionStatus>(value).ok());
        assert_eq!(
            decoded,
            Some(pending.as_contract_state().as_session_status())
        );
        assert_eq!(
            record.metadata.get("clarification_prompt"),
            Some(&"你想查哪个城市的天气？".to_string())
        );
        assert_eq!(
            record.metadata.get("clarification_original_request"),
            Some(&"帮我查天气".to_string())
        );
        assert_eq!(
            record.metadata.get("clarification_status_kind"),
            Some(&"awaiting_clarification".to_string())
        );
    }

    #[test]
    fn pending_clarification_status_surface_record_marks_surface_event() {
        let pending = PendingClarification {
            original_request: "帮我查天气".to_string(),
            clarification: "你想查哪个城市的天气？".to_string(),
        };
        let record = build_pending_clarification_status_surface_record(&pending);
        assert_eq!(
            record.metadata.get("session_status"),
            Some(&"awaiting_clarification".to_string())
        );
        assert_eq!(
            record.metadata.get("clarification_status_surface"),
            Some(&"true".to_string())
        );
    }

    #[test]
    fn pending_clarification_resolved_record_returns_to_thinking() {
        let pending = PendingClarification {
            original_request: "帮我查天气".to_string(),
            clarification: "你想查哪个城市的天气？".to_string(),
        };
        let record = build_pending_clarification_resolved_message(&pending);
        assert_eq!(
            record.metadata.get("session_status"),
            Some(&"thinking".to_string())
        );
        let decoded = record
            .metadata
            .get("session_status_json")
            .and_then(|value| serde_json::from_str::<SessionStatus>(value).ok());
        assert_eq!(decoded, Some(SessionStatus::Thinking));
        assert_eq!(
            record.metadata.get("clarification_resolved"),
            Some(&"true".to_string())
        );
        assert_eq!(
            record.metadata.get("clarification_status_kind"),
            Some(&"thinking".to_string())
        );
    }

    #[test]
    fn pending_clarification_cancelled_record_marks_failed_status() {
        let pending = PendingClarification {
            original_request: "帮我查天气".to_string(),
            clarification: "你想查哪个城市的天气？".to_string(),
        };
        let record = build_pending_clarification_cancelled_message(
            &pending,
            "clarification_cancelled_by_user",
        );
        assert_eq!(
            record.metadata.get("session_status"),
            Some(&"failed".to_string())
        );
        let decoded = record
            .metadata
            .get("session_status_json")
            .and_then(|value| serde_json::from_str::<SessionStatus>(value).ok());
        assert_eq!(
            decoded,
            Some(SessionStatus::Failed(
                "clarification_cancelled_by_user".to_string()
            ))
        );
        assert_eq!(
            record.metadata.get("clarification_cancelled"),
            Some(&"true".to_string())
        );
        assert_eq!(
            record.metadata.get("clarification_status_kind"),
            Some(&"failed".to_string())
        );
        assert_eq!(
            record.metadata.get("clarification_failure_reason"),
            Some(&"clarification_cancelled_by_user".to_string())
        );
    }

    #[test]
    fn recover_pending_clarification_from_history_prefers_latest_waiting_state() {
        let pending = PendingClarification {
            original_request: "帮我查天气".to_string(),
            clarification: "你想查哪个城市的天气？".to_string(),
        };
        let history = vec![build_pending_clarification_status_message_record(&pending)];
        assert_eq!(
            recover_pending_clarification_from_history(&history),
            Some(pending)
        );
    }

    #[test]
    fn recover_pending_clarification_from_history_stops_after_resolution() {
        let pending = PendingClarification {
            original_request: "帮我查天气".to_string(),
            clarification: "你想查哪个城市的天气？".to_string(),
        };
        let history = vec![
            build_pending_clarification_status_message_record(&pending),
            build_pending_clarification_resolved_message(&pending),
        ];
        assert_eq!(recover_pending_clarification_from_history(&history), None);
    }

    #[test]
    fn recover_pending_clarification_from_history_stops_after_cancellation() {
        let pending = PendingClarification {
            original_request: "帮我查天气".to_string(),
            clarification: "你想查哪个城市的天气？".to_string(),
        };
        let history = vec![
            build_pending_clarification_status_message_record(&pending),
            build_pending_clarification_cancelled_message(
                &pending,
                "clarification_cancelled_by_user",
            ),
        ];
        assert_eq!(recover_pending_clarification_from_history(&history), None);
    }

    #[test]
    fn session_execution_tracker_invalidates_prior_generations() {
        let tracker = SessionExecutionTracker::default();
        let first = tracker.start("s1");
        assert_eq!(first, 1);
        assert!(tracker.is_current("s1", first));

        tracker.invalidate("s1");
        assert!(!tracker.is_current("s1", first));

        let latest = tracker.start("s1");
        assert_eq!(latest, 3);
        assert!(tracker.is_current("s1", latest));
    }
}
