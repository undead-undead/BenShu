use crate::api::media::{append_media_context_parts, inbound_media_to_parts};
use crate::api::state::{AppError, AppState};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::sse::{Event, Sse},
    Json,
};
use benshu_brain::agent::message::{Content, Message as AgentMessage};
use benshu_brain::agent::protocol::{
    AgentEvent, AgentEventData, AgentRole, ChatOutcome, SafetyLevel, ToolCallData,
};
use benshu_brain::skills::tool::{
    capability_route_requires_real_tool_call, classify_query_capability_route, CapabilityRouteHint,
    Tool,
};
use benshu_builtin_tools::tool::delegation::DelegateTool;
use benshu_builtin_tools::tool::writing::creation_contract::{
    intent_requests_existing_work_continuation, CREATION_PLANNING_DIALOGUE_MARKER,
};
use benshu_builtin_tools::tool::writing::novel_workflow_driver::{
    run_novel_content_operation_for_delegate, run_novel_workflow_for_delegate,
    NovelContentOperationConfig, NovelWorkflowConfig, NovelWorkflowRuntimeState,
};
use benshu_builtin_tools::tool::writing::session_route::{
    self as writing_session_route, DirectWriterRoute,
};
use benshu_builtin_tools::tool::writing::session_surface as writing_session_surface;
use benshu_builtin_tools::tool::{FactManagementTool, MultimodalMemoryTool};
use benshu_compression::{compress_tool_output, preview_text};
use benshu_infra::bus::MediaAttachment;
use benshu_protocol_core::{DelegationMode, DelegationRecord};
use benshu_runtime_policy_core::{
    evaluate_creation_intake, is_recoverable_provider_disconnect,
    provider_health_issue_should_restart_runtime_host, provider_service_pause_reason,
    resolve_language_contract,
};
use benshu_state::{
    ArtifactQuery, ArtifactRecord, RuntimeEventRecord, RuntimeReceipt, TaskBoundary,
    TaskCheckpoint, TaskContract, TaskEvidenceRequirement, TaskState, TaskStatus, TaskVerification,
    TaskVerificationVerdict,
};
use benshu_telemetry::{
    ProfilerArtifact, ProfilerArtifactQuery, ProfilerExport, RunReplay, RunTrace, Scorecard,
    ScorecardQuery, WitnessBundle, WitnessLogEntry, WitnessLogQuery, WitnessSummary,
};
use futures::Stream;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{debug, warn};
use uuid::Uuid;

#[path = "chat_tool_host.rs"]
mod chat_tool_host;
use chat_tool_host::*;
#[path = "chat_panel_surface.rs"]
mod chat_panel_surface;
use chat_panel_surface::*;

const CHAT_FOREGROUND_SHORT_OBSERVATION_SECONDS: u64 = 5;
const CHAT_FOREGROUND_DEFAULT_OBSERVATION_SECONDS: u64 = 5;
const CHAT_FOREGROUND_REALTIME_LOOKUP_SECONDS: u64 = 10;
const CHAT_FOREGROUND_PLANNING_DIALOGUE_SECONDS: u64 = 5;
const TASK_WAIT_DEFAULT_SECONDS: u64 = 60;
const TASK_WAIT_MAX_SECONDS: u64 = 60 * 60;
const TASK_OUTPUT_DEFAULT_TAIL_LINES: usize = 80;
const TASK_OUTPUT_MAX_PREVIEW_BYTES: usize = 200_000;

#[derive(Debug, Clone)]
struct RecordedRuntimeEvent {
    event_id: Uuid,
    topic: String,
}

#[derive(Debug, Clone)]
struct SessionWorkContext {
    tasks: Vec<TaskState>,
    artifacts: Vec<ArtifactRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ForegroundControlIntent {
    Normal,
    StatusQuery,
    Stop,
    Pause,
    Resume,
    Reprioritize,
}

fn looks_like_global_control_command(trimmed: &str, matched: &str) -> bool {
    let normalized = trimmed.trim();
    if normalized.eq_ignore_ascii_case(matched) {
        return true;
    }
    if matched.chars().count() == 1 {
        return false;
    }
    if writing_session_route::message_should_bypass_foreground_control(normalized)
        && !looks_like_task_scoped_control_command(normalized, matched)
    {
        return false;
    }

    let core = normalize_control_command_text(normalized);
    if core.eq_ignore_ascii_case(matched) {
        return true;
    }
    let matched_core = normalize_control_command_text(matched);
    if !matched_core.is_empty() && core.eq_ignore_ascii_case(&matched_core) {
        return true;
    }

    looks_like_task_scoped_control_command(normalized, matched)
}

fn normalize_control_command_text(input: &str) -> String {
    let mut value = input
        .trim()
        .to_lowercase()
        .chars()
        .filter(|ch| {
            !ch.is_whitespace()
                && !matches!(
                    ch,
                    '，' | '。' | '！' | '？' | ',' | '.' | '!' | '?' | ';' | '；' | ':' | '：'
                )
        })
        .collect::<String>();

    let prefixes = ["please", "请你", "请", "麻烦你", "麻烦", "帮我", "你先"];
    loop {
        let before = value.clone();
        for prefix in prefixes {
            if let Some(rest) = value.strip_prefix(prefix) {
                value = rest.to_string();
                break;
            }
        }
        if value == before {
            break;
        }
    }

    let suffixes = [
        "一下吧",
        "一下",
        "吧",
        "当前这个任务",
        "当前任务",
        "这个任务",
        "这项任务",
        "任务",
        "执行",
        "操作",
    ];
    loop {
        let before = value.clone();
        for suffix in suffixes {
            if let Some(rest) = value.strip_suffix(suffix) {
                value = rest.to_string();
                break;
            }
        }
        if value == before {
            break;
        }
    }

    value
}

fn compact_control_command_text(input: &str) -> String {
    input
        .trim()
        .to_lowercase()
        .chars()
        .filter(|ch| {
            !ch.is_whitespace()
                && !matches!(
                    ch,
                    '，' | '。' | '！' | '？' | ',' | '.' | '!' | '?' | ';' | '；' | ':' | '：'
                )
        })
        .collect::<String>()
}

fn looks_like_task_scoped_control_command(trimmed: &str, matched: &str) -> bool {
    if matched.chars().count() <= 1 {
        return false;
    }

    let mut command = compact_control_command_text(trimmed);
    for prefix in ["please", "请你", "请", "麻烦你", "麻烦", "帮我", "你先"] {
        if let Some(rest) = command.strip_prefix(prefix) {
            command = rest.to_string();
            break;
        }
    }
    let matched_core = compact_control_command_text(matched);
    if matched_core.is_empty() || !command.starts_with(&matched_core) {
        return false;
    }

    let rest = &command[matched_core.len()..];
    if rest.is_empty() {
        return true;
    }

    rest.find("任务")
        .is_some_and(|offset| rest[..offset].chars().count() <= 8)
}

fn starts_with_task_scoped_runtime_control(trimmed: &str) -> bool {
    const RUNTIME_CONTROL_KEYWORDS: &[&str] = &[
        "stop",
        "cancel",
        "abort",
        "pause",
        "resume",
        "continue",
        "停止",
        "取消",
        "中断",
        "停下",
        "暂停",
        "先停一下",
        "等一下",
        "继续",
        "接着",
        "恢复",
    ];
    RUNTIME_CONTROL_KEYWORDS
        .iter()
        .any(|keyword| looks_like_task_scoped_control_command(trimmed, keyword))
}

fn starts_with_task_status_query(trimmed: &str) -> bool {
    let compact = compact_control_command_text(trimmed);
    [
        "任务进度",
        "任务进展",
        "任务状态",
        "查看任务进度",
        "查看任务状态",
    ]
    .iter()
    .any(|prefix| compact.starts_with(prefix))
}

fn classify_foreground_control_intent(content: &str) -> ForegroundControlIntent {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return ForegroundControlIntent::Normal;
    }

    let lowered = trimmed.to_lowercase();

    if writing_session_route::message_should_bypass_foreground_control(trimmed)
        && !starts_with_task_scoped_runtime_control(trimmed)
        && !starts_with_task_status_query(trimmed)
    {
        return ForegroundControlIntent::Normal;
    }

    let status_keywords = [
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
        "still working",
        "status",
        "progress",
    ];
    if status_keywords
        .iter()
        .copied()
        .any(|keyword| lowered.contains(keyword) || trimmed.contains(keyword))
    {
        return ForegroundControlIntent::StatusQuery;
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
        "停",
    ];
    if stop_keywords.iter().copied().any(|keyword| {
        (lowered.contains(keyword) || trimmed.contains(keyword))
            && looks_like_global_control_command(trimmed, keyword)
    }) {
        return ForegroundControlIntent::Stop;
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
    if pause_keywords.iter().copied().any(|keyword| {
        (lowered.contains(keyword) || trimmed.contains(keyword))
            && looks_like_global_control_command(trimmed, keyword)
    }) {
        return ForegroundControlIntent::Pause;
    }

    let resume_keywords = [
        "继续",
        "接着",
        "恢复",
        "开始吧",
        "继续吧",
        "继续执行",
        "resume",
        "continue",
        "go ahead",
    ];
    if resume_keywords.iter().copied().any(|keyword| {
        (lowered.contains(keyword) || trimmed.contains(keyword))
            && looks_like_global_control_command(trimmed, keyword)
    }) {
        return ForegroundControlIntent::Resume;
    }

    let has_resume_hint = ["继续", "接着", "恢复", "continue", "resume", "go ahead"]
        .iter()
        .any(|keyword| lowered.contains(keyword) || trimmed.contains(keyword));
    if has_resume_hint && writing_session_route::message_should_bypass_foreground_control(trimmed) {
        return ForegroundControlIntent::Normal;
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
    if reprioritize_keywords
        .iter()
        .copied()
        .any(|keyword| lowered.contains(keyword) || trimmed.contains(keyword))
    {
        return ForegroundControlIntent::Reprioritize;
    }

    ForegroundControlIntent::Normal
}

fn strip_tool_runtime_notice_sections(content: &str) -> String {
    const MARKERS: &[&str] = &[
        "\n---\n### NOTICE: First use of skill",
        "\n### NOTICE: First use of skill",
        " --- ### NOTICE: First use of skill",
        "--- ### NOTICE: First use of skill",
        "### NOTICE: First use of skill",
    ];

    for marker in MARKERS {
        if let Some(idx) = content.find(marker) {
            return content[..idx].trim_end().to_string();
        }
    }

    content.to_string()
}

fn sanitize_tool_trace_result(result: Option<String>) -> Option<String> {
    result.map(|value| {
        let cleaned = strip_tool_runtime_notice_sections(&value);
        compress_tool_output(&cleaned, 4_096).content
    })
}

fn quoted_segments(content: &str) -> Vec<String> {
    let mut values = Vec::new();
    let pairs = [
        ('《', '》'),
        ('「', '」'),
        ('“', '”'),
        ('"', '"'),
        ('\'', '\''),
    ];
    for (left, right) in pairs {
        let mut rest = content;
        while let Some((_, after_left)) = rest.split_once(left) {
            let Some((value, after_right)) = after_left.split_once(right) else {
                break;
            };
            let value = value.trim();
            if !value.is_empty() {
                values.push(value.to_string());
            }
            rest = after_right;
        }
        if !values.is_empty() {
            break;
        }
    }
    values
}

fn looks_like_memory_status_request(content: &str) -> bool {
    let lowered = content.to_lowercase();
    let asks_memory = content.contains("记忆系统")
        || content.contains("待审计")
        || content.contains("待审核")
        || content.contains("待复核")
        || content.contains("认知状态")
        || lowered.contains("memory status")
        || lowered.contains("memory audit");
    let asks_status = content.contains("状态")
        || content.contains("概况")
        || content.contains("健康")
        || content.contains("审计")
        || content.contains("复核")
        || lowered.contains("status")
        || lowered.contains("backlog")
        || lowered.contains("audit");
    asks_memory && asks_status
}

fn looks_like_memory_maintenance_request(content: &str) -> bool {
    let lowered = content.to_lowercase();
    [
        "记忆维护",
        "维护记忆",
        "维护一下记忆",
        "整理记忆",
        "记忆整理",
        "巩固记忆",
        "记忆巩固",
        "睡眠整理",
        "睡眠巩固",
    ]
    .iter()
    .any(|phrase| content.contains(phrase))
        || [
            "memory maintenance",
            "maintain memory",
            "memory consolidation",
            "consolidate memory",
        ]
        .iter()
        .any(|phrase| lowered.contains(phrase))
}

async fn try_handle_memory_maintenance_chat(
    state: &AppState,
    message: &str,
) -> Result<Option<Json<ChatResponse>>, AppError> {
    if !looks_like_memory_maintenance_request(message) {
        return Ok(None);
    }

    let primary_role = state.kernel.coordinator().primary_role();
    let Some(agent) = state.kernel.coordinator().get(&primary_role) else {
        return Ok(Some(Json(ChatResponse {
            response: "记忆维护没有执行：当前没有可用的主 agent。".to_string(),
            reasoning: None,
            tool_calls: None,
            artifacts: Vec::new(),
            chat_route: Some("coordinator::memory_maintenance".to_string()),
            tool_surface_mode: Some("memory_maintenance_fast_path".to_string()),
            runtime_persistence_status: Some("not_needed".to_string()),
            task_id: None,
            run_id: None,
            trace_id: None,
        })));
    };

    let result = match agent.run_memory_consolidation_once().await? {
        Some(result) => result,
        None => "记忆维护没有执行：当前主 agent 没有启用 sleep consolidator。".to_string(),
    };

    Ok(Some(Json(ChatResponse {
        response: result.clone(),
        reasoning: None,
        tool_calls: Some(vec![ToolCallTrace {
            name: "memory_consolidation".to_string(),
            args: serde_json::json!({ "action": "run_once" }).to_string(),
            result: Some(result),
            backup: None,
        }]),
        artifacts: Vec::new(),
        chat_route: Some("coordinator::memory_maintenance".to_string()),
        tool_surface_mode: Some("memory_maintenance_fast_path".to_string()),
        runtime_persistence_status: Some("not_needed".to_string()),
        task_id: None,
        run_id: None,
        trace_id: None,
    })))
}

async fn try_handle_memory_status_chat(
    state: &AppState,
    message: &str,
) -> Result<Option<Json<ChatResponse>>, AppError> {
    if !looks_like_memory_status_request(message) {
        return Ok(None);
    }

    let tool = FactManagementTool::new(state.kernel.memory().clone());
    let args = serde_json::json!({ "action": "get_status" }).to_string();
    let result = tool.call(&args).await.map_err(AppError::from)?;

    Ok(Some(Json(ChatResponse {
        response: result.clone(),
        reasoning: None,
        tool_calls: Some(vec![ToolCallTrace {
            name: "manage_facts".to_string(),
            args,
            result: Some(result),
            backup: None,
        }]),
        artifacts: Vec::new(),
        chat_route: Some("coordinator::memory_status".to_string()),
        tool_surface_mode: Some("memory_status_fast_path".to_string()),
        runtime_persistence_status: Some("not_needed".to_string()),
        task_id: None,
        run_id: None,
        trace_id: None,
    })))
}

fn looks_like_multimodal_memory_writeback(content: &str) -> bool {
    let lowered = content.to_lowercase();
    lowered.contains("多模态")
        && (lowered.contains("写回")
            || lowered.contains("受治理记忆")
            || lowered.contains("multimodal_memory_writeback")
            || lowered.contains("writeback"))
}

async fn try_handle_multimodal_memory_writeback_chat(
    state: &AppState,
    message: &str,
) -> Result<Option<Json<ChatResponse>>, AppError> {
    if !looks_like_multimodal_memory_writeback(message) {
        return Ok(None);
    }

    let segments = quoted_segments(message);
    let title = segments
        .first()
        .cloned()
        .unwrap_or_else(|| "multimodal-memory-record".to_string());
    let summary = segments
        .get(1)
        .cloned()
        .unwrap_or_else(|| "多模态记忆写回记录".to_string());
    let content = segments
        .get(2)
        .cloned()
        .unwrap_or_else(|| message.to_string());
    let lowered = message.to_lowercase();
    let kind = if lowered.contains("generation_provenance") || message.contains("生成溯源") {
        "generation_provenance"
    } else {
        "understanding"
    };
    let modality = if lowered.contains("audio") || message.contains("音频") {
        "audio"
    } else if lowered.contains("video") || message.contains("视频") {
        "video"
    } else if lowered.contains("pdf") {
        "pdf"
    } else if lowered.contains("document") || message.contains("文档") {
        "document"
    } else {
        "image"
    };

    let tool = MultimodalMemoryTool::new(state.kernel.memory().clone());
    let args = serde_json::json!({
        "kind": kind,
        "modality": modality,
        "title": title,
        "summary": summary,
        "content": content,
        "collection": "multimodal",
        "route": "api_chat_fast_path",
        "metadata": {
            "import_source": "api_chat",
            "trigger": "multimodal_memory_writeback"
        }
    });
    let args_text = args.to_string();
    let result = tool.call(&args_text).await.map_err(AppError::from)?;

    Ok(Some(Json(ChatResponse {
        response: format!("多模态记忆写回已完成。\n\n{}", result),
        reasoning: None,
        tool_calls: Some(vec![ToolCallTrace {
            name: "multimodal_memory_writeback".to_string(),
            args: args_text,
            result: Some(result),
            backup: None,
        }]),
        artifacts: Vec::new(),
        chat_route: Some("coordinator::multimodal_memory_writeback".to_string()),
        tool_surface_mode: Some("multimodal_memory_fast_path".to_string()),
        runtime_persistence_status: Some("not_needed".to_string()),
        task_id: None,
        run_id: None,
        trace_id: None,
    })))
}

async fn try_handle_session_artifact_read_chat(
    state: &AppState,
    session_id: &str,
    message: &str,
) -> Result<Option<Json<ChatResponse>>, AppError> {
    if writing_session_route::task_is_novel_content_operation(message) {
        return Ok(None);
    }
    if !writing_session_route::intent_requests_read_only_existing_artifact_answer(message) {
        return Ok(None);
    }
    let tasks = match state.kernel.state_task().list_by_session(session_id).await {
        Ok(tasks) => tasks,
        Err(error) => {
            warn!("Session artifact read task lookup failed: {}", error);
            return Ok(None);
        }
    };
    let Some(project_path) = writing_session_surface::latest_project_path_from_tasks(&tasks) else {
        return Ok(None);
    };
    let Some(project_path) = writing_session_surface::existing_project_path_for_candidate(
        chat_data_dir(&state),
        &project_path,
    )
    .await?
    else {
        return Ok(None);
    };
    let segment_numbers = writing_session_surface::referenced_artifact_segment_numbers(message);
    if segment_numbers.is_empty() {
        let Some(answer) =
            writing_session_surface::render_project_status_answer(&project_path).await?
        else {
            return Ok(None);
        };
        return Ok(Some(Json(ChatResponse {
            response: answer,
            reasoning: None,
            tool_calls: None,
            artifacts: Vec::new(),
            chat_route: Some("coordinator::session_artifact_status".to_string()),
            tool_surface_mode: Some("session_artifact_read_fast_path".to_string()),
            runtime_persistence_status: Some("not_needed".to_string()),
            task_id: None,
            run_id: None,
            trace_id: None,
        })));
    }
    let Some(answer) =
        writing_session_surface::render_project_segments_answer(&project_path, &segment_numbers)
            .await?
    else {
        return Ok(None);
    };

    Ok(Some(Json(ChatResponse {
        response: answer,
        reasoning: None,
        tool_calls: None,
        artifacts: Vec::new(),
        chat_route: Some("coordinator::session_artifact_read".to_string()),
        tool_surface_mode: Some("session_artifact_read_fast_path".to_string()),
        runtime_persistence_status: Some("not_needed".to_string()),
        task_id: None,
        run_id: None,
        trace_id: None,
    })))
}

#[derive(Clone, Deserialize)]
pub struct ChatRequest {
    pub message: String,
    pub session_id: Option<String>,
    pub _model: Option<String>,
    pub role: Option<String>,
    pub media: Option<Vec<MediaAttachment>>,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatStreamEvent {
    Accepted { session_id: Option<String> },
    Status { text: String },
    Artifact { artifact: ChatArtifactRef },
    Final { response: ChatResponse },
    Error { message: String },
}

#[derive(Serialize)]
pub struct ChatResponse {
    pub response: String,
    pub reasoning: Option<String>,
    pub tool_calls: Option<Vec<ToolCallTrace>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<ChatArtifactRef>,
    pub chat_route: Option<String>,
    pub tool_surface_mode: Option<String>,
    pub runtime_persistence_status: Option<String>,
    pub task_id: Option<Uuid>,
    pub run_id: Option<Uuid>,
    pub trace_id: Option<Uuid>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ChatArtifactRef {
    pub artifact_id: String,
    pub kind: String,
    pub uri: String,
    pub media_type: Option<String>,
    pub label: String,
}

#[derive(Serialize)]
pub struct ToolCallTrace {
    pub name: String,
    pub args: String,
    pub result: Option<String>,
    pub backup: Option<benshu_brain::skills::BackupInfo>,
}

const EPHEMERAL_CHAT_SESSION_PREFIX: &str = "ephemeral-chat";

fn resolve_chat_session_id(session_id: Option<&str>) -> (String, bool) {
    match session_id.map(str::trim).filter(|value| !value.is_empty()) {
        Some(session_id) => (session_id.to_string(), false),
        None => (
            format!("{}-{}", EPHEMERAL_CHAT_SESSION_PREFIX, Uuid::new_v4()),
            true,
        ),
    }
}

fn build_fresh_artifact_boundary_message(task: &str) -> Option<AgentMessage> {
    if !intent_requests_file_artifact(task) || intent_requests_existing_work_continuation(task) {
        return None;
    }

    let mut message = AgentMessage::system(
        "### FRESH ARTIFACT BOUNDARY\n\
The latest user request is a new artifact request, not an instruction to continue prior work. \
Do not reuse titles, project paths, character/entity names, artifact ids, or chapter numbers from prior sessions, memories, examples, or operational experience unless the user explicitly supplied them in this request. \
Create a fresh artifact identity appropriate to the requested genre/type, then persist the result through the equipped artifact tool."
            .to_string(),
    );
    message.metadata.insert(
        "fresh_artifact_boundary".to_string(),
        "new_artifact_identity".to_string(),
    );
    Some(message)
}

async fn build_session_work_context_message(
    state: &AppState,
    session_id: &str,
    user_request: &str,
) -> Option<AgentMessage> {
    if !should_attach_session_work_context(user_request) {
        return None;
    }

    let tasks = match state.kernel.state_task().list_by_session(session_id).await {
        Ok(tasks) => tasks,
        Err(error) => {
            warn!("Session work context task lookup failed: {}", error);
            Vec::new()
        }
    };
    let artifacts = match state
        .kernel
        .state_artifact()
        .query(&ArtifactQuery {
            session_id: Some(session_id.to_string()),
            ..ArtifactQuery::default()
        })
        .await
    {
        Ok(artifacts) => artifacts,
        Err(error) => {
            warn!("Session work context artifact lookup failed: {}", error);
            Vec::new()
        }
    };

    let context = SessionWorkContext { tasks, artifacts };
    let mut text = render_session_work_context(&context, user_request)?;
    if writing_session_route::intent_requests_read_only_existing_artifact_answer(user_request) {
        writing_session_surface::append_recent_text_artifact_previews(
            &mut text,
            chat_data_dir(state),
            &context.tasks,
            &context.artifacts,
            user_request,
        )
        .await;
        const MAX_READ_ONLY_CONTEXT_CHARS: usize = 7200;
        if text.chars().count() > MAX_READ_ONLY_CONTEXT_CHARS {
            text = text
                .chars()
                .take(MAX_READ_ONLY_CONTEXT_CHARS)
                .collect::<String>();
            text.push_str("\n[session work context truncated]");
        }
    }
    let mut message = AgentMessage::system(text);
    message.metadata.insert(
        "session_work_context".to_string(),
        "active_work_resolver".to_string(),
    );
    Some(message)
}

async fn prepend_session_work_target_to_request(
    state: &AppState,
    session_id: &str,
    user_request: &str,
) -> Option<String> {
    if user_request.contains("SESSION WORK TARGET") {
        return None;
    }
    if !intent_requests_existing_work_continuation(user_request) {
        return None;
    }
    let explicit_project_path = explicit_existing_project_path_from_request(state, user_request)
        .await
        .ok()
        .flatten();
    let tasks = state
        .kernel
        .state_task()
        .list_by_session(session_id)
        .await
        .ok();
    let project_path = if let Some(path) = explicit_project_path {
        path
    } else {
        let candidate = tasks
            .as_deref()
            .and_then(writing_session_surface::latest_project_path_from_tasks)?;
        writing_session_surface::existing_project_path_for_candidate(
            chat_data_dir(state),
            &candidate,
        )
        .await
        .ok()
        .flatten()?
    };
    Some(format!(
        "SESSION WORK TARGET\n\
project_path: {project_path}\n\
This is runtime context for continuing or revising the current session artifact. \
If the request delegates to a writing/artifact worker, include this exact project_path and continue the existing artifact instead of initializing a duplicate project, unless the user explicitly asks for a new project.\n\n\
USER REQUEST\n\
{user_request}"
    ))
}

fn should_attach_session_work_context(user_request: &str) -> bool {
    if intent_requests_file_artifact(user_request)
        && !intent_requests_existing_work_continuation(user_request)
        && !writing_session_route::intent_requests_read_only_existing_artifact_answer(user_request)
    {
        return false;
    }
    true
}

fn render_session_work_context(context: &SessionWorkContext, user_request: &str) -> Option<String> {
    let mut recent_tasks = context
        .tasks
        .iter()
        .filter(|task| !task.name.trim().is_empty())
        .collect::<Vec<_>>();
    recent_tasks.sort_by(|left, right| {
        session_task_rank(right)
            .cmp(&session_task_rank(left))
            .then_with(|| right.updated_at.cmp(&left.updated_at))
    });
    recent_tasks.truncate(3);

    let mut recent_artifacts = context.artifacts.iter().collect::<Vec<_>>();
    recent_artifacts.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    recent_artifacts.truncate(5);

    if recent_tasks.is_empty() && recent_artifacts.is_empty() {
        return None;
    }

    let mut text = String::from(
        "### SESSION WORK CONTEXT\n\
This is runtime state from the current chat session, not domain knowledge and not a new user request. Use it only when the user asks to continue, revise, verify, export, inspect, or otherwise refers to prior work. If the user asks for an unrelated new task, ignore this context.\n\
When it is relevant, continue the matching task/artifact instead of starting a duplicate project or document. Preserve existing artifact paths, task ids, continuity notes, and next actions unless the user explicitly asks to replace them.\n\n",
    );
    text.push_str(writing_session_route::session_work_context_guidance());
    text.push_str(&format!(
        "Latest user request: {}\n",
        preview_text(user_request.trim(), 240)
    ));

    if !recent_tasks.is_empty() {
        text.push_str("\nRecent session tasks:\n");
        for (index, task) in recent_tasks.iter().enumerate() {
            append_session_task_context(&mut text, index + 1, task);
        }
    }

    if !recent_artifacts.is_empty() {
        text.push_str("\nRecent session artifacts:\n");
        for (index, artifact) in recent_artifacts.iter().enumerate() {
            append_session_artifact_context(&mut text, index + 1, artifact);
        }
    }

    const MAX_CONTEXT_CHARS: usize = 3600;
    if text.chars().count() > MAX_CONTEXT_CHARS {
        text = text.chars().take(MAX_CONTEXT_CHARS).collect::<String>();
        text.push_str("\n[session work context truncated]");
    }

    Some(text)
}

fn session_task_rank(task: &TaskState) -> u8 {
    match task.status {
        TaskStatus::Running | TaskStatus::Queued | TaskStatus::Pending => 4,
        TaskStatus::AwaitingApproval { .. }
        | TaskStatus::Paused(_)
        | TaskStatus::Blocked { .. } => 3,
        TaskStatus::Completed => 2,
        TaskStatus::Deferred { .. } => 1,
        TaskStatus::Failed(_) | TaskStatus::Cancelled => 0,
    }
}

fn task_status_label(status: &TaskStatus) -> String {
    match status {
        TaskStatus::Pending => "pending".to_string(),
        TaskStatus::Queued => "queued".to_string(),
        TaskStatus::Running => "running".to_string(),
        TaskStatus::Completed => "completed".to_string(),
        TaskStatus::Failed(reason) => format!("failed: {}", preview_text(reason, 120)),
        TaskStatus::Cancelled => "cancelled".to_string(),
        TaskStatus::Paused(_) => "paused".to_string(),
        TaskStatus::AwaitingApproval { approval_kind, .. } => {
            format!("awaiting_approval:{approval_kind}")
        }
        TaskStatus::Blocked { reason } => format!("blocked: {}", preview_text(reason, 120)),
        TaskStatus::Deferred { reason, .. } => reason
            .as_deref()
            .map(|reason| format!("deferred: {}", preview_text(reason, 120)))
            .unwrap_or_else(|| "deferred".to_string()),
    }
}

fn append_session_task_context(output: &mut String, index: usize, task: &TaskState) {
    output.push_str(&format!(
        "{index}. id={} status={} agent={} updated={}\n",
        task.id,
        task_status_label(&task.status),
        task.agent_id,
        task.updated_at.to_rfc3339()
    ));
    output.push_str(&format!(
        "   name={} description={}\n",
        preview_text(task.name.trim(), 120),
        preview_text(task.description.trim(), 180)
    ));
    if let Some(contract) = task.contract.as_ref() {
        if let Some(intent) = contract
            .intent
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            output.push_str(&format!("   intent={}\n", preview_text(intent, 220)));
        }
        if let Some(language) = contract
            .response_language
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            output.push_str(&format!("   response_language={language}\n"));
        }
        if !contract.completion_criteria.is_empty() {
            output.push_str(&format!(
                "   completion={}\n",
                contract
                    .completion_criteria
                    .iter()
                    .take(3)
                    .map(|item| preview_text(item, 120))
                    .collect::<Vec<_>>()
                    .join(" | ")
            ));
        }
    }
    if task.total_steps.is_some() || task.current_step > 0 {
        output.push_str(&format!(
            "   progress={}/{}\n",
            task.current_step,
            task.total_steps
                .map(|value| value.to_string())
                .unwrap_or_else(|| "?".to_string())
        ));
    }
    let latest_checkpoints = task.checkpoints.iter().rev().take(2).collect::<Vec<_>>();
    if !latest_checkpoints.is_empty() {
        let checkpoint_text = latest_checkpoints
            .iter()
            .rev()
            .map(|checkpoint| {
                checkpoint
                    .summary
                    .as_deref()
                    .map(|summary| {
                        format!(
                            "{}: {}",
                            checkpoint.label,
                            preview_text(summary.trim(), 160)
                        )
                    })
                    .unwrap_or_else(|| checkpoint.label.clone())
            })
            .collect::<Vec<_>>()
            .join(" | ");
        output.push_str(&format!("   latest_checkpoints={checkpoint_text}\n"));
    }
    let work_refs = writing_session_surface::task_work_refs(task);
    if !work_refs.is_empty() {
        output.push_str(&format!("   work_refs={}\n", work_refs.join(" | ")));
    }
    if !task.artifacts.is_empty() {
        let artifact_text = task
            .artifacts
            .iter()
            .take(4)
            .map(|artifact| {
                format!(
                    "{}:{}",
                    artifact.kind,
                    preview_text(artifact.uri.trim(), 180)
                )
            })
            .collect::<Vec<_>>()
            .join(" | ");
        output.push_str(&format!("   artifacts={artifact_text}\n"));
    }
}

async fn explicit_existing_project_path_from_request(
    state: &AppState,
    user_request: &str,
) -> Result<Option<String>, AppError> {
    for path in writing_session_surface::writing_workspace_paths_from_text(user_request) {
        let Some(project_path) = writing_session_surface::infer_writing_project_path(&path) else {
            continue;
        };
        if let Some(existing) = writing_session_surface::existing_project_path_for_candidate(
            chat_data_dir(state),
            &project_path,
        )
        .await?
        {
            return Ok(Some(existing));
        }
    }
    Ok(None)
}

fn chat_data_dir(state: &AppState) -> &std::path::Path {
    state
        .config_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
}

fn append_session_artifact_context(output: &mut String, index: usize, artifact: &ArtifactRecord) {
    output.push_str(&format!(
        "{index}. id={} kind={} source={} updated={}\n",
        preview_text(&artifact.artifact_id, 100),
        preview_text(&artifact.kind, 80),
        preview_text(&artifact.source_kind, 80),
        artifact.updated_at.to_rfc3339()
    ));
    output.push_str(&format!(
        "   uri={} media={} tool={}\n",
        preview_text(artifact.uri.trim(), 220),
        artifact.media_type.as_deref().unwrap_or("unknown"),
        artifact.tool_name.as_deref().unwrap_or("unknown")
    ));
    if let Some(path) = artifact
        .virtual_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        output.push_str(&format!("   virtual_path={}\n", preview_text(path, 180)));
    }
    if !artifact.metadata.is_empty() {
        let metadata = artifact
            .metadata
            .iter()
            .take(4)
            .map(|(key, value)| format!("{key}={}", preview_text(value, 80)))
            .collect::<Vec<_>>()
            .join(" ");
        output.push_str(&format!("   metadata={metadata}\n"));
    }
}

pub async fn chat_handler(
    State(state): State<AppState>,
    Json(mut payload): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, AppError> {
    let (session_id, generated_session_id) = resolve_chat_session_id(payload.session_id.as_deref());
    if generated_session_id {
        warn!(
            "Chat request did not include session_id; using isolated ephemeral session {}",
            session_id
        );
    }
    let prime_role = state.kernel.coordinator().primary_role();

    // Switch agent if specified
    if let Some(role_name) = payload.role {
        match role_name.to_lowercase().as_str() {
            "benshu" => {
                state
                    .kernel
                    .coordinator()
                    .switch_session_agent(&session_id, prime_role.clone());
            }
            other => {
                return Err(AppError(anyhow::anyhow!(
                    "Direct specialist chat is disabled. Route requests through '{}' instead of '{}'.",
                    prime_role.name(),
                    other
                )));
            }
        }
    } else {
        state
            .kernel
            .coordinator()
            .switch_session_agent(&session_id, prime_role);
    }

    let mut allow_new_foreground_execution = true;
    if let Some((_, role)) = state
        .kernel
        .coordinator()
        .active_agents()
        .into_iter()
        .find(|(id, _)| id == &session_id)
    {
        if let Some(agent) = state.kernel.coordinator().get(&role) {
            if agent.has_active_foreground_task_for_session(Some(&session_id)) {
                let is_paused = agent.is_foreground_task_paused(Some(&session_id)).await;
                match classify_foreground_control_intent(&payload.message) {
                    ForegroundControlIntent::Stop => {
                        agent.cancel_foreground_task(Some(&session_id));
                        agent.ensure_active_token();
                        let cancelled_task_ids =
                            mark_running_session_tasks_cancelled(&state, &session_id).await?;
                        let task_id = cancelled_task_ids.first().copied();
                        return Ok(Json(ChatResponse {
                            response: "已停止当前任务。你现在可以发新的请求。".to_string(),
                            reasoning: None,
                            tool_calls: None,
                            artifacts: Vec::new(),
                            chat_route: Some("coordinator".to_string()),
                            tool_surface_mode: None,
                            runtime_persistence_status: Some("not_needed".to_string()),
                            task_id,
                            run_id: None,
                            trace_id: None,
                        }));
                    }
                    ForegroundControlIntent::Pause => {
                        let paused = agent.pause_foreground_task(Some(&session_id), None).await;
                        if paused {
                            mark_latest_session_task_paused(&state, &session_id).await?;
                        }
                        return Ok(Json(ChatResponse {
                            response: if paused {
                                "已暂停当前任务。它会停在下一个安全检查点；你可以回复“继续”，或者直接补充新的指令让我带着它继续。"
                                    .to_string()
                            } else {
                                "我没有找到可暂停的当前任务。你可以直接发新的请求。".to_string()
                            },
                            reasoning: None,
                            tool_calls: None,
                            artifacts: Vec::new(),
                            chat_route: Some("coordinator".to_string()),
                            tool_surface_mode: None,
                            runtime_persistence_status: Some("not_needed".to_string()),
                            task_id: None,
                            run_id: None,
                            trace_id: None,
                        }));
                    }
                    ForegroundControlIntent::StatusQuery => {
                        let latest_task_id =
                            latest_active_task_id_for_session(&state, &session_id).await;
                        return Ok(Json(ChatResponse {
                            response: match (latest_task_id, is_paused) {
                                (Some(_), true) => "当前任务已暂停，等待继续。\n\n回复“继续”会从暂停检查点恢复；也可以直接发补充指令，我会带着新指令继续。".to_string(),
                                (Some(_), false) => "当前任务仍在运行中。\n\n我不会因为状态查询去打断它；如果你想中断，请回复“停止”或“等一下”。".to_string(),
                                (None, _) =>
                                    "当前任务仍在运行中。我不会因为状态查询去打断它；如果你想中断，请回复“停止”或“等一下”。"
                                        .to_string(),
                            },
                            reasoning: None,
                            tool_calls: None,
                            artifacts: Vec::new(),
                            chat_route: Some("coordinator".to_string()),
                            tool_surface_mode: None,
                            runtime_persistence_status: Some("not_needed".to_string()),
                            task_id: latest_task_id,
                            run_id: None,
                            trace_id: None,
                        }));
                    }
                    ForegroundControlIntent::Resume => {
                        if let Some(task_id) =
                            resume_durable_paused_supervisor_task(&state, &session_id, None).await?
                        {
                            return Ok(Json(ChatResponse {
                                response:
                                    "已重新连接模型服务，并从最近的任务 checkpoint 继续执行。"
                                        .to_string(),
                                reasoning: None,
                                tool_calls: None,
                                artifacts: Vec::new(),
                                chat_route: Some("coordinator".to_string()),
                                tool_surface_mode: None,
                                runtime_persistence_status: Some("background_running".to_string()),
                                task_id: Some(task_id),
                                run_id: None,
                                trace_id: None,
                            }));
                        }
                        let resumed = agent.resume_foreground_task(Some(&session_id), None).await;
                        if resumed {
                            mark_latest_session_task_running(&state, &session_id).await?;
                        }
                        return Ok(Json(ChatResponse {
                            response: if resumed {
                                "已继续当前任务，会从最近的安全检查点接着执行。".to_string()
                            } else {
                                "我没有找到可继续的暂停任务。你可以直接发新的请求。".to_string()
                            },
                            reasoning: None,
                            tool_calls: None,
                            artifacts: Vec::new(),
                            chat_route: Some("coordinator".to_string()),
                            tool_surface_mode: None,
                            runtime_persistence_status: Some("not_needed".to_string()),
                            task_id: latest_active_task_id_for_session(&state, &session_id).await,
                            run_id: None,
                            trace_id: None,
                        }));
                    }
                    ForegroundControlIntent::Normal => {
                        if is_paused {
                            if let Some(task_id) = resume_durable_paused_supervisor_task(
                                &state,
                                &session_id,
                                Some(payload.message.as_str()),
                            )
                            .await?
                            {
                                return Ok(Json(ChatResponse {
                                    response:
                                        "已把你的补充指令加入当前暂停任务，并重新连接模型服务继续执行。"
                                            .to_string(),
                                    reasoning: None,
                                    tool_calls: None,
                                    artifacts: Vec::new(),
                                    chat_route: Some("coordinator".to_string()),
                                    tool_surface_mode: None,
                                    runtime_persistence_status: Some(
                                        "background_running".to_string(),
                                    ),
                                    task_id: Some(task_id),
                                    run_id: None,
                                    trace_id: None,
                                }));
                            }
                            let resumed = agent
                                .resume_foreground_task(
                                    Some(&session_id),
                                    Some(payload.message.as_str()),
                                )
                                .await;
                            if resumed {
                                mark_latest_session_task_running(&state, &session_id).await?;
                            }
                            return Ok(Json(ChatResponse {
                                response: if resumed {
                                    "已把你的补充指令加入当前暂停任务，并从最近的安全检查点继续执行。"
                                        .to_string()
                                } else {
                                    "我没有找到可继续的暂停任务。你可以直接发新的请求。".to_string()
                                },
                                reasoning: None,
                                tool_calls: None,
                                artifacts: Vec::new(),
                                chat_route: Some("coordinator".to_string()),
                                tool_surface_mode: None,
                                runtime_persistence_status: Some("not_needed".to_string()),
                                task_id: latest_active_task_id_for_session(&state, &session_id)
                                    .await,
                                run_id: None,
                                trace_id: None,
                            }));
                        }
                        return Ok(Json(ChatResponse {
                            response:
                                "当前任务还在运行，我不会用这条新消息打断它。等这轮结束后，请再发下一条任务；如果你要立刻中断，请回复“停止”或“等一下”。"
                                    .to_string(),
                            reasoning: None,
                            tool_calls: None,
                            artifacts: Vec::new(),
                            chat_route: Some("coordinator".to_string()),
                            tool_surface_mode: None,
                            runtime_persistence_status: Some("not_needed".to_string()),
                            task_id: None,
                            run_id: None,
                            trace_id: None,
                        }));
                    }
                    ForegroundControlIntent::Reprioritize => {
                        agent.cancel_foreground_task(Some(&session_id));
                        agent.ensure_active_token();
                        allow_new_foreground_execution = true;
                    }
                }
            }
        }
    }

    if !allow_new_foreground_execution {
        unreachable!("control gate should have returned before reaching execution");
    }

    let foreground_control_intent = classify_foreground_control_intent(&payload.message);
    match foreground_control_intent {
        ForegroundControlIntent::Stop => {
            let cancelled_task_ids =
                mark_running_session_tasks_cancelled(&state, &session_id).await?;
            if let Some(task_id) = cancelled_task_ids.first().copied() {
                state.kernel.coordinator().cancel_session(&session_id);
                return Ok(Json(ChatResponse {
                    response: "已停止当前后台任务。\n\n你现在可以发新的请求。".to_string(),
                    reasoning: None,
                    tool_calls: None,
                    artifacts: Vec::new(),
                    chat_route: Some("coordinator".to_string()),
                    tool_surface_mode: None,
                    runtime_persistence_status: Some("not_needed".to_string()),
                    task_id: Some(task_id),
                    run_id: None,
                    trace_id: None,
                }));
            }
            return Ok(Json(ChatResponse {
                response: "没有找到正在执行的任务；这条控制指令不会被当作新任务执行。".to_string(),
                reasoning: None,
                tool_calls: None,
                artifacts: Vec::new(),
                chat_route: Some("coordinator".to_string()),
                tool_surface_mode: None,
                runtime_persistence_status: Some("not_needed".to_string()),
                task_id: None,
                run_id: None,
                trace_id: None,
            }));
        }
        ForegroundControlIntent::Pause => {
            let task_id = latest_active_task_id_for_session(&state, &session_id).await;
            if task_id.is_some() {
                mark_latest_session_task_paused(&state, &session_id).await?;
                return Ok(Json(ChatResponse {
                    response:
                        "已暂停当前后台任务。它会停在下一个安全检查点；你可以回复“继续”，或者直接补充新的指令让我带着它继续。"
                            .to_string(),
                    reasoning: None,
                    tool_calls: None,
                    artifacts: Vec::new(),
                    chat_route: Some("coordinator".to_string()),
                    tool_surface_mode: None,
                    runtime_persistence_status: Some("not_needed".to_string()),
                    task_id,
                    run_id: None,
                    trace_id: None,
                }));
            }
            return Ok(Json(ChatResponse {
                response:
                    "没有找到正在执行的任务；如果你要暂停某个历史后台任务，请在任务页选择具体任务。"
                        .to_string(),
                reasoning: None,
                tool_calls: None,
                artifacts: Vec::new(),
                chat_route: Some("coordinator".to_string()),
                tool_surface_mode: None,
                runtime_persistence_status: Some("not_needed".to_string()),
                task_id: None,
                run_id: None,
                trace_id: None,
            }));
        }
        ForegroundControlIntent::Resume => {
            if let Some(task_id) =
                resume_durable_paused_supervisor_task(&state, &session_id, None).await?
            {
                return Ok(Json(ChatResponse {
                    response: "已重新连接模型服务，并从最近的任务 checkpoint 继续执行。"
                        .to_string(),
                    reasoning: None,
                    tool_calls: None,
                    artifacts: Vec::new(),
                    chat_route: Some("coordinator".to_string()),
                    tool_surface_mode: None,
                    runtime_persistence_status: Some("background_running".to_string()),
                    task_id: Some(task_id),
                    run_id: None,
                    trace_id: None,
                }));
            }
        }
        ForegroundControlIntent::StatusQuery => {
            if let Some(task) = latest_status_task_for_session(&state, &session_id).await {
                let response = render_latest_task_status_response(&task);
                return Ok(Json(ChatResponse {
                    response,
                    reasoning: None,
                    tool_calls: None,
                    artifacts: Vec::new(),
                    chat_route: Some("coordinator".to_string()),
                    tool_surface_mode: None,
                    runtime_persistence_status: Some("not_needed".to_string()),
                    task_id: Some(task.id),
                    run_id: None,
                    trace_id: None,
                }));
            }
        }
        _ => {}
    }

    if matches!(foreground_control_intent, ForegroundControlIntent::Normal) {
        if let Some(task) = latest_active_task_for_session(&state, &session_id).await {
            return Ok(Json(ChatResponse {
                response: active_session_task_interruption_response(&task),
                reasoning: None,
                tool_calls: None,
                artifacts: Vec::new(),
                chat_route: Some("coordinator".to_string()),
                tool_surface_mode: None,
                runtime_persistence_status: Some("not_needed".to_string()),
                task_id: Some(task.id),
                run_id: None,
                trace_id: None,
            }));
        }
    }

    if let Some(response) = try_handle_memory_maintenance_chat(&state, &payload.message).await? {
        return Ok(response);
    }

    if let Some(response) = try_handle_memory_status_chat(&state, &payload.message).await? {
        return Ok(response);
    }

    if let Some(response) =
        try_handle_multimodal_memory_writeback_chat(&state, &payload.message).await?
    {
        return Ok(response);
    }

    if let Some(response) =
        try_handle_session_artifact_read_chat(&state, &session_id, &payload.message).await?
    {
        return Ok(response);
    }

    if let Some(outcome) =
        try_handle_creation_draft_chat(&state, &session_id, &payload.message).await?
    {
        match outcome {
            CreationDraftChatOutcome::Respond(response) => return Ok(response),
            CreationDraftChatOutcome::ContinueWithMessage(message) => {
                payload.message = message;
            }
        }
    }

    if let Some(augmented) =
        prepend_session_work_target_to_request(&state, &session_id, &payload.message).await
    {
        payload.message = augmented;
    }

    // Wrap the message in a collection for the chat API
    let mut parts = vec![benshu_brain::agent::message::ContentPart::Text {
        text: payload.message.clone(),
    }];
    let input_message_count = 1 + payload.media.as_ref().map(|items| items.len()).unwrap_or(0);
    let media_parts = inbound_media_to_parts(state.document_router.clone(), payload.media).await;
    append_media_context_parts(&mut parts, media_parts);
    let message = AgentMessage::user(Content::parts(parts));
    let mut messages = Vec::new();
    if let Some(fresh_artifact_message) = build_fresh_artifact_boundary_message(&payload.message) {
        messages.push(fresh_artifact_message);
    }
    if let Some(work_context_message) =
        build_session_work_context_message(&state, &session_id, &payload.message).await
    {
        messages.push(work_context_message);
    }
    messages.push(message);

    if !chat_requires_supervised_execution(&messages, input_message_count) {
        return execute_direct_chat(state, session_id, messages).await;
    }

    let creation_planning_dialogue = creation_planning_dialogue_from_messages(&messages);
    match run_supervised_chat(
        state.clone(),
        session_id.clone(),
        messages,
        input_message_count,
    )
    .await?
    {
        SupervisedChatOutcome::Completed {
            outcome,
            supervisor_task_id,
            runtime_persistence_status,
        } => Ok(Json(chat_response_from_outcome(
            outcome,
            Some(supervisor_task_id),
            runtime_persistence_status,
            "coordinator",
        ))),
        SupervisedChatOutcome::BackgroundRunning { supervisor_task_id } => {
            Ok(Json(if creation_planning_dialogue {
                creation_planning_background_response(&state, &session_id, supervisor_task_id)
                    .await?
            } else {
                ChatResponse {
                    response: "任务已进入后台继续执行。\n\n你可以在面板任务页查看进度，或继续询问“任务进度”。前台 HTTP 连接不会限制这个任务的总执行时间。".to_string(),
                    reasoning: None,
                    tool_calls: None,
                    artifacts: Vec::new(),
                    chat_route: Some("coordinator::background_supervised".to_string()),
                    tool_surface_mode: None,
                    runtime_persistence_status: Some("background_running".to_string()),
                    task_id: Some(supervisor_task_id),
                    run_id: None,
                    trace_id: None,
                }
            }))
        }
    }
}

fn render_latest_task_status_response(task: &TaskState) -> String {
    if let Some(result) = task.result.as_ref() {
        let text = result
            .get("response_text")
            .and_then(Value::as_str)
            .or_else(|| result.get("text").and_then(Value::as_str))
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if let Some(text) = text {
            if let Some(visible) = writing_session_surface::naturalize_writing_response(text) {
                return visible;
            }
        }
    }

    let mut lines = vec![format!("最近任务状态：{}", task_status_label(&task.status))];

    if let Some(result) = task.result.as_ref() {
        let text = result
            .get("response_text")
            .and_then(Value::as_str)
            .or_else(|| result.get("text").and_then(Value::as_str))
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if let Some(text) = text {
            lines.push(String::new());
            let visible = writing_session_surface::naturalize_writing_response(text)
                .unwrap_or_else(|| redact_internal_paths_for_chat(text));
            lines.push(format!("最近输出：{}", preview_text(&visible, 500)));
        }
    }

    if !task.artifacts.is_empty() {
        lines.push(String::new());
        lines.push("相关文件：".to_string());
        for artifact in task.artifacts.iter().take(4) {
            lines.push(format!("- {}", chat_artifact_label(artifact)));
        }
    }

    lines.join("\n")
}

enum SupervisedChatOutcome {
    Completed {
        outcome: ChatOutcome,
        supervisor_task_id: Uuid,
        runtime_persistence_status: Option<String>,
    },
    BackgroundRunning {
        supervisor_task_id: Uuid,
    },
}

struct CompletedSupervisedChat {
    outcome: ChatOutcome,
    supervisor_task_id: Uuid,
    runtime_persistence_status: Option<String>,
}

fn chat_requires_supervised_execution(
    messages: &[AgentMessage],
    input_message_count: usize,
) -> bool {
    if input_message_count > 1 {
        return true;
    }

    let contract = build_chat_task_contract(messages);
    let intent = contract.intent.as_deref().unwrap_or_default();
    if intent.contains(CREATION_PLANNING_DIALOGUE_MARKER) {
        return true;
    }
    if evaluate_creation_intake(intent).should_clarify() {
        return false;
    }
    if DelegateTool::requested_text_target_chars(intent).is_some()
        || !contract.required_events.is_empty()
    {
        return true;
    }

    classify_query_capability_route(intent).is_some_and(capability_route_requires_real_tool_call)
}

fn creation_planning_dialogue_from_messages(messages: &[AgentMessage]) -> bool {
    let contract = build_chat_task_contract(messages);
    let intent = contract.intent.as_deref().unwrap_or_default();
    if writing_session_route::intent_is_direct_writer_continuation(intent) {
        return false;
    }
    creation_planning_dialogue_requested(intent)
}

fn creation_planning_prompt_from_messages(messages: &[AgentMessage]) -> Option<String> {
    messages
        .iter()
        .rev()
        .find(|message| matches!(message.role, benshu_brain::agent::message::Role::User))
        .map(|message| message.text())
        .map(|text| text.trim().to_string())
        .filter(|text| text.contains(CREATION_PLANNING_DIALOGUE_MARKER))
}

fn creation_planning_dialogue_requested(intent: &str) -> bool {
    writing_session_route::intent_is_creation_contract_planning(intent)
}

async fn execute_direct_chat(
    state: AppState,
    session_id: String,
    messages: Vec<AgentMessage>,
) -> Result<Json<ChatResponse>, AppError> {
    let creation_contract_turn = creation_planning_dialogue_from_messages(&messages);
    let result = state
        .kernel
        .coordinator()
        .chat_session(&session_id, messages)
        .await;

    match result {
        Ok(mut outcome) => {
            if creation_contract_turn {
                if let Ok(Some(draft)) = load_session_creation_draft(&state, &session_id).await {
                    outcome.response =
                        writing_session_surface::stabilize_creation_contract_panel_response(
                            &draft,
                            &outcome.response,
                        );
                }
            }
            if let Some(run_trace) = outcome.run_trace.as_mut() {
                run_trace
                    .metadata
                    .insert("chat_route".to_string(), "coordinator::direct".to_string());
            }
            Ok(Json(chat_response_from_outcome(
                outcome,
                None,
                Some("agent_checkpointed".to_string()),
                "coordinator::direct",
            )))
        }
        Err(error) => Err(AppError(error.into())),
    }
}

pub async fn chat_stream_handler(
    State(state): State<AppState>,
    Json(payload): Json<ChatRequest>,
) -> Sse<impl Stream<Item = Result<Event, axum::Error>>> {
    let accepted_session_id = payload.session_id.clone();
    let stream = async_stream::stream! {
        yield chat_stream_event(ChatStreamEvent::Accepted {
            session_id: accepted_session_id,
        });
        yield chat_stream_event(ChatStreamEvent::Status {
            text: "BenShu 已接收请求，正在进入聊天运行时。".to_string(),
        });

        match chat_handler(State(state), Json(payload)).await {
            Ok(Json(response)) => {
                for artifact in response.artifacts.iter().cloned() {
                    yield chat_stream_event(ChatStreamEvent::Artifact { artifact });
                }
                yield chat_stream_event(ChatStreamEvent::Final { response });
            }
            Err(err) => {
                yield chat_stream_event(ChatStreamEvent::Error {
                    message: err.0.to_string(),
                });
            }
        }
    };

    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default())
}

fn chat_stream_event(event: ChatStreamEvent) -> Result<Event, axum::Error> {
    Event::default().json_data(event).map_err(axum::Error::new)
}

async fn run_supervised_chat(
    state: AppState,
    session_id: String,
    messages: Vec<AgentMessage>,
    input_message_count: usize,
) -> Result<SupervisedChatOutcome, AppError> {
    let foreground_observation_seconds =
        chat_foreground_observation_seconds(&messages, input_message_count);
    let supervisor = create_chat_supervisor_task(
        &state,
        &session_id,
        &messages,
        input_message_count,
        foreground_observation_seconds,
    )
    .await?;
    let supervisor_task_id = supervisor.id;
    let supervisor_token_key = supervisor_task_id.to_string();
    state.cancel_tokens.insert(
        supervisor_token_key.clone(),
        tokio_util::sync::CancellationToken::new(),
    );
    let (result_tx, result_rx) = tokio::sync::oneshot::channel();

    tokio::spawn({
        let state = state.clone();
        async move {
            let (heartbeat_stop_tx, heartbeat_stop_rx) = tokio::sync::oneshot::channel();
            let heartbeat_handle = tokio::spawn(supervisor_task_heartbeat(
                state.clone(),
                supervisor_task_id,
                heartbeat_stop_rx,
            ));
            let (event_stop_tx, event_stop_rx) = tokio::sync::oneshot::channel();
            let event_handle = tokio::spawn(supervisor_agent_event_monitor(
                state.clone(),
                supervisor_task_id,
                session_id.clone(),
                event_stop_rx,
            ));
            let result =
                execute_supervised_chat(state.clone(), session_id, messages, supervisor_task_id)
                    .await;
            if let Err(error) = result.as_ref() {
                if let Err(status_error) =
                    mark_supervisor_task_failed(&state, supervisor_task_id, &error.to_string())
                        .await
                {
                    warn!(
                        task_id = %supervisor_task_id,
                        error = %status_error,
                        "failed to persist supervised chat error status"
                    );
                }
            }
            state.cancel_tokens.remove(&supervisor_token_key);
            let _ = result_tx.send(result);
            tokio::spawn(async move {
                let _ = heartbeat_stop_tx.send(());
                let _ = event_stop_tx.send(());
                let _ = heartbeat_handle.await;
                let _ = event_handle.await;
            });
        }
    });

    match tokio::time::timeout(
        std::time::Duration::from_secs(foreground_observation_seconds),
        result_rx,
    )
    .await
    {
        Ok(Ok(Ok(completed))) => Ok(SupervisedChatOutcome::Completed {
            outcome: completed.outcome,
            supervisor_task_id: completed.supervisor_task_id,
            runtime_persistence_status: completed.runtime_persistence_status,
        }),
        Ok(Ok(Err(error))) => Err(AppError(error)),
        Ok(Err(_closed)) => Err(AppError(anyhow::anyhow!(
            "supervised chat task terminated before reporting a result"
        ))),
        Err(_elapsed) => Ok(SupervisedChatOutcome::BackgroundRunning { supervisor_task_id }),
    }
}

fn chat_foreground_observation_seconds(
    messages: &[AgentMessage],
    input_message_count: usize,
) -> u64 {
    let contract = build_chat_task_contract(messages);
    let intent = contract.intent.as_deref().unwrap_or_default();
    let planning_dialogue = creation_planning_dialogue_requested(intent);
    let explicit_text_scale = DelegateTool::requested_text_target_chars(intent).is_some();
    let durable_side_effect = !contract.required_events.is_empty();
    let has_media = input_message_count > 1;
    let realtime_lookup = classify_query_capability_route(intent)
        .is_some_and(|route| matches!(route, CapabilityRouteHint::RealtimeLookup(_)));

    if planning_dialogue {
        CHAT_FOREGROUND_PLANNING_DIALOGUE_SECONDS
    } else if realtime_lookup && !durable_side_effect && !explicit_text_scale && !has_media {
        CHAT_FOREGROUND_REALTIME_LOOKUP_SECONDS
    } else if explicit_text_scale || durable_side_effect || has_media {
        CHAT_FOREGROUND_SHORT_OBSERVATION_SECONDS
    } else {
        CHAT_FOREGROUND_DEFAULT_OBSERVATION_SECONDS
    }
}

async fn create_chat_supervisor_task(
    state: &AppState,
    session_id: &str,
    messages: &[AgentMessage],
    input_message_count: usize,
    foreground_observation_seconds: u64,
) -> Result<TaskState, AppError> {
    let primary_role = state.kernel.coordinator().primary_role();
    let mut task = TaskState::new(
        "foreground_chat_supervisor",
        "Interactive chat run supervised beyond the foreground HTTP observation window",
        serde_json::json!({
            "entrypoint": "gateway.chat_supervisor",
            "input_message_count": input_message_count,
            "foreground_observation_seconds": foreground_observation_seconds,
            "lifecycle": "background_capable",
        }),
        primary_role.name(),
    );
    task.status = TaskStatus::Running;
    task.contract = Some(build_chat_task_contract(messages));
    task.session_id = Some(session_id.to_string());
    task.thread_id = Some(session_id.to_string());
    task.root_task_id = Some(task.id);
    task.tags = vec![
        "foreground".to_string(),
        "chat".to_string(),
        "background_supervised".to_string(),
    ];
    if build_chat_task_contract(messages)
        .intent
        .as_deref()
        .is_some_and(writing_session_route::intent_is_creation_contract_planning)
    {
        task.tags
            .push(writing_session_route::creation_contract_planning_tag().to_string());
    }
    task.updated_at = chrono::Utc::now();
    state
        .kernel
        .state_task()
        .save(task.clone())
        .await
        .map_err(AppError::from)?;
    Ok(task)
}

fn build_chat_task_contract(messages: &[AgentMessage]) -> TaskContract {
    let raw_intent = messages
        .iter()
        .rev()
        .find(|message| matches!(message.role, benshu_brain::agent::message::Role::User))
        .map(|message| message.text())
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty());
    let planning_dialogue = raw_intent
        .as_deref()
        .is_some_and(creation_planning_dialogue_requested);
    let intent = raw_intent
        .as_deref()
        .map(user_visible_chat_task_intent)
        .filter(|text| !text.is_empty());
    let lint_warnings = lint_task_intent(intent.as_deref());
    let language_contract = intent
        .as_deref()
        .map(resolve_language_contract)
        .unwrap_or_else(|| resolve_language_contract(""));
    let evidence_requirements = vec![TaskEvidenceRequirement {
        topic: "tool.*".to_string(),
        description: Some("tool side effects must have runtime event receipts".to_string()),
        minimum_count: None,
    }];

    let mut completion_criteria = vec![
        "Return a final answer, or a clear blocker with the next safe action".to_string(),
        "When tools are used, persist execution receipts before marking the task complete"
            .to_string(),
        "Do not infer user-requested durable storage or artifact completion from wording alone; rely on tool/runtime effect receipts"
            .to_string(),
        "Substantial artifacts must pass the artifact producer's quality contract before the task can claim completion"
            .to_string(),
    ];
    if !planning_dialogue
        && intent
            .as_deref()
            .and_then(DelegateTool::requested_text_target_chars)
            .is_some()
    {
        completion_criteria.push(
            "When the user requests an explicit text scale, the final artifact must report enough units to satisfy that scale; process notes, plans, outlines, or partial drafts are not completion evidence."
                .to_string(),
        );
    }

    let required_events = inferred_required_events_from_intent(intent.as_deref());

    TaskContract {
        intent,
        response_language: Some(language_contract.response_language),
        artifact_language: Some(language_contract.artifact_language),
        decisions: vec![
            "Use the configured panel/runtime resolver for model and provider bindings".to_string(),
            "Persist runtime evidence for tool, artifact, and knowledge side effects".to_string(),
            "Preserve the user's language for user-facing replies and generated artifact content unless the user explicitly requests another language".to_string(),
        ],
        boundaries: vec![
            TaskBoundary {
                scope: "runtime_config".to_string(),
                rule: "do not write runtime provider/model bindings back to AGENT.md".to_string(),
                reason: Some(
                    "runtime bindings belong to panel configuration and resolvers".to_string(),
                ),
            },
            TaskBoundary {
                scope: "runtime_state".to_string(),
                rule: "do not treat receipts, traces, or provenance as durable knowledge text"
                    .to_string(),
                reason: Some("execution metadata is task evidence, not user memory".to_string()),
            },
        ],
        completion_criteria,
        required_events,
        evidence_requirements,
        lint_warnings,
    }
}

fn user_visible_chat_task_intent(intent: &str) -> String {
    if !intent.contains(CREATION_PLANNING_DIALOGUE_MARKER) {
        return intent.trim().to_string();
    }
    extract_creation_planning_latest_user_request(intent)
        .map(|request| format!("生成写作合同草案：{request}"))
        .unwrap_or_else(|| "生成写作合同草案".to_string())
}

fn extract_creation_planning_latest_user_request(intent: &str) -> Option<String> {
    let marker = "用户最新要求：";
    let request_line = if let Some(after) = intent.split(marker).nth(1) {
        after.lines().next().map(str::trim)
    } else {
        intent
            .split(CREATION_PLANNING_DIALOGUE_MARKER)
            .nth(1)
            .and_then(|after| after.lines().map(str::trim).find(|line| !line.is_empty()))
    }?;
    let request = (!request_line.trim().is_empty())
        .then_some(request_line)?
        .trim_end_matches(['。', '.', '；', ';'])
        .trim()
        .to_string();
    (!request.is_empty()).then_some(request)
}

fn inferred_required_events_from_intent(intent: Option<&str>) -> Vec<String> {
    let Some(intent) = intent.map(str::trim).filter(|value| !value.is_empty()) else {
        return Vec::new();
    };
    let existing_work_continuation = intent_requests_existing_work_continuation(intent)
        || writing_session_route::intent_is_direct_writer_continuation(intent);
    if writing_session_route::intent_is_creation_contract_planning(intent) {
        return Vec::new();
    }
    let planning_dialogue = creation_planning_dialogue_requested(intent);
    if planning_dialogue && intent_requests_creation_contract_before_artifact(intent) {
        return Vec::new();
    }
    if writing_session_route::intent_requests_read_only_existing_artifact_answer(intent) {
        return Vec::new();
    }
    if !existing_work_continuation && planning_dialogue {
        return Vec::new();
    }
    if writing_session_route::intent_requests_metadata_only_content_operation(intent) {
        return vec![
            "artifact.written".to_string(),
            "artifact.verified".to_string(),
        ];
    }
    if !existing_work_continuation && evaluate_creation_intake(intent).should_clarify() {
        return Vec::new();
    }
    let mut events = Vec::new();
    if intent_requests_durable_knowledge_write(intent) {
        push_unique_event(&mut events, "knowledge.imported");
    }
    if intent_requests_primary_artifact_verification(intent) {
        push_unique_event(&mut events, "artifact.verified");
    } else if intent_requests_file_artifact(intent) {
        push_unique_event(&mut events, "artifact.written");
    }
    if intent_requests_pdf_artifact(intent) {
        push_unique_event(&mut events, "artifact.pdf");
    }
    if intent_requests_txt_artifact(intent) {
        push_unique_event(&mut events, "artifact.txt");
    }
    if intent_requests_markdown_artifact(intent) {
        push_unique_event(&mut events, "artifact.md");
    }
    events
}

fn intent_requests_creation_contract_before_artifact(intent: &str) -> bool {
    let lowered = intent.to_ascii_lowercase();
    let planning_surface = [
        "合同",
        "草案",
        "大纲",
        "框架",
        "设定",
        "规划",
        "contract",
        "draft",
        "outline",
        "framework",
        "plan",
    ];
    let defer_surface = [
        "先",
        "只定",
        "不写正文",
        "不要写正文",
        "确认后",
        "确认再",
        "再开始",
        "开始前",
        "定下来",
        "多轮对话",
        "before writing",
        "confirm first",
        "after confirmation",
    ];
    planning_surface
        .iter()
        .any(|term| intent.contains(term) || lowered.contains(term))
        && defer_surface
            .iter()
            .any(|term| intent.contains(term) || lowered.contains(term))
}

fn intent_requests_artifact_verification(intent: &str) -> bool {
    let lowered = intent.to_ascii_lowercase();
    let verify_surface = [
        "确保", "确认", "检查", "核验", "验证", "校验", "看看", "是否", "已经", "状态", "存在",
        "ensure", "verify", "confirm", "check", "inspect", "status", "exists", "already",
    ];
    let artifact_surface = [
        "产物",
        "文件",
        "文档",
        "项目",
        "草稿",
        "正文",
        "章节",
        "章",
        "篇",
        "文章",
        "论文",
        "报告",
        "故事",
        "小说",
        "大纲",
        "连续性",
        "artifact",
        "file",
        "document",
        "project",
        "draft",
        "chapter",
        "section",
        "article",
        "paper",
        "report",
        "story",
        "novel",
        "continuity",
    ];
    verify_surface
        .iter()
        .any(|term| intent.contains(term) || lowered.contains(term))
        && artifact_surface
            .iter()
            .any(|term| intent.contains(term) || lowered.contains(term))
}

fn intent_requests_primary_artifact_verification(intent: &str) -> bool {
    if !intent_requests_artifact_verification(intent) {
        return false;
    }
    if !intent_requests_file_artifact(intent) {
        return true;
    }
    let lowered = intent.to_ascii_lowercase();
    let contingent_surface = [
        "缺什么",
        "缺少",
        "如果",
        "如有",
        "必要时",
        "需要时",
        "视情况",
        "是否",
        "已经",
        "状态",
        "存在",
        "if needed",
        "if missing",
        "as needed",
        "whether",
        "already",
        "exists",
        "status",
    ];
    if contingent_surface
        .iter()
        .any(|term| intent.contains(term) || lowered.contains(term))
    {
        return true;
    }

    let unconditional_content_mutation = [
        "写", "续写", "创作", "生成", "创建", "修订", "修改", "修正", "改写", "润色", "补全",
        "完善", "扩写", "write", "draft", "create", "generate", "revise", "rewrite", "edit",
        "polish", "complete", "expand",
    ];
    !unconditional_content_mutation
        .iter()
        .any(|term| intent.contains(term) || lowered.contains(term))
}

fn push_unique_event(events: &mut Vec<String>, event: &str) {
    if !events.iter().any(|existing| existing == event) {
        events.push(event.to_string());
    }
}

fn intent_requests_durable_knowledge_write(intent: &str) -> bool {
    let lowered = intent.to_ascii_lowercase();
    if intent.contains("记住") || lowered.contains("remember this") || lowered.contains("remember:")
    {
        return true;
    }
    let storage_surface = ["知识库", "数据库", "资料库", "rag", "knowledge", "database"];
    let write_surface = [
        "存", "存到", "存入", "保存", "写入", "导入", "收录", "收进", "收入", "加入", "放到",
        "放入", "置入", "入", "入库", "归档", "记录", "记入", "纳入", "store", "save", "import",
        "ingest", "archive", "record",
    ];
    terms_have_close_match(intent, &lowered, &storage_surface, &write_surface, 64)
}

fn terms_have_close_match(
    original: &str,
    lowered: &str,
    left_terms: &[&str],
    right_terms: &[&str],
    max_byte_distance: usize,
) -> bool {
    let mut left_positions = Vec::new();
    let mut right_positions = Vec::new();
    collect_term_positions(original, lowered, left_terms, &mut left_positions);
    collect_term_positions(original, lowered, right_terms, &mut right_positions);
    left_positions.iter().any(|left| {
        right_positions
            .iter()
            .any(|right| left.abs_diff(*right) <= max_byte_distance)
    })
}

fn collect_term_positions(
    original: &str,
    lowered: &str,
    terms: &[&str],
    positions: &mut Vec<usize>,
) {
    for term in terms {
        positions.extend(original.match_indices(term).map(|(index, _)| index));
        let lowered_term = term.to_ascii_lowercase();
        if lowered_term != *term {
            positions.extend(lowered.match_indices(&lowered_term).map(|(index, _)| index));
        } else if term.is_ascii() {
            positions.extend(lowered.match_indices(term).map(|(index, _)| index));
        }
    }
    positions.sort_unstable();
    positions.dedup();
}

fn intent_requests_file_artifact(intent: &str) -> bool {
    let lowered = intent.to_ascii_lowercase();
    let artifact_surface = [
        "文件", "文档", "txt", "pdf", ".txt", ".pdf", "报告", "论文", "小说", "文章", "章节",
        "正文", "稿件", "草稿", "第", "章", "file", "document", "article", "chapter", "section",
        "draft", "artifact",
    ];
    let write_surface = [
        "写",
        "续写",
        "继续写",
        "生成",
        "创建",
        "保存",
        "做成",
        "导出",
        "修订",
        "修改",
        "修正",
        "改写",
        "润色",
        "补全",
        "完善",
        "整理",
        "更新",
        "校订",
        "编辑",
        "write",
        "continue writing",
        "create",
        "generate",
        "save",
        "export",
        "revise",
        "revision",
        "edit",
        "update",
        "rewrite",
        "polish",
        "complete",
        "expand",
        "refine",
    ];
    artifact_surface
        .iter()
        .any(|term| intent.contains(term) || lowered.contains(term))
        && write_surface
            .iter()
            .any(|term| intent.contains(term) || lowered.contains(term))
}

fn intent_requests_pdf_artifact(intent: &str) -> bool {
    if !intent_requests_file_artifact(intent) {
        return false;
    }
    let lowered = intent.to_ascii_lowercase();
    lowered.contains("pdf") || intent.contains("PDF") || intent.contains(".pdf")
}

fn intent_requests_txt_artifact(intent: &str) -> bool {
    let lowered = intent.to_ascii_lowercase();
    lowered.contains(".txt")
        || lowered.contains("txt")
        || intent.contains("纯文本")
        || intent.contains("文本文件")
}

fn intent_requests_markdown_artifact(intent: &str) -> bool {
    let lowered = intent.to_ascii_lowercase();
    lowered.contains(".md") || lowered.contains("markdown") || intent.contains("Markdown")
}

fn lint_task_intent(intent: Option<&str>) -> Vec<String> {
    let Some(intent) = intent.map(str::trim).filter(|value| !value.is_empty()) else {
        return vec![
            "task intent is empty; completion can only be judged from runtime evidence".to_string(),
        ];
    };

    let mut warnings = Vec::new();
    if intent.chars().count() > 2000 {
        warnings.push(
            "task intent is very large; workers should receive a compact plan context".to_string(),
        );
    }
    let lowered = intent.to_ascii_lowercase();
    let vague_terms = [
        "处理",
        "优化",
        "完善",
        "尽快",
        "合适",
        "handle",
        "optimize",
        "improve",
        "asap",
        "appropriate",
    ];
    if vague_terms
        .iter()
        .any(|term| intent.contains(term) || lowered.contains(term))
    {
        warnings.push(
            "task intent contains vague terms; treat this as advisory and rely on explicit evidence"
                .to_string(),
        );
    }
    warnings
}

async fn supervisor_task_heartbeat(
    state: AppState,
    supervisor_task_id: Uuid,
    mut stop_rx: tokio::sync::oneshot::Receiver<()>,
) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    interval.tick().await;

    loop {
        tokio::select! {
            _ = &mut stop_rx => break,
            _ = interval.tick() => {
                if let Err(error) = record_supervisor_task_heartbeat(&state, supervisor_task_id).await {
                    warn!(
                        task_id = %supervisor_task_id,
                        error = %error,
                        "failed to record supervised chat heartbeat"
                    );
                }
            }
        }
    }
}

async fn supervisor_agent_event_monitor(
    state: AppState,
    supervisor_task_id: Uuid,
    session_id: String,
    mut stop_rx: tokio::sync::oneshot::Receiver<()>,
) {
    let (child_stop_tx, child_stop_rx) = tokio::sync::watch::channel(false);
    let mut subscribed = std::collections::HashSet::<String>::new();
    let mut child_handles = Vec::new();
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = &mut stop_rx => {
                let _ = child_stop_tx.send(true);
                break;
            }
            _ = interval.tick() => {
                for role in state.kernel.coordinator().roles() {
                    let role_name = role.name().to_string();
                    if subscribed.contains(&role_name) {
                        continue;
                    }
                    let Some(agent) = state.kernel.coordinator().get(&role) else {
                        continue;
                    };
                    subscribed.insert(role_name.clone());
                    child_handles.push(tokio::spawn(supervisor_agent_role_event_monitor(
                        state.clone(),
                        supervisor_task_id,
                        session_id.clone(),
                        role_name,
                        agent.events(),
                        child_stop_rx.clone(),
                    )));
                }
            }
        }
    }

    for handle in child_handles {
        let _ = handle.await;
    }
}

async fn supervisor_agent_role_event_monitor(
    state: AppState,
    supervisor_task_id: Uuid,
    session_id: String,
    role_name: String,
    mut events_rx: tokio::sync::broadcast::Receiver<AgentEvent>,
    mut stop_rx: tokio::sync::watch::Receiver<bool>,
) {
    loop {
        tokio::select! {
            _ = stop_rx.changed() => {
                if *stop_rx.borrow() {
                    break;
                }
            }
            event = events_rx.recv() => {
                match event {
                    Ok(event) => {
                        if let Some(event_session_id) = event.session_id.as_deref() {
                            if event_session_id != session_id {
                                continue;
                            }
                        }
                        if let Err(error) = record_supervisor_agent_event_checkpoint(&state, supervisor_task_id, &role_name, &event).await {
                            warn!(
                                task_id = %supervisor_task_id,
                                role = %role_name,
                                error = %error,
                                "failed to record supervised chat agent event checkpoint"
                            );
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(count)) => {
                        if let Err(error) = record_supervisor_checkpoint(
                            &state,
                            supervisor_task_id,
                            &format!("agent:{role_name}:event_lag"),
                            Some(format!("{role_name} agent event stream lagged by {count} events")),
                        )
                        .await
                        {
                            warn!(
                                task_id = %supervisor_task_id,
                                role = %role_name,
                                error = %error,
                                "failed to record supervised chat event lag checkpoint"
                            );
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
}

async fn record_supervisor_agent_event_checkpoint(
    state: &AppState,
    supervisor_task_id: Uuid,
    role_name: &str,
    event: &AgentEvent,
) -> anyhow::Result<()> {
    let Some((label, summary)) = supervisor_agent_event_checkpoint(role_name, event) else {
        return Ok(());
    };
    record_supervisor_checkpoint(state, supervisor_task_id, &label, Some(summary)).await
}

fn supervisor_agent_event_checkpoint(
    role_name: &str,
    event: &AgentEvent,
) -> Option<(String, String)> {
    match &event.data {
        AgentEventData::StepStart { step } => Some((
            format!("agent:{role_name}:step"),
            format!("{role_name} reasoning step {step} started"),
        )),
        AgentEventData::ToolExecutionStart {
            tool,
            input,
            safety,
        } => Some((
            format!("agent:{role_name}:tool:{tool}:start"),
            format!(
                "{role_name}.{tool} started safety={safety:?} input={}",
                preview_text(input, 280)
            ),
        )),
        AgentEventData::ToolExecutionEnd {
            tool,
            output_preview,
            duration_ms,
            success,
        } => Some((
            format!("agent:{role_name}:tool:{tool}:end"),
            format!(
                "{role_name}.{tool} finished success={success} duration_ms={duration_ms} preview={}",
                preview_text(output_preview, 360)
            ),
        )),
        AgentEventData::ToolCall {
            tool,
            input,
            safety,
        } => Some((
            format!("agent:{role_name}:tool:{tool}:planned"),
            format!(
                "{role_name}.{tool} planned safety={safety:?} input={}",
                preview_text(input, 280)
            ),
        )),
        AgentEventData::Thought { content } => {
            let content = content.trim();
            (!content.is_empty()).then(|| {
                (
                    format!("agent:{role_name}:thought"),
                    format!("{role_name} progress: {}", preview_text(content, 320)),
                )
            })
        }
        AgentEventData::Error { message } => Some((
            format!("agent:{role_name}:error"),
            format!("{role_name} error: {}", preview_text(message, 500)),
        )),
        AgentEventData::Cancelled { reason } => Some((
            format!("agent:{role_name}:cancelled"),
            format!("{role_name} cancelled: {}", preview_text(reason, 500)),
        )),
        _ => None,
    }
}

async fn record_supervisor_checkpoint(
    state: &AppState,
    supervisor_task_id: Uuid,
    label: &str,
    summary: Option<String>,
) -> anyhow::Result<()> {
    let Some(mut task) = state
        .kernel
        .state_task()
        .load(&supervisor_task_id.to_string())
        .await?
    else {
        return Ok(());
    };
    if !matches!(task.status, TaskStatus::Running | TaskStatus::Queued) {
        return Ok(());
    }

    let now = chrono::Utc::now();
    task.updated_at = now;
    match task.checkpoints.last_mut() {
        Some(checkpoint) if checkpoint.label == label => {
            checkpoint.recorded_at = now;
            checkpoint.summary = summary;
        }
        _ => task.checkpoints.push(TaskCheckpoint {
            step: task.current_step.saturating_add(1),
            label: label.to_string(),
            recorded_at: now,
            summary,
        }),
    }
    state.kernel.state_task().save(task).await?;
    Ok(())
}

async fn record_supervisor_task_heartbeat(
    state: &AppState,
    supervisor_task_id: Uuid,
) -> anyhow::Result<()> {
    let Some(task) = state
        .kernel
        .state_task()
        .load(&supervisor_task_id.to_string())
        .await?
    else {
        return Ok(());
    };

    if !matches!(task.status, TaskStatus::Running | TaskStatus::Queued) {
        return Ok(());
    }

    let now = chrono::Utc::now();
    let observed_topics = match state
        .kernel
        .state_runtime_event()
        .list_by_task(supervisor_task_id)
        .await
    {
        Ok(events) => events
            .into_iter()
            .map(|event| event.topic)
            .collect::<Vec<_>>(),
        Err(error) => {
            warn!(
                task_id = %supervisor_task_id,
                %error,
                "failed to load runtime events for background progress summary"
            );
            Vec::new()
        }
    };
    let summary = Some(background_supervisor_progress_summary(
        &task,
        now,
        &observed_topics,
    ));
    record_supervisor_checkpoint(state, supervisor_task_id, "background:progress", summary).await
}

fn background_supervisor_progress_summary(
    task: &TaskState,
    now: chrono::DateTime<chrono::Utc>,
    observed_topics: &[String],
) -> String {
    let elapsed = now
        .signed_duration_since(task.created_at)
        .num_seconds()
        .max(0);
    let elapsed_text = if elapsed < 90 {
        format!("{elapsed}s")
    } else {
        format!("{}m{}s", elapsed / 60, elapsed % 60)
    };
    let intent = task
        .contract
        .as_ref()
        .and_then(|contract| contract.intent.as_deref())
        .unwrap_or_default();
    let mut pending = task
        .contract
        .as_ref()
        .map(|contract| contract.required_events.clone())
        .unwrap_or_default();
    pending.retain(|required| {
        !observed_topics
            .iter()
            .any(|observed| topic_matches(required, observed))
            && !task_artifacts_satisfy_required_event(task, required)
            && !task_checkpoints_satisfy_required_event(task, required)
    });
    pending.sort();
    pending.dedup();

    let phase = if pending.iter().any(|event| event == "knowledge.imported")
        && pending.iter().any(|event| event == "artifact.pdf")
    {
        "正在后台协调检索、知识导入和 PDF 产物步骤"
    } else if pending.iter().any(|event| event == "knowledge.imported") {
        "正在后台协调检索和知识导入步骤"
    } else if pending.iter().any(|event| {
        matches!(
            event.as_str(),
            "artifact.pdf" | "artifact.txt" | "artifact.md" | "artifact.written"
        )
    }) {
        "正在后台协调可写产物步骤"
    } else if !intent.trim().is_empty() {
        "正在后台执行用户请求"
    } else {
        "正在后台执行任务"
    };
    let latest_activity = task
        .checkpoints
        .iter()
        .rev()
        .find(|checkpoint| checkpoint.label != "background:progress")
        .and_then(|checkpoint| checkpoint.summary.as_deref())
        .map(redact_internal_paths_for_chat)
        .map(|summary| preview_text(&summary, 180).to_string());

    if pending.is_empty() {
        if let Some(activity) = latest_activity {
            format!("{phase}，已运行 {elapsed_text}。最近进度：{activity}。")
        } else {
            format!(
                "{phase}，已运行 {elapsed_text}。外层监督任务仍在运行；当前尚未收到子步骤 checkpoint 或工具 receipt。"
            )
        }
    } else {
        format!(
            "{phase}，已运行 {elapsed_text}。完成门仍在等待运行时证据：{}。",
            pending.join(", ")
        )
    }
}

fn task_artifacts_satisfy_required_event(task: &TaskState, required: &str) -> bool {
    match required {
        "artifact.written" => !task.artifacts.is_empty(),
        "artifact.verified" => !task.artifacts.is_empty(),
        "artifact.pdf" => task.artifacts.iter().any(|artifact| {
            artifact
                .media_type
                .as_deref()
                .is_some_and(|media_type| media_type.eq_ignore_ascii_case("application/pdf"))
                || artifact
                    .uri
                    .rsplit_once('.')
                    .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("pdf"))
        }),
        "artifact.txt" => task_artifacts_contain_extension(task, "txt"),
        "artifact.md" => {
            task_artifacts_contain_extension(task, "md")
                || task_artifacts_contain_media_type(task, "text/markdown")
        }
        _ => false,
    }
}

fn task_artifacts_contain_extension(task: &TaskState, expected: &str) -> bool {
    task.artifacts.iter().any(|artifact| {
        artifact
            .uri
            .rsplit_once('.')
            .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case(expected))
    })
}

fn task_artifacts_contain_media_type(task: &TaskState, expected: &str) -> bool {
    task.artifacts.iter().any(|artifact| {
        artifact
            .media_type
            .as_deref()
            .is_some_and(|media_type| media_type.eq_ignore_ascii_case(expected))
    })
}

fn extract_task_artifacts_from_tool_result(
    tool_name: &str,
    result: &str,
) -> Vec<benshu_state::TaskArtifactRef> {
    if !artifact_write_result_is_completion_candidate(result) {
        return Vec::new();
    }
    let mut workspace_artifacts = Vec::new();
    collect_task_artifacts_from_workspace_paths(tool_name, result, &mut workspace_artifacts);

    let topics = runtime_effect_topics(result);
    let artifact_receipt = topics
        .iter()
        .any(|topic| topic == "artifact.written" || topic.starts_with("artifact."));
    let write_receipt = result.contains("executed_tool: write_file")
        || result.contains("Checkpointed ")
        || result.contains("finished success=true")
        || result.contains("\"success\":true")
        || result.contains("\"success\": true");
    if !artifact_receipt && !write_receipt && workspace_artifacts.is_empty() {
        return Vec::new();
    }

    let mut artifacts = workspace_artifacts;
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(result) {
        collect_task_artifacts_from_json(tool_name, &value, &mut artifacts);
    }
    collect_task_artifacts_from_receipt_lines(tool_name, result, &mut artifacts);
    artifacts
}

fn collect_task_artifacts_from_workspace_paths(
    tool_name: &str,
    result: &str,
    artifacts: &mut Vec<benshu_state::TaskArtifactRef>,
) {
    for uri in task_artifact_workspace_paths_from_text(result) {
        let Some(media_type) = media_type_for_artifact_uri(&uri) else {
            continue;
        };
        push_task_artifact(tool_name, artifacts, &uri, "tool_output", Some(media_type));
    }
}

fn collect_task_artifacts_from_json(
    tool_name: &str,
    value: &serde_json::Value,
    artifacts: &mut Vec<benshu_state::TaskArtifactRef>,
) {
    match value {
        serde_json::Value::Object(object) => {
            if let Some(items) = object
                .get("evidence_artifacts")
                .and_then(|items| items.as_array())
            {
                for artifact in items {
                    let Some(uri) = artifact.get("uri").and_then(|value| value.as_str()) else {
                        continue;
                    };
                    push_task_artifact(
                        tool_name,
                        artifacts,
                        uri,
                        artifact
                            .get("kind")
                            .and_then(|value| value.as_str())
                            .unwrap_or("tool_output"),
                        artifact.get("media_type").and_then(|value| value.as_str()),
                    );
                }
            }

            for key in ["artifact_path", "output_path", "path", "uri"] {
                if let Some(uri) = object.get(key).and_then(|value| value.as_str()) {
                    push_task_artifact(
                        tool_name,
                        artifacts,
                        uri,
                        "tool_output",
                        media_type_for_artifact_uri(uri),
                    );
                }
            }

            for item in object.values() {
                collect_task_artifacts_from_json(tool_name, item, artifacts);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_task_artifacts_from_json(tool_name, item, artifacts);
            }
        }
        _ => {}
    }
}

fn collect_task_artifacts_from_receipt_lines(
    tool_name: &str,
    result: &str,
    artifacts: &mut Vec<benshu_state::TaskArtifactRef>,
) {
    for line in result.lines() {
        let trimmed = line.trim();
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        let key = key.trim();
        if !matches!(
            key,
            "artifact_path" | "output_path" | "path" | "uri" | "evidence_artifacts"
        ) {
            continue;
        }
        for uri in value
            .split(',')
            .map(str::trim)
            .filter(|uri| !uri.is_empty())
        {
            push_task_artifact(
                tool_name,
                artifacts,
                uri,
                "tool_output",
                media_type_for_artifact_uri(uri),
            );
        }
    }
}

fn push_task_artifact(
    tool_name: &str,
    artifacts: &mut Vec<benshu_state::TaskArtifactRef>,
    uri: &str,
    kind: &str,
    media_type: Option<&str>,
) {
    if artifacts.iter().any(|artifact| artifact.uri == uri) {
        return;
    }
    artifacts.push(benshu_state::TaskArtifactRef {
        artifact_id: format!("{tool_name}:{}", Uuid::new_v4()),
        kind: kind.to_string(),
        uri: uri.to_string(),
        media_type: media_type.map(ToOwned::to_owned),
    });
}

fn chat_artifacts_from_outcome(outcome: &ChatOutcome) -> Vec<ChatArtifactRef> {
    let mut artifacts = outcome
        .runtime_task
        .as_ref()
        .map(|task| task.artifacts.clone())
        .unwrap_or_default();

    if artifacts.is_empty() {
        if let Some(trace) = outcome.run_trace.as_ref() {
            artifacts = trace
                .artifacts
                .iter()
                .map(|artifact| benshu_state::TaskArtifactRef {
                    artifact_id: artifact.artifact_id.clone(),
                    kind: artifact.kind.clone(),
                    uri: artifact.uri.clone(),
                    media_type: artifact.media_type.clone(),
                })
                .collect();
        }
    }

    if artifacts.is_empty() {
        artifacts = outcome
            .tool_calls
            .iter()
            .flat_map(|call| {
                call.result
                    .as_deref()
                    .map(|result| extract_task_artifacts_from_tool_result(&call.name, result))
                    .unwrap_or_default()
            })
            .collect();
    }

    let mut out = Vec::new();
    for artifact in artifacts {
        push_chat_artifact(&mut out, artifact);
    }
    out
}

fn push_chat_artifact(out: &mut Vec<ChatArtifactRef>, artifact: benshu_state::TaskArtifactRef) {
    if out.iter().any(|existing| existing.uri == artifact.uri) {
        return;
    }
    out.push(ChatArtifactRef {
        label: chat_artifact_label(&artifact),
        artifact_id: artifact.artifact_id,
        kind: artifact.kind,
        uri: artifact.uri,
        media_type: artifact.media_type,
    });
}

fn chat_artifact_label(artifact: &benshu_state::TaskArtifactRef) -> String {
    let media = artifact.media_type.as_deref().unwrap_or_default();
    let lower_uri = artifact.uri.to_ascii_lowercase();
    if artifact.kind.contains("chapter") || lower_uri.contains("/chapters/") {
        return "打开章节".to_string();
    }
    if artifact.kind.contains("export") || lower_uri.contains("/exports/") {
        if lower_uri.ends_with(".txt") || media.eq_ignore_ascii_case("text/plain") {
            return "打开 TXT 导出".to_string();
        }
        return "打开导出文件".to_string();
    }
    if media.eq_ignore_ascii_case("application/pdf") || lower_uri.ends_with(".pdf") {
        return "打开 PDF".to_string();
    }
    if media.eq_ignore_ascii_case("text/markdown")
        || lower_uri.ends_with(".md")
        || lower_uri.ends_with(".markdown")
    {
        return "打开 Markdown".to_string();
    }
    if media.starts_with("text/") || lower_uri.ends_with(".txt") {
        return "打开文本".to_string();
    }
    if media.starts_with("image/") {
        return "打开图片".to_string();
    }
    "打开文件".to_string()
}

fn task_artifact_workspace_paths_from_text(text: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for raw in text.split_whitespace() {
        let lowered = raw.to_ascii_lowercase().replace('\\', "/");
        let candidate = if lowered.starts_with("data/generated/") {
            raw
        } else {
            raw.find('/')
                .map(|index| &raw[index..])
                .or_else(|| {
                    raw.find(":\\")
                        .and_then(|index| index.checked_sub(1).map(|start| &raw[start..]))
                })
                .unwrap_or(raw)
        };
        let path = trim_artifact_uri_candidate(candidate);
        if path_looks_like_task_artifact_workspace(path) && !paths.iter().any(|item| item == path) {
            paths.push(path.to_string());
        }
    }
    paths
}

fn trim_artifact_uri_candidate(candidate: &str) -> &str {
    let trimmed = candidate.trim_matches(|ch: char| {
        matches!(
            ch,
            '"' | '\''
                | '`'
                | ','
                | ';'
                | ')'
                | '('
                | ']'
                | '['
                | '{'
                | '}'
                | '，'
                | '。'
                | '：'
                | ':'
        )
    });
    let end = trimmed
        .char_indices()
        .find_map(|(index, ch)| {
            if matches!(
                ch,
                '"' | '\'' | '`' | ',' | ';' | ')' | ']' | '}' | '，' | '。' | '；' | '、'
            ) {
                Some(index)
            } else {
                None
            }
        })
        .unwrap_or(trimmed.len());
    trimmed[..end].trim_matches(|ch: char| {
        matches!(
            ch,
            '"' | '\'' | '`' | ',' | ';' | ')' | ']' | '}' | '，' | '。' | '；' | '、'
        )
    })
}

fn summary_has_saved_artifact_path(summary: &str) -> bool {
    let saved_signal = summary.contains("已保存")
        || summary.contains("文件：")
        || summary.contains("文件:")
        || summary.to_ascii_lowercase().contains("saved");
    saved_signal && !task_artifact_workspace_paths_from_text(summary).is_empty()
}

fn path_looks_like_task_artifact_workspace(path: &str) -> bool {
    if path.trim().is_empty() {
        return false;
    }
    let lowered = path.to_ascii_lowercase().replace('\\', "/");
    (path.starts_with('/') || path.contains(":\\") || lowered.starts_with("data/generated/"))
        && (lowered.contains("/generated/")
            || lowered.starts_with("data/generated/")
            || lowered.ends_with(".txt")
            || lowered.ends_with(".md")
            || lowered.ends_with(".pdf")
            || lowered.ends_with("/project.json"))
}

fn media_type_for_artifact_uri(uri: &str) -> Option<&'static str> {
    let extension = uri.rsplit_once('.').map(|(_, extension)| {
        extension
            .split(|ch: char| !ch.is_ascii_alphanumeric())
            .next()
            .unwrap_or(extension)
            .to_ascii_lowercase()
    })?;
    match extension.as_str() {
        "txt" => Some("text/plain"),
        "md" | "markdown" => Some("text/markdown"),
        "pdf" => Some("application/pdf"),
        "json" => Some("application/json"),
        "csv" => Some("text/csv"),
        "html" | "htm" => Some("text/html"),
        _ => None,
    }
}

fn task_checkpoints_satisfy_required_event(task: &TaskState, required: &str) -> bool {
    task.checkpoints.iter().any(|checkpoint| {
        checkpoint
            .summary
            .as_deref()
            .is_some_and(|summary| checkpoint_summary_satisfies_required_event(summary, required))
    })
}

fn checkpoint_summary_satisfies_required_event(summary: &str, required: &str) -> bool {
    match required {
        "knowledge.imported" => {
            summary.contains("runtime_effect: knowledge.imported")
                || summary.contains("runtime_effects: knowledge.imported")
                || summary.contains("Imported web knowledge into collection")
                || summary.contains("executed_tool: knowledge_import")
        }
        "artifact.written" => {
            artifact_write_result_is_completion_candidate(summary)
                && (summary.contains("runtime_effect: artifact.written")
                    || summary.contains("runtime_effects: artifact.written")
                    || (summary.contains("finished success=true")
                        && (summary.contains("\"artifact_path\"")
                            || summary.contains("artifact_path:")))
                    || summary.contains("executed_tool: write_file")
                    || summary.contains("wrote ")
                    || summary_has_saved_artifact_path(summary))
                && (summary.contains("status: completed")
                    || summary.contains("finished success=true")
                    || summary.contains("continuous_output")
                    || summary.contains("Checkpointed ")
                    || summary_has_saved_artifact_path(summary))
        }
        "artifact.verified" => {
            checkpoint_summary_satisfies_required_event(summary, "artifact.written")
                || ((summary.contains("runtime_effect: artifact.verified")
                    || summary.contains("runtime_effects: artifact.verified"))
                    && (summary.contains("\"artifact_path\"")
                        || summary.contains("artifact_path:")
                        || summary.contains("\"output_path\"")
                        || summary.contains("output_path:")
                        || summary.contains("\"project_path\"")
                        || summary.contains("project_path:"))
                    && (summary.contains("status: completed")
                        || summary.contains("finished success=true")
                        || summary.contains("\"success\":true")
                        || summary.contains("\"success\": true")))
        }
        "artifact.pdf" => {
            (summary.contains("runtime_effect: artifact.pdf")
                || summary.contains("runtime_effects: artifact.pdf")
                || summary.contains("application/pdf")
                || summary.contains(".pdf"))
                && (summary.contains("status: completed")
                    || summary.contains("finished success=true")
                    || summary.contains("executed_tool: write_file")
                    || summary.contains("Checkpointed "))
        }
        "artifact.txt" => {
            summary_has_artifact_format(summary, "txt")
                && (summary.contains("status: completed")
                    || summary.contains("finished success=true")
                    || summary.contains("executed_tool: write_file")
                    || summary.contains("Checkpointed ")
                    || summary_has_saved_artifact_path(summary))
        }
        "artifact.md" => {
            summary_has_artifact_format(summary, "md")
                && (summary.contains("status: completed")
                    || summary.contains("finished success=true")
                    || summary.contains("executed_tool: write_file")
                    || summary.contains("Checkpointed "))
        }
        _ => false,
    }
}

fn summary_has_artifact_format(summary: &str, format: &str) -> bool {
    let lowered = summary.to_ascii_lowercase();
    match format {
        "pdf" => {
            lowered.contains(".pdf")
                || lowered.contains("application/pdf")
                || lowered.contains("artifact.pdf")
                || lowered.contains("\"format\":\"pdf\"")
                || lowered.contains("\"format\": \"pdf\"")
                || lowered.contains("format: pdf")
        }
        "txt" => {
            lowered.contains(".txt")
                || lowered.contains("text/plain")
                || lowered.contains("artifact.txt")
                || lowered.contains("\"format\":\"txt\"")
                || lowered.contains("\"format\": \"txt\"")
                || lowered.contains("format: txt")
        }
        "md" => {
            lowered.contains(".md")
                || lowered.contains("text/markdown")
                || lowered.contains("artifact.md")
                || lowered.contains("\"format\":\"md\"")
                || lowered.contains("\"format\": \"md\"")
                || lowered.contains("\"format\":\"markdown\"")
                || lowered.contains("\"format\": \"markdown\"")
                || lowered.contains("format: md")
                || lowered.contains("format: markdown")
        }
        _ => false,
    }
}

fn semantic_runtime_events_for_checkpoint(
    checkpoint: &TaskCheckpoint,
) -> Vec<(String, serde_json::Value)> {
    let Some(summary) = checkpoint.summary.as_deref() else {
        return Vec::new();
    };
    let artifact_completion_candidate = artifact_write_result_is_completion_candidate(summary);
    let mut topics = runtime_effect_topics(summary)
        .into_iter()
        .filter(|topic| topic != "artifact.written" || artifact_completion_candidate)
        .collect::<Vec<_>>();
    if checkpoint.label.contains(":tool:")
        && checkpoint.label.ends_with(":end")
        && summary.contains("finished success=true")
    {
        if summary.contains("\"artifact_path\"") || summary.contains("artifact_path:") {
            push_unique_topic(&mut topics, "artifact.written");
        }
        if summary.contains("application/pdf") || summary.contains(".pdf") {
            push_unique_topic(&mut topics, "artifact.pdf");
            push_unique_topic(&mut topics, "artifact.written");
        }
        if summary_has_artifact_format(summary, "txt") {
            push_unique_topic(&mut topics, "artifact.txt");
            push_unique_topic(&mut topics, "artifact.written");
        }
        if summary_has_artifact_format(summary, "md") {
            push_unique_topic(&mut topics, "artifact.md");
            push_unique_topic(&mut topics, "artifact.written");
        }
        if summary_has_saved_artifact_path(summary) {
            push_unique_topic(&mut topics, "artifact.written");
        }
        if summary.contains("knowledge.imported")
            || summary.contains("Imported web knowledge into collection")
        {
            push_unique_topic(&mut topics, "knowledge.imported");
        }
    }
    if artifact_completion_candidate && summary_has_saved_artifact_path(summary) {
        push_unique_topic(&mut topics, "artifact.written");
        if summary_has_artifact_format(summary, "txt") {
            push_unique_topic(&mut topics, "artifact.txt");
        }
        if summary_has_artifact_format(summary, "md") {
            push_unique_topic(&mut topics, "artifact.md");
        }
        if summary_has_artifact_format(summary, "pdf") {
            push_unique_topic(&mut topics, "artifact.pdf");
        }
    }
    topics
        .into_iter()
        .map(|topic| {
            (
                topic,
                serde_json::json!({
                    "checkpoint_label": checkpoint.label,
                    "summary": preview_text(summary, 500),
                }),
            )
        })
        .collect()
}

fn push_unique_topic(topics: &mut Vec<String>, topic: &str) {
    if runtime_effect_topic_is_valid(topic) && !topics.iter().any(|item| item == topic) {
        topics.push(topic.to_string());
    }
}

async fn latest_active_task_for_session(state: &AppState, session_id: &str) -> Option<TaskState> {
    latest_session_task_for_session(state, session_id)
        .await
        .filter(|task| task_is_active_or_recoverable_in_current_gateway(state, task))
}

async fn latest_session_task_for_session(state: &AppState, session_id: &str) -> Option<TaskState> {
    let mut tasks = state
        .kernel
        .state_task()
        .list_by_session(session_id)
        .await
        .ok()?;
    tasks.sort_by(|left, right| {
        session_task_rank(right)
            .cmp(&session_task_rank(left))
            .then_with(|| right.updated_at.cmp(&left.updated_at))
            .then_with(|| right.created_at.cmp(&left.created_at))
    });
    tasks.into_iter().next()
}

async fn latest_status_task_for_session(state: &AppState, session_id: &str) -> Option<TaskState> {
    let mut tasks = state
        .kernel
        .state_task()
        .list_by_session(session_id)
        .await
        .ok()?;
    tasks.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| right.created_at.cmp(&left.created_at))
            .then_with(|| session_task_rank(right).cmp(&session_task_rank(left)))
    });
    tasks.into_iter().next()
}

async fn latest_active_task_id_for_session(state: &AppState, session_id: &str) -> Option<Uuid> {
    latest_active_task_for_session(state, session_id)
        .await
        .map(|task| task.id)
}

fn active_session_task_interruption_response(task: &TaskState) -> String {
    match task.status {
        TaskStatus::Paused(_) => "当前任务已暂停，等待继续。\n\n回复“继续”会从暂停检查点恢复；也可以先回复“停止”取消它，再发新的请求。".to_string(),
        _ => "当前任务还在运行，我不会用这条新消息打断它或开启重复任务。\n\n如果你想查看进度，请回复“进度”；如果要中断，请回复“停止”或“等一下”。".to_string(),
    }
}

async fn mark_latest_session_task_paused(
    state: &AppState,
    session_id: &str,
) -> Result<(), AppError> {
    let Some(mut task) = latest_active_task_for_session(state, session_id).await else {
        return Ok(());
    };
    if is_terminal_task_status(&task.status) || matches!(task.status, TaskStatus::Paused(_)) {
        return Ok(());
    }
    task.status = TaskStatus::Paused(chrono::Utc::now());
    task.updated_at = chrono::Utc::now();
    task.checkpoints.push(TaskCheckpoint {
        step: task.current_step,
        label: "paused_by_user".to_string(),
        recorded_at: task.updated_at,
        summary: Some("Task paused by user; resume keeps the same task context.".to_string()),
    });
    state.kernel.state_task().save(task).await?;
    Ok(())
}

async fn mark_latest_session_task_running(
    state: &AppState,
    session_id: &str,
) -> Result<(), AppError> {
    let Some(mut task) = latest_active_task_for_session(state, session_id).await else {
        return Ok(());
    };
    if !matches!(task.status, TaskStatus::Paused(_)) {
        return Ok(());
    }
    task.status = TaskStatus::Running;
    task.updated_at = chrono::Utc::now();
    task.checkpoints.push(TaskCheckpoint {
        step: task.current_step,
        label: "resumed_by_user".to_string(),
        recorded_at: task.updated_at,
        summary: Some("Task resumed by user from a paused checkpoint.".to_string()),
    });
    state.kernel.state_task().save(task).await?;
    Ok(())
}

async fn resume_durable_paused_supervisor_task(
    state: &AppState,
    session_id: &str,
    supplemental_instruction: Option<&str>,
) -> Result<Option<Uuid>, AppError> {
    let Some(mut task) = latest_active_task_for_session(state, session_id).await else {
        return Ok(None);
    };
    if !task_needs_durable_reschedule(&task) {
        return Ok(None);
    }
    let Some(contract) = task.contract.clone() else {
        return Ok(None);
    };
    let Some(mut prompt) = contract.intent.clone() else {
        return Ok(None);
    };
    if let Some(instruction) = supplemental_instruction
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        prompt.push_str(
            "\n\n用户在恢复暂停任务时补充了以下指令。请把它作为同一任务的补充约束，而不是新任务：\n",
        );
        prompt.push_str(instruction);
    }

    task.status = TaskStatus::Running;
    task.updated_at = chrono::Utc::now();
    task.result = Some(serde_json::json!({
        "resumed": true,
        "response_text": "任务正在从最近的持久化检查点继续执行。",
        "durable_resume": true,
    }));
    task.checkpoints.push(TaskCheckpoint {
        step: task.current_step,
        label: "durable_pause:rescheduled".to_string(),
        recorded_at: task.updated_at,
        summary: Some(
            "Paused task resumed by rescheduling the same durable supervisor contract.".to_string(),
        ),
    });
    let task_id = task.id;
    state.kernel.state_task().save(task).await?;

    spawn_existing_supervised_chat_task(
        state.clone(),
        session_id.to_string(),
        vec![AgentMessage::user(Content::text(prompt))],
        task_id,
    );

    Ok(Some(task_id))
}

fn task_needs_durable_reschedule(task: &TaskState) -> bool {
    matches!(task.status, TaskStatus::Paused(_))
        || task
            .result
            .as_ref()
            .and_then(|result| result.get("paused"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
}

fn spawn_existing_supervised_chat_task(
    state: AppState,
    session_id: String,
    messages: Vec<AgentMessage>,
    supervisor_task_id: Uuid,
) {
    tokio::spawn(async move {
        let (heartbeat_stop_tx, heartbeat_stop_rx) = tokio::sync::oneshot::channel();
        let heartbeat_handle = tokio::spawn(supervisor_task_heartbeat(
            state.clone(),
            supervisor_task_id,
            heartbeat_stop_rx,
        ));
        let (event_stop_tx, event_stop_rx) = tokio::sync::oneshot::channel();
        let event_handle = tokio::spawn(supervisor_agent_event_monitor(
            state.clone(),
            supervisor_task_id,
            session_id.clone(),
            event_stop_rx,
        ));
        let result = execute_supervised_chat(
            state.clone(),
            session_id.clone(),
            messages,
            supervisor_task_id,
        )
        .await;
        if let Err(error) = result {
            let _ =
                mark_supervisor_task_failed(&state, supervisor_task_id, &error.to_string()).await;
        }
        tokio::spawn(async move {
            let _ = heartbeat_stop_tx.send(());
            let _ = event_stop_tx.send(());
            let _ = heartbeat_handle.await;
            let _ = event_handle.await;
        });
    });
}

async fn execute_supervised_chat(
    state: AppState,
    session_id: String,
    messages: Vec<AgentMessage>,
    supervisor_task_id: Uuid,
) -> anyhow::Result<CompletedSupervisedChat> {
    if let Some(route) = direct_writer_route_from_messages(&messages) {
        return match execute_supervised_direct_writer_delegate(
            state.clone(),
            session_id,
            route,
            supervisor_task_id,
        )
        .await
        {
            Ok(completed) => Ok(completed),
            Err(error) => {
                mark_supervisor_task_failed(&state, supervisor_task_id, &error.to_string()).await?;
                Err(error)
            }
        };
    }

    let creation_planning_dialogue = build_chat_task_contract(&messages)
        .intent
        .as_deref()
        .is_some_and(creation_planning_dialogue_requested);
    let repair_base_messages = creation_planning_dialogue.then(|| messages.clone());

    if creation_planning_dialogue {
        let planning_prompt =
            creation_planning_prompt_from_messages(&messages).unwrap_or_else(|| {
                build_chat_task_contract(&messages)
                    .intent
                    .unwrap_or_default()
            });
        let outcome = execute_creation_planning_dialogue_transient(
            &state,
            &session_id,
            planning_prompt.as_str(),
            supervisor_task_id,
        )
        .await?;
        update_supervisor_task_from_outcome(&state, &session_id, supervisor_task_id, &outcome)
            .await?;
        let visible_task_status = state
            .kernel
            .state_task()
            .load(&supervisor_task_id.to_string())
            .await
            .ok()
            .flatten()
            .map(|task| task.status)
            .unwrap_or(TaskStatus::Completed);
        let visible_user_request = extract_creation_planning_latest_user_request(&planning_prompt);
        let visible_turn_status = persist_gateway_visible_chat_turn(
            &state,
            &session_id,
            visible_user_request.as_deref(),
            &outcome,
            &visible_task_status,
        )
        .await;
        let runtime_persistence_status =
            persist_chat_runtime_mainline(&state, &session_id, supervisor_task_id, &outcome).await;
        let runtime_persistence_status =
            if runtime_persistence_status.as_deref() == Some("not_needed") {
                visible_turn_status.or(runtime_persistence_status)
            } else {
                runtime_persistence_status.or(visible_turn_status)
            };
        return Ok(CompletedSupervisedChat {
            outcome,
            supervisor_task_id,
            runtime_persistence_status,
        });
    }

    let result = state
        .kernel
        .coordinator()
        .chat_session(&session_id, messages)
        .await;

    match result {
        Ok(mut outcome) => {
            if let Some(run_trace) = outcome.run_trace.as_mut() {
                run_trace
                    .metadata
                    .entry("chat_route".to_string())
                    .or_insert_with(|| "coordinator".to_string());
                run_trace.metadata.insert(
                    "foreground_supervisor_task_id".to_string(),
                    supervisor_task_id.to_string(),
                );
            }

            outcome = maybe_repair_creation_planning_outcome(
                &state,
                &session_id,
                repair_base_messages.as_deref().unwrap_or(&[]),
                creation_planning_dialogue,
                outcome,
                supervisor_task_id,
            )
            .await?;

            update_supervisor_task_from_outcome(&state, &session_id, supervisor_task_id, &outcome)
                .await?;
            let runtime_persistence_status = if outcome_is_lightweight_realtime_lookup(&outcome) {
                schedule_chat_runtime_mainline_persistence(
                    state.clone(),
                    session_id.clone(),
                    supervisor_task_id,
                    outcome.clone(),
                )
            } else {
                persist_chat_runtime_mainline(&state, &session_id, supervisor_task_id, &outcome)
                    .await
            };

            Ok(CompletedSupervisedChat {
                outcome,
                supervisor_task_id,
                runtime_persistence_status,
            })
        }
        Err(error) => {
            mark_supervisor_task_failed(&state, supervisor_task_id, &error.to_string()).await?;
            Err(error.into())
        }
    }
}

fn direct_writer_route_from_messages(messages: &[AgentMessage]) -> Option<DirectWriterRoute> {
    if let Some(intent) = build_chat_task_contract(messages).intent {
        if let Some(route) = writing_session_route::direct_writer_route_from_text(&intent) {
            return Some(route);
        }
    }

    let text = messages
        .iter()
        .rev()
        .find(|message| matches!(message.role, benshu_brain::agent::message::Role::User))?
        .content
        .as_text();
    writing_session_route::direct_writer_route_from_text(&text)
}

async fn execute_supervised_direct_writer_delegate(
    state: AppState,
    session_id: String,
    route: DirectWriterRoute,
    supervisor_task_id: Uuid,
) -> anyhow::Result<CompletedSupervisedChat> {
    if let Some(active) =
        active_writing_task_for_session(&state, &session_id, supervisor_task_id).await
    {
        let response = format!(
            "当前写作任务还在执行中，不能同时开启新的章节写作。\n\n状态：{}\n最近进度：{}\n\n你可以等待它完成，或先说“暂停/取消当前任务”。",
            task_status_parts(&active.status).0,
            active
                .checkpoints
                .last()
                .and_then(|checkpoint| checkpoint.summary.as_deref())
                .unwrap_or("暂无进度 checkpoint")
        );
        let outcome = ChatOutcome {
            response,
            thoughts: vec![
                "gateway direct writer continuation blocked by active writing task".to_string(),
            ],
            tool_calls: Vec::new(),
            metabolic_stats: None,
            ownership: benshu_protocol_core::TaskOwnership::direct(
                AgentRole::Custom("benshu".to_string()),
                Some(session_id.clone()),
            ),
            delegation: None,
            handover: None,
            runtime_task: None,
            run_trace: None,
        };
        if let Some(mut task_state) = state
            .kernel
            .state_task()
            .load(&supervisor_task_id.to_string())
            .await?
        {
            task_state.status = TaskStatus::Blocked {
                reason: "active writing task is already running for this session".to_string(),
            };
            task_state.updated_at = chrono::Utc::now();
            task_state.session_id = Some(session_id.clone());
            task_state.thread_id = Some(session_id.clone());
            task_state.checkpoints.push(TaskCheckpoint {
                step: task_state.current_step,
                label: "gateway:direct-writer-delegate:active-task-blocked".to_string(),
                recorded_at: task_state.updated_at,
                summary: Some(format!(
                    "同一 session 已有写作任务 {} 未完成，已拒绝并发章节写作。",
                    active.id
                )),
            });
            task_state.result = Some(serde_json::json!({
                "response_text": outcome.response,
                "active_task_id": active.id,
                "status": "blocked",
                "blocker": "active_writing_task"
            }));
            state.kernel.state_task().save(task_state).await?;
        }
        let visible_turn_status = persist_gateway_visible_chat_turn(
            &state,
            &session_id,
            visible_user_request_from_gateway_task(&route.task).as_deref(),
            &outcome,
            &TaskStatus::Blocked {
                reason: "active writing task is already running for this session".to_string(),
            },
        )
        .await;
        let runtime_mainline_status =
            persist_chat_runtime_mainline(&state, &session_id, supervisor_task_id, &outcome).await;
        let runtime_persistence_status = if runtime_mainline_status.as_deref() == Some("not_needed")
        {
            visible_turn_status.or(runtime_mainline_status)
        } else {
            runtime_mainline_status.or(visible_turn_status)
        };

        return Ok(CompletedSupervisedChat {
            outcome,
            supervisor_task_id,
            runtime_persistence_status,
        });
    }

    let is_content_operation = route.is_content_operation;
    record_supervisor_checkpoint(
        &state,
        supervisor_task_id,
        if is_content_operation {
            "gateway:direct-writer-content-operation"
        } else {
            "gateway:direct-writer-delegate"
        },
        Some(if is_content_operation {
            "已有写作项目内容操作已由 gateway 路由到 writer worker，跳过主 agent 空转推理。"
                .to_string()
        } else {
            "已有写作项目续写已由 gateway 路由到 writer worker，跳过主 agent 空转推理。".to_string()
        }),
    )
    .await?;

    let writer_role = AgentRole::Custom("writer".to_string());
    if state.kernel.coordinator().get(&writer_role).is_none() {
        if let Err(error) = state.factory.spawn_worker("writer").await {
            record_supervisor_checkpoint(
                &state,
                supervisor_task_id,
                "gateway:direct-writer-delegate:worker-load-failed",
                Some(format!("writer worker 懒加载失败：{error}")),
            )
            .await?;
            anyhow::bail!("writer worker is configured but failed to load: {error}");
        }
    }
    let Some(writer_agent) = state.kernel.coordinator().get(&writer_role) else {
        anyhow::bail!("writer worker is not loaded; cannot start governed writing workflow");
    };
    let workspace = chat_data_dir(&state)
        .parent()
        .unwrap_or_else(|| chat_data_dir(&state))
        .to_path_buf();
    let task = route.task.clone();
    let existing_project_path = route.project_path.clone();
    let creation_draft_path = route.draft_path.clone();
    let chapter_count = route.chapter_count;
    let requested_start_chapter = route.requested_start_chapter;
    let args = serde_json::json!({
        "role": "writer",
        "task": task.clone(),
        "full_user_request": task.clone(),
        "project_path": existing_project_path.clone(),
        "draft_path": creation_draft_path.clone(),
        "chapter_count": chapter_count,
        "requested_start_chapter": requested_start_chapter,
    });
    let started = std::time::Instant::now();
    let task_for_workflow = task.clone();
    let task_manager = state.kernel.state_task().clone();
    let event_manager = state.kernel.state_runtime_event().clone();
    let result = if is_content_operation {
        run_novel_content_operation_for_delegate_on_dedicated_stack(
            writer_agent,
            task_for_workflow,
            NovelContentOperationConfig {
                workspace,
                worker_label: "writer".to_string(),
            },
            supervisor_task_id,
            session_id.clone(),
        )
        .await?
    } else {
        run_novel_workflow_for_delegate_on_dedicated_stack(
            writer_agent,
            task_for_workflow,
            NovelWorkflowConfig {
                workspace,
                worker_label: "writer".to_string(),
                target_units: None,
                chapter_unit_target: None,
                chapter_count,
                requested_start_chapter,
                existing_project_path,
                creation_draft_path,
                runtime: NovelWorkflowRuntimeState {
                    task_id: Some(supervisor_task_id),
                    task_manager: Some(task_manager),
                    event_manager: Some(event_manager),
                },
            },
            supervisor_task_id,
            session_id.clone(),
        )
        .await?
    };
    let duration_ms = started.elapsed().as_millis().try_into().unwrap_or(u64::MAX);
    let tool_call = ToolCallData {
        receipt_id: Some(Uuid::new_v4().to_string()),
        tool_call_id: Some(format!("gateway-direct-writer-{supervisor_task_id}")),
        name: "delegate".to_string(),
        args: args.to_string(),
        result: Some(result.clone()),
        backup: None,
        duration_ms,
        timestamp: chrono::Utc::now().timestamp_millis().max(0) as u64,
        caller_id: Some("gateway.chat_supervisor".to_string()),
        safety_level: SafetyLevel::Green,
        cpu_pressure: None,
        vram_pressure: None,
        result_truncated: false,
        result_original_chars: None,
        result_omitted_chars: None,
        args_fingerprint: None,
        result_fingerprint: None,
        outcome: None,
        replay: None,
    };
    let outcome = ChatOutcome {
        response: preview_text(&result, 4000).to_string(),
        thoughts: vec!["gateway direct writer continuation route".to_string()],
        tool_calls: vec![tool_call],
        metabolic_stats: None,
        ownership: benshu_protocol_core::TaskOwnership::direct(
            AgentRole::Custom("benshu".to_string()),
            Some(session_id.clone()),
        ),
        delegation: None,
        handover: None,
        runtime_task: None,
        run_trace: None,
    };

    update_supervisor_task_from_outcome(&state, &session_id, supervisor_task_id, &outcome).await?;
    let visible_task_status = state
        .kernel
        .state_task()
        .load(&supervisor_task_id.to_string())
        .await
        .ok()
        .flatten()
        .map(|task| task.status)
        .unwrap_or(TaskStatus::Completed);
    let visible_turn_status = persist_gateway_visible_chat_turn(
        &state,
        &session_id,
        visible_user_request_from_gateway_task(&route.task).as_deref(),
        &outcome,
        &visible_task_status,
    )
    .await;
    let runtime_mainline_status =
        persist_chat_runtime_mainline(&state, &session_id, supervisor_task_id, &outcome).await;
    let runtime_persistence_status = if runtime_mainline_status.as_deref() == Some("not_needed") {
        visible_turn_status.or(runtime_mainline_status)
    } else {
        runtime_mainline_status.or(visible_turn_status)
    };

    Ok(CompletedSupervisedChat {
        outcome,
        supervisor_task_id,
        runtime_persistence_status,
    })
}

async fn run_novel_workflow_for_delegate_on_dedicated_stack(
    writer_agent: std::sync::Arc<dyn benshu_brain::agent::multi_agent::MultiAgent>,
    task: String,
    config: NovelWorkflowConfig,
    supervisor_task_id: Uuid,
    session_id: String,
) -> anyhow::Result<String> {
    let handle = tokio::runtime::Handle::current();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let thread_name = format!("benshu-writer-workflow-{supervisor_task_id}");
    std::thread::Builder::new()
        .name(thread_name)
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let result =
                handle.block_on(
                    benshu_brain::skills::CURRENT_RUNTIME_SECURITY_CONTEXT.scope(
                        benshu_brain::skills::RuntimeSecurityContext {
                            task_id: Some(supervisor_task_id.to_string()),
                            session_id: Some(session_id),
                            ..Default::default()
                        },
                        async move {
                            run_novel_workflow_for_delegate(writer_agent, &task, config).await
                        },
                    ),
                );
            let _ = tx.send(result);
        })
        .map_err(|error| anyhow::anyhow!("failed to spawn writer workflow thread: {error}"))?;
    rx.await.map_err(|_| {
        anyhow::anyhow!("writer workflow thread terminated before reporting a result")
    })?
}

async fn run_novel_content_operation_for_delegate_on_dedicated_stack(
    writer_agent: std::sync::Arc<dyn benshu_brain::agent::multi_agent::MultiAgent>,
    task: String,
    config: NovelContentOperationConfig,
    supervisor_task_id: Uuid,
    session_id: String,
) -> anyhow::Result<String> {
    let handle = tokio::runtime::Handle::current();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let thread_name = format!("benshu-writer-content-operation-{supervisor_task_id}");
    std::thread::Builder::new()
        .name(thread_name)
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let result = handle.block_on(
                benshu_brain::skills::CURRENT_RUNTIME_SECURITY_CONTEXT.scope(
                    benshu_brain::skills::RuntimeSecurityContext {
                        task_id: Some(supervisor_task_id.to_string()),
                        session_id: Some(session_id),
                        ..Default::default()
                    },
                    async move {
                        run_novel_content_operation_for_delegate(writer_agent, &task, config).await
                    },
                ),
            );
            let _ = tx.send(result);
        })
        .map_err(|error| {
            anyhow::anyhow!("failed to spawn writer content operation thread: {error}")
        })?;
    rx.await.map_err(|_| {
        anyhow::anyhow!("writer content operation thread terminated before reporting a result")
    })?
}

async fn active_writing_task_for_session(
    state: &AppState,
    session_id: &str,
    current_task_id: Uuid,
) -> Option<TaskState> {
    let mut tasks = state
        .kernel
        .state_task()
        .list_by_session(session_id)
        .await
        .ok()?;
    tasks.sort_by(|left, right| {
        session_task_rank(right)
            .cmp(&session_task_rank(left))
            .then_with(|| right.updated_at.cmp(&left.updated_at))
            .then_with(|| right.created_at.cmp(&left.created_at))
    });
    tasks.into_iter().find(|task| {
        task.id != current_task_id
            && task_is_active_or_recoverable_in_current_gateway(state, task)
            && writing_session_route::task_looks_like_writing_task(task)
    })
}

fn task_is_active_or_recoverable_in_current_gateway(state: &AppState, task: &TaskState) -> bool {
    match task.status {
        TaskStatus::Running | TaskStatus::Queued | TaskStatus::Pending => state
            .cancel_tokens
            .contains_key(task.id.to_string().as_str()),
        TaskStatus::Paused(_) => true,
        _ => false,
    }
}

fn outcome_is_lightweight_realtime_lookup(outcome: &ChatOutcome) -> bool {
    outcome.delegation.is_none()
        && outcome.handover.is_none()
        && !outcome.tool_calls.is_empty()
        && outcome.tool_calls.iter().all(|call| {
            matches!(
                call.name.as_str(),
                "weather_lookup" | "price_lookup" | "fx_lookup" | "latest_info_lookup"
            )
        })
}

fn schedule_chat_runtime_mainline_persistence(
    state: AppState,
    session_id: String,
    supervisor_task_id: Uuid,
    outcome: ChatOutcome,
) -> Option<String> {
    if outcome.runtime_task.is_none() && outcome.run_trace.is_none() {
        return Some("not_needed".to_string());
    }

    tokio::spawn(async move {
        let status =
            persist_chat_runtime_mainline(&state, &session_id, supervisor_task_id, &outcome).await;
        debug!(
            task_id = %supervisor_task_id,
            status = ?status,
            "Chat runtime mainline persistence completed in async tail"
        );
    });

    Some("queued_async".to_string())
}

async fn update_supervisor_task_from_outcome(
    state: &AppState,
    session_id: &str,
    supervisor_task_id: Uuid,
    outcome: &ChatOutcome,
) -> anyhow::Result<()> {
    let Some(mut task) = state
        .kernel
        .state_task()
        .load(&supervisor_task_id.to_string())
        .await?
    else {
        return Ok(());
    };
    if matches!(task.status, TaskStatus::Cancelled) {
        return Ok(());
    }

    let recorded_events =
        record_tool_runtime_events(state, session_id, supervisor_task_id, outcome).await?;
    let mut task_status = supervisor_status_from_outcome(outcome).unwrap_or(TaskStatus::Completed);
    let is_creation_contract_task =
        writing_session_route::task_is_creation_contract_planning(&task);
    task.updated_at = chrono::Utc::now();
    task.current_step = outcome.tool_calls.len() as u32;
    task.total_steps = Some(outcome.tool_calls.len() as u32);
    task.run_id = outcome
        .runtime_task
        .as_ref()
        .and_then(|runtime_task| runtime_task.run_id)
        .or_else(|| outcome.run_trace.as_ref().map(|trace| trace.run_id));
    task.trace_id = outcome
        .runtime_task
        .as_ref()
        .and_then(|runtime_task| runtime_task.trace_id)
        .or_else(|| outcome.run_trace.as_ref().map(|trace| trace.run_id));
    task.session_id = Some(session_id.to_string());
    task.thread_id = Some(session_id.to_string());
    let mut checkpoints = task.checkpoints.clone();
    checkpoints.extend(
        outcome
            .tool_calls
            .iter()
            .enumerate()
            .map(|(idx, call)| TaskCheckpoint {
                step: (idx + 1) as u32,
                label: format!("tool:{}", call.name),
                recorded_at: chrono::Utc::now(),
                summary: Some(format!(
                    "duration_ms={} preview={}",
                    call.duration_ms,
                    benshu_compression::preview_text(
                        call.result.as_deref().unwrap_or_default(),
                        240
                    )
                )),
            }),
    );
    task.checkpoints = checkpoints;
    task.artifacts = outcome
        .runtime_task
        .as_ref()
        .map(|runtime_task| runtime_task.artifacts.clone())
        .unwrap_or_default();
    if task.artifacts.is_empty() {
        if let Some(trace) = outcome.run_trace.as_ref() {
            task.artifacts = trace
                .artifacts
                .iter()
                .map(|artifact| benshu_state::TaskArtifactRef {
                    artifact_id: artifact.artifact_id.clone(),
                    kind: artifact.kind.clone(),
                    uri: artifact.uri.clone(),
                    media_type: artifact.media_type.clone(),
                })
                .collect();
        }
    }
    if task.artifacts.is_empty() {
        task.artifacts = outcome
            .tool_calls
            .iter()
            .flat_map(|call| {
                call.result
                    .as_deref()
                    .map(|result| extract_task_artifacts_from_tool_result(&call.name, result))
                    .unwrap_or_default()
            })
            .collect();
    }
    let verification = build_task_verification(&task, outcome, &recorded_events);
    task.evidence.insert(
        "runtime_events".to_string(),
        serde_json::json!({
            "event_ids": verification.evidence_event_ids.clone(),
            "tool_call_count": outcome.tool_calls.len(),
            "artifact_count": task.artifacts.len(),
        }),
    );
    if matches!(task_status, TaskStatus::Completed)
        && verification.verdict != TaskVerificationVerdict::Pass
    {
        task_status = TaskStatus::Blocked {
            reason: verification
                .summary
                .clone()
                .unwrap_or_else(|| "completion evidence is insufficient".to_string()),
        };
    }
    if is_creation_contract_task
        && !matches!(
            task_status,
            TaskStatus::Cancelled
                | TaskStatus::Failed { .. }
                | TaskStatus::Paused(_)
                | TaskStatus::Blocked { .. }
        )
    {
        if let Some(bound_status) =
            creation_contract_task_status_from_session_draft(state, session_id).await
        {
            task_status = bound_status;
        }
    }
    let provider_pause_reason = provider_disconnect_reason_from_outcome(outcome);
    if let Some(reason) = provider_pause_reason.as_ref() {
        if matches!(task_status, TaskStatus::Paused(_)) {
            task.checkpoints
                .push(supervisor_provider_disconnect_checkpoint(reason));
        }
    }
    let mut response_text = supervisor_user_visible_response_text(outcome, &task_status);
    if is_creation_contract_task {
        if let Ok(Some(draft)) = load_session_creation_draft(state, session_id).await {
            let draft_response =
                writing_session_surface::stabilize_creation_contract_panel_response(
                    &draft,
                    &response_text,
                );
            if !draft_response.trim().is_empty() {
                response_text = draft_response;
            }
        }
    }
    task.status = task_status;
    let mut result = serde_json::json!({
        "response_text": response_text,
        "thought_count": outcome.thoughts.len(),
        "tool_call_count": outcome.tool_calls.len(),
        "handover": outcome.handover,
        "delegation": outcome.delegation,
        "provider_disconnect_reason": provider_pause_reason,
    });
    if is_creation_contract_task {
        let (lifecycle_status, provisional) =
            if let Ok(Some(draft)) = load_session_creation_draft(state, session_id).await {
                let provisional =
                    !writing_session_surface::creation_contract_draft_is_confirmable(&draft);
                (
                    writing_session_surface::creation_contract_panel_status_for_draft(&draft),
                    provisional,
                )
            } else {
                (
                creation_contract_lifecycle_status_from_session_draft(state, session_id)
                    .await
                    .unwrap_or_else(|| {
                        writing_session_surface::creation_contract_lifecycle_status_for_task_status(
                            &task.status,
                        )
                    }),
                !matches!(task.status, TaskStatus::Completed),
            )
            };
        result["creation_contract"] = writing_session_surface::creation_contract_panel_payload(
            lifecycle_status,
            result
                .get("response_text")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default(),
            provisional,
        );
    }
    task.result = Some(result);
    task.verification = Some(verification);
    state.kernel.state_task().save(task).await?;
    if let Some(reason) = provider_disconnect_reason_from_outcome(outcome) {
        request_main_brain_runtime_recovery(state, supervisor_task_id, &reason);
    }
    Ok(())
}

async fn persist_gateway_visible_chat_turn(
    state: &AppState,
    session_id: &str,
    user_visible_request: Option<&str>,
    outcome: &ChatOutcome,
    status: &TaskStatus,
) -> Option<String> {
    let assistant_text = supervisor_user_visible_response_text(outcome, status)
        .replace(CREATION_PLANNING_DIALOGUE_MARKER, "")
        .trim()
        .to_string();
    persist_gateway_visible_chat_text(state, session_id, user_visible_request, &assistant_text)
        .await
}

async fn persist_gateway_visible_chat_text(
    state: &AppState,
    session_id: &str,
    user_visible_request: Option<&str>,
    assistant_text: &str,
) -> Option<String> {
    let Some(memory) = state.kernel.coordinator().memory.get() else {
        return Some("skipped_no_memory".to_string());
    };
    let assistant_text = assistant_text
        .replace(CREATION_PLANNING_DIALOGUE_MARKER, "")
        .trim()
        .to_string();
    if assistant_text.is_empty() {
        return Some("skipped_empty_response".to_string());
    }

    let mut session = match memory.retrieve_session(session_id).await {
        Ok(Some(session)) => session,
        Ok(None) => benshu_brain::agent::session::AgentSession::new(session_id.to_string()),
        Err(error) => {
            warn!(
                "Gateway visible chat turn persistence skipped: failed to load session {}: {}",
                session_id, error
            );
            return Some("failed_load_session".to_string());
        }
    };

    let duplicate_recent_assistant = session.messages.iter().rev().take(8).any(|existing| {
        existing.role == benshu_brain::agent::message::Role::Assistant
            && existing.content.as_text().trim() == assistant_text.trim()
    });
    if duplicate_recent_assistant {
        return Some("visible_turn_already_persisted".to_string());
    }

    if let Some(user_text) = user_visible_request
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(strip_gateway_runtime_request_markers)
        .filter(|text| !text.trim().is_empty())
    {
        append_visible_message_if_new(
            &mut session.messages,
            benshu_brain::agent::message::Role::User,
            AgentMessage::user(user_text),
        );
    }
    append_visible_message_if_new(
        &mut session.messages,
        benshu_brain::agent::message::Role::Assistant,
        AgentMessage::assistant(assistant_text),
    );
    session.step = session.step.saturating_add(1).max(1);
    session.status = benshu_brain::agent::session::SessionStatus::Completed;
    session.updated_at = chrono::Utc::now();
    session
        .agent_role
        .get_or_insert_with(|| "benshu".to_string());

    match memory.store_session(session).await {
        Ok(()) => Some("visible_turn_persisted".to_string()),
        Err(error) => {
            warn!(
                "Gateway visible chat turn persistence failed for session {}: {}",
                session_id, error
            );
            Some("failed_store_session".to_string())
        }
    }
}

fn append_visible_message_if_new(
    messages: &mut Vec<AgentMessage>,
    role: benshu_brain::agent::message::Role,
    message: AgentMessage,
) {
    let new_text = message.content.as_text();
    let duplicate_tail = messages.last().is_some_and(|existing| {
        existing.role == role && existing.content.as_text().trim() == new_text.trim()
    });
    if !duplicate_tail {
        messages.push(message);
    }
}

fn visible_user_request_from_gateway_task(task: &str) -> Option<String> {
    writing_session_route::writing_command_from_task(task)
        .map(|command| command.user_request)
        .or_else(|| extract_gateway_task_label_line(task, "用户原话："))
        .or_else(|| extract_gateway_task_label_line(task, "用户最新要求："))
        .or_else(|| {
            task.find("USER REQUEST\n")
                .map(|idx| task[idx + "USER REQUEST\n".len()..].trim().to_string())
        })
        .map(strip_gateway_runtime_request_markers)
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

fn extract_gateway_task_label_line(task: &str, label: &str) -> Option<String> {
    let idx = task.find(label)?;
    let rest = &task[idx + label.len()..];
    rest.lines()
        .next()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
}

fn strip_gateway_runtime_request_markers(text: impl AsRef<str>) -> String {
    let text = text.as_ref();
    if let Some(idx) = text.find("USER REQUEST\n") {
        return text[idx + "USER REQUEST\n".len()..].trim().to_string();
    }
    text.replace("SESSION WORK TARGET", "").trim().to_string()
}

async fn record_tool_runtime_events(
    state: &AppState,
    session_id: &str,
    supervisor_task_id: Uuid,
    outcome: &ChatOutcome,
) -> anyhow::Result<Vec<RecordedRuntimeEvent>> {
    let run_id = outcome
        .runtime_task
        .as_ref()
        .and_then(|runtime_task| runtime_task.run_id)
        .or_else(|| outcome.run_trace.as_ref().map(|trace| trace.run_id));
    let trace_id = outcome
        .runtime_task
        .as_ref()
        .and_then(|runtime_task| runtime_task.trace_id)
        .or_else(|| outcome.run_trace.as_ref().map(|trace| trace.run_id));

    let mut recorded_events = Vec::new();
    for call in &outcome.tool_calls {
        let result = call.result.as_deref().unwrap_or_default();
        let status = tool_event_status(result);
        let receipt_id = call
            .receipt_id
            .as_deref()
            .and_then(|id| Uuid::parse_str(id).ok())
            .unwrap_or_else(Uuid::new_v4);
        let receipt = RuntimeReceipt {
            receipt_id,
            status: status.to_string(),
            started_at: None,
            finished_at: Some(chrono::Utc::now()),
            actor: call.caller_id.clone(),
            action: Some(call.name.clone()),
            input_fingerprint: call.args_fingerprint.clone(),
            output_fingerprint: call.result_fingerprint.clone(),
            output_preview: Some(preview_text(result, 500).to_string()),
            blocker: worker_result_blocker(result),
        };
        let mut event = RuntimeEventRecord::new(format!("tool.{status}"))
            .with_task(supervisor_task_id)
            .with_actor(
                call.caller_id
                    .clone()
                    .unwrap_or_else(|| "agent".to_string()),
            )
            .with_receipt(receipt)
            .with_payload(serde_json::json!({
                "tool": call.name.clone(),
                "duration_ms": call.duration_ms,
                "result_truncated": call.result_truncated,
                "result_original_chars": call.result_original_chars,
                "result_omitted_chars": call.result_omitted_chars,
            }));
        event.run_id = run_id;
        event.trace_id = trace_id;
        event.session_id = Some(session_id.to_string());
        event.thread_id = Some(session_id.to_string());
        event.scope = Some("tool_execution".to_string());
        event
            .metadata
            .insert("tool_name".to_string(), call.name.clone());
        if let Some(args_fingerprint) = &call.args_fingerprint {
            event
                .metadata
                .insert("args_fingerprint".to_string(), args_fingerprint.clone());
        }
        if let Some(result_fingerprint) = &call.result_fingerprint {
            event
                .metadata
                .insert("result_fingerprint".to_string(), result_fingerprint.clone());
        }
        let stored = state.kernel.state_runtime_event().append(event).await?;
        recorded_events.push(RecordedRuntimeEvent {
            event_id: stored.event_id,
            topic: stored.topic.clone(),
        });
        for (topic, payload) in semantic_runtime_events_for_tool(&call.name, result, status) {
            let mut semantic_event = RuntimeEventRecord::new(topic)
                .with_task(supervisor_task_id)
                .with_actor(
                    call.caller_id
                        .clone()
                        .unwrap_or_else(|| "agent".to_string()),
                )
                .with_payload(payload);
            semantic_event.run_id = run_id;
            semantic_event.trace_id = trace_id;
            semantic_event.session_id = Some(session_id.to_string());
            semantic_event.thread_id = Some(session_id.to_string());
            semantic_event.scope = Some("semantic_tool_effect".to_string());
            semantic_event.parent_event_id = Some(stored.event_id);
            semantic_event
                .metadata
                .insert("tool_name".to_string(), call.name.clone());
            let stored_semantic = state
                .kernel
                .state_runtime_event()
                .append(semantic_event)
                .await?;
            recorded_events.push(RecordedRuntimeEvent {
                event_id: stored_semantic.event_id,
                topic: stored_semantic.topic.clone(),
            });
        }
    }
    if let Some(task) = state
        .kernel
        .state_task()
        .load(&supervisor_task_id.to_string())
        .await?
    {
        let mut seen_checkpoint_effects = std::collections::HashSet::new();
        for checkpoint in &task.checkpoints {
            for (topic, payload) in semantic_runtime_events_for_checkpoint(checkpoint) {
                let summary_fingerprint = checkpoint
                    .summary
                    .as_deref()
                    .map(|summary| preview_text(summary, 180).to_string())
                    .unwrap_or_default();
                if !seen_checkpoint_effects.insert((topic.clone(), summary_fingerprint)) {
                    continue;
                }
                if recorded_events.iter().any(|event| event.topic == topic) {
                    continue;
                }
                let mut checkpoint_event = RuntimeEventRecord::new(topic)
                    .with_task(supervisor_task_id)
                    .with_actor("checkpoint".to_string())
                    .with_payload(payload);
                checkpoint_event.run_id = run_id;
                checkpoint_event.trace_id = trace_id;
                checkpoint_event.session_id = Some(session_id.to_string());
                checkpoint_event.thread_id = Some(session_id.to_string());
                checkpoint_event.scope = Some("delegated_checkpoint_effect".to_string());
                checkpoint_event
                    .metadata
                    .insert("checkpoint_label".to_string(), checkpoint.label.clone());
                let stored_checkpoint_event = state
                    .kernel
                    .state_runtime_event()
                    .append(checkpoint_event)
                    .await?;
                recorded_events.push(RecordedRuntimeEvent {
                    event_id: stored_checkpoint_event.event_id,
                    topic: stored_checkpoint_event.topic.clone(),
                });
            }
        }
    }
    Ok(recorded_events)
}

fn tool_event_status(result: &str) -> &'static str {
    if tool_result_content_is_runtime_error(result) {
        return "failed";
    }
    if producer_quality_blocker(result).is_some() {
        return "blocked";
    }
    match worker_result_status(result) {
        Some("blocked") => "blocked",
        Some("failed") | Some("error") => "failed",
        _ => "completed",
    }
}

fn build_task_verification(
    task: &TaskState,
    outcome: &ChatOutcome,
    recorded_events: &[RecordedRuntimeEvent],
) -> TaskVerification {
    let mut missing_events = Vec::new();
    let evidence_event_ids = recorded_events
        .iter()
        .map(|event| event.event_id)
        .collect::<Vec<_>>();
    if !outcome.tool_calls.is_empty()
        && recorded_events
            .iter()
            .filter(|event| topic_matches("tool.*", &event.topic))
            .count()
            < outcome.tool_calls.len()
    {
        missing_events.push("tool.*".to_string());
    }
    if let Some(contract) = &task.contract {
        for required in &contract.required_events {
            if !recorded_events
                .iter()
                .any(|event| topic_matches(required, &event.topic))
                && !task_artifacts_satisfy_required_event(task, required)
                && !task_checkpoints_satisfy_required_event(task, required)
            {
                missing_events.push(required.clone());
            }
        }
    }

    let mut warnings = task
        .contract
        .as_ref()
        .map(|contract| contract.lint_warnings.clone())
        .unwrap_or_default();
    if !outcome.tool_calls.is_empty() && recorded_events.is_empty() {
        warnings.push(
            "tool calls were observed but no durable runtime evidence was recorded".to_string(),
        );
    }

    let has_failed_or_blocked_tool = outcome.tool_calls.iter().any(|call| {
        matches!(
            tool_event_status(call.result.as_deref().unwrap_or_default()),
            "failed" | "blocked"
        )
    });
    let has_required_contract_events = task
        .contract
        .as_ref()
        .is_some_and(|contract| !contract.required_events.is_empty());
    let final_response_status = worker_result_status(&outcome.response);
    let final_response_failed_or_blocked =
        matches!(final_response_status, Some("failed" | "error" | "blocked"));
    let producer_quality_blocker = task
        .contract
        .as_ref()
        .filter(|contract| {
            contract
                .required_events
                .iter()
                .any(|event| event.starts_with("artifact."))
        })
        .and_then(|_| outcome_producer_quality_blocker(outcome));
    let target_units_blocker = (!writing_session_route::task_is_creation_contract_planning(task))
        .then(|| {
            task.contract
                .as_ref()
                .and_then(|contract| contract.intent.as_deref())
                .filter(|intent| !intent.contains(CREATION_PLANNING_DIALOGUE_MARKER))
                .filter(|intent| {
                    writing_session_route::task_allows_file_artifact_target_verification(intent)
                })
                .filter(|intent| intent_requests_file_artifact(intent))
                .and_then(|intent| DelegateTool::requested_text_target_chars(intent))
                .and_then(|target| outcome_requested_text_target_blocker(outcome, target))
        })
        .flatten();
    let recovered_tool_failure = has_failed_or_blocked_tool
        && has_required_contract_events
        && missing_events.is_empty()
        && !final_response_failed_or_blocked;
    if recovered_tool_failure {
        warnings.push(
            "earlier blocked or failed tool attempts were recovered by later completion evidence"
                .to_string(),
        );
    }
    let verdict = if producer_quality_blocker.is_some()
        || target_units_blocker.is_some()
        || final_response_failed_or_blocked
        || (has_failed_or_blocked_tool && !recovered_tool_failure)
    {
        TaskVerificationVerdict::Fail
    } else if !missing_events.is_empty() {
        TaskVerificationVerdict::Uncertain
    } else {
        TaskVerificationVerdict::Pass
    };
    let summary = match verdict {
        TaskVerificationVerdict::Pass => Some("completion evidence accepted".to_string()),
        TaskVerificationVerdict::Fail => {
            producer_quality_blocker
                .or(target_units_blocker)
                .or_else(|| {
                    Some("one or more tool results reported blocked or failed status".to_string())
                })
        }
        TaskVerificationVerdict::Uncertain => Some(format!(
            "missing required runtime evidence: {}",
            missing_events.join(", ")
        )),
        TaskVerificationVerdict::Skip => Some("verification skipped".to_string()),
        TaskVerificationVerdict::PendingReview => Some("verification pending review".to_string()),
    };

    TaskVerification {
        verdict,
        missing_events,
        evidence_event_ids,
        warnings,
        summary,
    }
}

fn outcome_requested_text_target_blocker(
    outcome: &ChatOutcome,
    target_units: usize,
) -> Option<String> {
    if target_units == 0 {
        return None;
    }
    if outcome_reports_requested_turn_completion(outcome) {
        return None;
    }
    let observed = outcome_reported_unit_count(outcome);
    let minimum = requested_text_completion_floor(target_units);
    match observed {
        Some(units) if units >= minimum => None,
        Some(units) => Some(format!(
            "artifact text scale is incomplete: reported {units} units, target {target_units}"
        )),
        None => Some(format!(
            "artifact text scale is unverified: requested target {target_units} units but no final unit count was reported"
        )),
    }
}

fn outcome_reports_requested_turn_completion(outcome: &ChatOutcome) -> bool {
    std::iter::once(outcome.response.as_str())
        .chain(
            outcome
                .tool_calls
                .iter()
                .filter_map(|call| call.result.as_deref()),
        )
        .any(text_reports_requested_turn_completion)
}

fn text_reports_requested_turn_completion(text: &str) -> bool {
    let compact = text
        .to_ascii_lowercase()
        .replace([' ', '\n', '\r', '\t'], "");
    compact.contains("completion_scope:requested_turn")
        || compact.contains("\"completion_scope\":\"requested_turn\"")
        || (compact.contains("turn_complete:true") && compact.contains("project_complete:false"))
        || (compact.contains("\"turn_complete\":true")
            && compact.contains("\"project_complete\":false"))
}

fn requested_text_completion_floor(target_units: usize) -> usize {
    target_units.saturating_mul(95).div_ceil(100)
}

fn outcome_reported_unit_count(outcome: &ChatOutcome) -> Option<usize> {
    outcome
        .tool_calls
        .iter()
        .filter_map(|call| call.result.as_deref())
        .chain(std::iter::once(outcome.response.as_str()))
        .filter_map(max_reported_unit_count_in_text)
        .max()
}

fn max_reported_unit_count_in_text(text: &str) -> Option<usize> {
    let mut values = Vec::new();
    if let Ok(value) = serde_json::from_str::<Value>(text) {
        collect_unit_counts_from_json(&value, &mut values);
    }
    collect_unit_counts_from_receipt_text(text, &mut values);
    values.into_iter().max()
}

fn collect_unit_counts_from_json(value: &Value, values: &mut Vec<usize>) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                if unit_count_key_is_relevant(key) {
                    if let Some(count) = json_value_to_unit_count(value) {
                        values.push(count);
                    }
                }
                collect_unit_counts_from_json(value, values);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_unit_counts_from_json(item, values);
            }
        }
        _ => {}
    }
}

fn unit_count_key_is_relevant(key: &str) -> bool {
    matches!(
        key,
        "unit_count"
            | "total_units"
            | "approved_units"
            | "completed_units"
            | "generated_units"
            | "final_unit_count"
            | "final_units"
            | "units"
            | "word_count"
            | "words"
            | "character_count"
            | "characters"
            | "char_count"
            | "chars"
            | "字数"
    )
}

fn json_value_to_unit_count(value: &Value) -> Option<usize> {
    value
        .as_u64()
        .and_then(|count| usize::try_from(count).ok())
        .or_else(|| {
            value
                .as_str()
                .and_then(|text| parse_loose_positive_integer(text))
        })
}

fn collect_unit_counts_from_receipt_text(text: &str, values: &mut Vec<usize>) {
    for line in text.lines() {
        let trimmed = line.trim();
        collect_unit_count_pair(trimmed.split_once(':'), values);
        collect_unit_count_pair(trimmed.split_once('='), values);
        for token in trimmed.split(|ch: char| ch.is_whitespace() || matches!(ch, ';' | ',')) {
            collect_unit_count_pair(token.split_once('='), values);
            collect_unit_count_pair(token.split_once(':'), values);
        }
    }
}

fn collect_unit_count_pair(pair: Option<(&str, &str)>, values: &mut Vec<usize>) {
    let Some((key, value)) = pair else {
        return;
    };
    let key = key
        .trim()
        .trim_start_matches('-')
        .trim()
        .trim_matches(|ch: char| matches!(ch, '"' | '\'' | '{' | '}' | '[' | ']'));
    if unit_count_key_is_relevant(key) {
        if let Some(count) = parse_loose_positive_integer(value) {
            values.push(count);
        }
    }
}

fn parse_loose_positive_integer(text: &str) -> Option<usize> {
    let digits = text
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .collect::<String>();
    if digits.is_empty() {
        None
    } else {
        digits.parse::<usize>().ok()
    }
}

fn outcome_producer_quality_blocker(outcome: &ChatOutcome) -> Option<String> {
    outcome
        .tool_calls
        .iter()
        .rev()
        .filter_map(|call| call.result.as_deref())
        .chain(std::iter::once(outcome.response.as_str()))
        .find_map(producer_quality_blocker)
}

fn producer_quality_blocker(content: &str) -> Option<String> {
    if content.trim().is_empty() {
        return None;
    }
    if let Ok(value) = serde_json::from_str::<Value>(content) {
        if json_value_reports_quality_blocker(&value) {
            return Some(
                "artifact producer quality contract requires revision before completion"
                    .to_string(),
            );
        }
    }

    let lowered = content.to_ascii_lowercase();
    let compact = lowered.replace([' ', '\n', '\r', '\t'], "");
    if lowered.contains("quality_contract: fail")
        || lowered.contains("quality_contract: failed")
        || lowered.contains("quality_contract: needs_revision")
        || lowered.contains("artifact.needs_revision")
        || lowered.contains("runtime_effect: artifact.needs_revision")
        || compact.contains("\"runtime_effect\":\"artifact.needs_revision\"")
        || lowered.contains("latest_draft_requires_revision_before_approval_or_export")
        || (lowered.contains("writing_policy") && compact.contains("\"passed\":false"))
        || (lowered.contains("writing_policy") && lowered.contains("passed: false"))
        || compact.contains("\"verdict\":\"needs_revision\"")
        || lowered.contains("verdict: needs_revision")
        || lowered.contains("requires revision before approval")
    {
        return Some(
            "artifact producer quality contract requires revision before completion".to_string(),
        );
    }
    None
}

fn json_value_reports_quality_blocker(value: &Value) -> bool {
    match value {
        Value::Object(object) => {
            if object
                .get("quality_contract")
                .is_some_and(json_scalar_is_quality_failure)
            {
                return true;
            }
            if object
                .get("writing_policy")
                .is_some_and(json_policy_reports_quality_blocker)
            {
                return true;
            }
            if object
                .get("artifact_policy")
                .is_some_and(json_policy_reports_quality_blocker)
            {
                return true;
            }
            if object.get("review").is_some_and(|value| {
                value
                    .get("verdict")
                    .is_some_and(json_scalar_is_quality_failure)
                    || json_value_reports_quality_blocker(value)
            }) {
                return true;
            }
            if object
                .get("verdict")
                .is_some_and(json_scalar_is_quality_failure)
            {
                return true;
            }
            if object.get("next_action").is_some_and(|value| {
                value
                    .as_str()
                    .is_some_and(|action| action.to_ascii_lowercase().starts_with("revise"))
            }) && (object.contains_key("review")
                || object.contains_key("writing_policy")
                || object.contains_key("artifact_policy"))
            {
                return true;
            }
            object.values().any(json_value_reports_quality_blocker)
        }
        Value::Array(items) => items.iter().any(json_value_reports_quality_blocker),
        _ => false,
    }
}

fn json_policy_reports_quality_blocker(value: &Value) -> bool {
    match value {
        Value::Object(object) => {
            if object
                .get("passed")
                .is_some_and(|value| matches!(value, Value::Bool(false)))
            {
                return true;
            }
            if object
                .get("pass")
                .is_some_and(|value| matches!(value, Value::Bool(false)))
            {
                return true;
            }
            if object
                .get("status")
                .is_some_and(json_scalar_is_quality_failure)
            {
                return true;
            }
            if object
                .get("verdict")
                .is_some_and(json_scalar_is_quality_failure)
            {
                return true;
            }
            object.values().any(json_policy_reports_quality_blocker)
        }
        Value::Array(items) => items.iter().any(json_policy_reports_quality_blocker),
        _ => json_scalar_is_quality_failure(value),
    }
}

fn json_scalar_is_quality_failure(value: &Value) -> bool {
    match value {
        Value::Bool(false) => true,
        Value::String(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "fail" | "failed" | "needs_revision" | "rejected"
        ),
        _ => false,
    }
}

fn semantic_runtime_events_for_tool(
    tool_name: &str,
    result: &str,
    status: &str,
) -> Vec<(String, serde_json::Value)> {
    if status != "completed" {
        return Vec::new();
    }
    let lowered = result.to_ascii_lowercase();
    let mut events = Vec::new();
    let artifact_completion_candidate = artifact_write_result_is_completion_candidate(result);
    for topic in runtime_effect_topics(result) {
        if topic == "artifact.written" && !artifact_completion_candidate {
            continue;
        }
        events.push((
            topic,
            serde_json::json!({
                "tool": tool_name,
                "summary": preview_text(result, 500),
            }),
        ));
    }
    if tool_name == "knowledge_import_url"
        || lowered.contains("executed_tool: knowledge_import_url")
        || lowered.contains("imported web knowledge into collection")
    {
        events.push((
            "knowledge.imported".to_string(),
            serde_json::json!({
                "tool": tool_name,
                "summary": preview_text(result, 500),
            }),
        ));
    }
    if artifact_completion_candidate
        && (tool_name == "write_file"
            || lowered.contains("executed_tool: write_file")
            || lowered.contains("executed_tool: pdf_build")
            || lowered.contains("successfully wrote")
            || lowered.contains("checkpointed"))
    {
        events.push((
            "artifact.written".to_string(),
            serde_json::json!({
                "tool": tool_name,
                "summary": preview_text(result, 500),
            }),
        ));
    }
    if lowered.contains("executed_tool: pdf_build")
        || lowered.contains(".pdf")
        || lowered.contains(" pdf ")
        || lowered.contains("pdf document")
    {
        events.push((
            "artifact.pdf".to_string(),
            serde_json::json!({
                "tool": tool_name,
                "summary": preview_text(result, 500),
            }),
        ));
    }
    if summary_has_artifact_format(result, "txt") {
        events.push((
            "artifact.txt".to_string(),
            serde_json::json!({
                "tool": tool_name,
                "summary": preview_text(result, 500),
            }),
        ));
    }
    if summary_has_artifact_format(result, "md") {
        events.push((
            "artifact.md".to_string(),
            serde_json::json!({
                "tool": tool_name,
                "summary": preview_text(result, 500),
            }),
        ));
    }
    if lowered.contains("quality_contract: pass") {
        events.push((
            "artifact.quality".to_string(),
            serde_json::json!({
                "tool": tool_name,
                "summary": preview_text(result, 500),
            }),
        ));
    }
    events.sort_by(|left, right| left.0.cmp(&right.0));
    events.dedup_by(|left, right| left.0 == right.0);
    events
}

fn artifact_write_result_is_completion_candidate(result: &str) -> bool {
    !artifact_write_result_is_process_artifact_only(result)
}

fn artifact_write_result_is_process_artifact_only(result: &str) -> bool {
    let lowered = result.to_ascii_lowercase();
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
    ];
    if process_path_markers
        .iter()
        .any(|marker| lowered.contains(marker))
    {
        return true;
    }

    let status_or_progress_note = compact.contains("statusreport")
        || lowered.contains("状态报告")
        || lowered.contains("progress report")
        || lowered.contains("task status");
    let reports_internal_progress = lowered.contains("completion_scope")
        || lowered.contains("initial stage")
        || lowered.contains("file discovery")
        || lowered.contains("路径验证")
        || lowered.contains("blockers:");
    status_or_progress_note && reports_internal_progress
}

fn runtime_effect_topics(result: &str) -> Vec<String> {
    let mut topics = Vec::new();
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(result) {
        collect_runtime_effect_topics_from_value(&value, &mut topics);
    }
    for line in result.lines() {
        let trimmed = line.trim();
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        let key = key.trim();
        if key != "runtime_effect" && key != "runtime_effects" {
            continue;
        }
        for topic in value.split([',', ' ', '\t']) {
            let topic = topic.trim();
            if runtime_effect_topic_is_valid(topic) && !topics.iter().any(|item| item == topic) {
                topics.push(topic.to_string());
            }
        }
    }
    topics
}

fn collect_runtime_effect_topics_from_value(value: &serde_json::Value, topics: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(object) => {
            for (key, value) in object {
                if key == "runtime_effect" || key == "runtime_effects" {
                    collect_runtime_effect_topic_values(value, topics);
                } else {
                    collect_runtime_effect_topics_from_value(value, topics);
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_runtime_effect_topics_from_value(item, topics);
            }
        }
        _ => {}
    }
}

fn collect_runtime_effect_topic_values(value: &serde_json::Value, topics: &mut Vec<String>) {
    match value {
        serde_json::Value::String(value) => {
            for topic in value.split([',', ' ', '\t']) {
                let topic = topic.trim();
                if runtime_effect_topic_is_valid(topic) && !topics.iter().any(|item| item == topic)
                {
                    topics.push(topic.to_string());
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_runtime_effect_topic_values(item, topics);
            }
        }
        _ => {}
    }
}

fn runtime_effect_topic_is_valid(topic: &str) -> bool {
    topic.contains('.')
        && topic
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
}

fn topic_matches(required: &str, observed: &str) -> bool {
    if required == "artifact.verified" && observed == "artifact.written" {
        return true;
    }
    if let Some(prefix) = required.strip_suffix(".*") {
        observed.starts_with(prefix)
    } else {
        required == observed
    }
}

fn supervisor_status_from_outcome(outcome: &ChatOutcome) -> Option<TaskStatus> {
    for result in outcome
        .tool_calls
        .iter()
        .rev()
        .filter_map(|call| call.result.as_deref())
    {
        if is_recoverable_provider_disconnect(result) {
            return Some(TaskStatus::Paused(chrono::Utc::now()));
        }
        if tool_result_content_is_runtime_error(result) {
            return Some(TaskStatus::Failed(
                worker_result_blocker(result).unwrap_or_else(|| {
                    benshu_compression::preview_text(result.trim(), 320).to_string()
                }),
            ));
        }
        if worker_result_status(result).is_some() {
            return task_status_from_worker_result(result);
        }
        if let Some(reason) = inferred_blocker_reason_without_status(result) {
            return Some(TaskStatus::Blocked { reason });
        }
    }
    if is_recoverable_provider_disconnect(&outcome.response) {
        return Some(TaskStatus::Paused(chrono::Utc::now()));
    }
    if let Some(reason) = creation_contract_quality_blocker_from_outcome(outcome) {
        return Some(TaskStatus::Blocked { reason });
    }
    None
}

fn creation_contract_quality_blocker_from_outcome(outcome: &ChatOutcome) -> Option<String> {
    let blocked = outcome
        .run_trace
        .as_ref()
        .and_then(|trace| {
            trace
                .metadata
                .get(writing_session_surface::CREATION_CONTRACT_QUALITY_BLOCKED_METADATA_KEY)
        })
        .is_some_and(|value| value == "true");
    writing_session_surface::creation_contract_quality_blocker_from_panel_response(
        &outcome.response,
        blocked,
    )
}

fn provider_disconnect_reason_from_outcome(outcome: &ChatOutcome) -> Option<String> {
    outcome
        .tool_calls
        .iter()
        .rev()
        .filter_map(|call| call.result.as_deref())
        .find(|result| is_recoverable_provider_disconnect(result))
        .or_else(|| {
            is_recoverable_provider_disconnect(&outcome.response)
                .then_some(outcome.response.as_str())
        })
        .map(provider_service_pause_reason)
}

fn task_status_from_worker_result(result: &str) -> Option<TaskStatus> {
    let status = worker_result_status(result)?;
    let blocker = worker_result_blocker(result);
    let lowered = result.to_ascii_lowercase();
    if matches!(status, "completed")
        && (lowered.contains("artifact.needs_revision")
            || lowered.contains("\"outcome_status\":\"needs_revision\"")
            || lowered.contains("\"outcome_status\": \"needs_revision\"")
            || lowered.contains("\"accepted\":false")
            || lowered.contains("\"accepted\": false"))
    {
        return Some(TaskStatus::Blocked {
            reason: blocker.unwrap_or_else(|| {
                "产物已保存为草稿，但质量门未通过，不能当作已批准结果继续使用。".to_string()
            }),
        });
    }
    match status {
        "paused" => Some(TaskStatus::Paused(chrono::Utc::now())),
        "blocked" => Some(TaskStatus::Blocked {
            reason: blocker.unwrap_or_else(|| {
                benshu_compression::preview_text(result.trim(), 320).to_string()
            }),
        }),
        "failed" | "error" => Some(TaskStatus::Failed(blocker.unwrap_or_else(|| {
            benshu_compression::preview_text(result.trim(), 320).to_string()
        }))),
        "completed" => None,
        _ => None,
    }
}

fn supervisor_user_visible_response_text(outcome: &ChatOutcome, status: &TaskStatus) -> String {
    let response = strip_tool_runtime_notice_sections(&outcome.response)
        .trim()
        .to_string();
    if matches!(status, TaskStatus::Paused(_)) {
        if response.trim().is_empty() {
            return "模型服务中断或本轮模型调用超时，当前任务已暂停在最近的 checkpoint。调整模型/上下文/输出预算后可以继续当前任务。"
                .to_string();
        }
        if is_recoverable_provider_disconnect(&response)
            && (response.starts_with("status:") || response.contains("llm_turn_timeout"))
        {
            return "本轮模型调用没有在单步保护时间内产出可交付内容，当前任务已暂停在最近的 checkpoint。你可以继续当前任务；系统会沿用已有上下文，而不是从头开始。"
                .to_string();
        }
    }
    if let Some(text) = writing_session_surface::naturalize_writing_response(&response) {
        return text;
    }
    if matches!(status, TaskStatus::Completed) {
        return response;
    }
    let reason = match status {
        TaskStatus::Blocked { reason } | TaskStatus::Failed(reason) => reason.trim(),
        _ => return response,
    };
    if reason.is_empty() || response.contains(reason) {
        return response;
    }

    let generic_blocker_response = response.is_empty()
        || (response.contains("外部阻塞") && response.chars().count() <= 80)
        || (response.to_ascii_lowercase().contains("blocked") && response.chars().count() <= 80);
    if generic_blocker_response {
        format!(
            "当前任务被阻塞：{reason}\n\n没有继续导入知识库或生成产物，因为缺少满足原始请求的可验证证据。"
        )
    } else {
        format!("{response}\n\n当前任务被阻塞：{reason}")
    }
}

fn worker_result_status(result: &str) -> Option<&str> {
    result.lines().find_map(|line| {
        let trimmed = line.trim();
        trimmed
            .strip_prefix("status:")
            .map(str::trim)
            .filter(|value| !value.is_empty())
    })
}

fn worker_result_blocker(result: &str) -> Option<String> {
    result.lines().find_map(|line| {
        let trimmed = line.trim();
        trimmed
            .strip_prefix("blockers:")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| benshu_compression::preview_text(value, 320).to_string())
    })
}

fn inferred_blocker_reason_without_status(result: &str) -> Option<String> {
    if worker_result_status(result).is_some() {
        return None;
    }
    let trimmed = result.trim();
    if trimmed.is_empty() {
        return None;
    }
    for marker in [
        "当前任务被阻塞：",
        "当前具体卡点：",
        "当前卡点：",
        "current blocker:",
        "blocked because:",
    ] {
        if let Some((_, tail)) = trimmed.split_once(marker) {
            let reason = tail
                .lines()
                .next()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .unwrap_or(tail.trim());
            return Some(benshu_compression::preview_text(reason, 320).to_string());
        }
    }

    let lowered = trimmed.to_ascii_lowercase();
    let looks_blocked = trimmed.contains("不能声明完成")
        || trimmed.contains("没有产生可验证")
        || trimmed.contains("缺少满足原始请求")
        || lowered.contains("cannot claim completion")
        || lowered.contains("no verifiable")
        || lowered.contains("missing required runtime evidence");
    looks_blocked.then(|| benshu_compression::preview_text(trimmed, 320).to_string())
}

fn tool_result_content_is_runtime_error(content: &str) -> bool {
    let lowered = content.to_ascii_lowercase();
    lowered.trim_start().starts_with("error:")
        || lowered.contains("error executing tool")
        || lowered.contains("runtime tool error")
        || lowered.contains("tool execution error")
        || lowered.contains("tool not found")
        || lowered.contains("execution timed out before a usable result")
        || lowered.contains("provider error")
        || lowered.contains("http error")
        || lowered.contains("error sending request")
}

fn supervisor_provider_disconnect_checkpoint(reason: &str) -> TaskCheckpoint {
    TaskCheckpoint {
        step: 0,
        label: "foreground_chat:paused:provider_service".to_string(),
        recorded_at: chrono::Utc::now(),
        summary: Some(reason.to_string()),
    }
}

fn request_main_brain_runtime_recovery(state: &AppState, supervisor_task_id: Uuid, reason: &str) {
    let state = state.clone();
    let reason = reason.to_string();
    tokio::spawn(async move {
        warn!(
            target: "benshu::runtime_host_control",
            supervisor_task_id = %supervisor_task_id,
            reason = %reason,
            "Provider health issue detected; evaluating main-brain runtime host recovery."
        );
        let should_restart = provider_health_issue_should_restart_runtime_host(&reason);
        let restarted = if should_restart {
            crate::api::handlers::system::restart_main_brain_runtime_host_if_configured(&state)
                .await
        } else {
            false
        };
        if let Ok(Some(mut task)) = state
            .kernel
            .state_task()
            .load(&supervisor_task_id.to_string())
            .await
        {
            task.updated_at = chrono::Utc::now();
            task.checkpoints.push(TaskCheckpoint {
                step: task.current_step,
                label: if restarted {
                    "runtime_host:restart:requested"
                } else if !should_restart {
                    "runtime_host:restart:skipped"
                } else {
                    "runtime_host:restart:unavailable"
                }
                .to_string(),
                recorded_at: task.updated_at,
                summary: Some(if restarted {
                    "Main-brain runtime host restart command completed after provider disconnect."
                        .to_string()
                } else if !should_restart {
                    "Provider stream/turn timed out, but the model service still appears recoverable; runtime host restart was skipped to avoid interrupting other in-flight work."
                        .to_string()
                } else {
                    "Provider disconnect was detected, but no configured runtime host restart completed."
                        .to_string()
                }),
            });
            let _ = state.kernel.state_task().save(task).await;
        }
        watch_provider_recovery_and_resume_durable_task(state, supervisor_task_id).await;
    });
}

fn task_latest_pause_was_provider_disconnect(task: &TaskState) -> bool {
    task.checkpoints
        .iter()
        .rev()
        .find(|checkpoint| {
            checkpoint.label == "paused_by_user"
                || checkpoint.label == "foreground_chat:paused:provider_service"
        })
        .is_some_and(|checkpoint| checkpoint.label == "foreground_chat:paused:provider_service")
}

fn main_brain_provider_health_url(state: &AppState) -> Option<String> {
    let base_url = state
        .app_config
        .read()
        .agents
        .get("benshu")?
        .base_url
        .clone()?
        .trim()
        .trim_end_matches('/')
        .to_string();
    let base_url = base_url.as_str();
    if base_url.is_empty() {
        return None;
    }
    Some(if base_url.ends_with("/v1") {
        format!("{base_url}/models")
    } else {
        format!("{base_url}/v1/models")
    })
}

async fn watch_provider_recovery_and_resume_durable_task(
    state: AppState,
    supervisor_task_id: Uuid,
) {
    let Some(health_url) = main_brain_provider_health_url(&state) else {
        return;
    };
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
    {
        Ok(client) => client,
        Err(_) => return,
    };
    // A provider restart can take minutes for a large local model. This watcher
    // is bounded and cold (5-30s backoff); it does not create a hot retry loop.
    for attempt in 0..24u64 {
        let task = match state
            .kernel
            .state_task()
            .load(&supervisor_task_id.to_string())
            .await
        {
            Ok(Some(task)) => task,
            _ => return,
        };
        if !matches!(task.status, TaskStatus::Paused(_))
            || !task_needs_durable_reschedule(&task)
            || !task_latest_pause_was_provider_disconnect(&task)
        {
            return;
        }
        if client
            .get(&health_url)
            .send()
            .await
            .is_ok_and(|response| response.status().is_success())
        {
            let Some(session_id) = task.thread_id.or(task.session_id) else {
                return;
            };
            match resume_durable_paused_supervisor_task(&state, &session_id, None).await {
                Ok(Some(_)) => {
                    tracing::info!(
                        target: "benshu::runtime_host_control",
                        supervisor_task_id = %supervisor_task_id,
                        "Provider health recovered; durable paused task was rescheduled automatically."
                    );
                }
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!(
                        target: "benshu::runtime_host_control",
                        supervisor_task_id = %supervisor_task_id,
                        error = %error.0,
                        "Provider recovered, but durable task rescheduling failed."
                    );
                }
            }
            return;
        }
        let delay = 5u64.saturating_add(attempt.saturating_mul(2)).min(30);
        tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
    }
}

pub(crate) fn resume_provider_paused_durable_tasks_after_gateway_restart(state: &AppState) {
    let state = state.clone();
    tokio::spawn(async move {
        let Ok(tasks) = state.kernel.state_task().list().await else {
            return;
        };
        for task in tasks {
            if matches!(task.status, TaskStatus::Paused(_))
                && task_needs_durable_reschedule(&task)
                && task_latest_pause_was_provider_disconnect(&task)
            {
                let state = state.clone();
                tokio::spawn(async move {
                    watch_provider_recovery_and_resume_durable_task(state, task.id).await;
                });
            }
        }
    });
}

async fn mark_supervisor_task_failed(
    state: &AppState,
    supervisor_task_id: Uuid,
    reason: &str,
) -> anyhow::Result<()> {
    let Some(mut task) = state
        .kernel
        .state_task()
        .load(&supervisor_task_id.to_string())
        .await?
    else {
        return Ok(());
    };
    if !supervisor_task_accepts_execution_error(&task.status) {
        return Ok(());
    }
    let is_creation_contract_task =
        writing_session_route::task_is_creation_contract_planning(&task);
    task.updated_at = chrono::Utc::now();
    if reason.contains("Task preempted by new input") || reason.contains("task preempted") {
        task.status = TaskStatus::Cancelled;
        let mut result = serde_json::json!({
            "cancelled": true,
            "cancelled_at": task.updated_at,
            "reason": "preempted_by_user_control",
        });
        if is_creation_contract_task {
            result["creation_contract"] = writing_session_surface::creation_contract_panel_payload(
                "cancelled",
                "写作合同草案任务已被用户控制指令取消。",
                false,
            );
        }
        task.result = Some(result);
        state.kernel.state_task().save(task).await?;
        return Ok(());
    }
    if is_recoverable_provider_disconnect(reason) {
        let pause_reason = provider_service_pause_reason(reason);
        task.status = TaskStatus::Paused(task.updated_at);
        task.checkpoints
            .push(supervisor_provider_disconnect_checkpoint(&pause_reason));
        let response_text = "模型服务中断，当前任务已暂停在最近的 checkpoint。系统会尝试重启模型服务；重启成功后可以继续当前任务。";
        let mut result = serde_json::json!({
            "error": reason,
            "recoverable": true,
            "paused": true,
            "reason": pause_reason,
            "response_text": response_text
        });
        if is_creation_contract_task {
            result["creation_contract"] = writing_session_surface::creation_contract_panel_payload(
                "paused",
                response_text,
                false,
            );
        }
        task.result = Some(result);
        state.kernel.state_task().save(task).await?;
        request_main_brain_runtime_recovery(state, supervisor_task_id, &pause_reason);
        return Ok(());
    }
    task.status = TaskStatus::Failed(reason.to_string());
    let mut result = serde_json::json!({ "error": reason });
    if is_creation_contract_task {
        result["creation_contract"] =
            writing_session_surface::creation_contract_panel_payload("failed", reason, false);
    }
    task.result = Some(result);
    state.kernel.state_task().save(task).await?;
    Ok(())
}

fn supervisor_task_accepts_execution_error(status: &TaskStatus) -> bool {
    !is_terminal_task_status(status) && !matches!(status, TaskStatus::Paused(_))
}

async fn persist_chat_runtime_mainline(
    state: &AppState,
    session_id: &str,
    supervisor_task_id: Uuid,
    outcome: &ChatOutcome,
) -> Option<String> {
    if outcome.runtime_task.is_none() && outcome.run_trace.is_none() {
        return Some("not_needed".to_string());
    }

    let permit = match state.runtime_persist_limiter.clone().acquire_owned().await {
        Ok(permit) => permit,
        Err(error) => {
            warn!(
                "Chat runtime mainline persistence skipped because limiter closed: {}",
                error
            );
            return Some("skipped_limiter_closed".to_string());
        }
    };

    let mut runtime_task = outcome.runtime_task.clone();
    if runtime_task
        .as_ref()
        .is_some_and(|task| task.id == supervisor_task_id)
    {
        runtime_task = None;
    }
    if let Some(task) = runtime_task.as_mut() {
        task.parent_task_id = Some(supervisor_task_id);
        task.root_task_id = Some(supervisor_task_id);
        task.session_id
            .get_or_insert_with(|| session_id.to_string());
        task.thread_id.get_or_insert_with(|| session_id.to_string());
    }
    let extra_tasks = runtime_task
        .as_ref()
        .and_then(|task| {
            outcome
                .delegation
                .as_ref()
                .map(|delegation| derive_delegation_child_task(task, delegation))
        })
        .into_iter()
        .collect();
    let mut run_trace = outcome.run_trace.clone();
    let result = state
        .kernel
        .persist_runtime_mainline(
            runtime_task,
            extra_tasks,
            run_trace.as_mut(),
            Some("runtime_main_path"),
        )
        .await;
    drop(permit);

    if let Err(error) = result {
        warn!("Chat runtime mainline persistence failed: {}", error);
        Some("failed".to_string())
    } else {
        Some("persisted".to_string())
    }
}

fn chat_response_from_outcome(
    outcome: ChatOutcome,
    task_id: Option<Uuid>,
    runtime_persistence_status: Option<String>,
    chat_route: &str,
) -> ChatResponse {
    let mut artifacts = chat_artifacts_from_outcome(&outcome);
    let naturalized_writing_response = writing_session_surface::naturalize_writing_exchange(
        &outcome.response,
        outcome
            .tool_calls
            .iter()
            .filter_map(|call| call.result.as_deref()),
    );
    let response_text = naturalized_writing_response.clone().unwrap_or_else(|| {
        strip_tool_runtime_notice_sections(&outcome.response)
            .replace(CREATION_PLANNING_DIALOGUE_MARKER, "")
            .trim()
            .to_string()
    });
    let hide_internal_tool_calls = naturalized_writing_response.is_some();
    let run_id = outcome
        .runtime_task
        .as_ref()
        .and_then(|task| task.run_id)
        .or_else(|| outcome.run_trace.as_ref().map(|trace| trace.run_id));
    let trace_id = outcome
        .runtime_task
        .as_ref()
        .and_then(|task| task.trace_id)
        .or_else(|| outcome.run_trace.as_ref().map(|trace| trace.run_id));
    let tool_surface_mode = outcome
        .run_trace
        .as_ref()
        .and_then(|trace| trace.metadata.get("tool_surface_mode").cloned());
    if hide_internal_tool_calls {
        hide_local_artifact_paths_for_chat(&mut artifacts);
    }

    ChatResponse {
        response: response_text,
        reasoning: if hide_internal_tool_calls || outcome.thoughts.is_empty() {
            None
        } else {
            Some(outcome.thoughts.join("\n"))
        },
        chat_route: Some(chat_route.to_string()),
        tool_surface_mode,
        runtime_persistence_status,
        artifacts,
        tool_calls: if hide_internal_tool_calls || outcome.tool_calls.is_empty() {
            None
        } else {
            Some(
                outcome
                    .tool_calls
                    .into_iter()
                    .map(|t| ToolCallTrace {
                        name: t.name,
                        args: t.args,
                        result: sanitize_tool_trace_result(t.result),
                        backup: t.backup,
                    })
                    .collect(),
            )
        },
        task_id,
        run_id,
        trace_id,
    }
}

use std::collections::HashMap;

#[derive(Serialize)]
pub struct SessionDto {
    pub id: String,
    pub agent_role: String,
}

#[derive(Serialize)]
pub struct SessionTaskDto {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub status: String,
    pub status_detail: Option<String>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub thread_id: Option<String>,
    pub run_id: Option<Uuid>,
    pub trace_id: Option<Uuid>,
    pub witness_id: Option<Uuid>,
    pub parent_task_id: Option<Uuid>,
    pub root_task_id: Option<Uuid>,
    pub delegation_request_id: Option<String>,
    pub delegation_state: Option<String>,
    pub delegated_by: Option<String>,
    pub delegated_to: Option<String>,
    pub delegation_return_mode: Option<String>,
    pub artifacts: Vec<benshu_state::task::TaskArtifactRef>,
    pub checkpoints: Vec<benshu_state::task::TaskCheckpoint>,
    pub contract: Option<TaskContract>,
    pub verification: Option<TaskVerification>,
    pub evidence: HashMap<String, Value>,
}

#[derive(Deserialize)]
pub struct TaskWaitRequest {
    pub max_wait_seconds: Option<u64>,
    pub return_on_progress: Option<bool>,
}

#[derive(Deserialize)]
pub struct TaskOutputQuery {
    pub tail_lines: Option<usize>,
}

#[derive(Serialize)]
pub struct TaskStatusDto {
    pub task: SessionTaskDto,
    pub result: Option<Value>,
}

#[derive(Serialize)]
pub struct TaskWaitDto {
    pub reason: String,
    pub task: SessionTaskDto,
    pub result: Option<Value>,
}

#[derive(Serialize)]
pub struct TaskArtifactPreviewDto {
    pub artifact_id: String,
    pub kind: String,
    pub uri: String,
    pub media_type: Option<String>,
    pub preview: Option<String>,
    pub truncated: bool,
}

#[derive(Serialize)]
pub struct TaskOutputDto {
    pub task: SessionTaskDto,
    pub result: Option<Value>,
    pub artifact_previews: Vec<TaskArtifactPreviewDto>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DelegationInboxDto {
    pub message_id: String,
    pub source: String,
    pub target: String,
    pub kind: String,
    pub request_id: Option<String>,
    pub session_id: Option<String>,
    pub trace_id: Option<String>,
    pub task_id: Option<String>,
    pub parent_task_id: Option<String>,
    pub root_task_id: Option<String>,
    pub summary: String,
    pub visible_owner: Option<String>,
    pub memory_owner: Option<String>,
    pub approval_owner: Option<String>,
    pub delegated_by: Option<String>,
    pub delegated_to: Option<String>,
    pub final_response_owner: Option<String>,
    pub return_mode: Option<String>,
    pub delegation_state: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionDelegationTraceDto {
    pub session_id: String,
    pub active_role: String,
    pub runtime_profile: Option<String>,
    pub owner_rollup: Option<Value>,
    pub inbox: Vec<DelegationInboxDto>,
}

fn session_delegation_trace_from_metadata(
    session_id: &str,
    active_role: String,
    runtime_profile: Option<String>,
    owner_rollup_raw: Option<String>,
    inbox_raw: Option<String>,
) -> SessionDelegationTraceDto {
    let inbox = inbox_raw
        .and_then(|raw| serde_json::from_str::<Vec<DelegationInboxDto>>(&raw).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|entry| entry.session_id.as_deref() == Some(session_id))
        .collect();

    let owner_rollup = owner_rollup_raw
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .filter(|payload| payload.get("session_id").and_then(|v| v.as_str()) == Some(session_id));

    SessionDelegationTraceDto {
        session_id: session_id.to_string(),
        active_role,
        runtime_profile,
        owner_rollup,
        inbox,
    }
}

fn task_status_parts(status: &TaskStatus) -> (String, Option<String>) {
    match status {
        TaskStatus::Pending => ("pending".to_string(), None),
        TaskStatus::Queued => ("queued".to_string(), None),
        TaskStatus::Running => ("running".to_string(), None),
        TaskStatus::Completed => ("completed".to_string(), None),
        TaskStatus::Cancelled => ("cancelled".to_string(), None),
        TaskStatus::Failed(error) => ("failed".to_string(), Some(error.clone())),
        TaskStatus::Paused(at) => ("paused".to_string(), Some(at.to_rfc3339())),
        TaskStatus::AwaitingApproval {
            approval_kind,
            summary,
        } => (
            "awaiting_approval".to_string(),
            Some(format!("{approval_kind}: {summary}")),
        ),
        TaskStatus::Blocked { reason } => ("blocked".to_string(), Some(reason.clone())),
        TaskStatus::Deferred { until, reason } => (
            "deferred".to_string(),
            Some(match reason {
                Some(reason) => format!("until {} ({})", until.to_rfc3339(), reason),
                None => format!("until {}", until.to_rfc3339()),
            }),
        ),
    }
}

fn to_session_task_dto(task: TaskState) -> SessionTaskDto {
    let (status, status_detail) = task_status_parts(&task.status);
    let mut artifacts = task.artifacts;
    hide_local_task_artifact_paths_for_panel(&mut artifacts);
    let mut checkpoints = task.checkpoints;
    sanitize_task_checkpoints_for_panel(&mut checkpoints);
    SessionTaskDto {
        id: task.id,
        name: task.name,
        description: sanitize_panel_task_text(&task.description),
        status,
        status_detail: status_detail.map(|value| sanitize_panel_task_text(&value)),
        updated_at: task.updated_at,
        thread_id: task.thread_id.map(|value| sanitize_panel_task_text(&value)),
        run_id: task.run_id,
        trace_id: task.trace_id,
        witness_id: task.witness_id,
        parent_task_id: task.parent_task_id,
        root_task_id: task.root_task_id,
        delegation_request_id: task
            .delegation_request_id
            .map(|value| sanitize_panel_task_text(&value)),
        delegation_state: task
            .delegation_state
            .map(|value| sanitize_panel_task_text(&value)),
        delegated_by: task
            .delegated_by
            .map(|value| sanitize_panel_task_text(&value)),
        delegated_to: task
            .delegated_to
            .map(|value| sanitize_panel_task_text(&value)),
        delegation_return_mode: task
            .delegation_return_mode
            .map(|value| sanitize_panel_task_text(&value)),
        artifacts,
        checkpoints,
        contract: task.contract.map(sanitize_task_contract_for_panel),
        verification: task.verification,
        evidence: sanitize_task_evidence_for_panel(task.evidence),
    }
}

async fn load_task_or_not_found(state: &AppState, id: &str) -> Result<TaskState, AppError> {
    state
        .kernel
        .state_task()
        .load(id)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError(anyhow::anyhow!("Task not found: {}", id)))
}

async fn refresh_creation_contract_task_from_session_draft(
    state: &AppState,
    task: TaskState,
) -> Result<TaskState, AppError> {
    if !writing_session_route::task_is_creation_contract_planning(&task) {
        return Ok(task);
    }
    if matches!(
        task.status,
        TaskStatus::Cancelled | TaskStatus::Failed(_) | TaskStatus::Paused(_)
    ) {
        return Ok(task);
    }
    let Some(session_id) = task.thread_id.clone().or_else(|| task.session_id.clone()) else {
        return Ok(task);
    };

    let draft = load_session_creation_draft(state, &session_id).await?;
    Ok(refresh_creation_contract_task_result(task, draft.as_ref()))
}

fn refresh_creation_contract_task_result(
    mut task: TaskState,
    draft: Option<
        &benshu_builtin_tools::tool::writing::creation_contract::SessionCreationDraftState,
    >,
) -> TaskState {
    let (lifecycle_status, response_text, provisional) = if let Some(draft) = draft {
        let mut display_draft = draft.clone();
        match &task.status {
            TaskStatus::Completed => {}
            TaskStatus::Blocked { .. } => display_draft.set_lifecycle_status(
                benshu_builtin_tools::tool::writing::creation_contract::CreationDraftLifecycleStatus::Blocked,
            ),
            _ => display_draft.set_lifecycle_status(
                benshu_builtin_tools::tool::writing::creation_contract::CreationDraftLifecycleStatus::DraftingContract,
            ),
        }
        let confirmable = matches!(&task.status, TaskStatus::Completed)
            && writing_session_surface::creation_contract_draft_is_confirmable(&display_draft);
        (
            writing_session_surface::creation_contract_panel_status_for_draft(&display_draft),
            writing_session_surface::stabilize_creation_contract_panel_response(&display_draft, ""),
            !confirmable,
        )
    } else {
        let lifecycle_status =
            writing_session_surface::creation_contract_lifecycle_status_for_task_status(
                &task.status,
            );
        let response_text = task
            .result
            .as_ref()
            .and_then(|value| value.get("response_text"))
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| task_status_label(&task.status));
        (lifecycle_status, response_text, true)
    };

    let mut result = task
        .result
        .take()
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    result.insert(
        "response_text".to_string(),
        serde_json::Value::String(response_text.clone()),
    );
    result.insert(
        "creation_contract".to_string(),
        writing_session_surface::creation_contract_panel_payload(
            lifecycle_status,
            response_text,
            provisional,
        ),
    );
    task.result = Some(serde_json::Value::Object(result));

    task
}

fn is_terminal_task_status(status: &TaskStatus) -> bool {
    matches!(
        status,
        TaskStatus::Completed
            | TaskStatus::Failed(_)
            | TaskStatus::Cancelled
            | TaskStatus::Blocked { .. }
    )
}

fn task_progress_signature(task: &TaskState) -> String {
    serde_json::json!({
        "status": task_status_parts(&task.status).0,
        "updated_at": task.updated_at,
        "current_step": task.current_step,
        "total_steps": task.total_steps,
        "checkpoint_count": task.checkpoints.len(),
        "artifact_count": task.artifacts.len(),
        "verification": task.verification.clone(),
        "has_result": task.result.is_some(),
    })
    .to_string()
}

async fn read_artifact_preview(uri: &str, tail_lines: usize) -> (Option<String>, bool) {
    let path = if let Some(path) = uri.strip_prefix("file://") {
        std::path::PathBuf::from(path)
    } else {
        std::path::PathBuf::from(uri)
    };
    let path = if path.is_absolute() {
        path
    } else {
        match std::env::current_dir() {
            Ok(cwd) => cwd.join(path),
            Err(_) => path,
        }
    };
    let Ok(metadata) = tokio::fs::metadata(&path).await else {
        return (None, false);
    };
    if !metadata.is_file() {
        return (None, false);
    }
    let truncated_by_size = metadata.len() as usize > TASK_OUTPUT_MAX_PREVIEW_BYTES;
    let content = match tokio::fs::read_to_string(&path).await {
        Ok(content) => content,
        Err(_) => return (None, false),
    };
    let lines = content.lines().collect::<Vec<_>>();
    let truncated_by_lines = lines.len() > tail_lines;
    let start = lines.len().saturating_sub(tail_lines);
    let preview = lines[start..].join("\n");
    let preview = if preview.len() > TASK_OUTPUT_MAX_PREVIEW_BYTES {
        benshu_compression::preview_text(&preview, TASK_OUTPUT_MAX_PREVIEW_BYTES)
    } else {
        preview
    };
    (
        Some(preview),
        truncated_by_size || truncated_by_lines || content.len() > TASK_OUTPUT_MAX_PREVIEW_BYTES,
    )
}

fn derive_delegation_child_task(parent: &TaskState, delegation: &DelegationRecord) -> TaskState {
    let delegated_to = delegation.delegated_to.name().to_string();
    let delegated_by = delegation.delegated_by.name().to_string();
    let mode = match delegation.mode {
        DelegationMode::InternalRecommendation => "internal_recommendation",
        DelegationMode::InternalAssignment => "internal_assignment",
        DelegationMode::SessionTransfer => "session_transfer",
    };
    let summary = delegation
        .summary
        .clone()
        .unwrap_or_else(|| format!("{delegated_by} delegated work to {delegated_to}"));
    let mut child = TaskState::new(
        format!("delegation::{delegated_to}"),
        summary.clone(),
        serde_json::json!({
            "delegated_by": delegated_by,
            "delegated_to": delegated_to,
            "mode": mode,
            "task_owner": delegation.task_owner.name(),
            "summary": delegation.summary,
        }),
        parent.agent_id.clone(),
    );
    child.status = match delegation.mode {
        DelegationMode::InternalAssignment => TaskStatus::Queued,
        DelegationMode::InternalRecommendation | DelegationMode::SessionTransfer => {
            TaskStatus::Completed
        }
    };
    child.session_id = parent.session_id.clone();
    child.thread_id = parent.thread_id.clone();
    child.run_id = parent.run_id;
    child.trace_id = parent.trace_id;
    child.parent_task_id = Some(parent.id);
    child.root_task_id = parent.root_task_id.or(Some(parent.id));
    child.delegation_request_id = Some(format!("delegation:{}", child.id));
    child.delegation_state = Some(match delegation.mode {
        DelegationMode::InternalRecommendation | DelegationMode::InternalAssignment => {
            "created".to_string()
        }
        DelegationMode::SessionTransfer => "transferred".to_string(),
    });
    child.delegated_by = Some(delegated_by.clone());
    child.delegated_to = Some(delegated_to.clone());
    child.delegation_return_mode = Some(match delegation.mode {
        DelegationMode::SessionTransfer => "session_transfer".to_string(),
        DelegationMode::InternalRecommendation | DelegationMode::InternalAssignment => {
            "return_to_owner".to_string()
        }
    });
    child.priority = parent.priority;
    child.result = Some(serde_json::json!({
        "delegated_to": delegation.delegated_to.name(),
        "mode": mode,
        "summary": summary,
        "delegation_state": child.delegation_state,
    }));
    child.tags = vec![
        "delegation".to_string(),
        "multi_agent".to_string(),
        mode.to_string(),
    ];
    child
}

pub async fn list_sessions(State(state): State<AppState>) -> Json<Vec<SessionDto>> {
    let mut session_map = HashMap::new();

    for (id, role) in state.kernel.coordinator().active_agents() {
        let display_name = role.name().to_string();
        session_map.insert(id, display_name);
    }

    if let Ok(stored_sessions) = state
        .kernel
        .search_engine()
        .engram_store()
        .kv()
        .list_sessions()
    {
        for (id, _data) in stored_sessions {
            session_map
                .entry(id)
                .or_insert_with(|| "benshu (Archived)".to_string());
        }
    }

    let sessions: Vec<SessionDto> = session_map
        .into_iter()
        .map(|(id, agent_role)| SessionDto { id, agent_role })
        .collect();

    Json(sessions)
}

pub async fn get_session_history(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<benshu_brain::agent::message::Message>>, AppError> {
    if let Some(mem) = state.kernel.coordinator().memory.get() {
        if let Ok(Some(session)) = mem.retrieve_session(&id).await {
            if !session.messages.is_empty() {
                return Ok(Json(session.messages));
            }
        }

        let msgs = mem.retrieve("user", Some(&id), 100).await;
        if !msgs.is_empty() {
            return Ok(Json(msgs));
        }
    }

    if let Ok(Some(data)) = state.kernel.search_engine().engram_store().get_session(&id) {
        if let Ok(messages) =
            serde_json::from_str::<Vec<benshu_brain::agent::message::Message>>(&data)
        {
            return Ok(Json(messages));
        }
    }

    Ok(Json(vec![]))
}

pub async fn get_session_delegation_trace(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<SessionDelegationTraceDto>, AppError> {
    let active_role = state
        .kernel
        .coordinator()
        .active_agents()
        .into_iter()
        .find_map(|(session_id, role)| (session_id == id).then(|| role))
        .unwrap_or_else(|| state.kernel.coordinator().primary_role());

    let Some(memory) = state.kernel.coordinator().memory.get() else {
        return Ok(Json(session_delegation_trace_from_metadata(
            &id,
            active_role.name().to_string(),
            None,
            None,
            None,
        )));
    };

    let role_name = active_role.name().to_string();
    let prefix = format!("brain.comm.{}", role_name);
    let inbox_key = format!("{prefix}.inbox.recent_json");
    let owner_rollup_key = format!("{prefix}.owner_rollup.last_json");
    let runtime_profile_key = format!("{prefix}.runtime_profile");

    let inbox_raw = memory
        .get_metadata(&inbox_key)
        .await
        .map_err(AppError::from)?;
    let owner_rollup_raw = memory
        .get_metadata(&owner_rollup_key)
        .await
        .map_err(AppError::from)?;
    let runtime_profile = memory
        .get_metadata(&runtime_profile_key)
        .await
        .map_err(AppError::from)?;

    Ok(Json(session_delegation_trace_from_metadata(
        &id,
        role_name,
        runtime_profile,
        owner_rollup_raw,
        inbox_raw,
    )))
}

pub async fn list_session_tasks(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<SessionTaskDto>>, AppError> {
    let tasks = state
        .kernel
        .state_task()
        .list_by_session(&id)
        .await
        .map_err(AppError::from)?;

    Ok(Json(tasks.into_iter().map(to_session_task_dto).collect()))
}

pub async fn get_task_status(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<TaskStatusDto>, AppError> {
    let task = load_task_or_not_found(&state, &id).await?;
    let task = refresh_creation_contract_task_from_session_draft(&state, task).await?;
    let result = sanitize_task_result_for_panel(task.result.clone());
    Ok(Json(TaskStatusDto {
        result,
        task: to_session_task_dto(task),
    }))
}

pub async fn wait_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<TaskWaitRequest>,
) -> Result<Json<TaskWaitDto>, AppError> {
    let max_wait = payload
        .max_wait_seconds
        .unwrap_or(TASK_WAIT_DEFAULT_SECONDS)
        .min(TASK_WAIT_MAX_SECONDS);
    let return_on_progress = payload.return_on_progress.unwrap_or(true);
    let Some(initial) = state.kernel.state_task().load(&id).await? else {
        return Err(AppError(anyhow::anyhow!("Task not found: {}", id)));
    };
    if is_terminal_task_status(&initial.status) || max_wait == 0 {
        let reason = if is_terminal_task_status(&initial.status) {
            "already_finished"
        } else {
            "timeout"
        };
        let initial = refresh_creation_contract_task_from_session_draft(&state, initial).await?;
        let result = sanitize_task_result_for_panel(initial.result.clone());
        return Ok(Json(TaskWaitDto {
            reason: reason.to_string(),
            result,
            task: to_session_task_dto(initial),
        }));
    }

    let mut last_signature = task_progress_signature(&initial);
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(max_wait);
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = tokio::time::sleep_until(deadline) => {
                let task = load_task_or_not_found(&state, &id).await?;
                let task = refresh_creation_contract_task_from_session_draft(&state, task).await?;
                let result = sanitize_task_result_for_panel(task.result.clone());
                return Ok(Json(TaskWaitDto {
                    reason: if is_terminal_task_status(&task.status) { "finished" } else { "timeout" }.to_string(),
                    result,
                    task: to_session_task_dto(task),
                }));
            }
            _ = interval.tick() => {
                let task = load_task_or_not_found(&state, &id).await?;
                let task = refresh_creation_contract_task_from_session_draft(&state, task).await?;
                if is_terminal_task_status(&task.status) {
                    let result = sanitize_task_result_for_panel(task.result.clone());
                    return Ok(Json(TaskWaitDto {
                        reason: "finished".to_string(),
                        result,
                        task: to_session_task_dto(task),
                    }));
                }
                let signature = task_progress_signature(&task);
                if return_on_progress && signature != last_signature {
                    let result = sanitize_task_result_for_panel(task.result.clone());
                    return Ok(Json(TaskWaitDto {
                        reason: "progress".to_string(),
                        result,
                        task: to_session_task_dto(task),
                    }));
                }
                last_signature = signature;
            }
        }
    }
}

pub async fn get_task_output(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<TaskOutputQuery>,
) -> Result<Json<TaskOutputDto>, AppError> {
    let task = load_task_or_not_found(&state, &id).await?;
    let task = refresh_creation_contract_task_from_session_draft(&state, task).await?;
    let tail_lines = query.tail_lines.unwrap_or(TASK_OUTPUT_DEFAULT_TAIL_LINES);
    let mut artifact_previews = Vec::new();
    for artifact in &task.artifacts {
        let (preview, truncated) = read_artifact_preview(&artifact.uri, tail_lines).await;
        artifact_previews.push(TaskArtifactPreviewDto {
            artifact_id: artifact.artifact_id.clone(),
            kind: artifact.kind.clone(),
            uri: if artifact_uri_looks_local(&artifact.uri) {
                format!("artifact:{}", artifact.artifact_id)
            } else {
                sanitize_panel_task_text(&artifact.uri)
            },
            media_type: artifact.media_type.clone(),
            preview: preview.as_deref().map(sanitize_panel_task_text),
            truncated,
        });
    }

    let result = sanitize_task_result_for_panel(task.result.clone());
    Ok(Json(TaskOutputDto {
        result,
        task: to_session_task_dto(task),
        artifact_previews,
    }))
}

pub async fn cancel_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let mut task = load_task_or_not_found(&state, &id).await?;
    if let Some(session_id) = task.session_id.clone() {
        state.kernel.coordinator().cancel_session(&session_id);
    }
    if !is_terminal_task_status(&task.status) {
        task.status = TaskStatus::Cancelled;
        task.updated_at = chrono::Utc::now();
        task.result = Some(serde_json::json!({
            "cancelled": true,
            "cancelled_at": task.updated_at,
        }));
        state.kernel.state_task().save(task).await?;
    }
    Ok(StatusCode::OK)
}

pub async fn get_run_trace(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<RunTrace>, (StatusCode, String)> {
    let trace_id = Uuid::parse_str(&id).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            format!("invalid trace id: {}", error),
        )
    })?;

    let Some(trace) = state.kernel.telemetry().get_run_trace(&trace_id) else {
        return Err((
            StatusCode::NOT_FOUND,
            format!("trace not found for id {}", trace_id),
        ));
    };

    Ok(Json(trace))
}

pub async fn get_run_replay(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<RunReplay>, (StatusCode, String)> {
    let trace_id = Uuid::parse_str(&id).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            format!("invalid trace id: {}", error),
        )
    })?;

    let Some(replay) = state.kernel.telemetry().get_run_replay(&trace_id) else {
        return Err((
            StatusCode::NOT_FOUND,
            format!("replay not found for trace id {}", trace_id),
        ));
    };

    Ok(Json(replay))
}

pub async fn get_witness_summary(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<WitnessSummary>, (StatusCode, String)> {
    let witness_id = Uuid::parse_str(&id).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            format!("invalid witness id: {}", error),
        )
    })?;

    let Some(witness) = state.kernel.telemetry().get_witness_summary(&witness_id) else {
        return Err((
            StatusCode::NOT_FOUND,
            format!("witness not found for id {}", witness_id),
        ));
    };

    Ok(Json(witness))
}

pub async fn get_witness_bundle(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<WitnessBundle>, (StatusCode, String)> {
    let witness_id = Uuid::parse_str(&id).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            format!("invalid witness id: {}", error),
        )
    })?;

    let Some(bundle) = state.kernel.telemetry().get_witness_bundle(&witness_id) else {
        return Err((
            StatusCode::NOT_FOUND,
            format!("witness bundle not found for id {}", witness_id),
        ));
    };

    Ok(Json(bundle))
}

pub async fn get_scorecard(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Scorecard>, (StatusCode, String)> {
    let Some(scorecard) = state.kernel.telemetry().get_scorecard(&id) else {
        return Err((
            StatusCode::NOT_FOUND,
            format!("scorecard not found for id {}", id),
        ));
    };

    Ok(Json(scorecard))
}

pub async fn list_scorecards(
    State(state): State<AppState>,
    Query(query): Query<ScorecardQuery>,
) -> Json<Vec<Scorecard>> {
    Json(state.kernel.telemetry().query_scorecards(&query))
}

pub async fn get_witness_log(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<WitnessLogEntry>, (StatusCode, String)> {
    let witness_id = Uuid::parse_str(&id).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            format!("invalid witness id: {}", error),
        )
    })?;

    let Some(entry) = state.kernel.telemetry().get_witness_log(&witness_id) else {
        return Err((
            StatusCode::NOT_FOUND,
            format!("witness log not found for id {}", witness_id),
        ));
    };

    Ok(Json(entry))
}

pub async fn query_witness_logs(
    State(state): State<AppState>,
    Query(query): Query<WitnessLogQuery>,
) -> Json<Vec<WitnessLogEntry>> {
    Json(state.kernel.telemetry().query_witness_logs(&query))
}

pub async fn get_run_profiler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ProfilerArtifact>, (StatusCode, String)> {
    let run_id = Uuid::parse_str(&id).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            format!("invalid run id: {}", error),
        )
    })?;

    let Some(artifact) = state.kernel.telemetry().get_run_profiler_artifact(&run_id) else {
        return Err((
            StatusCode::NOT_FOUND,
            format!("profiler artifact not found for run {}", run_id),
        ));
    };

    Ok(Json(artifact))
}

pub async fn query_profiler_artifacts(
    State(state): State<AppState>,
    Query(query): Query<ProfilerArtifactQuery>,
) -> Json<Vec<ProfilerArtifact>> {
    Json(state.kernel.telemetry().query_profiler_artifacts(&query))
}

pub async fn export_profiler_artifacts(
    State(state): State<AppState>,
    Query(query): Query<ProfilerArtifactQuery>,
) -> Json<ProfilerExport> {
    Json(state.kernel.telemetry().export_profiler_artifacts(&query))
}

pub async fn delete_session(State(state): State<AppState>, Path(id): Path<String>) -> StatusCode {
    tracing::info!("Manually clearing session: {}", id);

    if let Some(mem) = state.kernel.coordinator().memory.get() {
        if let Ok(Some(session)) = mem.retrieve_session(&id).await {
            state
                .kernel
                .skill_loader()
                .cleanup_session(&id, &session.hardened_skills)
                .await;
        }
    }

    let removed_from_coord = state.kernel.coordinator().remove_session(&id);

    if let Some(mem) = state.kernel.coordinator().memory.get() {
        let _ = mem.delete_session(&id).await;
    }

    let _ = state
        .kernel
        .search_engine()
        .engram_store()
        .kv()
        .delete_session(&id);

    if removed_from_coord {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::OK
    }
}

pub async fn cancel_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let cancelled_by_coord = state.kernel.coordinator().cancel_session(&id);
    let cancelled_task_ids = mark_running_session_tasks_cancelled(&state, &id).await?;
    if cancelled_by_coord || !cancelled_task_ids.is_empty() {
        tracing::info!("Cancelled active foreground task for session {}", id);
        Ok(StatusCode::OK)
    } else {
        Err(AppError(anyhow::anyhow!(
            "No active foreground session found for '{}'",
            id
        )))
    }
}

async fn mark_running_session_tasks_cancelled(
    state: &AppState,
    session_id: &str,
) -> Result<Vec<Uuid>, AppError> {
    let tasks = state
        .kernel
        .state_task()
        .list_by_session(session_id)
        .await?;
    let mut cancelled_task_ids = Vec::new();
    for mut task in tasks {
        if is_terminal_task_status(&task.status) {
            continue;
        }
        cancelled_task_ids.push(task.id);
        task.status = TaskStatus::Cancelled;
        task.updated_at = chrono::Utc::now();
        task.result = Some(serde_json::json!({
            "cancelled": true,
            "cancelled_at": task.updated_at,
        }));
        state.kernel.state_task().save(task).await?;
    }
    Ok(cancelled_task_ids)
}

#[cfg(test)]
mod tests {
    use super::{
        background_supervisor_progress_summary, build_task_verification, cancel_session,
        classify_foreground_control_intent, derive_delegation_child_task,
        extract_task_artifacts_from_tool_result, get_run_replay, get_witness_summary,
        list_session_tasks, looks_like_memory_maintenance_request,
        refresh_creation_contract_task_result, render_session_work_context,
        session_delegation_trace_from_metadata, supervisor_agent_event_checkpoint,
        supervisor_user_visible_response_text, task_status_from_worker_result, DelegationInboxDto,
        ForegroundControlIntent, RecordedRuntimeEvent, SessionWorkContext,
    };
    use crate::api::state::AppState;
    use async_trait::async_trait;
    use axum::{
        extract::{Path, State},
        http::StatusCode,
        Json,
    };
    use benshu_auth::{OAuthManager, VaultTokenStore};
    use benshu_brain::agent::agent_identity::AgentIdentity;
    use benshu_brain::agent::message::Message;
    use benshu_brain::agent::multi_agent::AgentRole;
    use benshu_brain::agent::multi_agent::{AgentMessage, MultiAgent};
    use benshu_brain::agent::protocol::{AgentEvent, AgentEventData, ChatOutcome, ToolCallData};
    use benshu_brain::config::AppConfig;
    use benshu_brain::error::{Error as BrainError, Result as BrainResult};
    use benshu_builtin_tools::tool::document_understand::DocumentUnderstandTool;
    use benshu_infra::bus::MessageBus;
    use benshu_infra::SafetyLevel;
    use benshu_kernel::{service::factory::AgentFactory, KernelBootstrapper};
    use benshu_protocol_core::{DelegationMode, DelegationRecord, TaskOwnership};
    use benshu_state::{
        ArtifactLifecycle, ArtifactRecord, ArtifactScope, TaskArtifactRef, TaskCheckpoint,
        TaskContract, TaskState, TaskStatus, TaskVerificationVerdict,
    };
    use benshu_telemetry::{RunTrace, RuntimeStage, RuntimeStageTrace, TraceStatus};
    use chrono::Utc;
    use serde_json::json;
    use std::collections::{HashMap, HashSet, VecDeque};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use tempfile::TempDir;
    use tokio::sync::{broadcast, mpsc};
    use uuid::Uuid;

    fn gateway_supervision_required_for_text(text: &str) -> bool {
        let messages = vec![benshu_brain::agent::message::Message::user(
            benshu_brain::agent::message::Content::text(text),
        )];
        super::chat_requires_supervised_execution(&messages, 1)
    }

    #[test]
    fn generic_contract_repair_control_message_uses_the_supervised_contract_route() {
        assert!(gateway_supervision_required_for_text(
            "继续让合同自动修复器处理当前剩余缺口，保留当前最佳候选，不修改题材、总字数和章节档位；通过前不写正文。"
        ));
    }

    #[test]
    fn memory_maintenance_route_requires_memory_to_be_the_action_target() {
        assert!(looks_like_memory_maintenance_request("请运行一次记忆维护"));
        assert!(looks_like_memory_maintenance_request("帮我整理记忆"));
        assert!(looks_like_memory_maintenance_request(
            "run memory consolidation"
        ));
        assert!(!looks_like_memory_maintenance_request(
            "写一部科幻小说：检修员维护城轨，财团用居民记忆换能源"
        ));
        assert!(!looks_like_memory_maintenance_request(
            "故事里的管理员维护一座保存记忆的档案馆"
        ));
    }

    #[test]
    fn foreground_control_distinguishes_runtime_pause_from_content_pause() {
        assert_eq!(
            classify_foreground_control_intent("暂停一下"),
            ForegroundControlIntent::Pause
        );
        assert_eq!(
            classify_foreground_control_intent("请暂停当前任务"),
            ForegroundControlIntent::Pause
        );
        assert_eq!(
            classify_foreground_control_intent("暂停当前后台生成任务，保留上下文和已保存产物"),
            ForegroundControlIntent::Pause
        );
        assert_eq!(
            classify_foreground_control_intent("继续"),
            ForegroundControlIntent::Resume
        );
        assert_eq!(
            classify_foreground_control_intent("恢复当前后台生成任务，沿用已有任务合同"),
            ForegroundControlIntent::Resume
        );
        assert_eq!(
            classify_foreground_control_intent("让主角在门前暂停一下脚步"),
            ForegroundControlIntent::Normal
        );
        assert_eq!(
            classify_foreground_control_intent("请把主题改成暂停恢复机制，并继续"),
            ForegroundControlIntent::Normal
        );
        assert_eq!(
            classify_foreground_control_intent(
                "继续写下一章并保存文件，聊天框只回复进度、字数、路径和简短摘要。"
            ),
            ForegroundControlIntent::Normal
        );
        assert_eq!(
            classify_foreground_control_intent("任务进度到哪了？"),
            ForegroundControlIntent::StatusQuery
        );
        assert_eq!(
            classify_foreground_control_intent(
                "任务进度。只告诉我已完成到第几章、能否继续下一章；不要展示JSON、内部路径或工具参数。"
            ),
            ForegroundControlIntent::StatusQuery
        );
    }

    #[test]
    fn active_session_task_response_blocks_duplicate_foreground_work() {
        let task = TaskState::new(
            "foreground_chat",
            "running creation contract",
            serde_json::json!({}),
            "benshu",
        );
        let response = super::active_session_task_interruption_response(&task);
        assert!(response.contains("不会用这条新消息打断它或开启重复任务"));
        assert!(!response.contains(&task.id.to_string()));
    }

    #[test]
    fn worker_blocked_result_maps_to_blocked_task_status() {
        let status = task_status_from_worker_result(
            "status: blocked\nworker: researcher\nblockers: source intent mismatch: requested free/no-cost collection but source evidence indicates recommendation ranking\nresult:\n...",
        )
        .expect("blocked task status");

        match status {
            TaskStatus::Blocked { reason } => {
                assert!(reason.contains("source intent mismatch"));
            }
            other => panic!("expected blocked status, got {other:?}"),
        }
    }

    #[test]
    fn blocked_supervisor_response_surfaces_concrete_reason() {
        let outcome =
            test_chat_outcome("委派执行遇到了外部阻塞，当前还不能继续完成这一步。", vec![]);
        let text = supervisor_user_visible_response_text(
            &outcome,
            &TaskStatus::Blocked {
                reason: "browser search returned an anti-bot challenge page".to_string(),
            },
        );

        assert!(text.contains("anti-bot challenge"));
        assert!(text.contains("没有继续导入知识库或生成产物"));
    }

    #[test]
    fn context_limit_blocker_maps_to_blocked_task_status() {
        let status = task_status_from_worker_result(
            "status: blocked\nerror_kind: context_limit_exceeded\nblockers: 上下文超过当前运行时窗口，系统没有静默裁剪。\nprompt_tokens: 130000\nconfigured_context_tokens: 128000\nrequested_output_tokens: 4096\noverflow_tokens: 6096",
        )
        .expect("blocked task status");

        match status {
            TaskStatus::Blocked { reason } => {
                assert!(reason.contains("上下文超过当前运行时窗口"));
            }
            other => panic!("expected blocked status, got {other:?}"),
        }
    }

    #[test]
    fn completed_worker_result_with_needs_revision_maps_to_blocked_task_status() {
        let status = task_status_from_worker_result(
            r#"status: completed
worker: writer
executed_tool: novel_studio
result: {"success":true,"runtime_effect":"artifact.needs_revision","accepted":false,"outcome_status":"needs_revision","artifact_path":"/tmp/chapter.md"}"#,
        )
        .expect("blocked task status");

        match status {
            TaskStatus::Blocked { reason } => {
                assert!(reason.contains("质量门未通过"), "{reason}");
            }
            other => panic!("expected blocked status, got {other:?}"),
        }
    }

    #[test]
    fn completed_novel_studio_result_is_naturalized_for_chat_surface() {
        let outcome = test_chat_outcome(
            "status: completed\nworker: writer\nexecuted_tool: novel_studio\nworkflow_driver: writing.longform_fiction\nproject_path: /tmp/novel\nexport_path: /tmp/novel/exports/current.txt\noutput_path: /tmp/novel/exports/current.txt\nformat: txt\nmedia_type: text/plain\nruntime_effects: artifact.written, artifact.txt\ncompletion_scope: requested_turn\nproject_complete: false\nturn_complete: true\nunit_count: 3341\ntotal_units: 3341\nchapters_completed: 1\nchapters_planned: 1\nresult: chapter 1 saved; path=/tmp/novel/chapters/0001.md; audit=passed",
            vec![],
        );

        let text = supervisor_user_visible_response_text(&outcome, &TaskStatus::Completed);

        assert!(text.contains("本轮写作已完成"), "{text}");
        assert!(text.contains("完成章节：1/1"), "{text}");
        assert!(text.contains("TXT：已生成"), "{text}");
        assert!(!text.contains("/tmp/novel"), "{text}");
        assert!(
            !text.contains("executed_tool: novel_studio"),
            "raw receipt should not leak to chat surface: {text}"
        );
    }

    #[test]
    fn latest_implicit_delegate_blocker_wins_over_earlier_phase_boundary() {
        let outcome = test_chat_outcome(
            "委派 worker 已返回中间结果，但还没有产生可验证的本地产物写入回执，所以这一步不能声明完成。\n\n当前具体卡点：检索只返回搜索摘要，缺少可入库素材。",
            vec![
                test_tool_call(
                    "delegate",
                    "status: blocked\nworker: writer\nblockers: this artifact owner worker lacks external acquisition tools",
                ),
                test_tool_call(
                    "delegate",
                    "委派 worker 已返回中间结果，但还没有产生可验证的本地产物写入回执，所以这一步不能声明完成。\n\n当前具体卡点：检索只返回搜索摘要，缺少可入库素材。",
                ),
            ],
        );

        let status = super::supervisor_status_from_outcome(&outcome).expect("blocked status");
        match status {
            TaskStatus::Blocked { reason } => {
                assert!(reason.contains("检索只返回搜索摘要"));
                assert!(!reason.contains("artifact owner worker"));
            }
            other => panic!("expected blocked status, got {other:?}"),
        }
    }

    #[test]
    fn creation_contract_quality_blocker_maps_to_blocked_task_status() {
        let mut outcome = test_chat_outcome(
            "合同草案还没有通过质量门，因此我没有把它作为可确认合同，也不会进入正文写作。\n\n需要继续修复的问题：小说合同缺少可解析的稳定角色锚点\n\n请继续要求我“修订合同草案”。",
            vec![],
        );
        let mut metadata = HashMap::new();
        metadata.insert(
            "creation_contract_quality_blocked".to_string(),
            "true".to_string(),
        );
        let trace = RunTrace {
            run_id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            agent_id: "benshu".to_string(),
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
            tools: Vec::new(),
            artifacts: Vec::new(),
            degradation_notes: Vec::new(),
            witness: None,
            metadata,
        };
        outcome.run_trace = Some(trace);

        let status = super::supervisor_status_from_outcome(&outcome).expect("blocked status");
        match status {
            TaskStatus::Blocked { reason } => {
                assert!(reason.contains("稳定角色锚点"));
            }
            other => panic!("expected blocked status, got {other:?}"),
        }
    }

    #[test]
    fn background_progress_summary_names_pending_evidence() {
        let mut task = TaskState::new(
            "foreground_chat_supervisor",
            "background task",
            json!({}),
            "benshu",
        );
        task.contract = Some(TaskContract {
            intent: Some("查找论文，存入知识库，并做成pdf".to_string()),
            response_language: Some("zh-CN".to_string()),
            artifact_language: Some("zh-CN".to_string()),
            decisions: vec![],
            boundaries: vec![],
            completion_criteria: vec![],
            required_events: vec!["knowledge.imported".to_string(), "artifact.pdf".to_string()],
            evidence_requirements: vec![],
            lint_warnings: vec![],
        });
        let summary = background_supervisor_progress_summary(
            &task,
            task.created_at + chrono::Duration::seconds(95),
            &[],
        );

        assert!(summary.contains("知识导入"));
        assert!(summary.contains("PDF"));
        assert!(summary.contains("knowledge.imported"));
        assert!(summary.contains("artifact.pdf"));
        assert!(summary.contains("1m35s"));
    }

    #[test]
    fn background_progress_summary_explains_missing_child_receipts() {
        let mut task = TaskState::new(
            "foreground_chat_supervisor",
            "background task",
            json!({}),
            "benshu",
        );
        task.contract = Some(TaskContract {
            intent: Some("执行一个需要工具和 worker 的长任务".to_string()),
            response_language: Some("zh-CN".to_string()),
            artifact_language: Some("zh-CN".to_string()),
            decisions: vec![],
            boundaries: vec![],
            completion_criteria: vec![],
            required_events: vec![],
            evidence_requirements: vec![],
            lint_warnings: vec![],
        });

        let summary = background_supervisor_progress_summary(
            &task,
            task.created_at + chrono::Duration::seconds(180),
            &[],
        );

        assert!(summary.contains("外层监督任务仍在运行"));
        assert!(summary.contains("checkpoint"));
        assert!(summary.contains("receipt"));
        assert!(!summary.contains("正在等待 worker 或工具返回结果"));
    }

    #[test]
    fn background_progress_summary_surfaces_latest_activity() {
        let mut task = TaskState::new(
            "foreground_chat_supervisor",
            "background task",
            json!({}),
            "benshu",
        );
        task.contract = Some(TaskContract {
            intent: Some("执行一个需要工具和 worker 的长任务".to_string()),
            response_language: Some("zh-CN".to_string()),
            artifact_language: Some("zh-CN".to_string()),
            decisions: vec![],
            boundaries: vec![],
            completion_criteria: vec![],
            required_events: vec![],
            evidence_requirements: vec![],
            lint_warnings: vec![],
        });
        task.checkpoints.push(benshu_state::TaskCheckpoint {
            step: 1,
            label: "agent:benshu:tool:delegate:start".to_string(),
            recorded_at: task.created_at + chrono::Duration::seconds(10),
            summary: Some("benshu.delegate started safety=Green input={...}".to_string()),
        });

        let summary = background_supervisor_progress_summary(
            &task,
            task.created_at + chrono::Duration::seconds(180),
            &[],
        );

        assert!(summary.contains("最近进度"));
        assert!(summary.contains("benshu.delegate started"));
    }

    #[test]
    fn background_progress_summary_hides_satisfied_runtime_evidence() {
        let mut task = TaskState::new(
            "foreground_chat_supervisor",
            "background task",
            json!({}),
            "benshu",
        );
        task.contract = Some(TaskContract {
            intent: Some("存入知识库并保存成文件".to_string()),
            response_language: Some("zh-CN".to_string()),
            artifact_language: Some("zh-CN".to_string()),
            decisions: vec![],
            boundaries: vec![],
            completion_criteria: vec![],
            required_events: vec![
                "knowledge.imported".to_string(),
                "artifact.written".to_string(),
            ],
            evidence_requirements: vec![],
            lint_warnings: vec![],
        });
        task.artifacts.push(benshu_state::TaskArtifactRef {
            artifact_id: "artifact-1".to_string(),
            kind: "continuous_output".to_string(),
            uri: "/tmp/output.txt".to_string(),
            media_type: Some("text/plain".to_string()),
        });

        let summary = background_supervisor_progress_summary(
            &task,
            task.created_at + chrono::Duration::seconds(120),
            &["knowledge.imported".to_string()],
        );

        assert!(summary.contains("最近进度") || summary.contains("仍在运行"));
        assert!(!summary.contains("knowledge.imported"));
        assert!(!summary.contains("artifact.written"));
    }

    #[test]
    fn background_progress_summary_uses_checkpoint_receipts() {
        let mut task = TaskState::new(
            "foreground_chat_supervisor",
            "background task",
            json!({}),
            "benshu",
        );
        task.contract = Some(TaskContract {
            intent: Some("存入知识库并保存成文件".to_string()),
            response_language: Some("zh-CN".to_string()),
            artifact_language: Some("zh-CN".to_string()),
            decisions: vec![],
            boundaries: vec![],
            completion_criteria: vec![],
            required_events: vec![
                "knowledge.imported".to_string(),
                "artifact.written".to_string(),
            ],
            evidence_requirements: vec![],
            lint_warnings: vec![],
        });
        task.checkpoints.push(benshu_state::TaskCheckpoint {
            step: 1,
            label: "worker:knowledge:fast_path:completed".to_string(),
            recorded_at: task.created_at + chrono::Duration::seconds(10),
            summary: Some(
                "Worker `knowledge` completed direct execution. Preview: status: completed\nworker: knowledge\nexecuted_tool: knowledge_import_url\nresult:\nruntime_effect: knowledge.imported"
                    .to_string(),
            ),
        });

        let summary = background_supervisor_progress_summary(
            &task,
            task.created_at + chrono::Duration::seconds(120),
            &[],
        );

        assert!(!summary.contains("knowledge.imported"));
        assert!(summary.contains("artifact.written"));
    }

    #[test]
    fn supervisor_agent_event_checkpoint_names_running_tool() {
        let event = AgentEvent {
            session_id: Some("session-a".to_string()),
            data: AgentEventData::ToolExecutionStart {
                tool: "browser_browse".to_string(),
                input: "{\"query\":\"搜索网页\"}".to_string(),
                safety: SafetyLevel::Green,
            },
        };

        let (label, summary) =
            supervisor_agent_event_checkpoint("benshu", &event).expect("checkpoint");

        assert_eq!(label, "agent:benshu:tool:browser_browse:start");
        assert!(summary.contains("benshu.browser_browse started"));
        assert!(summary.contains("搜索网页"));
    }

    fn test_tool_call(name: &str, result: &str) -> ToolCallData {
        ToolCallData {
            receipt_id: None,
            tool_call_id: None,
            name: name.to_string(),
            args: "{}".to_string(),
            result: Some(result.to_string()),
            backup: None,
            duration_ms: 1,
            timestamp: 0,
            caller_id: None,
            safety_level: SafetyLevel::Green,
            cpu_pressure: None,
            vram_pressure: None,
            result_truncated: false,
            result_original_chars: None,
            result_omitted_chars: None,
            args_fingerprint: None,
            result_fingerprint: None,
            outcome: None,
            replay: None,
        }
    }

    fn test_chat_outcome(response: &str, tool_calls: Vec<ToolCallData>) -> ChatOutcome {
        let owner = AgentRole::Custom("benshu".to_string());
        ChatOutcome {
            response: response.to_string(),
            thoughts: Vec::new(),
            tool_calls,
            metabolic_stats: None,
            ownership: TaskOwnership::direct(owner, None),
            delegation: None,
            handover: None,
            runtime_task: None,
            run_trace: None,
        }
    }

    #[test]
    fn lightweight_realtime_outcomes_defer_runtime_mainline_tail() {
        let outcome = test_chat_outcome(
            "北京市当前天气，阴，24°C。",
            vec![test_tool_call("weather_lookup", "{}")],
        );

        assert!(super::outcome_is_lightweight_realtime_lookup(&outcome));
    }

    #[test]
    fn delegated_or_non_realtime_outcomes_keep_synchronous_mainline_persistence() {
        let non_realtime =
            test_chat_outcome("已保存文件。", vec![test_tool_call("write_file", "saved")]);
        assert!(!super::outcome_is_lightweight_realtime_lookup(
            &non_realtime
        ));

        let mut delegated = test_chat_outcome(
            "已查到结果。",
            vec![test_tool_call("latest_info_lookup", "{}")],
        );
        delegated.delegation = Some(DelegationRecord {
            delegated_by: AgentRole::Custom("benshu".to_string()),
            delegated_to: AgentRole::Custom("researcher".to_string()),
            mode: DelegationMode::InternalAssignment,
            task_owner: AgentRole::Custom("researcher".to_string()),
            session_id: Some("session-a".to_string()),
            summary: Some("done".to_string()),
        });
        assert!(!super::outcome_is_lightweight_realtime_lookup(&delegated));
    }

    fn test_event(topic: &str) -> RecordedRuntimeEvent {
        RecordedRuntimeEvent {
            event_id: Uuid::new_v4(),
            topic: topic.to_string(),
        }
    }

    #[test]
    fn task_verification_passes_when_later_required_evidence_recovers_blocked_tool() {
        let mut task = TaskState::new(
            "foreground_chat",
            "source-derived artifact",
            json!({}),
            "benshu",
        );
        task.contract = Some(TaskContract {
            required_events: vec![
                "knowledge.imported".to_string(),
                "artifact.written".to_string(),
                "artifact.pdf".to_string(),
            ],
            ..Default::default()
        });
        let outcome = test_chat_outcome(
            "status: completed\nworker: coder\nexecuted_tool: pdf_build",
            vec![
                test_tool_call(
                    "delegate",
                    "status: blocked\nworker: browser\nblockers: search failed",
                ),
                test_tool_call(
                    "delegate",
                    "status: completed\nworker: coder\nexecuted_tool: pdf_build",
                ),
            ],
        );
        let events = vec![
            test_event("tool.call"),
            test_event("tool.call"),
            test_event("knowledge.imported"),
            test_event("artifact.written"),
            test_event("artifact.pdf"),
        ];

        let verification = build_task_verification(&task, &outcome, &events);

        assert_eq!(verification.verdict, TaskVerificationVerdict::Pass);
        assert!(verification.warnings.iter().any(|warning| {
            warning.contains("earlier blocked or failed tool attempts were recovered")
        }));
    }

    #[test]
    fn task_verification_still_fails_when_final_response_is_blocked() {
        let mut task = TaskState::new("foreground_chat", "blocked task", json!({}), "benshu");
        task.contract = Some(TaskContract {
            required_events: vec!["knowledge.imported".to_string()],
            ..Default::default()
        });
        let outcome = test_chat_outcome(
            "status: blocked\nworker: researcher\nblockers: no usable evidence",
            vec![test_tool_call(
                "delegate",
                "status: blocked\nworker: researcher\nblockers: no usable evidence",
            )],
        );
        let events = vec![test_event("tool.call"), test_event("knowledge.imported")];

        let verification = build_task_verification(&task, &outcome, &events);

        assert_eq!(verification.verdict, TaskVerificationVerdict::Fail);
    }

    #[test]
    fn task_verification_pauses_provider_disconnect_without_status_line() {
        let task = TaskState::new("foreground_chat", "runtime error task", json!({}), "benshu");
        let outcome = test_chat_outcome(
            "工具执行失败，任务未完成。",
            vec![test_tool_call(
                "delegate",
                "Error executing tool 'delegate': Tool execution error: delegate - Runtime tool error in `delegate`: Internal error: error sending request for url (http://127.0.0.1/v1/chat/completions)",
            )],
        );
        let events = vec![test_event("tool.failed")];

        let verification = build_task_verification(&task, &outcome, &events);

        assert_eq!(verification.verdict, TaskVerificationVerdict::Fail);
        assert!(matches!(
            super::supervisor_status_from_outcome(&outcome),
            Some(TaskStatus::Paused(_))
        ));
    }

    #[test]
    fn supervisor_pauses_direct_llm_turn_timeout_without_tool_call() {
        let outcome = test_chat_outcome(
            "status: blocked\nerror_kind: llm_turn_timeout\nblockers: 本轮模型调用在 240 秒内没有返回可执行工具调用或可交付内容。",
            vec![],
        );

        assert!(matches!(
            super::supervisor_status_from_outcome(&outcome),
            Some(TaskStatus::Paused(_))
        ));
        let response = super::supervisor_user_visible_response_text(
            &outcome,
            &TaskStatus::Paused(chrono::Utc::now()),
        );
        assert!(response.contains("当前任务已暂停"));
        assert!(!response.starts_with("status:"));
    }

    #[test]
    fn provider_recovery_never_overrides_a_later_explicit_user_pause() {
        let mut task = TaskState::new(
            "foreground_chat",
            "durable provider recovery",
            json!({}),
            "benshu",
        );
        task.status = TaskStatus::Paused(chrono::Utc::now());
        task.checkpoints
            .push(super::supervisor_provider_disconnect_checkpoint(
                "provider unavailable",
            ));
        assert!(super::task_latest_pause_was_provider_disconnect(&task));

        task.checkpoints.push(TaskCheckpoint {
            step: 0,
            label: "paused_by_user".to_string(),
            recorded_at: chrono::Utc::now(),
            summary: Some("explicit user pause".to_string()),
        });
        assert!(!super::task_latest_pause_was_provider_disconnect(&task));
    }

    #[test]
    fn supervisor_execution_error_only_finalizes_an_active_task() {
        assert!(super::supervisor_task_accepts_execution_error(
            &TaskStatus::Running
        ));
        assert!(!super::supervisor_task_accepts_execution_error(
            &TaskStatus::Paused(chrono::Utc::now())
        ));
        assert!(!super::supervisor_task_accepts_execution_error(
            &TaskStatus::Completed
        ));
        assert!(!super::supervisor_task_accepts_execution_error(
            &TaskStatus::Blocked {
                reason: "quality gate".to_string(),
            }
        ));
        assert!(!super::supervisor_task_accepts_execution_error(
            &TaskStatus::Failed("runtime failure".to_string())
        ));
        assert!(!super::supervisor_task_accepts_execution_error(
            &TaskStatus::Cancelled
        ));
    }

    #[test]
    fn chat_contract_infers_generic_durable_effect_requirements() {
        let messages = vec![benshu_brain::agent::message::Message::user(
            benshu_brain::agent::message::Content::text(
                "把已有的大语言模型推理优化论文资料放到数据库，并将现有研究论文文件导出为 PDF。",
            ),
        )];

        let contract = super::build_chat_task_contract(&messages);

        assert!(contract
            .required_events
            .iter()
            .any(|event| event == "knowledge.imported"));
        assert!(contract
            .required_events
            .iter()
            .any(|event| event == "artifact.written"));
        assert!(contract
            .required_events
            .iter()
            .any(|event| event == "artifact.pdf"));
        assert!(contract
            .completion_criteria
            .iter()
            .any(|criterion| criterion.contains("runtime effect receipts")));
    }

    #[test]
    fn chat_contract_records_user_language_once_at_intake() {
        let messages = vec![benshu_brain::agent::message::Message::user(
            benshu_brain::agent::message::Content::text("帮我写一个草根逆袭的玄幻小说。"),
        )];

        let contract = super::build_chat_task_contract(&messages);

        assert_eq!(contract.response_language.as_deref(), Some("zh-CN"));
        assert_eq!(contract.artifact_language.as_deref(), Some("zh-CN"));
        assert!(contract
            .decisions
            .iter()
            .any(|decision| decision.contains("Preserve the user's language")));
    }

    #[test]
    fn completion_gate_reads_unit_counts_from_worker_receipts() {
        let text = r#"status: completed
state: {"approved_units":4618,"chapter_unit_target":4000,"target_units":4000}
result: chapter 1 saved; path=/tmp/current.txt; unit_count=4618; total_units=4618; audit=passed"#;

        assert_eq!(super::max_reported_unit_count_in_text(text), Some(4618));
    }

    #[test]
    fn completion_gate_reads_chars_from_quality_metrics_receipts() {
        let text = r#"runtime_effect: artifact.quality
quality_metrics:
- chars: 3162
- citations: 3
- required_sections_present: 7"#;

        assert_eq!(super::max_reported_unit_count_in_text(text), Some(3162));
    }

    #[test]
    fn foreground_observation_is_short_for_durable_long_tasks() {
        let messages = vec![benshu_brain::agent::message::Message::user(
            benshu_brain::agent::message::Content::text("请写一部 50 万字小说并保存成 txt 文件。"),
        )];

        assert_eq!(
            super::chat_foreground_observation_seconds(&messages, 1),
            super::CHAT_FOREGROUND_SHORT_OBSERVATION_SECONDS
        );
    }

    #[test]
    fn foreground_observation_gives_plain_chat_a_completion_window() {
        let messages = vec![benshu_brain::agent::message::Message::user(
            benshu_brain::agent::message::Content::text("你好，介绍一下你能做什么。"),
        )];

        assert_eq!(
            super::chat_foreground_observation_seconds(&messages, 1),
            super::CHAT_FOREGROUND_SHORT_OBSERVATION_SECONDS
        );
    }

    #[test]
    fn plain_chat_does_not_require_supervised_gateway_task() {
        let messages = vec![benshu_brain::agent::message::Message::user(
            benshu_brain::agent::message::Content::text("什么是 git？用一句话解释。"),
        )];

        assert!(!super::chat_requires_supervised_execution(&messages, 1));
    }

    #[test]
    fn thin_creation_opening_stays_direct_until_user_adds_contract_detail() {
        let messages = vec![benshu_brain::agent::message::Message::user(
            benshu_brain::agent::message::Content::text("帮我写小说"),
        )];

        assert!(!super::chat_requires_supervised_execution(&messages, 1));
        assert_eq!(
            super::chat_foreground_observation_seconds(&messages, 1),
            super::CHAT_FOREGROUND_DEFAULT_OBSERVATION_SECONDS
        );
    }

    #[test]
    fn gateway_supervision_hardness_matrix_keeps_plain_chat_direct() {
        let cases = [
            "什么是 git？",
            "git 是什么？",
            "bash 是什么？",
            "解释一下 Docker 容器是什么。",
            "用一句话解释 CPU 和 GPU 的区别。",
            "为什么天空是蓝色的？",
            "帮我润色这句话：今天心情很好。",
            "你能做什么？",
            "给我讲一个简短的笑话。",
            "Rust 的 ownership 是什么？",
        ];

        for case in cases {
            assert!(
                !gateway_supervision_required_for_text(case),
                "plain chat should stay direct: {case}"
            );
        }
    }

    #[test]
    fn gateway_supervision_hardness_matrix_routes_tasks_to_supervisor() {
        let cases = [
            "今天北京天气怎么样？",
            "比特币现在多少钱？",
            "纳斯达克点数多少？",
            "今天最新时事新闻是什么？",
            "搜索 Rust release notes 并给来源。",
            "请读取 https://example.com 的页面标题。",
            "把这条偏好记住：回答要简洁。",
            "我上一条让你记住的那句话是什么？",
            "写一篇报告并保存成 txt 文件。",
            "把这个网页内容存到知识库。",
            "请总结我上传的 PDF。",
            "用 powershell 列出当前目录。",
            "帮我修一下这个 Rust 仓库里的 bug 并提交补丁。",
            "写一篇5000字文章。",
        ];

        for case in cases {
            assert!(
                gateway_supervision_required_for_text(case),
                "task-like chat should be supervised: {case}"
            );
        }
    }

    #[test]
    fn gateway_supervision_hardness_matrix_routes_media_or_batch_to_supervisor() {
        let messages = vec![benshu_brain::agent::message::Message::user(
            benshu_brain::agent::message::Content::text("解释一下这张图。"),
        )];

        assert!(super::chat_requires_supervised_execution(&messages, 2));
    }

    #[test]
    fn realtime_lookup_requires_supervised_gateway_task() {
        let messages = vec![benshu_brain::agent::message::Message::user(
            benshu_brain::agent::message::Content::text("今天北京天气怎么样？"),
        )];

        assert!(super::chat_requires_supervised_execution(&messages, 1));
        assert_eq!(
            super::chat_foreground_observation_seconds(&messages, 1),
            super::CHAT_FOREGROUND_DEFAULT_OBSERVATION_SECONDS
        );
    }

    #[test]
    fn searching_for_existing_pdf_is_not_pdf_artifact_creation() {
        let messages = vec![benshu_brain::agent::message::Message::user(
            benshu_brain::agent::message::Content::text("搜索 bitcoin whitepaper pdf"),
        )];

        let contract = super::build_chat_task_contract(&messages);
        assert!(!contract
            .required_events
            .iter()
            .any(|event| event == "artifact.pdf"));
        assert_eq!(
            super::chat_foreground_observation_seconds(&messages, 1),
            super::CHAT_FOREGROUND_DEFAULT_OBSERVATION_SECONDS
        );
    }

    #[test]
    fn durable_artifact_request_requires_supervised_gateway_task() {
        let messages = vec![benshu_brain::agent::message::Message::user(
            benshu_brain::agent::message::Content::text("写一篇报告并保存成 txt 文件。"),
        )];

        assert!(super::chat_requires_supervised_execution(&messages, 1));
    }

    #[test]
    fn creation_planning_dialogue_is_not_treated_as_long_artifact_execution() {
        let messages = vec![benshu_brain::agent::message::Message::user(
            benshu_brain::agent::message::Content::text(format!(
                "{}\n只定大纲，不写正文。目标50万字，每章3000字，导出格式txt。",
                super::CREATION_PLANNING_DIALOGUE_MARKER
            )),
        )];

        assert_eq!(
            super::chat_foreground_observation_seconds(&messages, 1),
            super::CHAT_FOREGROUND_PLANNING_DIALOGUE_SECONDS
        );
        assert_eq!(
            super::CHAT_FOREGROUND_PLANNING_DIALOGUE_SECONDS,
            super::CHAT_FOREGROUND_SHORT_OBSERVATION_SECONDS
        );
        assert!(super::build_chat_task_contract(&messages)
            .required_events
            .is_empty());
        let contract = super::build_chat_task_contract(&messages);
        assert_eq!(
            contract.intent.as_deref(),
            Some("生成写作合同草案：只定大纲，不写正文。目标50万字，每章3000字，导出格式txt")
        );
        assert!(super::inferred_required_events_from_intent(contract.intent.as_deref()).is_empty());
        assert!(!contract
            .completion_criteria
            .iter()
            .any(|criterion| criterion.contains("explicit text scale")));

        let natural_messages = vec![benshu_brain::agent::message::Message::user(
            benshu_brain::agent::message::Content::text(
                "写一部5万字的短篇爱情小说，每章2500字。先和我多轮对话把框架定下来，情感要细腻，有完整结尾。",
            ),
        )];
        assert_eq!(
            super::chat_foreground_observation_seconds(&natural_messages, 1),
            super::CHAT_FOREGROUND_PLANNING_DIALOGUE_SECONDS
        );
        assert!(super::chat_requires_supervised_execution(
            &natural_messages,
            1
        ));
        assert!(super::creation_planning_dialogue_from_messages(
            &natural_messages
        ));
        assert!(super::build_chat_task_contract(&natural_messages)
            .required_events
            .is_empty());

        let confirm_first_messages = vec![benshu_brain::agent::message::Message::user(
            benshu_brain::agent::message::Content::text(
                "写异界修仙小说，每章2500字，至少5万字起。先给我完整创作合同，我确认后再开始写。",
            ),
        )];
        assert!(super::creation_planning_dialogue_from_messages(
            &confirm_first_messages
        ));
        let confirm_first_contract = super::build_chat_task_contract(&confirm_first_messages);
        assert!(confirm_first_contract.required_events.is_empty());
        assert!(!confirm_first_contract
            .completion_criteria
            .iter()
            .any(|criterion| criterion.contains("explicit text scale")));

        let direct_writer_messages = vec![benshu_brain::agent::message::Message::user(
            benshu_brain::agent::message::Content::text(
                super::writing_session_route::mark_direct_writer_continuation_task(
                    "用户已经确认开始写第一章。\n简述：先和我多轮对话定大纲，写5万字都市玄幻小说。",
                ),
            ),
        )];
        assert!(!super::creation_planning_dialogue_from_messages(
            &direct_writer_messages
        ));

        let existing_project_messages = vec![benshu_brain::agent::message::Message::user(
            benshu_brain::agent::message::Content::text(
                "SESSION WORK TARGET\n\
project_path: /home/user/benshu/data/generated/novels/demo\n\n\
USER REQUEST\n\
继续写第8章到第10章，保持现有合同、角色和世界观不变。",
            ),
        )];
        assert!(!super::creation_planning_dialogue_from_messages(
            &existing_project_messages
        ));
    }

    #[test]
    fn creation_contract_with_title_and_character_design_does_not_require_body_artifacts() {
        let intent = "生成写作合同草案：请从零创作一本赛博朋克长篇，先完成创作合同，合同确认后再开始写作；书名、角色姓名、世界观细节和主线结构由你设计";

        assert!(super::writing_session_route::intent_is_creation_contract_planning(intent));
        assert!(
            super::inferred_required_events_from_intent(Some(intent)).is_empty(),
            "creation planning must win over metadata-only wording"
        );
    }

    #[test]
    fn creation_contract_recovery_does_not_require_body_artifacts() {
        let intent = "生成写作合同草案：请继续使用现有自动修复流程补齐当前合同，直到通过合同质量门并给我可确认合同；不要开始写正文";

        assert!(super::intent_requests_existing_work_continuation(intent));
        assert!(super::writing_session_route::intent_is_creation_contract_planning(intent));
        assert!(
            super::inferred_required_events_from_intent(Some(intent)).is_empty(),
            "an explicit contract-planning intent remains planning during recovery"
        );
    }

    #[test]
    fn creation_contract_panel_response_is_explicitly_confirmable() {
        let draft =
            benshu_builtin_tools::tool::writing::creation_contract::build_initial_creation_draft(
                "session-a",
                "fiction",
                "先和我定大纲，写一部5万字异世界重生玄幻小说，每章2500字。",
            )
            .expect("draft");

        let response = benshu_builtin_tools::tool::writing::session_surface::stabilize_creation_contract_panel_response(
            &draft,
            "### 标准小说合同草案\n书名：灵潮归途\n主角：沈照\n结构：第01章《矿坑醒魂》：本章目标：建立起点。",
        );

        assert!(response.contains("还在补齐"));
        assert!(response.contains("还没有开始写正文"));
        assert!(response.contains("补充或修改"));
        assert!(response.contains("开始写"));
        assert!(response.contains("当前需求摘要（不可确认）"));
        assert!(response.contains("只有可展示合同通过质量门后"));

        let raw_patch_response = benshu_builtin_tools::tool::writing::session_surface::stabilize_creation_contract_panel_response(
            &draft,
            r#"{"patch_type":"character_patch","characters":[{"canonical_name":"内部候选"}]}"#,
        );
        assert!(!raw_patch_response.contains("patch_type"));
        assert!(!raw_patch_response.contains("character_patch"));
        assert!(!raw_patch_response.contains("canonical_name"));
    }

    #[test]
    fn creation_contract_draft_refresh_never_overwrites_task_status() {
        let draft =
            benshu_builtin_tools::tool::writing::creation_contract::build_initial_creation_draft(
                "session-a",
                "fiction",
                "写一部10万字都市小说，每章2500字。",
            )
            .expect("draft");

        for status in [TaskStatus::Running, TaskStatus::Completed] {
            let mut task = TaskState::new(
                "foreground_chat_supervisor",
                "creation contract planning",
                json!({}),
                "benshu",
            );
            task.status = status.clone();

            let refreshed = refresh_creation_contract_task_result(task, Some(&draft));

            assert_eq!(refreshed.status, status);
            assert!(refreshed
                .result
                .as_ref()
                .and_then(|value| value.get("creation_contract"))
                .is_some());
        }
    }

    #[test]
    fn running_creation_contract_task_never_exposes_intermediate_confirmable_draft() {
        let mut draft =
            benshu_builtin_tools::tool::writing::creation_contract::build_initial_creation_draft(
                "session-running-contract",
                "fiction",
                "写一部10万字都市小说，每章2500字。",
            )
            .expect("draft");
        draft.set_lifecycle_status(
            benshu_builtin_tools::tool::writing::creation_contract::CreationDraftLifecycleStatus::Approved,
        );
        let mut task = TaskState::new(
            "foreground_chat_supervisor",
            "creation contract planning",
            json!({}),
            "benshu",
        );
        task.status = TaskStatus::Running;

        let refreshed = refresh_creation_contract_task_result(task, Some(&draft));
        let result = refreshed.result.expect("result");
        let text = result
            .get("response_text")
            .and_then(serde_json::Value::as_str)
            .expect("response text");

        assert!(!text.starts_with("可确认合同"), "{text}");
        assert!(text.contains("不可确认"), "{text}");
        assert_eq!(
            result
                .pointer("/creation_contract/provisional")
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );
    }

    #[test]
    fn requested_turn_completion_does_not_require_full_project_target() {
        let result = "status: completed\ncompletion_scope: requested_turn\nproject_complete: false\nturn_complete: true\ntotal_units: 36785";

        assert!(super::text_reports_requested_turn_completion(result));
    }

    #[test]
    fn missing_chat_session_id_uses_isolated_ephemeral_session() {
        let (session_id, generated) = super::resolve_chat_session_id(None);

        assert!(generated);
        assert!(session_id.starts_with("ephemeral-chat-"));
        assert_ne!(session_id, "default-web-session");
    }

    #[test]
    fn blank_chat_session_id_uses_isolated_ephemeral_session() {
        let (session_id, generated) = super::resolve_chat_session_id(Some("  "));

        assert!(generated);
        assert!(session_id.starts_with("ephemeral-chat-"));
        assert_ne!(session_id, "default-web-session");
    }

    #[test]
    fn explicit_chat_session_id_is_preserved() {
        let (session_id, generated) = super::resolve_chat_session_id(Some(" session-a "));

        assert!(!generated);
        assert_eq!(session_id, "session-a");
    }

    #[test]
    fn fresh_artifact_request_does_not_attach_session_work_context() {
        assert!(!super::should_attach_session_work_context(
            "请写一章新的玄幻小说，情节完整，保存成 txt 文档。"
        ));
    }

    #[test]
    fn existing_artifact_follow_up_attaches_session_work_context() {
        assert!(super::should_attach_session_work_context(
            "总结一下刚才生成的第一章内容。"
        ));
        assert!(super::should_attach_session_work_context("继续写下一章。"));
    }

    #[test]
    fn session_work_context_renders_generic_continuation_packet() {
        let mut task = TaskState::new(
            "foreground_chat_supervisor",
            "Interactive chat run supervised beyond the foreground HTTP observation window",
            json!({}),
            "benshu",
        );
        task.id = Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap();
        task.status = TaskStatus::Running;
        task.session_id = Some("session-a".to_string());
        task.thread_id = Some("session-a".to_string());
        task.contract = Some(TaskContract {
            intent: Some("写一个长文档并保存为文件".to_string()),
            completion_criteria: vec!["artifact output exists and remains reusable".to_string()],
            ..Default::default()
        });
        task.current_step = 3;
        task.total_steps = Some(8);
        task.checkpoints.push(TaskCheckpoint {
            step: 3,
            label: "worker:writer:progress".to_string(),
            recorded_at: Utc::now(),
            summary: Some("saved section 3 and selected next_action=continue".to_string()),
        });
        task.artifacts.push(TaskArtifactRef {
            artifact_id: "artifact-1".to_string(),
            kind: "text".to_string(),
            uri: "data/generated/tasks/session-a/output.txt".to_string(),
            media_type: Some("text/plain".to_string()),
        });
        let artifact = ArtifactRecord {
            artifact_id: "artifact-1".to_string(),
            kind: "text".to_string(),
            uri: "data/generated/tasks/session-a/output.txt".to_string(),
            scope: ArtifactScope::Outputs,
            lifecycle: ArtifactLifecycle::Session,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            agent_id: "writer".to_string(),
            task_id: Some(task.id),
            run_id: None,
            trace_id: None,
            session_id: Some("session-a".to_string()),
            thread_id: Some("session-a".to_string()),
            tool_name: Some("writing_studio".to_string()),
            media_type: Some("text/plain".to_string()),
            virtual_path: Some("outputs/current.txt".to_string()),
            source_kind: "task_state".to_string(),
            metadata: HashMap::new(),
        };

        let rendered = render_session_work_context(
            &SessionWorkContext {
                tasks: vec![task],
                artifacts: vec![artifact],
            },
            "继续下一步",
        )
        .expect("context");

        assert!(rendered.contains("SESSION WORK CONTEXT"));
        assert!(rendered.contains("Use it only when the user asks to continue"));
        assert!(rendered.contains("continue the matching task/artifact"));
        assert!(rendered.contains("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"));
        assert!(rendered.contains("data/generated/tasks/session-a/output.txt"));
        assert!(rendered.contains("latest_checkpoints"));
        assert!(rendered.contains("work_refs"));
    }

    #[test]
    fn session_work_context_is_absent_without_tasks_or_artifacts() {
        assert!(render_session_work_context(
            &SessionWorkContext {
                tasks: Vec::new(),
                artifacts: Vec::new(),
            },
            "继续"
        )
        .is_none());
    }

    #[test]
    fn session_task_dto_hides_internal_paths_for_panel() {
        let mut task = TaskState::new(
            "foreground_chat_supervisor",
            "saved at /home/biubiuboy/BenShu/data/generated/reports/run/output.md",
            json!({
                "response_text": "已完成",
                "debug_path": "/home/biubiuboy/BenShu/data/generated/reports/run/state.json"
            }),
            "benshu",
        );
        task.status = TaskStatus::Completed;
        task.thread_id = Some("session-a".to_string());
        task.result = Some(json!({
            "response_text": "已完成",
            "debug_path": "/home/biubiuboy/BenShu/data/generated/reports/run/state.json"
        }));
        task.contract = Some(TaskContract {
            intent: Some(
                "output_path: /home/biubiuboy/BenShu/data/generated/reports/run".to_string(),
            ),
            ..Default::default()
        });
        task.checkpoints.push(TaskCheckpoint {
            step: 1,
            label: "artifact-completed".to_string(),
            recorded_at: Utc::now(),
            summary: Some(
                "path=/home/biubiuboy/BenShu/data/generated/reports/run/output.md; export_path=data/generated/reports/run/export.txt"
                    .to_string(),
            ),
        });
        task.artifacts.push(TaskArtifactRef {
            artifact_id: "artifact-1".to_string(),
            kind: "tool_output".to_string(),
            uri: "/home/biubiuboy/BenShu/data/generated/reports/run/output.md".to_string(),
            media_type: Some("text/markdown".to_string()),
        });

        let result = super::sanitize_task_result_for_panel(task.result.clone()).expect("result");
        let dto = super::to_session_task_dto(task);
        let rendered = serde_json::to_string(&json!({
            "task": dto,
            "result": result,
        }))
        .expect("json");

        assert!(!rendered.contains("/home/biubiuboy"), "{rendered}");
        assert!(!rendered.contains("data/generated"), "{rendered}");
        assert!(rendered.contains("artifact:artifact-1"), "{rendered}");
        assert!(rendered.contains("[internal path hidden]"), "{rendered}");
        assert!(rendered.contains("[artifact path hidden]"), "{rendered}");
    }

    #[test]
    fn creation_intake_defers_knowledge_ingest_and_artifact_events_until_confirmation() {
        let messages = vec![benshu_brain::agent::message::Message::user(
            benshu_brain::agent::message::Content::text(
                "搜索一个科幻星际类型小说，尝试入知识库，根据这个的基础来写小说50万字。",
            ),
        )];

        let contract = super::build_chat_task_contract(&messages);

        assert!(contract.required_events.is_empty(), "{contract:?}");
        assert!(!contract
            .completion_criteria
            .iter()
            .any(|criterion| criterion.contains("explicit text scale")));
    }

    #[test]
    fn colloquial_knowledge_ingest_is_deferred_during_creation_intake() {
        let messages = vec![benshu_brain::agent::message::Message::user(
            benshu_brain::agent::message::Content::text(
                "搜索一部公网可下载的热门玄幻小说，把可以读取到的正文或有效素材收进知识库，然后写一部小说并保存成txt。",
            ),
        )];

        let contract = super::build_chat_task_contract(&messages);

        assert!(contract.required_events.is_empty(), "{contract:?}");
    }

    #[test]
    fn confirmed_writer_task_requires_deferred_knowledge_ingest_and_artifact_events() {
        let task = super::writing_session_route::mark_direct_writer_continuation_task(
            "搜索一部公网可下载的热门玄幻小说，把可以读取到的正文或有效素材收进知识库，然后写一部小说并保存成txt。",
        );
        let messages = vec![benshu_brain::agent::message::Message::user(
            benshu_brain::agent::message::Content::text(task),
        )];

        let contract = super::build_chat_task_contract(&messages);

        assert!(contract
            .required_events
            .iter()
            .any(|event| event == "knowledge.imported"));
        assert!(contract
            .required_events
            .iter()
            .any(|event| event == "artifact.written"));
        assert!(contract
            .required_events
            .iter()
            .any(|event| event == "artifact.txt"));
    }

    #[test]
    fn chat_contract_treats_continuation_chapter_writing_as_artifact() {
        let intent = "继续写第二章，保持前文人物名字和世界观规则。";
        assert!(super::intent_requests_existing_work_continuation(intent));
        assert!(super::intent_requests_file_artifact(intent));
        assert!(
            !super::writing_session_route::intent_requests_read_only_existing_artifact_answer(
                intent
            )
        );
        let inferred = super::inferred_required_events_from_intent(Some(intent));
        assert!(
            inferred.iter().any(|event| event == "artifact.written"),
            "inferred events: {inferred:?}"
        );

        let messages = vec![benshu_brain::agent::message::Message::user(
            benshu_brain::agent::message::Content::text(intent),
        )];

        let contract = super::build_chat_task_contract(&messages);

        assert!(contract
            .required_events
            .iter()
            .any(|event| event == "artifact.written"));
        assert!(!contract
            .required_events
            .iter()
            .any(|event| event == "knowledge.imported"));
    }

    #[test]
    fn finish_existing_artifact_request_is_not_read_only() {
        let message = "继续这本《碎灵余烬》，如果还没有真正完整结尾，就从当前进度接着写到完整结尾。不要新开书，不要贴正文全文，聊天里只告诉我进度、章节号、字数、文件路径、简短摘要和审查状态。";

        assert!(
            !super::writing_session_route::intent_requests_read_only_existing_artifact_answer(
                message
            )
        );
        assert!(super::intent_requests_existing_work_continuation(message));
    }

    #[test]
    fn chat_contract_treats_existing_artifact_summary_as_read_only() {
        let messages = vec![benshu_brain::agent::message::Message::user(
            benshu_brain::agent::message::Content::text(
                "总结一下刚才生成的第一章内容，并告诉我主角是谁、保存的 txt 路径在哪里。",
            ),
        )];

        let contract = super::build_chat_task_contract(&messages);

        assert!(contract.required_events.is_empty());
    }

    #[test]
    fn chat_contract_treats_negated_continuation_followup_as_read_only() {
        let messages = vec![benshu_brain::agent::message::Message::user(
            benshu_brain::agent::message::Content::text(
                "总结一下刚才生成的第一章内容，并告诉我主角是谁。不要继续写新章节。",
            ),
        )];

        let contract = super::build_chat_task_contract(&messages);

        assert!(contract.required_events.is_empty());
    }

    #[test]
    fn artifact_write_receipt_satisfies_artifact_verification_contract() {
        let mut task = TaskState::new(
            "foreground_chat",
            "artifact verification task",
            json!({}),
            "benshu",
        );
        task.contract = Some(TaskContract {
            intent: Some("检查第三章是否已经保存进项目".to_string()),
            required_events: vec!["artifact.verified".to_string()],
            ..Default::default()
        });
        let outcome = test_chat_outcome("第三章已保存。", vec![]);
        let events = vec![test_event("artifact.written")];

        let verification = build_task_verification(&task, &outcome, &events);

        assert_eq!(verification.verdict, TaskVerificationVerdict::Pass);
    }

    #[test]
    fn persisted_artifact_path_satisfies_artifact_verification_contract() {
        let mut task = TaskState::new(
            "foreground_chat",
            "artifact verification task",
            json!({}),
            "benshu",
        );
        task.contract = Some(TaskContract {
            intent: Some("写一篇小说并保存成 txt 文档".to_string()),
            required_events: vec!["artifact.verified".to_string(), "artifact.txt".to_string()],
            ..Default::default()
        });
        task.artifacts.push(benshu_state::TaskArtifactRef {
            artifact_id: "artifact-1".to_string(),
            kind: "tool_output".to_string(),
            uri: "/tmp/story/final.txt".to_string(),
            media_type: Some("text/plain".to_string()),
        });
        let outcome = test_chat_outcome("已完成。", vec![]);
        let events = vec![];

        let verification = build_task_verification(&task, &outcome, &events);

        assert_eq!(verification.verdict, TaskVerificationVerdict::Pass);
    }

    #[test]
    fn plain_text_artifact_does_not_satisfy_markdown_event() {
        let mut task = TaskState::new("foreground_chat", "novel chapter task", json!({}), "benshu");
        task.contract = Some(TaskContract {
            intent: Some("按这个开始，写第一章。".to_string()),
            required_events: vec!["artifact.md".to_string()],
            ..Default::default()
        });
        task.artifacts.push(benshu_state::TaskArtifactRef {
            artifact_id: "artifact-1".to_string(),
            kind: "tool_output".to_string(),
            uri: "/tmp/novel/exports/current.txt".to_string(),
            media_type: Some("text/plain".to_string()),
        });
        let outcome = test_chat_outcome("已完成。", vec![]);
        let events = vec![];

        let verification = build_task_verification(&task, &outcome, &events);

        assert_eq!(verification.verdict, TaskVerificationVerdict::Uncertain);
        assert!(verification
            .missing_events
            .iter()
            .any(|event| event == "artifact.md"));
    }

    #[test]
    fn explicit_markdown_request_still_requires_markdown_artifact() {
        let mut task = TaskState::new(
            "foreground_chat",
            "markdown artifact task",
            json!({}),
            "benshu",
        );
        task.contract = Some(TaskContract {
            intent: Some("写第一章并保存成 Markdown 文件。".to_string()),
            required_events: vec!["artifact.md".to_string()],
            ..Default::default()
        });
        task.artifacts.push(benshu_state::TaskArtifactRef {
            artifact_id: "artifact-1".to_string(),
            kind: "tool_output".to_string(),
            uri: "/tmp/novel/exports/current.txt".to_string(),
            media_type: Some("text/plain".to_string()),
        });
        let outcome = test_chat_outcome("已完成。", vec![]);
        let events = vec![];

        let verification = build_task_verification(&task, &outcome, &events);

        assert_eq!(verification.verdict, TaskVerificationVerdict::Uncertain);
        assert!(verification
            .missing_events
            .iter()
            .any(|event| event == "artifact.md"));
    }

    #[test]
    fn chat_contract_requires_metadata_revision_write_and_verification() {
        let intent = "请修订第二章，补全摘要、关键事实和连续性更新。";
        assert!(super::intent_requests_existing_work_continuation(intent));
        assert!(super::intent_requests_file_artifact(intent));
        assert!(
            !super::writing_session_route::intent_requests_read_only_existing_artifact_answer(
                intent
            )
        );
        let inferred = super::inferred_required_events_from_intent(Some(intent));
        assert_eq!(inferred, vec!["artifact.written", "artifact.verified"]);

        let messages = vec![benshu_brain::agent::message::Message::user(
            benshu_brain::agent::message::Content::text(intent),
        )];

        let contract = super::build_chat_task_contract(&messages);

        assert_eq!(
            contract.required_events,
            vec!["artifact.written", "artifact.verified"]
        );
    }

    #[test]
    fn chat_contract_does_not_infer_knowledge_import_from_read_memory_guard() {
        let messages = vec![benshu_brain::agent::message::Message::user(
            benshu_brain::agent::message::Content::text(
                "继续完成已有小说项目的第三章，确保第三章保存进项目并更新连续性，不要只查询记忆后就结束。",
            ),
        )];

        let contract = super::build_chat_task_contract(&messages);

        assert!(contract
            .required_events
            .iter()
            .any(|event| event == "artifact.verified"));
        assert!(!contract
            .required_events
            .iter()
            .any(|event| event == "artifact.written"));
        assert!(!contract
            .required_events
            .iter()
            .any(|event| event == "knowledge.imported"));
    }

    #[test]
    fn task_verification_is_uncertain_when_requested_artifact_has_no_receipt() {
        let mut task = TaskState::new("foreground_chat", "artifact task", json!({}), "benshu");
        task.contract = Some(TaskContract {
            intent: Some("写一篇小说并保存成 txt 文档".to_string()),
            required_events: vec!["artifact.written".to_string()],
            ..Default::default()
        });
        let outcome = test_chat_outcome("已完成。", vec![]);
        let events = vec![];

        let verification = build_task_verification(&task, &outcome, &events);

        assert_eq!(verification.verdict, TaskVerificationVerdict::Uncertain);
        assert!(verification
            .missing_events
            .iter()
            .any(|event| event == "artifact.written"));
    }

    #[test]
    fn task_verification_blocks_artifact_completion_when_quality_policy_requires_revision() {
        let mut task = TaskState::new(
            "foreground_chat",
            "artifact quality task",
            json!({}),
            "benshu",
        );
        task.contract = Some(TaskContract {
            intent: Some("继续写第二章并保持设定不漂移".to_string()),
            required_events: vec!["artifact.written".to_string()],
            ..Default::default()
        });
        let outcome = test_chat_outcome(
            "已写入第二章。",
            vec![test_tool_call(
                "delegate",
                r#"{
                    "success": true,
                    "runtime_effect": "artifact.written",
                    "artifact_path": "/tmp/chapter.md",
                    "review": { "verdict": "needs_revision" },
                    "writing_policy": {
                        "passed": false,
                        "blockers": [
                            "latest_draft_requires_revision_before_approval_or_export"
                        ]
                    }
                }"#,
            )],
        );
        let events = vec![test_event("tool.completed"), test_event("artifact.written")];

        let verification = build_task_verification(&task, &outcome, &events);

        assert_eq!(verification.verdict, TaskVerificationVerdict::Fail);
        assert!(verification
            .summary
            .as_deref()
            .is_some_and(|summary| summary.contains("quality contract")));
    }

    #[test]
    fn task_verification_blocks_scaled_text_artifact_when_units_are_too_small() {
        let mut task = TaskState::new(
            "foreground_chat",
            "scaled writing task",
            json!({}),
            "benshu",
        );
        task.contract = Some(TaskContract {
            intent: Some("根据知识库写一部原创小说50万字".to_string()),
            required_events: vec!["artifact.written".to_string()],
            ..Default::default()
        });
        let outcome = test_chat_outcome(
            "已完成。",
            vec![test_tool_call(
                "delegate",
                r#"{
                  "success": true,
                  "runtime_effect": "artifact.written",
                  "artifact_path": "/tmp/plan.md",
                  "state": {
                    "document_type": "plan",
                    "units": 257
                  }
                }"#,
            )],
        );
        let events = vec![test_event("tool.completed"), test_event("artifact.written")];

        let verification = build_task_verification(&task, &outcome, &events);

        assert_eq!(verification.verdict, TaskVerificationVerdict::Fail);
        assert!(verification
            .summary
            .as_deref()
            .is_some_and(|summary| summary.contains("target 500000")));
    }

    #[test]
    fn task_verification_skips_full_scale_check_for_creation_contract_planning() {
        let mut task = TaskState::new(
            "foreground_chat_supervisor",
            "creation contract planning",
            json!({}),
            "benshu",
        );
        task.contract = Some(TaskContract {
            intent: Some("生成写作合同草案：写都市玄幻小说，每章2500字，至少5万字起".to_string()),
            completion_criteria: vec![
                "When the user requests an explicit text scale, the final artifact must report enough units to satisfy that scale; process notes, plans, outlines, or partial drafts are not completion evidence."
                    .to_string(),
            ],
            ..Default::default()
        });
        let outcome = test_chat_outcome("可确认合同：\n书名：血瞳新纪", Vec::new());

        let verification = build_task_verification(&task, &outcome, &[]);

        assert_eq!(verification.verdict, TaskVerificationVerdict::Pass);
    }

    #[test]
    fn task_verification_allows_existing_novel_content_crud_below_chapter_target() {
        let mut task = TaskState::new(
            "foreground_chat",
            "existing novel content edit",
            json!({}),
            "benshu",
        );
        task.contract = Some(TaskContract {
            intent: Some(super::writing_session_route::mark_novel_content_operation_task(
                "当前项目合同简述：每章2500字\n用户原话：删掉第二章重复段落\n操作类型：删除章节内容。",
            )),
            required_events: vec!["artifact.written".to_string()],
            ..Default::default()
        });
        let outcome = test_chat_outcome(
            "已按用户要求修订。",
            vec![test_tool_call(
                "delegate",
                r#"status: completed
operation: delete_chapter
runtime_effect: artifact.written
unit_count: 1625
quality_gate_passed: true
audit_status: completed"#,
            )],
        );
        let events = vec![test_event("tool.completed"), test_event("artifact.written")];

        let verification = build_task_verification(&task, &outcome, &events);

        assert_eq!(verification.verdict, TaskVerificationVerdict::Pass);
    }

    #[test]
    fn task_verification_ignores_quality_policy_when_no_artifact_is_required() {
        let mut task = TaskState::new("foreground_chat", "non artifact task", json!({}), "benshu");
        task.contract = Some(TaskContract {
            intent: Some("审查草稿但不要求保存产物".to_string()),
            required_events: vec![],
            ..Default::default()
        });
        let outcome = test_chat_outcome(
            r#"{"review":{"verdict":"needs_revision"},"writing_policy":{"passed":false}}"#,
            vec![],
        );

        let verification = build_task_verification(&task, &outcome, &[]);

        assert_eq!(verification.verdict, TaskVerificationVerdict::Pass);
    }

    #[test]
    fn semantic_events_include_artifact_quality_only_when_contract_passes() {
        let passed = super::semantic_runtime_events_for_tool(
            "delegate",
            "status: completed\nworker: coder\nexecuted_tool: pdf_build\nquality_contract: pass",
            "completed",
        );
        let failed = super::semantic_runtime_events_for_tool(
            "delegate",
            "status: completed\nworker: coder\nexecuted_tool: pdf_build\nquality_contract: fail",
            "completed",
        );

        assert!(passed.iter().any(|(topic, _)| topic == "artifact.quality"));
        assert!(!failed.iter().any(|(topic, _)| topic == "artifact.quality"));
    }

    #[test]
    fn semantic_events_accept_explicit_runtime_effect_receipts() {
        let events = super::semantic_runtime_events_for_tool(
            "delegate",
            "status: completed\nworker: knowledge\nruntime_effect: knowledge.imported\nruntime_effects: artifact.written, artifact.pdf",
            "completed",
        );

        assert!(events
            .iter()
            .any(|(topic, _)| topic == "knowledge.imported"));
        assert!(events.iter().any(|(topic, _)| topic == "artifact.written"));
        assert!(events.iter().any(|(topic, _)| topic == "artifact.pdf"));
    }

    #[test]
    fn semantic_events_accept_json_runtime_effect_receipts() {
        let events = super::semantic_runtime_events_for_tool(
            "novel_studio",
            r#"{"success":true,"runtime_effect":"artifact.written","artifact_path":"/tmp/chapter.md"}"#,
            "completed",
        );

        assert!(events.iter().any(|(topic, _)| topic == "artifact.written"));
    }

    #[test]
    fn process_status_report_write_is_not_final_artifact_evidence() {
        let result = "runtime_effect: artifact.written\npath: /tmp/work/status_report.txt\nbytes: 297\n\nSuccessfully wrote 297 bytes to /tmp/work/status_report.txt";
        let events = super::semantic_runtime_events_for_tool("write_file", result, "completed");

        assert!(!events.iter().any(|(topic, _)| topic == "artifact.written"));
        assert!(!super::checkpoint_summary_satisfies_required_event(
            "writer.write_file finished success=true duration_ms=4 preview=runtime_effect: artifact.written path: /tmp/work/status_report.txt bytes: 297 Successfully wrote 297 bytes",
            "artifact.written"
        ));
    }

    #[test]
    fn normal_written_artifact_still_counts_as_completion_evidence() {
        let result = "runtime_effect: artifact.written\npath: /tmp/work/chapter_0003.md\nbytes: 4096\n\nSuccessfully wrote 4096 bytes to /tmp/work/chapter_0003.md";
        let events = super::semantic_runtime_events_for_tool("write_file", result, "completed");

        assert!(events.iter().any(|(topic, _)| topic == "artifact.written"));
        assert!(super::checkpoint_summary_satisfies_required_event(
            "writer.write_file finished success=true duration_ms=4 preview=runtime_effect: artifact.written path: /tmp/work/chapter_0003.md bytes: 4096 Successfully wrote 4096 bytes",
            "artifact.written"
        ));
    }

    #[test]
    fn txt_artifact_intent_requires_txt_format_evidence() {
        let events =
            super::inferred_required_events_from_intent(Some("请写一部完整小说并保存成 txt 文档"));

        assert!(events.iter().any(|event| event == "artifact.written"));
        assert!(events.iter().any(|event| event == "artifact.txt"));
        assert!(!super::checkpoint_summary_satisfies_required_event(
            "writer.novel_studio finished success=true preview={\"success\":true,\"runtime_effect\":\"artifact.written\",\"artifact_path\":\"/tmp/novel/chapters/0001.md\"}",
            "artifact.txt"
        ));
        assert!(super::checkpoint_summary_satisfies_required_event(
            "writer.novel_studio finished success=true preview={\"success\":true,\"runtime_effects\":[\"artifact.written\",\"artifact.exported\",\"artifact.txt\"],\"output_path\":\"/tmp/novel/exports/final.txt\",\"format\":\"txt\"}",
            "artifact.txt"
        ));
    }

    #[test]
    fn checkpoint_effects_accept_delegated_child_artifact_path() {
        let checkpoint = benshu_state::TaskCheckpoint {
            step: 39,
            label: "agent:writer:tool:novel_studio:end".to_string(),
            recorded_at: chrono::Utc::now(),
            summary: Some(
                "writer.novel_studio finished success=true duration_ms=6 preview={ \"artifact_path\": \"/tmp/chapter.md\" }"
                    .to_string(),
            ),
        };

        let events = super::semantic_runtime_events_for_checkpoint(&checkpoint);

        assert!(events.iter().any(|(topic, _)| topic == "artifact.written"));
        assert!(super::checkpoint_summary_satisfies_required_event(
            checkpoint.summary.as_deref().expect("summary"),
            "artifact.written"
        ));
    }

    #[test]
    fn task_artifacts_are_extracted_from_plain_worker_artifact_receipts() {
        let result = "status: completed\nworker: writer\nexecuted_tool: write_file\npath: /tmp/story/final.txt\nruntime_effect: artifact.written\nruntime_effect: artifact.txt\nmedia_type: text/plain\nresult:\nCheckpointed 12 steps and wrote 37072 bytes";

        let artifacts = extract_task_artifacts_from_tool_result("delegate", result);

        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].uri, "/tmp/story/final.txt");
        assert_eq!(artifacts[0].media_type.as_deref(), Some("text/plain"));
    }

    #[test]
    fn task_artifacts_are_extracted_from_json_worker_artifact_receipts() {
        let result = r#"{"success":true,"runtime_effects":["artifact.written","artifact.txt"],"output_path":"/tmp/story/final.txt","format":"txt"}"#;

        let artifacts = extract_task_artifacts_from_tool_result("novel_studio", result);

        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].uri, "/tmp/story/final.txt");
        assert_eq!(artifacts[0].media_type.as_deref(), Some("text/plain"));
    }

    #[test]
    fn task_artifacts_are_extracted_from_local_workspace_path_summaries() {
        let result = "已保存写作/文件产物检查点。 - 章节：第 2 章 - 文件：/home/user/benshu/data/generated/novels/example/chapters/0002.md - 项目：/home/user/benshu/data/generated/novels/example";

        let artifacts = extract_task_artifacts_from_tool_result("delegate", result);

        assert_eq!(artifacts.len(), 1);
        assert_eq!(
            artifacts[0].uri,
            "/home/user/benshu/data/generated/novels/example/chapters/0002.md"
        );
        assert_eq!(artifacts[0].media_type.as_deref(), Some("text/markdown"));
    }

    #[test]
    fn task_artifact_path_extraction_trims_attached_ui_punctuation() {
        let result = "文件：/home/user/benshu/data/generated/novels/example/chapters/0003.md`。目前状态为 text/markdown";

        let artifacts = extract_task_artifacts_from_tool_result("delegate", result);

        assert_eq!(artifacts.len(), 1);
        assert_eq!(
            artifacts[0].uri,
            "/home/user/benshu/data/generated/novels/example/chapters/0003.md"
        );
        assert_eq!(artifacts[0].media_type.as_deref(), Some("text/markdown"));
    }

    #[test]
    fn chinese_saved_file_checkpoint_satisfies_artifact_written_event() {
        let summary = "Worker `writer` returned delegated result. Preview: 已保存写作/文件产物检查点。 - 章节：第 4 章 - 文件：/home/user/benshu/data/generated/novels/example/chapters/0004.md - 项目：/home/user/benshu/data/generated/novels/example";

        assert!(super::checkpoint_summary_satisfies_required_event(
            summary,
            "artifact.written"
        ));

        let checkpoint = benshu_state::TaskCheckpoint {
            step: 4,
            label: "worker:writer:delegated_task:completed".to_string(),
            recorded_at: chrono::Utc::now(),
            summary: Some(summary.to_string()),
        };
        let events = super::semantic_runtime_events_for_checkpoint(&checkpoint);

        assert!(events.iter().any(|(topic, _)| topic == "artifact.written"));
    }

    #[test]
    fn process_status_reports_are_not_extracted_as_task_artifacts() {
        let result = "runtime_effect: artifact.written\npath: /tmp/work/status_report.txt\ncompletion_scope: initial stage\nblockers: none";

        let artifacts = extract_task_artifacts_from_tool_result("write_file", result);

        assert!(artifacts.is_empty());
    }

    #[test]
    fn worker_completed_result_does_not_override_supervisor_status() {
        assert!(task_status_from_worker_result("status: completed\nresult: ok").is_none());
    }

    #[test]
    fn delegation_child_task_preserves_parent_root_relationship() {
        let mut parent = TaskState::new(
            "chat",
            "prime agent foreground chat",
            json!({"message": "hi"}),
            "benshu",
        );
        parent.session_id = Some("session-1".to_string());
        parent.thread_id = Some("thread-main".to_string());
        parent.run_id = Some(Uuid::new_v4());
        parent.trace_id = Some(Uuid::new_v4());
        parent.root_task_id = Some(Uuid::new_v4());

        let delegation = DelegationRecord {
            delegated_by: AgentRole::Custom("benshu".to_string()),
            delegated_to: AgentRole::Researcher,
            mode: DelegationMode::InternalRecommendation,
            task_owner: AgentRole::Custom("benshu".to_string()),
            session_id: Some("session-1".to_string()),
            summary: Some("Prime retained ownership while researcher was recommended.".to_string()),
        };

        let child = derive_delegation_child_task(&parent, &delegation);

        assert_eq!(child.parent_task_id, Some(parent.id));
        assert_eq!(child.root_task_id, parent.root_task_id);
        assert_eq!(child.session_id.as_deref(), Some("session-1"));
        assert_eq!(child.thread_id.as_deref(), Some("thread-main"));
        assert_eq!(child.delegation_state.as_deref(), Some("created"));
        assert_eq!(child.delegated_by.as_deref(), Some("benshu"));
        assert_eq!(child.delegated_to.as_deref(), Some("researcher"));
        assert_eq!(
            child.delegation_return_mode.as_deref(),
            Some("return_to_owner")
        );
        assert!(child.tags.iter().any(|tag| tag == "delegation"));
        assert!(child.name.contains("delegation::researcher"));
    }

    #[test]
    fn delegation_session_trace_filters_inbox_and_owner_rollup() {
        let inbox = serde_json::to_string(&vec![
            DelegationInboxDto {
                message_id: "m-1".into(),
                source: "benshu".into(),
                target: "researcher".into(),
                kind: "result".into(),
                request_id: Some("req-1".into()),
                session_id: Some("session-a".into()),
                trace_id: Some("trace-a".into()),
                task_id: Some("task-a".into()),
                parent_task_id: None,
                root_task_id: Some("root-a".into()),
                summary: "session a result".into(),
                visible_owner: Some("benshu".into()),
                memory_owner: Some("benshu".into()),
                approval_owner: Some("benshu".into()),
                delegated_by: Some("benshu".into()),
                delegated_to: Some("researcher".into()),
                final_response_owner: Some("benshu".into()),
                return_mode: Some("return_to_owner".into()),
                delegation_state: Some("returned".into()),
            },
            DelegationInboxDto {
                message_id: "m-2".into(),
                source: "benshu".into(),
                target: "coder".into(),
                kind: "task_request".into(),
                request_id: Some("req-2".into()),
                session_id: Some("session-b".into()),
                trace_id: Some("trace-b".into()),
                task_id: Some("task-b".into()),
                parent_task_id: None,
                root_task_id: Some("root-b".into()),
                summary: "session b request".into(),
                visible_owner: Some("benshu".into()),
                memory_owner: Some("benshu".into()),
                approval_owner: Some("benshu".into()),
                delegated_by: Some("benshu".into()),
                delegated_to: Some("coder".into()),
                final_response_owner: Some("benshu".into()),
                return_mode: Some("return_to_owner".into()),
                delegation_state: Some("created".into()),
            },
        ])
        .expect("serialize inbox");
        let owner_rollup = serde_json::to_string(&json!({
            "session_id": "session-a",
            "request_id": "req-1",
            "final_response_owner": "benshu"
        }))
        .expect("serialize owner rollup");

        let trace = session_delegation_trace_from_metadata(
            "session-a",
            "benshu".to_string(),
            Some("embedded".to_string()),
            Some(owner_rollup),
            Some(inbox),
        );

        assert_eq!(trace.session_id, "session-a");
        assert_eq!(trace.active_role, "benshu");
        assert_eq!(trace.runtime_profile.as_deref(), Some("embedded"));
        assert_eq!(trace.inbox.len(), 1);
        assert_eq!(trace.inbox[0].message_id, "m-1");
        assert_eq!(trace.inbox[0].root_task_id.as_deref(), Some("root-a"));
        assert_eq!(trace.inbox[0].visible_owner.as_deref(), Some("benshu"));
        assert_eq!(
            trace
                .owner_rollup
                .as_ref()
                .and_then(|value| value.get("request_id"))
                .and_then(|value| value.as_str()),
            Some("req-1")
        );
    }

    #[test]
    fn delegation_session_trace_tolerates_malformed_metadata() {
        let trace = session_delegation_trace_from_metadata(
            "session-z",
            "benshu".to_string(),
            None,
            Some("{not-json".to_string()),
            Some("{also-not-json".to_string()),
        );

        assert!(trace.inbox.is_empty());
        assert!(trace.owner_rollup.is_none());
        assert!(trace.runtime_profile.is_none());
    }

    struct DummyCancelableAgent {
        role: AgentRole,
        cancelled: Arc<AtomicBool>,
        events_tx: broadcast::Sender<benshu_brain::agent::AgentEvent>,
    }

    impl DummyCancelableAgent {
        fn new(role: AgentRole, cancelled: Arc<AtomicBool>) -> Self {
            let (events_tx, _) = broadcast::channel(8);
            Self {
                role,
                cancelled,
                events_tx,
            }
        }
    }

    #[async_trait]
    impl MultiAgent for DummyCancelableAgent {
        fn role(&self) -> AgentRole {
            self.role.clone()
        }

        async fn handle_message(
            &self,
            _message: AgentMessage,
        ) -> BrainResult<Option<AgentMessage>> {
            Err(BrainError::AgentCommunication(
                "dummy test agent should not receive messages".to_string(),
            ))
        }

        async fn process(&self, _input: &str) -> BrainResult<String> {
            Err(BrainError::AgentCoordination(
                "dummy test agent should not process foreground work".to_string(),
            ))
        }

        async fn chat(
            &self,
            _messages: Vec<Message>,
            _session_id: Option<String>,
        ) -> BrainResult<ChatOutcome> {
            Err(BrainError::AgentCoordination(
                "dummy test agent should not run chat".to_string(),
            ))
        }

        fn agent_identity(&self) -> Option<Arc<parking_lot::RwLock<Option<AgentIdentity>>>> {
            None
        }

        fn events(&self) -> broadcast::Receiver<benshu_brain::agent::AgentEvent> {
            self.events_tx.subscribe()
        }

        fn security(&self) -> Option<Arc<dyn benshu_brain::security::SecurityHandler>> {
            None
        }

        fn cancel(&self) {
            self.cancelled.store(true, Ordering::SeqCst);
        }

        fn ensure_active_token(&self) {}
    }

    async fn build_test_state() -> anyhow::Result<(AppState, TempDir)> {
        let temp_dir = tempfile::tempdir()?;
        let base_dir = temp_dir.path().to_path_buf();
        let config = AppConfig {
            agent_path: Some(base_dir.join("agents")),
            ..Default::default()
        };
        let kernel = Arc::new(
            KernelBootstrapper::new(base_dir.clone(), config.clone())
                .boot()
                .await?,
        );
        let shared_config = Arc::new(parking_lot::RwLock::new(config));
        let enabled_tools = Arc::new(parking_lot::RwLock::new(HashSet::new()));
        let factory = Arc::new(AgentFactory::new(
            kernel.clone(),
            shared_config.clone(),
            enabled_tools.clone(),
            None,
        ));
        factory.install_worker_spawner();
        let oauth = Arc::new(OAuthManager::new(Arc::new(VaultTokenStore::new(
            kernel.vault().clone(),
        ))));
        let (log_sender, _) = broadcast::channel(32);
        let (connector_trigger, _) = mpsc::unbounded_channel();

        let state = AppState {
            kernel: kernel.clone(),
            app_config: shared_config.clone(),
            factory,
            document_router: Arc::new(DocumentUnderstandTool::new(
                None,
                None,
                kernel.sensory().clone(),
            )),
            agent_templates: vec![],
            oauth,
            approvals: Arc::new(crate::api::security::ApprovalManager::new()),
            enabled_tools,
            config_path: base_dir.join("benshu.yaml"),
            log_sender,
            connector_trigger,
            log_history: Arc::new(parking_lot::RwLock::new(VecDeque::new())),
            running_connectors: Arc::new(parking_lot::RwLock::new(HashSet::new())),
            channel_observability: Arc::new(parking_lot::RwLock::new(HashMap::new())),
            cancel_tokens: Arc::new(dashmap::DashMap::new()),
            runtime_persist_limiter: Arc::new(tokio::sync::Semaphore::new(2)),
            bus: Arc::new(MessageBus::new(64)),
            internal_key: "test-internal-key".to_string(),
            deployment_mode: crate::LaunchMode::Embedded,
            intent_router: Arc::new(benshu_knowledge::IntentRouter::new()),
            nlu: kernel.nlu().clone(),
        };

        Ok((state, temp_dir))
    }

    fn sample_run_trace(task_id: Uuid, session_id: Uuid) -> RunTrace {
        let started_at = Utc::now();
        RunTrace {
            run_id: Uuid::new_v4(),
            session_id,
            agent_id: "benshu".to_string(),
            status: TraceStatus::Succeeded,
            started_at,
            finished_at: Some(started_at),
            task_id: Some(task_id),
            thread_id: Some("gateway-stage-a-thread".to_string()),
            provider: Some("local".to_string()),
            model: Some("qwen2.5-0.5b-instruct-q4_k_m.gguf".to_string()),
            prompt_tokens: Some(32),
            completion_tokens: Some(12),
            stages: vec![RuntimeStageTrace {
                stage: RuntimeStage::Ingress,
                status: TraceStatus::Succeeded,
                started_at,
                finished_at: Some(started_at),
                detail: Some("gateway stage a smoke".to_string()),
                metadata: HashMap::new(),
            }],
            tools: Vec::new(),
            artifacts: Vec::new(),
            degradation_notes: Vec::new(),
            witness: None,
            metadata: HashMap::new(),
        }
    }

    #[tokio::test]
    async fn gateway_runtime_read_paths_cover_task_replay_witness_and_session_stop() {
        let (state, _temp_dir) = build_test_state().await.expect("state should boot");

        let session_key = "gateway-stage-a-session".to_string();
        let session_uuid = Uuid::new_v4();
        let cancelled = Arc::new(AtomicBool::new(false));
        let role = AgentRole::Custom("benshu".to_string());
        state
            .kernel
            .coordinator()
            .register(Arc::new(DummyCancelableAgent::new(
                role.clone(),
                cancelled.clone(),
            )));
        state
            .kernel
            .coordinator()
            .switch_session_agent(&session_key, role);

        let mut task = TaskState::new(
            "foreground_chat",
            "gateway stage a persisted runtime task",
            json!({"message": "gateway stage a"}),
            "benshu",
        );
        task.session_id = Some(session_key.clone());
        task.thread_id = Some("gateway-stage-a-thread".to_string());

        let mut trace = sample_run_trace(task.id, session_uuid);
        task.run_id = Some(trace.run_id);
        task.trace_id = Some(trace.run_id);

        let persisted = state
            .kernel
            .persist_runtime_mainline(
                Some(task.clone()),
                vec![],
                Some(&mut trace),
                Some("gateway_stage_a"),
            )
            .await
            .expect("runtime mainline should persist");

        let persisted_task = persisted.task.expect("task should persist");
        let witness_bundle = persisted
            .witness_bundle
            .expect("witness bundle should persist");

        let Json(tasks) =
            match list_session_tasks(State(state.clone()), Path(session_key.clone())).await {
                Ok(json) => json,
                Err(err) => panic!("task query should succeed: {}", err.0),
            };
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, persisted_task.id);
        assert_eq!(tasks[0].trace_id, Some(trace.run_id));
        assert_eq!(tasks[0].witness_id, Some(witness_bundle.witness_id));

        let Json(replay) = get_run_replay(State(state.clone()), Path(trace.run_id.to_string()))
            .await
            .expect("replay query should succeed");
        assert_eq!(replay.trace_id, trace.run_id);
        assert_eq!(replay.task_id, Some(persisted_task.id));
        assert!(replay.replayable);

        let Json(witness) = get_witness_summary(
            State(state.clone()),
            Path(witness_bundle.witness_id.to_string()),
        )
        .await
        .expect("witness query should succeed");
        assert_eq!(witness.witness_id, witness_bundle.witness_id);
        assert_eq!(witness.run_id, Some(trace.run_id));
        assert!(witness.replayable);

        let status = match cancel_session(State(state), Path(session_key)).await {
            Ok(status) => status,
            Err(err) => panic!("session cancel should succeed: {}", err.0),
        };
        assert_eq!(status, StatusCode::OK);
        assert!(
            cancelled.load(Ordering::SeqCst),
            "gateway session stop should cancel only the active foreground agent"
        );
    }
}
