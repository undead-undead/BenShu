use base64::Engine;
use benshu_hardness::{
    classify_failure, decide_finalization_fallback, decide_lookup_evidence_recovery,
    decide_reflexion_strategy_upgrade, decide_tool_first_recovery,
    extract_reflexion_critique_reason, retry_allows_reflexion_upgrade, should_run_reflexion_review,
    EvidenceQuality, FailureClass, FinalizationFallbackInput, FinalizationFallbackKind,
    LookupEvidenceRecoveryInput, RecoveryAction, ReflexionReviewInput, ReflexionUpgradeDecision,
    ReflexionUpgradeInput, ReflexionUpgradeReason, ToolFirstRecoveryInput,
};
use futures::StreamExt;
use moka::future::Cache;
use parking_lot::RwLock;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};
use uuid::Uuid;

use benshu_provider_core::{ContextLimitError, ContinuationHint};
use benshu_runtime_policy_core::{resolve_language_contract, LanguageContract};

use crate::agent::attempt::Attempt;
use crate::agent::evolution::complexity::ComplexityEstimator;
use crate::agent::history::QueryHistory;
use crate::agent::message::{Content, ContentPart, ImageSource, Message, Role};
use crate::agent::multi_agent::MultiAgent;
use crate::agent::prompt_surface::{PromptSegmentKind, PromptSurfaceReport};
use crate::agent::protocol::AgentLiaison;
use crate::agent::protocol::*;
use crate::agent::provider::Provider;
use crate::agent::streaming::{FinishReason, ProviderTelemetry, StreamingChoice};
use crate::agent::tactical::TacticalOrchestrator;
use crate::agent::truth_verification_policy::TruthVerificationPolicyEngine;
use crate::error::{Error, Result};
use crate::hooks::{HookEvent, HookResult, HookTiming};
use crate::skills::tool::{
    capability_route_requires_real_tool_call, capability_route_system_message,
    capability_route_tool_allowlist_for_query, capability_route_tool_required_failure_message,
    classify_query_capability_route, coordinator_chat_lite_tool_names_for_query,
    coordinator_default_tool_names_for_query, coordinator_routing_judgment_only_message,
    coordinator_specialist_selection_message, coordinator_task_mode_label,
    coordinator_task_mode_should_include_media_followup_prompt,
    coordinator_task_mode_should_include_reasoning_prompt,
    coordinator_task_mode_should_include_route_prompt,
    coordinator_task_mode_should_include_truth_guidance, coordinator_task_mode_system_message,
    query_prefers_knowledge_base_retrieval, query_prefers_session_continuity_answer,
    query_requests_followup_execution_after_lookup, query_requests_image_generation,
    query_requests_routing_judgment_only, select_coordinator_task_mode, CapabilityRouteHint,
    CoordinatorTaskMode, RealtimeLookupKind, ToolDefinition, ToolSet,
};
use crate::skills::ThrottleLevel;

const CREATION_PLANNING_DIALOGUE_MARKER: &str = "[BENSHU_CREATION_PLANNING_DIALOGUE]";

mod collection_evidence;
mod context_pruning;
mod execution_guard;
mod fallback_text;
mod frontstage_tools;
mod knowledge_delivery;
mod media_delivery;
mod media_followup;
mod orchestration_chain;
mod output_contract;
mod post_import_delivery;
mod pseudo_tool;
mod skill_assets;
mod source_selection;
mod tool_delivery;
mod turn_state;

pub(crate) use media_followup::{
    apply_capability_route as apply_media_followup_capability_route,
    capability_contract as media_followup_capability_contract,
    latest_turn_simple_media_understanding, latest_user_message_has_media,
    latest_user_message_with_media,
    render_strategy_prompt as render_media_followup_strategy_prompt,
    route_requires_real_tool_call_for_turn, should_force_direct_multimodal_answer,
    strategies_from_messages as media_followup_strategies_from_messages,
};

#[allow(unused_imports)]
pub(crate) use skill_assets::{
    approved_forge_request_from_messages, available_skill_assets_from_messages,
    forged_session_tool_already_executed, forged_session_tool_names_from_messages,
    matched_skill_asset_path_from_messages, matched_skill_manual_name,
    matched_skill_manual_name_from_messages, resolve_skill_asset_path_from_messages,
    runtime_session_title, skill_asset_already_loaded, skill_manual_already_loaded,
    tool_result_reads_skill_asset, tool_result_reads_skill_manual,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CreationPlanningStage {
    Skeleton,
    Characters,
    Plot,
    Governance,
    Generic,
}

/// Reasoner-specific engineering constants
pub mod reasoner_constants {
    /// Ratio of max steps allowed for audit rejections before halting (熔断阈值)
    pub const MAX_AUDIT_RETRY_RATIO: f32 = 0.5;
    /// Ratio of context window to keep for recent messages during smart pruning
    pub const RECENT_HISTORY_RATIO: f32 = 0.4;
    /// Milliseconds in a second
    pub const SEC_TO_MS: u64 = 1000;

    /// Markers for internal interventions
    pub const MARKER_SECURITY_REJECTION: &str = "### SECURITY AUDIT REJECTION";
    pub const MARKER_TACTICAL_PIVOT: &str = "### TACTICAL PIVOT ADVICE";
    pub const MARKER_EFFICIENCY_WARNING: &str = "### SYSTEM EFFICIENCY WARNING";
    pub const MARKER_REFLEXION_CRITIQUE: &str = "### REFLEXION CRITIQUE";
    pub const MARKER_FORGE_APPROVED: &str = "### FORGE_APPROVED";
    pub const MARKER_TOOL_EXECUTION_REQUIRED: &str = "### TOOL EXECUTION REQUIRED";

    /// Distillation cache TTL (minutes)
    pub const DISTILLATION_CACHE_TTL: u64 = 30;
    /// Distillation cache max items
    pub const DISTILLATION_CACHE_MAX_SIZE: u64 = 1000;
    /// Max tokens per single reasoning step for remote/API models
    pub const API_MAX_STEP_TOKENS: usize = 4096;
    /// Max tokens per single reasoning step for local models
    pub const LOCAL_MAX_STEP_TOKENS: usize = 16_384;
    /// Max session tokens for API-based models (balance cost vs. task completion)
    pub const API_SESSION_TOKEN_QUOTA: usize = 180_000;
    /// Max session tokens for Local models (high ceiling to allow deep reasoning)
    pub const LOCAL_SESSION_TOKEN_QUOTA: usize = 600_000;
    /// Max output tokens for a turn whose job is selecting or repairing tool
    /// execution. The artifact itself should be produced by workers/tools, not
    /// by a long frontstage reasoning response.
    pub const EXECUTION_TOOL_TURN_MAX_TOKENS: u64 = 2_048;
    /// Max output tokens for lightweight direct answers.
    pub const SHORT_ANSWER_MAX_TOKENS: u64 = 512;
    /// Max output tokens for ordinary concise explanations.
    pub const BRIEF_EXPLANATION_MAX_TOKENS: u64 = 128;
    /// Max output tokens for explanatory direct answers.
    pub const EXPLANATION_MAX_TOKENS: u64 = 2_048;
    /// Max output tokens for artifact-oriented planning/drafting turns.
    pub const ARTIFACT_STEP_MAX_TOKENS: u64 = 4_096;
    /// Max output tokens for governed long-form steps.
    pub const LONGFORM_STEP_MAX_TOKENS: u64 = 4_096;
    /// Minimum request budget for local model calls. Local OpenAI-compatible
    /// servers often return tool-call turns via non-stream responses, so the
    /// foreground HTTP observation window must not become the model request
    /// budget.
    pub const LOCAL_MIN_LLM_TIMEOUT_SECS: u64 = 300;
    /// Short local answer timeout floor.
    pub const LOCAL_SHORT_LLM_TIMEOUT_SECS: u64 = 60;
    /// Medium local answer/tool timeout floor.
    pub const LOCAL_MEDIUM_LLM_TIMEOUT_SECS: u64 = 120;
    /// Local artifact step timeout floor.
    pub const LOCAL_ARTIFACT_LLM_TIMEOUT_SECS: u64 = 240;
    /// Upper bound for a single local model request. Long tasks should still
    /// progress through multiple observable steps instead of one unbounded call.
    pub const LOCAL_MAX_LLM_TIMEOUT_SECS: u64 = 1_800;
    /// Local output tokens per additional timeout second.
    pub const LOCAL_OUTPUT_TOKENS_PER_TIMEOUT_SEC: u64 = 32;
    /// Max audit retries
    pub const AUDIT_MAX_RETRIES: usize = 2;
    /// Audit retry backoff (ms)
    pub const AUDIT_RETRY_BACKOFF_MS: u64 = 500;
}

/// Result of a single reasoning step
#[derive(Debug, Clone)]
pub struct ReasonerStep {
    pub text: String,
    pub thoughts: Vec<String>,
    pub tool_calls: Vec<(String, String, serde_json::Value)>,
    pub usage: Option<TokenUsage>,
}

pub struct Reasoner<P: Provider> {
    provider: Arc<P>,
    config: ReasonerConfig,
    tools: ToolSet,
    enabled_tools: Option<Arc<RwLock<std::collections::HashSet<String>>>>,
    tactical_orchestrator: Arc<dyn TacticalOrchestrator>,
    complexity_estimator: ComplexityEstimator,
    distillation_cache: Cache<Vec<u8>, Message>,
    // Reasoning Step Rate Limiter (Prevent thread exhaustion)
    rate_limiter: Arc<tokio::sync::Semaphore>,
}

impl<P: Provider> Reasoner<P> {
    const KNOWLEDGE_IMPORT_EVIDENCE_ARG_MAX_BYTES: usize = 2_400;
    const KNOWLEDGE_IMPORT_SUMMARY_ARG_MAX_BYTES: usize = 1_800;
    const KNOWLEDGE_IMPORT_QUERY_ARG_MAX_BYTES: usize = 1_200;

    fn coordinator_default_tool_allowlist(query: Option<&str>) -> HashSet<String> {
        coordinator_default_tool_names_for_query(query)
    }

    fn coordinator_default_tool_allowlist_for_mode(
        mode: CoordinatorTaskMode,
        query: Option<&str>,
    ) -> HashSet<String> {
        match mode {
            CoordinatorTaskMode::ChatLite => coordinator_chat_lite_tool_names_for_query(query),
            _ => Self::coordinator_default_tool_allowlist(query),
        }
    }

    fn condensed_frontstage_preamble(&self) -> String {
        let preamble = self.config.preamble.trim();
        let lowered = preamble.to_lowercase();
        if lowered.contains("benshu")
            || lowered.contains("primary assistant")
            || lowered.contains("trusted ai assistant")
            || lowered.contains("frontstage")
        {
            "You are BenShu, the user's frontstage AI assistant. \
             You are the only public-facing assistant. Keep internal routing and worker topology hidden, \
             answer directly when possible, preserve access to memory and RAG, and choose specialists when execution-heavy work is needed. \
             Stay concise for lightweight chat turns, but do not give up your frontstage coordination, memory, retrieval, or specialist-selection role."
                .to_string()
        } else {
            preamble.to_string()
        }
    }

    fn compact_frontstage_core_tool_definition(tool: ToolDefinition) -> ToolDefinition {
        frontstage_tools::compact_frontstage_core_tool_definition(tool)
    }

    fn auxiliary_session_id(&self, scope: &str) -> Option<String> {
        self.config
            .session_id
            .as_ref()
            .map(|session_id| format!("{}::{}", session_id, scope.trim()))
    }

    fn continuation_id_part(value: &str) -> String {
        let normalized = value
            .trim()
            .chars()
            .map(|ch| match ch {
                'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' => ch,
                _ => '_',
            })
            .collect::<String>()
            .split('_')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("_");
        let clipped = normalized.chars().take(96).collect::<String>();
        if clipped.is_empty() {
            "unknown".to_string()
        } else {
            clipped
        }
    }

    fn runtime_fingerprint(text: &str) -> String {
        format!("{:016x}", seahash::hash(text.as_bytes()))
    }

    fn continuation_turn_id(messages: &[Message]) -> String {
        let user_turns = messages
            .iter()
            .filter(|message| matches!(message.role, Role::User))
            .count()
            .max(1);
        let latest_user = Self::latest_user_query(messages).unwrap_or_default();
        let source = if latest_user.trim().is_empty() {
            messages
                .iter()
                .map(|message| message.text())
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            latest_user
        };
        format!("turn-{user_turns}-{}", Self::runtime_fingerprint(&source))
    }

    fn visible_prompt_fingerprint(
        &self,
        system_prompt: &str,
        messages: &[Message],
        tools: &[ToolDefinition],
        max_tokens: Option<u64>,
    ) -> String {
        let mut surface = String::new();
        surface.push_str("model:");
        surface.push_str(&self.config.model);
        surface.push_str("\nagent:");
        surface.push_str(self.config.agent_name.as_deref().unwrap_or("agent"));
        surface.push_str("\nmax_tokens:");
        surface.push_str(
            &max_tokens
                .map(|value| value.to_string())
                .unwrap_or_else(|| "provider_default".to_string()),
        );
        surface.push_str("\nsystem:");
        surface.push_str(system_prompt);
        surface.push_str("\nmessages:");
        for message in messages {
            surface.push_str(message.role.as_str());
            surface.push(':');
            surface.push_str(&message.text());
            surface.push('\n');
        }
        surface.push_str("tools:");
        for tool in tools {
            surface.push_str(&tool.name);
            surface.push('\n');
        }
        Self::runtime_fingerprint(&surface)
    }

    fn continuation_hint_for_request(
        &self,
        system_prompt: &str,
        messages: &[Message],
        tools: &[ToolDefinition],
        max_tokens: Option<u64>,
    ) -> Option<ContinuationHint> {
        let session_id = self.config.session_id.as_ref()?;
        let agent_name =
            Self::continuation_id_part(self.config.agent_name.as_deref().unwrap_or("agent"));
        let session_part = Self::continuation_id_part(session_id);
        let turn_id = Self::continuation_turn_id(messages);
        let prompt_fingerprint =
            self.visible_prompt_fingerprint(system_prompt, messages, tools, max_tokens);
        let worker_run_id = format!("{session_part}::{agent_name}::{turn_id}");
        let continuation_frontier_id = format!("{worker_run_id}::frontier-{prompt_fingerprint}");
        Some(ContinuationHint {
            user_session_id: Some(session_id.clone()),
            turn_id: Some(turn_id),
            worker_run_id: Some(worker_run_id),
            continuation_frontier_id: Some(continuation_frontier_id),
            visible_prompt_fingerprint: Some(prompt_fingerprint),
            ..Default::default()
        })
    }

    fn write_continuation_hint_to_extra(extra: &mut serde_json::Value, hint: &ContinuationHint) {
        let Some(extra_map) = extra.as_object_mut() else {
            return;
        };
        extra_map.insert(
            "continuation_hint_present".to_string(),
            serde_json::json!(true),
        );
        if let Some(value) = hint.user_session_id.as_ref() {
            extra_map.insert(
                "continuation_user_session_id".to_string(),
                serde_json::json!(value),
            );
        }
        if let Some(value) = hint.turn_id.as_ref() {
            extra_map.insert("continuation_turn_id".to_string(), serde_json::json!(value));
        }
        if let Some(value) = hint.worker_run_id.as_ref() {
            extra_map.insert(
                "continuation_worker_run_id".to_string(),
                serde_json::json!(value),
            );
        }
        if let Some(value) = hint.continuation_frontier_id.as_ref() {
            extra_map.insert(
                "continuation_frontier_id".to_string(),
                serde_json::json!(value),
            );
        }
        if let Some(value) = hint.visible_prompt_fingerprint.as_ref() {
            extra_map.insert(
                "continuation_visible_prompt_fingerprint".to_string(),
                serde_json::json!(value),
            );
        }
    }

    fn write_continuation_hint_to_metadata(
        metadata: &mut std::collections::HashMap<String, String>,
        hint: &ContinuationHint,
    ) {
        if let Some(value) = hint.user_session_id.as_ref() {
            metadata.insert(
                "runtime_continuation_user_session_id".to_string(),
                value.clone(),
            );
        }
        if let Some(value) = hint.turn_id.as_ref() {
            metadata.insert("runtime_continuation_turn_id".to_string(), value.clone());
        }
        if let Some(value) = hint.worker_run_id.as_ref() {
            metadata.insert(
                "runtime_continuation_worker_run_id".to_string(),
                value.clone(),
            );
        }
        if let Some(value) = hint.continuation_frontier_id.as_ref() {
            metadata.insert(
                "runtime_continuation_frontier_id".to_string(),
                value.clone(),
            );
        }
        if let Some(value) = hint.visible_prompt_fingerprint.as_ref() {
            metadata.insert(
                "runtime_continuation_visible_prompt_fingerprint".to_string(),
                value.clone(),
            );
        }
    }

    fn latest_user_query(messages: &[Message]) -> Option<String> {
        messages
            .iter()
            .rev()
            .find(|message| matches!(message.role, Role::User))
            .and_then(|message| match &message.content {
                Content::Text(text) => Some(text.trim().to_string()),
                Content::Parts(parts) => {
                    let text = parts
                        .iter()
                        .filter_map(|part| match part {
                            ContentPart::Text { text } => Some(text.trim()),
                            _ => None,
                        })
                        .filter(|text| !text.is_empty())
                        .collect::<Vec<_>>()
                        .join("\n")
                        .trim()
                        .to_string();
                    if text.is_empty() {
                        None
                    } else {
                        Some(text)
                    }
                }
                _ => {
                    let text = message.content.as_text();
                    let trimmed = text.trim();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(trimmed.to_string())
                    }
                }
            })
    }

    fn latest_knowledge_persistence_query(messages: &[Message]) -> Option<String> {
        messages.iter().rev().find_map(|message| {
            if !matches!(message.role, Role::User) {
                return None;
            }
            let query = match &message.content {
                Content::Text(text) => text.trim().to_string(),
                Content::Parts(parts) => parts
                    .iter()
                    .filter_map(|part| match part {
                        ContentPart::Text { text } => Some(text.trim()),
                        _ => None,
                    })
                    .filter(|text| !text.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n")
                    .trim()
                    .to_string(),
                _ => message.content.as_text().trim().to_string(),
            };
            (!query.is_empty() && Self::query_requests_knowledge_persistence(&query))
                .then_some(query)
        })
    }

    fn query_requests_knowledge_persistence(query: &str) -> bool {
        knowledge_delivery::query_requests_knowledge_persistence(query)
    }

    fn requested_text_target_chars(query: &str) -> Option<usize> {
        let lowered = query.to_lowercase();
        let bytes = query.as_bytes();
        for (idx, ch) in query.char_indices() {
            if ch != '万' {
                continue;
            }
            let prefix = &query[..idx];
            let digits = prefix
                .chars()
                .rev()
                .take_while(|ch| ch.is_ascii_digit())
                .collect::<Vec<_>>();
            if !digits.is_empty() {
                let number = digits.into_iter().rev().collect::<String>();
                if let Ok(value) = number.parse::<usize>() {
                    return Some(value.saturating_mul(10_000));
                }
            }
        }
        for marker in ["字", "字符", "words", "word", "chars", "characters"] {
            let mut search_start = 0;
            while let Some(offset) = lowered[search_start..].find(marker) {
                let marker_start = search_start + offset;
                let prefix = &lowered[..marker_start];
                let digits = prefix
                    .chars()
                    .rev()
                    .skip_while(|ch| ch.is_ascii_whitespace() || *ch == ',' || *ch == '_')
                    .take_while(|ch| ch.is_ascii_digit())
                    .collect::<Vec<_>>();
                if !digits.is_empty() {
                    let number = digits.into_iter().rev().collect::<String>();
                    if let Ok(value) = number.parse::<usize>() {
                        return Some(value);
                    }
                }
                search_start = marker_start + marker.len();
                if search_start >= bytes.len() {
                    break;
                }
            }
        }
        None
    }

    fn query_requests_local_file_continuation(query: &str) -> bool {
        if Self::query_is_creation_planning_dialogue(query) {
            return false;
        }
        let lowered = query.to_lowercase();
        if Self::query_requests_knowledge_persistence(query)
            || lowered.contains("search")
            || lowered.contains("lookup")
            || lowered.contains("fetch")
            || lowered.contains("download")
            || query.contains("搜索")
            || query.contains("查找")
            || query.contains("检索")
            || query.contains("获取")
            || query.contains("下载")
            || query.contains("收进知识库")
            || query.contains("存入知识库")
            || query.contains("导入知识库")
        {
            return false;
        }
        let asks_continuation = lowered.contains("continue")
            || lowered.contains("append")
            || lowered.contains("extend")
            || lowered.contains("续写")
            || lowered.contains("继续")
            || lowered.contains("追加")
            || lowered.contains("扩写")
            || lowered.contains("补写");
        let mentions_local_artifact = lowered.contains(".txt")
            || lowered.contains(".md")
            || lowered.contains("local file")
            || lowered.contains("text artifact")
            || lowered.contains("已保存")
            || query.contains("文档")
            || query.contains("文件");
        asks_continuation && mentions_local_artifact
    }

    fn query_requests_post_import_delivery(query: &str) -> bool {
        post_import_delivery::query_requests_post_import_delivery(query)
    }

    fn query_is_creation_planning_dialogue(query: &str) -> bool {
        query.contains(CREATION_PLANNING_DIALOGUE_MARKER)
    }

    fn query_requests_file_artifact(query: &str) -> bool {
        if Self::query_is_creation_planning_dialogue(query) {
            return false;
        }
        post_import_delivery::query_requests_file_artifact(query)
    }

    fn query_requests_artifact_mutation(query: &str) -> bool {
        if Self::query_is_creation_planning_dialogue(query) {
            return false;
        }
        if Self::query_requests_existing_artifact_read_only_answer(query) {
            return false;
        }
        if Self::query_requests_file_artifact(query) || Self::query_requests_code_artifact(query) {
            return true;
        }

        let lowered = query.to_lowercase();
        let mutation_terms = [
            "write", "draft", "create", "generate", "save", "revise", "rewrite", "edit", "update",
            "polish", "complete", "expand", "continue", "append",
        ];
        let artifact_terms = [
            "artifact",
            "file",
            "document",
            "doc",
            "draft",
            "section",
            "chapter",
            "article",
            "paper",
            "report",
            "story",
            "novel",
            "outline",
            "summary",
            "continuity",
        ];
        let has_mutation = mutation_terms.iter().any(|term| lowered.contains(term))
            || [
                "写", "撰写", "创作", "创建", "生成", "保存", "修订", "修改", "修正", "改写",
                "编辑", "润色", "补全", "完善", "扩写", "续写", "继续", "追加", "更新", "校订",
                "整理",
            ]
            .iter()
            .any(|term| query.contains(term));
        let has_artifact = artifact_terms.iter().any(|term| lowered.contains(term))
            || [
                "产物",
                "文件",
                "文档",
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
                "摘要",
                "关键事实",
                "连续性",
                "设定",
                "角色",
            ]
            .iter()
            .any(|term| query.contains(term));

        has_mutation && has_artifact
    }

    fn query_requests_existing_artifact_read_only_answer(query: &str) -> bool {
        let lowered = query.to_lowercase();
        let read_terms = [
            "summarize",
            "summary",
            "tell me",
            "what is",
            "who is",
            "where",
            "path",
            "read",
            "inspect",
        ];
        let existing_terms = [
            "current",
            "previous",
            "last",
            "already",
            "existing",
            "generated",
            "saved",
            "exported",
        ];
        let read = read_terms.iter().any(|term| lowered.contains(term))
            || [
                "总结",
                "概括",
                "告诉我",
                "看一下",
                "查看",
                "读取",
                "主角",
                "内容",
                "路径",
                "在哪",
                "哪里",
            ]
            .iter()
            .any(|term| query.contains(term));
        let existing = existing_terms.iter().any(|term| lowered.contains(term))
            || [
                "刚才",
                "刚刚",
                "上次",
                "上一轮",
                "之前",
                "前面",
                "已经",
                "已生成",
                "生成的",
                "保存的",
                "导出的",
                "这个",
                "那个",
            ]
            .iter()
            .any(|term| query.contains(term));
        if !read || !existing {
            return false;
        }
        let mutation = [
            "重新写",
            "重新生成",
            "重新创建",
            "再写",
            "再生成",
            "继续写",
            "续写",
            "另写",
            "新写",
            "修改",
            "修订",
            "改写",
            "润色",
            "补全",
            "扩写",
            "保存成",
            "导出为",
            "做成",
            "rewrite",
            "regenerate",
            "write another",
            "continue writing",
            "revise",
            "edit",
            "polish",
            "expand",
            "export as",
            "save as",
        ];
        !mutation
            .iter()
            .any(|term| query.contains(term) || lowered.contains(term))
    }

    fn query_requests_artifact_verification(query: &str) -> bool {
        let lowered = query.to_lowercase();
        let verify_terms = [
            "ensure", "verify", "confirm", "check", "inspect", "status", "exists", "already",
        ];
        let artifact_terms = [
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
        let has_verify = verify_terms.iter().any(|term| lowered.contains(term))
            || [
                "确保", "确认", "检查", "核验", "验证", "校验", "看看", "是否", "已经", "状态",
                "存在",
            ]
            .iter()
            .any(|term| query.contains(term));
        let has_artifact = artifact_terms.iter().any(|term| lowered.contains(term))
            || [
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
            ]
            .iter()
            .any(|term| query.contains(term));

        if !(has_verify && has_artifact) {
            return false;
        }

        let contingent_or_existence = [
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
        ]
        .iter()
        .any(|term| query.contains(term) || lowered.contains(term));
        if contingent_or_existence {
            return true;
        }

        let unconditional_content_mutation = [
            "写", "续写", "创作", "生成", "创建", "修订", "修改", "修正", "改写", "润色", "补全",
            "完善", "扩写", "write", "draft", "create", "generate", "revise", "rewrite", "edit",
            "polish", "complete", "expand",
        ];
        !unconditional_content_mutation
            .iter()
            .any(|term| query.contains(term) || lowered.contains(term))
    }

    fn tool_result_has_artifact_written_effect(result: &str) -> bool {
        if Self::tool_result_is_read_only_or_advisory(result) {
            return false;
        }
        if Self::tool_result_has_failed_or_blocked_status(result) {
            return false;
        }
        if Self::tool_result_is_process_artifact_only(result) {
            return false;
        }

        if let Some(blob) = Self::tool_result_json_blob(result) {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&blob) {
                if Self::json_value_has_artifact_written_receipt(&value) {
                    return true;
                }
            }
        }

        if Self::tool_result_has_saved_artifact_path(result) {
            return true;
        }

        let lowered = result.to_ascii_lowercase();
        let mentions_written_effect = lowered.contains("runtime_effect: artifact.written")
            || lowered.contains("runtime_effect: artifact.exported")
            || lowered.contains("runtime_effects: artifact.written")
            || lowered.contains("runtime_effects: artifact.exported")
            || (lowered.contains("runtime_effect")
                && (lowered.contains("artifact.written") || lowered.contains("artifact.exported")));
        let has_receipt_shape = lowered.contains("artifact_path")
            || lowered.contains("output_path")
            || lowered.contains("project_path:")
            || lowered.contains("\"project_path\"")
            || lowered.contains("\npath:")
            || lowered.contains(" path:")
            || lowered.contains(" bytes:")
            || lowered.contains("\nbytes:")
            || lowered.contains("successfully wrote");

        mentions_written_effect && has_receipt_shape
    }

    fn tool_result_has_saved_artifact_path(result: &str) -> bool {
        let lowered = result.to_ascii_lowercase();
        let has_saved_signal = result.contains("已保存")
            || result.contains("已写入")
            || result.contains("已导出")
            || lowered.contains("saved")
            || lowered.contains("wrote")
            || lowered.contains("written");
        if !has_saved_signal {
            return false;
        }

        let has_path_label = result.contains("文件：")
            || result.contains("文件:")
            || lowered.contains("artifact_path")
            || lowered.contains("output_path")
            || lowered.contains("\npath:")
            || lowered.contains(" path:");
        let has_artifact_workspace = lowered.contains("/data/generated/")
            || lowered.contains("\\data\\generated\\")
            || lowered.contains("/generated/")
            || lowered.contains("\\generated\\");
        let has_artifact_extension = [
            ".md", ".txt", ".pdf", ".html", ".htm", ".json", ".csv", ".docx",
        ]
        .iter()
        .any(|extension| lowered.contains(extension));

        has_path_label && has_artifact_workspace && has_artifact_extension
    }

    fn tool_result_has_governed_artifact_checkpoint(result: &str) -> bool {
        if Self::tool_result_has_failed_or_blocked_status(result) {
            return false;
        }

        fn inspect(value: &serde_json::Value) -> bool {
            let Some(object) = value.as_object() else {
                return match value {
                    serde_json::Value::Array(items) => items.iter().any(inspect),
                    _ => false,
                };
            };

            let success = object
                .get("success")
                .and_then(|value| value.as_bool())
                .unwrap_or(true);
            if !success {
                return false;
            }

            let has_workspace_anchor = object.contains_key("project_path")
                || object.contains_key("artifact_path")
                || object.contains_key("contract_path")
                || object.contains_key("manifest_path")
                || object.contains_key("output_path");
            let has_governed_state = object
                .get("state")
                .and_then(|state| state.as_object())
                .is_some_and(|state| {
                    state.contains_key("target_units")
                        || state.contains_key("approved_units")
                        || state.contains_key("chapters")
                        || state.contains_key("sections")
                        || state.contains_key("exports")
                });
            let has_continuation_signal = object.contains_key("next_action")
                || object.contains_key("next_actions")
                || object.contains_key("pipeline")
                || object.contains_key("writing_policy")
                || object
                    .get("stage")
                    .and_then(|value| value.as_str())
                    .is_some_and(|stage| {
                        matches!(
                            stage,
                            "source_intake"
                                | "contract"
                                | "planner"
                                | "composer"
                                | "architect"
                                | "writer"
                                | "auditor"
                                | "reviser"
                                | "assigned_worker_policy_packet"
                        )
                    });
            if has_workspace_anchor && has_governed_state && has_continuation_signal {
                return true;
            }

            object.values().any(inspect)
        }

        if let Some(blob) = Self::tool_result_json_blob(result) {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&blob) {
                if inspect(&value) {
                    return true;
                }
            }
        }

        let lowered = result.to_ascii_lowercase();
        (lowered.contains("project_path")
            || lowered.contains("artifact_path")
            || lowered.contains("contract_path"))
            && (lowered.contains("target_units")
                || lowered.contains("approved_units")
                || lowered.contains("chapters")
                || lowered.contains("sections"))
            && (lowered.contains("next_action")
                || lowered.contains("next_actions")
                || lowered.contains("writing_policy")
                || lowered.contains("pipeline"))
    }

    fn tool_result_has_artifact_verified_effect(result: &str) -> bool {
        if Self::tool_result_has_artifact_written_effect(result) {
            return true;
        }
        if Self::tool_result_has_failed_or_blocked_status(result) {
            return false;
        }

        if let Some(blob) = Self::tool_result_json_blob(result) {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&blob) {
                return Self::json_value_has_artifact_verified_receipt(&value);
            }
        }

        let lowered = result.to_ascii_lowercase();
        let mentions_verified_effect = lowered.contains("runtime_effect: artifact.verified")
            || lowered.contains("runtime_effects: artifact.verified")
            || (lowered.contains("runtime_effect") && lowered.contains("artifact.verified"));
        let has_receipt_shape = lowered.contains("artifact_path")
            || lowered.contains("output_path")
            || lowered.contains("project_path")
            || lowered.contains("\npath:")
            || lowered.contains(" path:");

        mentions_verified_effect && has_receipt_shape
    }

    fn tool_result_has_failed_or_blocked_status(result: &str) -> bool {
        let lowered = result.to_ascii_lowercase();
        let compact = lowered.replace([' ', '\n', '\r', '\t'], "");
        if lowered.contains("status: blocked")
            || lowered.contains("status: blocker")
            || lowered.contains("status: failed")
            || lowered.contains("continuous_task_status: failed")
            || compact.contains("\"status\":\"blocker\"")
            || lowered.contains("\"status\":\"blocked\"")
            || lowered.contains("\"status\": \"blocked\"")
            || lowered.contains("\"status\":\"failed\"")
            || lowered.contains("\"status\": \"failed\"")
            || lowered.contains("\"continuous_task_status\":\"failed")
            || lowered.contains("\"continuous_task_status\": \"failed")
        {
            return true;
        }

        false
    }

    fn tool_result_is_process_artifact_only(result: &str) -> bool {
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

    fn tool_result_is_read_only_or_advisory(result: &str) -> bool {
        let lowered = result.to_ascii_lowercase();
        if lowered.contains("\"read_only\":true")
            || lowered.contains("\"read_only\": true")
            || lowered.contains("read_only: true")
        {
            return true;
        }

        if let Some(blob) = Self::tool_result_json_blob(result) {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&blob) {
                if value
                    .get("read_only")
                    .and_then(|read_only| read_only.as_bool())
                    .unwrap_or(false)
                {
                    return true;
                }

                if value.get("next_actions").is_some()
                    && !Self::json_value_has_artifact_written_receipt(&value)
                {
                    return true;
                }
            }
        }

        lowered.contains("\"next_actions\"") && !lowered.contains("\"artifact_path\"")
    }

    fn json_value_has_artifact_written_receipt(value: &serde_json::Value) -> bool {
        let success = value
            .get("success")
            .and_then(|success| success.as_bool())
            .unwrap_or(true);
        if !success {
            return false;
        }

        let runtime_effect_matches = value
            .get("runtime_effect")
            .and_then(|effect| effect.as_str())
            .is_some_and(|effect| matches!(effect, "artifact.written" | "artifact.exported"))
            || value
                .get("runtime_effects")
                .is_some_and(|effects| match effects {
                    serde_json::Value::Array(items) => items.iter().any(|item| {
                        matches!(
                            item.as_str(),
                            Some("artifact.written" | "artifact.exported")
                        )
                    }),
                    serde_json::Value::String(text) => {
                        text.contains("artifact.written") || text.contains("artifact.exported")
                    }
                    _ => false,
                });
        if !runtime_effect_matches {
            return false;
        }

        ["artifact_path", "output_path", "project_path", "path"]
            .iter()
            .any(|key| {
                value
                    .get(*key)
                    .and_then(|path| path.as_str())
                    .is_some_and(|path| !path.trim().is_empty())
            })
            || value
                .get("bytes")
                .and_then(|bytes| bytes.as_u64())
                .is_some_and(|bytes| bytes > 0)
    }

    fn json_value_has_artifact_verified_receipt(value: &serde_json::Value) -> bool {
        let success = value
            .get("success")
            .and_then(|success| success.as_bool())
            .unwrap_or(true);
        if !success {
            return false;
        }

        let runtime_effect_matches = value
            .get("runtime_effect")
            .and_then(|effect| effect.as_str())
            .is_some_and(|effect| {
                matches!(
                    effect,
                    "artifact.verified" | "artifact.written" | "artifact.exported"
                )
            })
            || value
                .get("runtime_effects")
                .is_some_and(|effects| match effects {
                    serde_json::Value::Array(items) => items.iter().any(|item| {
                        matches!(
                            item.as_str(),
                            Some("artifact.verified" | "artifact.written" | "artifact.exported")
                        )
                    }),
                    serde_json::Value::String(text) => {
                        text.contains("artifact.verified")
                            || text.contains("artifact.written")
                            || text.contains("artifact.exported")
                    }
                    _ => false,
                });
        if !runtime_effect_matches {
            return false;
        }

        ["artifact_path", "output_path", "project_path", "path"]
            .iter()
            .any(|key| {
                value
                    .get(*key)
                    .and_then(|path| path.as_str())
                    .is_some_and(|path| !path.trim().is_empty())
            })
    }

    fn requested_artifact_formats(query: &str) -> Vec<&'static str> {
        let lowered = query.to_ascii_lowercase();
        let mut formats = Vec::new();
        if lowered.contains(".pdf") || lowered.contains("pdf") {
            formats.push("pdf");
        }
        if lowered.contains(".txt")
            || lowered.contains("txt")
            || query.contains("纯文本")
            || query.contains("文本文件")
        {
            formats.push("txt");
        }
        if lowered.contains(".md") || lowered.contains("markdown") || query.contains("Markdown") {
            formats.push("md");
        }
        formats.sort_unstable();
        formats.dedup();
        formats
    }

    fn tool_result_matches_requested_artifact_format(query: &str, result: &str) -> bool {
        let formats = Self::requested_artifact_formats(query);
        if formats.is_empty() {
            return true;
        }

        formats
            .iter()
            .any(|format| Self::tool_result_mentions_artifact_format(result, format))
    }

    fn tool_result_mentions_artifact_format(result: &str, format: &str) -> bool {
        if let Some(blob) = Self::tool_result_json_blob(result) {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&blob) {
                if Self::json_value_mentions_artifact_format(&value, format) {
                    return true;
                }
            }
        }

        let lowered = result.to_ascii_lowercase();
        match format {
            "pdf" => {
                lowered.contains(".pdf")
                    || lowered.contains("application/pdf")
                    || lowered.contains("artifact.pdf")
                    || lowered.contains("\"format\":\"pdf\"")
                    || lowered.contains("\"format\": \"pdf\"")
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

    fn json_value_mentions_artifact_format(value: &serde_json::Value, format: &str) -> bool {
        match value {
            serde_json::Value::Object(object) => object.iter().any(|(key, value)| {
                let key = key.as_str();
                if matches!(
                    key,
                    "artifact_path"
                        | "output_path"
                        | "path"
                        | "uri"
                        | "format"
                        | "media_type"
                        | "content_type"
                        | "runtime_effect"
                        | "runtime_effects"
                ) && Self::json_leaf_mentions_artifact_format(value, format)
                {
                    return true;
                }
                Self::json_value_mentions_artifact_format(value, format)
            }),
            serde_json::Value::Array(items) => items
                .iter()
                .any(|item| Self::json_value_mentions_artifact_format(item, format)),
            _ => Self::json_leaf_mentions_artifact_format(value, format),
        }
    }

    fn json_leaf_mentions_artifact_format(value: &serde_json::Value, format: &str) -> bool {
        let Some(text) = value.as_str() else {
            return false;
        };
        let lowered = text.to_ascii_lowercase();
        match format {
            "pdf" => {
                lowered == "pdf"
                    || lowered == "artifact.pdf"
                    || lowered == "application/pdf"
                    || lowered.ends_with(".pdf")
            }
            "txt" => {
                lowered == "txt"
                    || lowered == "artifact.txt"
                    || lowered == "text/plain"
                    || lowered.ends_with(".txt")
            }
            "md" => {
                lowered == "md"
                    || lowered == "markdown"
                    || lowered == "artifact.md"
                    || lowered == "text/markdown"
                    || lowered.ends_with(".md")
            }
            _ => false,
        }
    }

    fn tool_result_satisfies_artifact_request(query: &str, result: &str) -> bool {
        if Self::query_requests_artifact_verification(query)
            && Self::tool_result_has_artifact_verified_effect(result)
        {
            return Self::tool_result_matches_requested_artifact_format(query, result);
        }

        if let Some(target_chars) = Self::requested_text_target_chars(query) {
            let floor = target_chars.saturating_mul(95).div_ceil(100);
            if Self::tool_result_reported_unit_count(result).is_none_or(|units| units < floor) {
                return false;
            }
        }

        (Self::query_requests_artifact_mutation(query) || Self::query_requests_file_artifact(query))
            && Self::tool_result_has_artifact_written_effect(result)
            && Self::tool_result_matches_requested_artifact_format(query, result)
    }

    fn tool_result_is_scaled_artifact_continuation(query: &str, result: &str) -> bool {
        Self::requested_text_target_chars(query).is_some()
            && (Self::tool_result_has_artifact_written_effect(result)
                || Self::tool_result_has_governed_artifact_checkpoint(result))
            && !Self::tool_result_satisfies_artifact_request(query, result)
    }

    fn tool_result_reported_unit_count(result: &str) -> Option<usize> {
        fn collect(value: &serde_json::Value, best: &mut Option<usize>) {
            match value {
                serde_json::Value::Object(map) => {
                    for (key, value) in map {
                        let normalized = key
                            .chars()
                            .filter(|ch| *ch != '_' && *ch != '-' && !ch.is_ascii_whitespace())
                            .collect::<String>()
                            .to_ascii_lowercase();
                        if matches!(
                            normalized.as_str(),
                            "units"
                                | "unitcount"
                                | "totalunits"
                                | "reportedunits"
                                | "charcount"
                                | "chars"
                                | "characters"
                                | "wordcount"
                                | "words"
                                | "bytes"
                        ) {
                            if let Some(number) = value.as_u64().and_then(|number| {
                                usize::try_from(number).ok().filter(|number| *number > 0)
                            }) {
                                *best = Some(best.unwrap_or(0).max(number));
                            }
                        }
                        collect(value, best);
                    }
                }
                serde_json::Value::Array(items) => {
                    for item in items {
                        collect(item, best);
                    }
                }
                _ => {}
            }
        }

        let mut best = None;
        if let Some(blob) = Self::tool_result_json_blob(result) {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&blob) {
                collect(&value, &mut best);
            }
        }

        for line in result.lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let normalized = key
                .chars()
                .filter(|ch| *ch != '_' && *ch != '-' && !ch.is_ascii_whitespace())
                .collect::<String>()
                .to_ascii_lowercase();
            if !matches!(
                normalized.as_str(),
                "units"
                    | "unitcount"
                    | "totalunits"
                    | "reportedunits"
                    | "charcount"
                    | "chars"
                    | "characters"
                    | "wordcount"
                    | "words"
                    | "bytes"
            ) {
                continue;
            }
            let digits = value
                .chars()
                .filter(|ch| ch.is_ascii_digit())
                .collect::<String>();
            if let Ok(number) = digits.parse::<usize>() {
                if number > 0 {
                    best = Some(best.unwrap_or(0).max(number));
                }
            }
        }

        best
    }

    fn tool_result_has_knowledge_persistence_effect(result: &str) -> bool {
        let lowered = result.to_ascii_lowercase();
        if lowered.contains("status: blocked")
            || lowered.contains("status: failed")
            || lowered.contains("status: needs_confirmation")
            || lowered.contains("\"status\":\"blocked\"")
            || lowered.contains("\"status\": \"blocked\"")
            || lowered.contains("\"status\":\"failed\"")
            || lowered.contains("\"status\": \"failed\"")
            || lowered.contains("\"status\":\"needs_confirmation\"")
            || lowered.contains("\"status\": \"needs_confirmation\"")
        {
            return false;
        }
        lowered.contains("runtime_effect: knowledge.")
            || lowered.contains("runtime_effects: knowledge.")
            || lowered.contains("\"runtime_effect\":\"knowledge.")
            || lowered.contains("\"runtime_effect\": \"knowledge.")
            || (lowered.contains("\"runtime_effects\"") && lowered.contains("knowledge."))
            || lowered.contains("imported web knowledge into collection")
            || lowered.contains("knowledge document created")
            || lowered.contains("knowledge document updated")
    }

    fn latest_successful_result_satisfies_execution_request(
        messages: &[Message],
        query: &str,
    ) -> bool {
        let Some((_tool_name, result)) = Self::latest_successful_tool_result(messages) else {
            return false;
        };
        if turn_state::tool_result_is_blocked(&result) {
            return false;
        }

        if Self::tool_result_satisfies_artifact_request(query, &result) {
            return true;
        }

        Self::query_requests_knowledge_persistence(query)
            && Self::tool_result_has_knowledge_persistence_effect(&result)
    }

    fn should_finalize_instead_of_recovering_pseudo_tool(
        messages: &[Message],
        generated_text: &str,
    ) -> bool {
        if !Self::is_pseudo_tool_call_leak(generated_text) {
            return false;
        }
        let Some(query) = Self::latest_user_query(messages) else {
            return false;
        };

        Self::latest_successful_result_satisfies_execution_request(messages, &query)
    }

    fn query_requests_code_artifact(query: &str) -> bool {
        let lowered = query.to_lowercase();
        let code_file_extensions = [
            ".rs", ".py", ".ts", ".tsx", ".js", ".jsx", ".go", ".java", ".cpp", ".c", ".h", ".cs",
            ".php", ".rb", ".swift", ".kt", ".toml",
        ];
        if code_file_extensions
            .iter()
            .any(|extension| Self::query_mentions_code_file_extension(&lowered, extension))
        {
            return true;
        }

        let code_objects = [
            "code",
            "coding",
            "program",
            "script",
            "function",
            "component",
            "repository",
            "crate",
            "rust",
            "python",
            "javascript",
            "typescript",
            "bug",
            "代码",
            "程序",
            "脚本",
            "函数",
            "组件",
            "仓库",
            "源码",
            "模块",
            "接口",
            "漏洞",
            "缺陷",
        ];
        let code_actions = [
            "write",
            "create",
            "generate",
            "update",
            "implement",
            "compile",
            "test",
            "refactor",
            "debug",
            "fix",
            "patch",
            "build",
            "写",
            "创建",
            "生成",
            "更新",
            "实现",
            "编译",
            "测试",
            "调试",
            "修复",
            "重构",
            "构建",
            "补丁",
        ];
        let has_code_object = code_objects
            .iter()
            .any(|marker| lowered.contains(marker) || query.contains(marker));
        let has_code_action = code_actions
            .iter()
            .any(|marker| lowered.contains(marker) || query.contains(marker));

        has_code_object && has_code_action
    }

    fn query_requests_written_artifact(query: &str) -> bool {
        let lowered = query.to_lowercase();
        let writing_markers = [
            "novel",
            "story",
            "fiction",
            "article",
            "paper",
            "essay",
            "report",
            "manuscript",
            "draft",
            "chapter",
            "poem",
            "write-up",
            "writeup",
            "小说",
            "故事",
            "文章",
            "论文",
            "作文",
            "报告",
            "文稿",
            "稿件",
            "正文",
            "章节",
            "剧本",
            "诗",
            "散文",
            "创作",
            "续写",
            "改写",
            "润色",
        ];
        writing_markers
            .iter()
            .any(|marker| lowered.contains(marker) || query.contains(marker))
    }

    fn query_requests_explicit_code_output(query: &str) -> bool {
        let lowered = query.to_lowercase();
        let strong_code_markers = [
            "code",
            "coding",
            "program",
            "script",
            "function",
            "component",
            "compile",
            "refactor",
            "debug",
            "repository",
            "crate",
            "代码",
            "程序",
            "脚本",
            "函数",
            "组件",
            "编译",
            "调试",
            "重构",
            "仓库",
        ];
        strong_code_markers
            .iter()
            .any(|marker| lowered.contains(marker) || query.contains(marker))
            || [
                ".rs", ".py", ".ts", ".tsx", ".js", ".jsx", ".go", ".java", ".cpp", ".c", ".h",
                ".cs", ".php", ".rb", ".swift", ".kt", ".toml",
            ]
            .iter()
            .any(|extension| Self::query_mentions_code_file_extension(&lowered, extension))
    }

    fn query_mentions_code_file_extension(lowered_query: &str, extension: &str) -> bool {
        let mut search_start = 0;
        while let Some(offset) = lowered_query[search_start..].find(extension) {
            let start = search_start + offset;
            let end = start + extension.len();
            let before = lowered_query[..start].chars().next_back();
            let after = lowered_query[end..].chars().next();

            let follows_path_or_name = before
                .is_some_and(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '/'));
            let starts_token = before.is_none_or(|ch| {
                ch.is_whitespace() || matches!(ch, '`' | '"' | '\'' | '(' | '[' | '{' | '<')
            });
            let ends_token = after.is_none_or(|ch| {
                ch.is_whitespace()
                    || matches!(
                        ch,
                        '`' | '"' | '\'' | ')' | ']' | '}' | '>' | ',' | ';' | ':' | '/' | '\\'
                    )
            });

            if ends_token && (follows_path_or_name || starts_token) {
                return true;
            }
            search_start = end;
            if search_start >= lowered_query.len() {
                break;
            }
        }
        false
    }

    fn file_artifact_delegate_role(format_query: &str, task_context: &str) -> &'static str {
        if Self::query_requests_written_artifact(format_query)
            && !Self::query_requests_explicit_code_output(format_query)
        {
            return "writer";
        }
        if Self::query_requests_code_artifact(format_query) {
            return "coder";
        }

        let combined = format!("{format_query}\n{task_context}");
        if Self::query_requests_code_artifact(&combined) {
            "coder"
        } else {
            "writer"
        }
    }

    fn file_artifact_followup_finalize_thought(messages: &[Message], query: &str) -> String {
        if Self::latest_tool_error_result(messages).is_some() {
            return "ORCHESTRATION FINALIZE: file-artifact follow-up returned a runtime blocker; returning that blocker instead of reporting artifact completion.".to_string();
        }

        let artifact_complete = Self::latest_successful_tool_result_text(messages, "delegate")
            .is_some_and(|result| Self::tool_result_satisfies_artifact_request(query, &result));
        if artifact_complete {
            "ORCHESTRATION FINALIZE: knowledge import completed, and the requested file artifact was written before delivery.".to_string()
        } else {
            "ORCHESTRATION FINALIZE: file-artifact follow-up did not satisfy the artifact contract; returning the current blocker/progress instead of reporting completion.".to_string()
        }
    }

    fn push_post_import_delivery_instruction(messages: &mut Vec<Message>, query: &str) {
        messages.push(Message::system(
            "BENSHU_ORCHESTRATION_CHAIN_FINAL_DELIVERY".to_string(),
        ));
        let artifact_worker = Self::file_artifact_delegate_role(query, query);
        let artifact_instruction = if post_import_delivery::query_requests_file_artifact(query) {
            format!(" The original request also asks for a local file artifact. You must delegate to the `{artifact_worker}` worker to create the requested artifact with the available file/artifact tool before finalizing. For substantial artifacts, require a generic artifact quality contract: minimum structure, evidence/citation grounding when applicable, sufficient depth, and self-review/revision notes. For oversized artifacts, use the generic checkpointed continuation flow: initialize a document identity block with a self-chosen title when the user did not provide one, then keep appending bounded checkpoints toward the requested size. Do not report completion until the artifact contract or a real runtime blocker says so.")
        } else {
            String::new()
        };
        messages.push(Message::system(format!(
            "The knowledge import step is complete, but the original user request also requires a final user-facing delivery. Use the existing researcher result and knowledge import receipt already in this conversation to answer the remaining request. Do not call another import. Do not invent missing data. If the fetched source does not contain enough structured records for the requested analysis/prediction, say exactly what is missing and provide a safe next action.{artifact_instruction} Original user request: {query}"
        )));
    }

    fn synthesize_post_import_delivery(query: &str, messages: &[Message]) -> Option<String> {
        post_import_delivery::synthesize_post_import_delivery(
            query,
            messages,
            Self::query_prefers_chinese(query),
        )
    }

    fn latest_loop_guard_abort_for_tool(messages: &[Message], tool_name: &str) -> bool {
        turn_state::latest_loop_guard_abort_for_tool(messages, tool_name)
    }

    fn latest_loop_guard_reuse_for_tool(messages: &[Message], tool_name: &str) -> bool {
        turn_state::latest_loop_guard_reuse_for_tool(messages, tool_name)
    }

    fn latest_loop_guard_reuse_tool_name(messages: &[Message]) -> Option<String> {
        turn_state::latest_loop_guard_reuse_tool_name(messages)
    }

    fn latest_runtime_tool_error_for_tool(messages: &[Message], tool_name: &str) -> Option<String> {
        turn_state::latest_runtime_tool_error_for_tool(messages, tool_name)
    }

    fn current_turn_messages(messages: &[Message]) -> &[Message] {
        turn_state::current_turn_messages(messages)
    }

    fn lookup_loop_guard_failure_message(query: &str) -> String {
        let persistence_note = if Self::query_requests_knowledge_persistence(query) {
            "因此我没有把不可靠或不完整的网页结果写入知识库。"
        } else {
            "因此我没有继续重复搜索。"
        };
        let next_step = if Self::query_requests_knowledge_persistence(query) {
            "你可以稍后重试，或者给我一个明确的论文 URL，我可以直接读取并保存进知识库。"
        } else {
            "你可以稍后重试，或者给我一个明确网页/来源 URL，我可以直接读取并基于来源回答。"
        };

        format!(
            "这次外部检索没有稳定完成：浏览器/搜索链路连续失败，系统已触发循环保护，停止继续重复搜索。{persistence_note}\n\n\
             {next_step}"
        )
    }

    fn tool_discovery_loop_guard_failure_message(query: &str) -> String {
        let persistence_note = if Self::query_requests_knowledge_persistence(query) {
            "因为还没有拿到可验证的来源 URL，我不会把不确定内容写入知识库。"
        } else {
            "我不会继续重复查找同一组工具。"
        };
        let next_step = if Self::query_requests_knowledge_persistence(query) {
            "你可以稍后重试，或者直接给我一个明确网页/论文 URL，我可以继续读取并保存。"
        } else {
            "你可以稍后重试，或者直接给我一个明确网页/来源 URL，我可以继续读取并回答。"
        };

        format!(
            "这次任务没有稳定推进：主脑反复查找同一组工具，系统已触发循环保护并停止空转。{persistence_note}\n\n\
             {next_step}"
        )
    }

    fn latest_successful_tool_result_text(messages: &[Message], tool_name: &str) -> Option<String> {
        turn_state::latest_successful_tool_result_text(messages, tool_name)
    }

    fn latest_blocked_tool_result(messages: &[Message]) -> Option<(String, String)> {
        turn_state::latest_blocked_tool_result(messages)
    }

    fn tool_result_content_is_runtime_error(content: &str) -> bool {
        turn_state::tool_result_content_is_runtime_error(content)
    }

    fn tool_result_is_blocked(result: &str) -> bool {
        turn_state::tool_result_is_blocked(result)
    }

    fn latest_successful_tool_name(messages: &[Message]) -> Option<String> {
        turn_state::latest_successful_tool_name(messages)
    }

    fn latest_successful_tool_result_for_names(
        messages: &[Message],
        tool_names: &[&str],
    ) -> Option<(String, String)> {
        turn_state::latest_successful_tool_result_for_names(messages, tool_names)
    }

    fn context_only_tool_name_for_artifact_completion(tool_name: &str) -> bool {
        matches!(
            tool_name,
            "shared_board"
                | "search_history"
                | "manage_facts"
                | "knowledge_search"
                | "read_file"
                | "list_dir"
                | "tool_search"
        )
    }

    fn recent_context_only_artifact_progress_stalled(messages: &[Message], query: &str) -> bool {
        if Self::query_requests_existing_artifact_read_only_answer(query) {
            return false;
        }
        if !(Self::query_requests_artifact_mutation(query)
            || Self::query_requests_file_artifact(query))
        {
            return false;
        }
        if Self::latest_successful_result_satisfies_execution_request(messages, query) {
            return false;
        }

        let mut context_only_count = 0;
        for message in turn_state::current_turn_messages(messages).iter().rev() {
            if !matches!(message.role, Role::Tool) {
                continue;
            }
            if message
                .metadata
                .get("tool_error")
                .is_some_and(|value| value == "true")
            {
                continue;
            }
            let Some(tool_name) = message.metadata.get("tool_name") else {
                continue;
            };
            if Self::tool_result_satisfies_artifact_request(query, &message.text()) {
                return false;
            }
            if Self::context_only_tool_name_for_artifact_completion(tool_name) {
                context_only_count += 1;
                if context_only_count >= 2 {
                    return true;
                }
            } else {
                return false;
            }
        }
        false
    }

    fn latest_successful_durable_effect_tool_result(
        messages: &[Message],
    ) -> Option<(String, String)> {
        turn_state::latest_successful_durable_effect_tool_result(messages)
    }

    fn latest_tool_error_result(messages: &[Message]) -> Option<(String, String)> {
        turn_state::latest_tool_error_result(messages)
    }

    fn tool_failure_delivery_text(query: &str, tool_name: &str, error: &str) -> String {
        let compact_error = Self::compact_tool_result_for_recovery(error);
        fallback_text::tool_failure_delivery_text(
            tool_name,
            &compact_error,
            Self::query_prefers_chinese(query),
            Self::query_requests_knowledge_persistence(query),
        )
    }

    fn tool_error_is_not_equipped(error: &str) -> bool {
        let lowered = error.to_ascii_lowercase();
        lowered.contains("tool is not equipped")
            || lowered.contains("tool not equipped")
            || lowered.contains("tool_not_equipped")
    }

    fn tool_error_is_loop_prevention(error: &str) -> bool {
        let lowered = error.to_ascii_lowercase();
        lowered.contains("loop prevention triggered")
            || lowered.contains("plan stagnation")
            || lowered.contains("called 4 times")
    }

    fn tool_error_is_recoverable_contract(error: &str) -> bool {
        let lowered = error.to_ascii_lowercase();
        if Self::structured_tool_observation_not_found(&lowered) {
            return false;
        }
        lowered.contains("\"success\":false")
            || lowered.contains("\"success\": false")
            || lowered.contains("missing_required")
            || lowered.contains("missing required")
            || lowered.contains(" is required")
            || lowered.contains(" required for ")
            || lowered.contains("bare tool invocation")
            || lowered.contains("not file content; call that tool directly")
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

    fn available_tools_from_not_equipped_error(error: &str) -> Option<String> {
        if let Some((_, tail)) = error.split_once("available_tools:") {
            let tools = tail
                .split(['\n', '\r'])
                .next()
                .unwrap_or_default()
                .trim()
                .trim_matches('`')
                .trim();
            if !tools.is_empty() {
                return Some(tools.to_string());
            }
        }
        let marker = "Available tools right now:";
        let (_, tail) = error.split_once(marker)?;
        let tools = tail
            .split(['.', '\n', '\r'])
            .next()
            .unwrap_or_default()
            .trim()
            .trim_matches('`')
            .trim();
        if tools.is_empty() {
            None
        } else {
            Some(tools.to_string())
        }
    }

    fn delegate_worker_tool_boundary_error(error: &str) -> bool {
        Self::tool_error_is_not_equipped(error)
            && error.contains("worker:")
            && (error.contains("runtime_error_preview:")
                || error.contains("delegated worker hit a tool boundary"))
    }

    fn delegate_worker_role_from_error(error: &str) -> Option<String> {
        error.lines().find_map(|line| {
            let trimmed = line.trim();
            let role = trimmed.strip_prefix("worker:")?.trim();
            (!role.is_empty()).then_some(role.to_string())
        })
    }

    fn delegate_blocker_is_recoverable_workspace_boundary(result: &str) -> bool {
        let lowered = result.to_ascii_lowercase();
        Self::tool_result_is_blocked(result)
            && (lowered.contains("outside the current benshu workspace")
                || lowered.contains("outside authorized workspaces")
                || lowered.contains("workspace_root:"))
            && (lowered.contains("executed_tool: read_file")
                || lowered.contains("executed_tool: write_file")
                || lowered.contains("executed_tool: filesystem")
                || lowered.contains("path:"))
    }

    fn delegate_phase_boundary_suggested_role(result: &str) -> Option<String> {
        if !Self::tool_result_is_blocked(result) {
            return None;
        }
        let lowered = result.to_ascii_lowercase();
        if !lowered.contains("error_kind: phase_boundary")
            && !lowered.contains("\"error_kind\":\"phase_boundary\"")
            && !lowered.contains("\"error_kind\": \"phase_boundary\"")
        {
            return None;
        }

        result.lines().find_map(|line| {
            let role = line.trim().strip_prefix("suggested_role:")?.trim();
            (!role.is_empty()).then_some(role.trim_matches('`').to_string())
        })
    }

    fn workspace_boundary_recovery_prompt(result: &str) -> String {
        let workspace_root = result
            .lines()
            .find_map(|line| line.trim().strip_prefix("workspace_root:"))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| {
                std::env::current_dir()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|_| "the current BenShu workspace".to_string())
            });
        let compact = Self::compact_tool_result_for_recovery(result);
        format!(
            "BENSHU_WORKSPACE_BOUNDARY_RECOVERY\n\n\
             A delegated worker hit a local path boundary before completing the task.\n\
             Continue the same user request, but retry with paths inside this workspace root only: {workspace_root}.\n\
             Do not treat hidden sibling directories, near-miss path names, or diagnostic examples as the correct target.\n\
             If the requested artifact/project cannot be located inside the workspace, report a compact blocker instead of using an outside path.\n\n\
             Previous blocker:\n{compact}"
        )
    }

    fn tool_boundary_recovery_prompt(
        failed_tool_name: &str,
        error: &str,
        fallback_available_tools: &[String],
    ) -> String {
        let available = Self::available_tools_from_not_equipped_error(error).unwrap_or_else(|| {
            if fallback_available_tools.is_empty() {
                "none".to_string()
            } else {
                fallback_available_tools.join(", ")
            }
        });
        format!(
            "BENSHU_TOOL_BOUNDARY_RECOVERY\n\n\
             The previous tool call `{failed_tool_name}` was rejected because that tool is not equipped for this agent.\n\
             Do not call `{failed_tool_name}` again in this turn unless it appears in the available-tool list below.\n\
             Continue the same task using only these currently available tools: {available}.\n\
             If the available tools cannot complete the task, return a compact blocker with `status`, `result`, `source_urls`, and `blockers` instead of calling unavailable orchestration or worker-management tools."
        )
    }

    fn tool_contract_recovery_prompt(
        failed_tool_name: &str,
        error: &str,
        fallback_available_tools: &[String],
    ) -> String {
        let available = if fallback_available_tools.is_empty() {
            "the currently available tools".to_string()
        } else {
            fallback_available_tools.join(", ")
        };
        let compact_error = Self::compact_tool_result_for_recovery(error);
        let artifact_context = Self::artifact_recovery_context_block(error);
        format!(
            "BENSHU_TOOL_CONTRACT_RECOVERY\n\n\
             The previous `{failed_tool_name}` call returned a structured tool contract or argument error, not a completed result.\n\
             Do not summarize that error as success and do not finalize from it.\n\
             Continue the same task using {available}. If the tool result includes `example_shape`, `missing_required`, or `next_step_hint`, fill the required arguments and call the appropriate tool again.\n\
             If a writable artifact requires `content`, generate the actual body/content first and include it in the tool arguments; when content exists only as a URL, knowledge receipt, imported document path, or local path, call an equipped retrieval/read tool directly first, then pass the returned body or excerpt to the owning artifact tool.\n\
             If the actual body is too long or awkward to place inside a tool-call JSON, output only the body text as the next assistant message; the runtime can attach that body to the pending content-required tool call. Do not output a plan, status note, or another empty content call in that case.\n\
             Tool names are top-level calls, never values for another tool's `action` field or file `content`. If the previous result says `wrong_tool_action` or `bare tool invocation`, do not write the tool call text into a file and do not put another tool's name in the failed tool's `action` field; call that separate equipped tool directly, then pass its returned content/path back to the owning artifact tool.\n\
             Do not call write/edit/file tools to record recovery notes, progress reports, status reports, execution logs, or blocker notes. Process notes may be useful, but they are not completion evidence for a write/update/export request.\n\
             If the previous result includes existing artifact/project identifiers, reuse those identifiers and prefer revise/update/export actions over init/create/new actions.\n\
             If the task truly cannot be completed with the available tools, return a compact blocker instead of treating the contract error as completion.\n\n\
             Previous tool result:\n{compact_error}{artifact_context}"
        )
    }

    fn contract_error_json_value(error: &str) -> Option<serde_json::Value> {
        if let Some(value) = Self::parse_tool_result_json(error) {
            return Some(value);
        }
        let trimmed = error.trim();
        let start = trimmed.find('{')?;
        let end = trimmed.rfind('}')?;
        if end <= start {
            return None;
        }
        serde_json::from_str::<serde_json::Value>(&trimmed[start..=end]).ok()
    }

    fn contract_error_requires_content(value: &serde_json::Value) -> bool {
        let error_kind_requires_content = value
            .get("error_kind")
            .and_then(|value| value.as_str())
            .map(|kind| kind == "missing_required_content")
            .unwrap_or(false);
        let required_mentions_content = ["required_fields", "missing_required", "requires"]
            .iter()
            .any(|key| {
                value
                    .get(*key)
                    .and_then(|value| value.as_array())
                    .map(|fields| {
                        fields.iter().any(|field| {
                            field
                                .as_str()
                                .map(|field| field.to_ascii_lowercase().contains("content"))
                                .unwrap_or(false)
                        })
                    })
                    .unwrap_or(false)
            });
        let example_mentions_content = value
            .get("example_shape")
            .and_then(|value| value.as_object())
            .map(|object| object.contains_key("content"))
            .unwrap_or(false);
        error_kind_requires_content || required_mentions_content || example_mentions_content
    }

    fn contract_error_content_repair_args(error: &str) -> Option<serde_json::Value> {
        let value = Self::contract_error_json_value(error)?;
        if !Self::contract_error_requires_content(&value) {
            return None;
        }
        let mut args = value
            .get("example_shape")
            .cloned()
            .and_then(|value| value.as_object().cloned())
            .unwrap_or_default();
        if !args.contains_key("action") {
            if let Some(action) = value.get("action").and_then(|value| value.as_str()) {
                args.insert(
                    "action".to_string(),
                    serde_json::Value::String(action.to_string()),
                );
            }
        }
        if args.is_empty() {
            return None;
        }
        Some(serde_json::Value::Object(args))
    }

    fn generated_content_repair_text_is_substantive(text: &str) -> bool {
        let trimmed = text.trim();
        if trimmed.chars().count() < 160 {
            return false;
        }
        if Self::is_pseudo_tool_call_leak(trimmed)
            || Self::tool_result_content_is_runtime_error(trimmed)
        {
            return false;
        }
        let lowered = trimmed.to_ascii_lowercase();
        let process_markers = [
            "status:",
            "blockers:",
            "missing_required_content",
            "next_step_hint",
            "example_shape",
            "tool contract",
            "工具合同",
            "恢复提示",
            "我将调用",
            "我需要调用",
            "接下来调用",
            "下面是计划",
            "进度报告",
            "状态报告",
        ];
        if process_markers
            .iter()
            .any(|marker| lowered.contains(&marker.to_ascii_lowercase()))
        {
            return false;
        }
        true
    }

    fn content_required_tool_call_from_generated_text(
        messages: &[Message],
        generated_text: &str,
    ) -> Option<(String, String, serde_json::Value)> {
        if !Self::generated_content_repair_text_is_substantive(generated_text) {
            return None;
        }
        let (tool_name, error) = Self::latest_tool_error_result(messages)?;
        if matches!(
            tool_name.as_str(),
            "delegate" | "handover" | "multi_agent_audit" | "tool_search"
        ) {
            return None;
        }
        let mut args = Self::contract_error_content_repair_args(&error)?;
        let object = args.as_object_mut()?;
        object.insert(
            "content".to_string(),
            serde_json::Value::String(generated_text.trim().to_string()),
        );
        Some((
            "content-required-contract-repair".to_string(),
            tool_name,
            args,
        ))
    }

    fn pending_content_action_tool_call_from_generated_text(
        messages: &[Message],
        generated_text: &str,
    ) -> Option<(String, String, serde_json::Value)> {
        if !Self::generated_content_repair_text_is_substantive(generated_text) {
            return None;
        }
        let (fallback_tool_name, result) = Self::latest_successful_tool_result(messages)?;
        let value = Self::parse_tool_result_json(&result).or_else(|| {
            Self::tool_result_json_blob(&result)
                .and_then(|blob| serde_json::from_str::<serde_json::Value>(blob).ok())
        })?;
        let pending = value.get("pending_content_action")?.as_object()?;
        let tool_name = pending
            .get("tool")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(fallback_tool_name.as_str());
        if !Self::tool_name_is_safe_pending_content_target(tool_name) {
            return None;
        }
        let content_field = pending
            .get("content_field")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("content");
        if !Self::pending_content_field_is_safe(content_field) {
            return None;
        }
        let mut args = pending.get("args")?.as_object()?.clone();
        args.insert(
            content_field.to_string(),
            serde_json::Value::String(generated_text.trim().to_string()),
        );
        let marker = Self::pending_content_action_marker(tool_name, &args);
        Some((
            marker,
            tool_name.to_string(),
            serde_json::Value::Object(args),
        ))
    }

    fn latest_successful_tool_result_has_pending_content_action(messages: &[Message]) -> bool {
        let Some((_, result)) = Self::latest_successful_tool_result(messages) else {
            return false;
        };
        let value = Self::parse_tool_result_json(&result).or_else(|| {
            Self::tool_result_json_blob(&result)
                .and_then(|blob| serde_json::from_str::<serde_json::Value>(blob).ok())
        });
        value.is_some_and(|value| value.get("pending_content_action").is_some())
    }

    fn latest_tool_error_requires_generated_content(messages: &[Message]) -> bool {
        let Some((_, error)) = Self::latest_tool_error_result(messages) else {
            return false;
        };
        let Some(value) = Self::contract_error_json_value(&error) else {
            return false;
        };
        Self::contract_error_requires_content(&value)
    }

    fn turn_requires_generated_artifact_content(messages: &[Message]) -> bool {
        Self::latest_successful_tool_result_has_pending_content_action(messages)
            || Self::latest_tool_error_requires_generated_content(messages)
    }

    fn tool_name_is_safe_pending_content_target(tool_name: &str) -> bool {
        tool_name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
            && !matches!(
                tool_name,
                "delegate" | "handover" | "multi_agent_audit" | "tool_search"
            )
    }

    fn pending_content_field_is_safe(field: &str) -> bool {
        field
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    }

    fn pending_content_action_marker(
        tool_name: &str,
        args: &serde_json::Map<String, serde_json::Value>,
    ) -> String {
        let identity = format!(
            "tool={tool_name};action={};project={};document={};chapter={};section={}",
            args.get("action")
                .and_then(|value| value.as_str())
                .unwrap_or_default(),
            args.get("project_path")
                .and_then(|value| value.as_str())
                .unwrap_or_default(),
            args.get("document_path")
                .and_then(|value| value.as_str())
                .unwrap_or_default(),
            args.get("chapter_number")
                .map(|value| value.to_string())
                .unwrap_or_default(),
            args.get("section_id")
                .and_then(|value| value.as_str())
                .unwrap_or_default(),
        );
        let mut hash = 0xcbf29ce484222325u64;
        for byte in identity.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        format!("BENSHU_PENDING_CONTENT_ACTION:{hash:016x}")
    }

    fn declared_next_action_tool_call_from_result(
        tool_name: &str,
        result: &str,
    ) -> Option<(String, String, serde_json::Value)> {
        if matches!(
            tool_name,
            "delegate" | "handover" | "multi_agent_audit" | "tool_search"
        ) {
            return None;
        }
        let value = Self::parse_tool_result_json(result).or_else(|| {
            Self::tool_result_json_blob(result)
                .and_then(|blob| serde_json::from_str::<serde_json::Value>(blob).ok())
        })?;
        let object = value.as_object()?;
        let action = object
            .get("next_action")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|action| Self::declared_next_action_is_executable(action))?;
        let mut args = serde_json::Map::new();
        args.insert(
            "action".to_string(),
            serde_json::Value::String(action.to_string()),
        );
        for key in [
            "project_path",
            "document_path",
            "artifact_path",
            "output_path",
            "chapter_number",
            "chapter_title",
            "section_id",
            "section_title",
            "format",
            "output",
            "approved_only",
            "target_units",
            "chapter_unit_target",
        ] {
            if let Some(value) = object.get(key).cloned() {
                args.insert(key.to_string(), value);
            }
        }
        if args.len() <= 1 {
            return None;
        }
        let marker = Self::declared_next_action_marker(tool_name, action, &args);
        Some((
            marker,
            tool_name.to_string(),
            serde_json::Value::Object(args),
        ))
    }

    fn declared_next_action_is_executable(action: &str) -> bool {
        let trimmed = action.trim();
        if trimmed.is_empty()
            || trimmed.len() > 64
            || trimmed.contains(',')
            || trimmed.contains('/')
            || trimmed.contains('|')
            || trimmed.contains(" or ")
            || trimmed.contains("或")
        {
            return false;
        }
        let lowered = trimmed.to_ascii_lowercase();
        if matches!(
            lowered.as_str(),
            "none"
                | "status"
                | "list"
                | "list_projects"
                | "export_or_status"
                | "done"
                | "complete"
                | "completed"
        ) {
            return false;
        }
        trimmed
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    }

    fn declared_next_action_marker(
        tool_name: &str,
        action: &str,
        args: &serde_json::Map<String, serde_json::Value>,
    ) -> String {
        let identity = format!(
            "tool={tool_name};action={action};project={};document={};chapter={};section={}",
            args.get("project_path")
                .and_then(|value| value.as_str())
                .unwrap_or_default(),
            args.get("document_path")
                .and_then(|value| value.as_str())
                .unwrap_or_default(),
            args.get("chapter_number")
                .map(|value| value.to_string())
                .unwrap_or_default(),
            args.get("section_id")
                .and_then(|value| value.as_str())
                .unwrap_or_default(),
        );
        let mut hash = 0xcbf29ce484222325u64;
        for byte in identity.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        format!("BENSHU_DECLARED_NEXT_ACTION_CONTINUATION:{hash:016x}")
    }

    fn tool_contract_recovery_marker(prefix: &str, failed_tool_name: &str, error: &str) -> String {
        let identity = Self::tool_contract_recovery_identity(failed_tool_name, error);
        let mut hash = 0xcbf29ce484222325u64;
        for byte in identity.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        format!("{prefix}:{hash:016x}")
    }

    fn tool_contract_recovery_identity(failed_tool_name: &str, error: &str) -> String {
        let mut parts = vec![format!("tool={failed_tool_name}")];
        for field in [
            "error_kind",
            "action",
            "attempted_action",
            "missing_required",
        ] {
            if let Some(value) = Self::jsonish_field_value(error, field) {
                parts.push(format!("{field}={value}"));
            }
        }
        if parts.len() > 1 {
            return parts.join("|").to_ascii_lowercase();
        }

        let fallback = Self::compact_tool_result_for_recovery(error);
        parts.push(fallback.chars().take(360).collect());
        parts.join("|").to_ascii_lowercase()
    }

    fn jsonish_field_value(text: &str, field: &str) -> Option<String> {
        let quoted = format!("\"{field}\"");
        let start = text.find(&quoted).map(|index| index + quoted.len())?;
        let after_field = &text[start..];
        let colon = after_field.find(':')?;
        let mut rest = after_field[colon + 1..].trim_start();
        if rest.starts_with('"') {
            rest = &rest[1..];
            let end = rest.find('"')?;
            return Some(rest[..end].trim().to_string()).filter(|value| !value.is_empty());
        }
        if rest.starts_with('[') {
            let end = rest.find(']')?;
            return Some(rest[..=end].trim().to_string()).filter(|value| !value.is_empty());
        }
        let value = rest
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric() || matches!(*ch, '_' | '-' | '.'))
            .collect::<String>();
        (!value.is_empty()).then_some(value)
    }

    fn loop_guard_recovery_prompt(
        failed_tool_name: &str,
        error: &str,
        fallback_available_tools: &[String],
    ) -> Option<String> {
        let alternatives = fallback_available_tools
            .iter()
            .filter(|tool| tool.as_str() != failed_tool_name)
            .cloned()
            .collect::<Vec<_>>();
        if alternatives.is_empty() {
            return None;
        }
        let compact_error = Self::compact_tool_result_for_recovery(error);
        Some(format!(
            "BENSHU_LOOP_GUARD_RECOVERY\n\n\
             The previous `{failed_tool_name}` call was blocked by the runtime loop guard because repeated calls were no longer producing progress.\n\
             Do not call `{failed_tool_name}` again in this turn. Continue the same task using one of these alternative currently available tools: {}.\n\
             Use prior concrete candidate URLs or page clues when available; otherwise return a compact blocker with `status`, `result`, `source_urls`, and `blockers` instead of starting another same-tool search loop.\n\n\
             Runtime guard detail:\n{}",
            alternatives.join(", "),
            compact_error
        ))
    }

    fn should_retry_tool_boundary_recovery(&self, failed_tool_name: &str, error: &str) -> bool {
        Self::tool_error_is_not_equipped(error) && !self.tool_is_enabled(failed_tool_name)
    }

    fn latest_delegate_role(messages: &[Message]) -> Option<String> {
        turn_state::latest_delegate_role(messages)
    }

    fn has_system_marker(messages: &[Message], marker: &str) -> bool {
        turn_state::has_system_marker(messages, marker)
    }

    fn has_system_marker_after_latest_user(messages: &[Message], marker: &str) -> bool {
        turn_state::has_system_marker_after_latest_user(messages, marker)
    }

    fn has_system_marker_after_latest_successful_tool_result(
        messages: &[Message],
        tool_name: &str,
        marker: &str,
    ) -> bool {
        let mut marker_seen = false;
        for message in Self::current_turn_messages(messages).iter().rev() {
            if matches!(message.role, Role::System) && message.text().contains(marker) {
                marker_seen = true;
                continue;
            }
            if !matches!(message.role, Role::Tool) {
                continue;
            }
            if message
                .metadata
                .get("tool_name")
                .is_some_and(|name| name == tool_name)
                && !message
                    .metadata
                    .get("tool_error")
                    .is_some_and(|value| value == "true")
            {
                return marker_seen;
            }
        }
        false
    }

    fn text_contains_url(text: &str) -> bool {
        source_selection::text_contains_url(text)
    }

    fn compact_tool_result_for_recovery(result: &str) -> String {
        const LIMIT: usize = 4_000;
        benshu_compression::head_tail_with_notice(
            result,
            LIMIT,
            benshu_compression::TruncationNotice::RepeatedSpecialistResult,
        )
        .content
    }

    fn explicit_source_url_in_result(result: &str) -> Option<String> {
        source_selection::explicit_source_url_in_result(result)
    }

    fn extract_knowledge_import_summary(result: &str) -> Option<String> {
        post_import_delivery::extract_knowledge_import_summary(result)
    }

    fn latest_lookup_result_for_followup_execution(messages: &[Message]) -> Option<String> {
        let delegate_lookup = turn_state::current_turn_messages(messages)
            .iter()
            .rev()
            .find_map(|message| {
                if !matches!(message.role, Role::Tool) {
                    return None;
                }
                if !message
                    .metadata
                    .get("tool_name")
                    .is_some_and(|name| name == "delegate")
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

                let result = message.text();
                if turn_state::tool_result_is_blocked(&result) {
                    return None;
                }
                let lowered = result.to_ascii_lowercase();
                if lowered.contains("worker: researcher")
                    && (lowered.contains("executed_tool: web_search")
                        || lowered.contains("executed_tool: web_fetch")
                        || lowered.contains("executed_tool: browser_browse"))
                    && Self::text_contains_url(&result)
                {
                    Some(result)
                } else if Self::delegated_lookup_result_envelope_contains_tool_evidence(&result) {
                    Some(result)
                } else if lowered.contains("worker: browser")
                    && lowered.contains("executed_tool: browser_browse")
                    && (Self::text_contains_url(&result)
                        || !knowledge_delivery::ranked_metadata_items_from_result(&result)
                            .is_empty())
                {
                    Some(result)
                } else {
                    None
                }
            });
        if delegate_lookup.is_some() {
            return delegate_lookup;
        }

        Self::latest_successful_tool_result_for_names(messages, &["web_search", "web_fetch"])
            .and_then(|(_, result)| {
                (!turn_state::tool_result_is_blocked(&result) && Self::text_contains_url(&result))
                    .then_some(result)
            })
    }

    fn delegated_lookup_result_envelope_contains_tool_evidence(result: &str) -> bool {
        if turn_state::tool_result_is_blocked(result) || !Self::text_contains_url(result) {
            return false;
        }
        let lowered = result.to_ascii_lowercase();
        let mentions_lookup_tool = lowered.contains("web_search")
            || lowered.contains("web_fetch")
            || lowered.contains("browser_browse");
        let has_tool_result_shape = lowered.contains("\"url\"")
            || lowered.contains("source_url:")
            || lowered.contains("工具 `")
            || lowered.contains("tool `")
            || lowered.contains("tool result")
            || lowered.contains("结果如下");

        mentions_lookup_tool && has_tool_result_shape
    }

    fn compact_lookup_evidence_for_file_artifact(result: &str) -> String {
        let mut kept = Vec::new();
        let mut in_result_summary = false;
        for line in result.lines() {
            let trimmed = line.trim_end();
            if trimmed.starts_with("fetched_result:") {
                break;
            }
            if trimmed.starts_with("result_summary:") {
                in_result_summary = true;
                kept.push(trimmed.to_string());
                continue;
            }
            if in_result_summary
                || trimmed.starts_with("status:")
                || trimmed.starts_with("worker:")
                || trimmed.starts_with("executed_tool:")
                || trimmed.starts_with("source_url:")
                || trimmed.starts_with("search_query:")
            {
                kept.push(trimmed.to_string());
            }
        }

        let compact = if kept.is_empty() {
            result.lines().take(80).collect::<Vec<_>>().join("\n")
        } else {
            kept.join("\n")
        };
        compact.replace('\\', "/")
    }

    fn compact_utf8_for_tool_arg(text: &str, max_bytes: usize) -> String {
        if text.len() <= max_bytes {
            return text.to_string();
        }

        let mut boundary = 0;
        for (idx, ch) in text.char_indices() {
            let next = idx + ch.len_utf8();
            if next > max_bytes {
                break;
            }
            boundary = next;
        }

        let original_bytes = text.len();
        let mut compact = text[..boundary].trim_end().to_string();
        compact.push_str(&format!(
            "\n[truncated_for_tool_arg: original_bytes={original_bytes}, shown_bytes={boundary}]"
        ));
        compact
    }

    fn compact_lookup_evidence_for_knowledge_import(result: &str) -> String {
        let surface = Self::lookup_result_source_body_surface(result);
        let compact = benshu_compression::head_tail_with_notice(
            &surface,
            1_200,
            benshu_compression::TruncationNotice::ContextSafety,
        )
        .content;
        Self::compact_utf8_for_tool_arg(&compact, Self::KNOWLEDGE_IMPORT_EVIDENCE_ARG_MAX_BYTES)
    }

    fn compact_lookup_summary_for_knowledge_import(result: &str) -> String {
        let summary = Self::compact_lookup_evidence_for_file_artifact(result);
        Self::compact_utf8_for_tool_arg(&summary, Self::KNOWLEDGE_IMPORT_SUMMARY_ARG_MAX_BYTES)
    }

    fn compact_user_request_for_tool_arg(query: &str) -> String {
        Self::compact_utf8_for_tool_arg(query.trim(), Self::KNOWLEDGE_IMPORT_QUERY_ARG_MAX_BYTES)
    }

    fn query_requests_source_content_for_knowledge(query: &str) -> bool {
        let lowered = query.to_ascii_lowercase();
        let asks_for_content_depth = [
            "正文",
            "全文",
            "原文",
            "内容",
            "下载",
            "可下载",
            "download",
            "full text",
            "source text",
            "source content",
            "document body",
            "article body",
        ]
        .iter()
        .any(|term| lowered.contains(&term.to_ascii_lowercase()) || query.contains(term));

        Self::query_requests_knowledge_persistence(query) && asks_for_content_depth
    }

    fn query_accepts_structured_material_surrogate_for_knowledge(query: &str) -> bool {
        if !Self::query_requests_knowledge_persistence(query) {
            return false;
        }
        let lowered = query.to_ascii_lowercase();
        [
            "摘要也可以",
            "摘要即可",
            "摘要就行",
            "元数据也可以",
            "公开元数据也可以",
            "列表也可以",
            "书单也可以",
            "只要摘要",
            "只存摘要",
            "summary is ok",
            "summaries are ok",
            "metadata is ok",
            "metadata is acceptable",
            "list is acceptable",
            "catalog is acceptable",
        ]
        .iter()
        .any(|term| lowered.contains(&term.to_ascii_lowercase()) || query.contains(term))
    }

    fn lookup_result_has_structured_material_surrogate(result: &str) -> bool {
        let lowered = result.to_ascii_lowercase();
        let has_material_scope = lowered.contains("evidence_scope:")
            || lowered.contains("result_summary:")
            || lowered.contains("observed_item_records:")
            || lowered.contains("public metadata")
            || lowered.contains("summary:");
        let has_item_or_source = !knowledge_delivery::ranked_metadata_items_from_result(result)
            .is_empty()
            || lowered.contains("source_url:")
            || lowered.contains("plain text:")
            || lowered.contains("source:");
        let is_bare_index =
            Self::lookup_result_looks_like_collection_index_not_source_content(result)
                && !lowered.contains("result_summary:")
                && knowledge_delivery::ranked_metadata_items_from_result(result).is_empty();

        has_material_scope && has_item_or_source && !is_bare_index
    }

    fn lookup_result_is_metadata_surrogate_not_source_content(result: &str) -> bool {
        let lowered = result.to_ascii_lowercase();
        lowered.contains("evidence_scope: public_metadata_surrogate_not_full_source_content")
            || lowered.contains("public metadata surrogate")
            || lowered.contains("public metadata only")
            || lowered.contains("full source content was not imported")
            || lowered.contains("full copyrighted text was not scraped")
            || ((lowered.contains("public metadata") || lowered.contains("result_summary:"))
                && !Self::lookup_result_has_source_body_signal(result))
            || Self::lookup_result_looks_like_collection_index_not_source_content(result)
    }

    fn lookup_result_has_source_body_signal(result: &str) -> bool {
        let lowered = result.to_ascii_lowercase();
        [
            "chapter 1",
            "chapter one",
            "第1章",
            "第一章",
            "正文",
            "full text",
            "plain text:",
            "content_body",
            "article body",
            "document body",
        ]
        .iter()
        .any(|marker| lowered.contains(&marker.to_ascii_lowercase()) || result.contains(marker))
    }

    fn lookup_result_looks_like_collection_index_not_source_content(result: &str) -> bool {
        let lowered = result.to_ascii_lowercase();
        let has_index_language = [
            "list of ",
            "item list",
            "novel list",
            "book list",
            "latest release",
            "most popular",
            "completed novels",
            "category",
            "genre",
            "ranking",
            "filter",
            "sort",
        ]
        .iter()
        .any(|marker| lowered.contains(marker));
        let has_index_url = [
            "/genre/",
            "/category/",
            "/tag/",
            "/tags/",
            "/rank",
            "/ranking",
            "/list",
            "/search",
            "/browse",
            "/catalog",
        ]
        .iter()
        .any(|marker| lowered.contains(marker));
        let has_source_body_signal = Self::lookup_result_has_source_body_signal(result);

        (has_index_language || has_index_url) && !has_source_body_signal
    }

    fn lookup_result_satisfies_requested_knowledge_depth(query: &str, result: &str) -> bool {
        !Self::query_requests_source_content_for_knowledge(query)
            || !Self::lookup_result_is_metadata_surrogate_not_source_content(result)
            || (Self::query_accepts_structured_material_surrogate_for_knowledge(query)
                && Self::lookup_result_has_structured_material_surrogate(result))
    }

    fn lookup_result_satisfies_requested_material_alignment(query: &str, result: &str) -> bool {
        if !Self::query_requests_source_content_for_knowledge(query) {
            return true;
        }
        let evidence = Self::lookup_result_source_body_surface(result);
        if let Some(blob) = Self::tool_result_json_blob(&evidence) {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&blob) {
                if let Some(content) = value
                    .get("content")
                    .or_else(|| value.get("body"))
                    .or_else(|| value.get("text"))
                    .and_then(|value| value.as_str())
                {
                    return !content.trim().is_empty();
                }
            }
        }
        !evidence.trim().is_empty()
    }

    fn lookup_result_source_body_surface(result: &str) -> String {
        let mut surface = if let Some((_, tail)) = result.split_once("fetched_result:") {
            tail
        } else if let Some((_, tail)) = result.split_once("result:") {
            tail
        } else {
            result
        };
        for marker in [
            "\n\nsearch_result_preview:",
            "\n\nsearch_result:",
            "\n\nKnowledge import receipt:",
            "\n\nOriginal user request:",
            "\n\n完整用户请求",
        ] {
            if let Some((head, _)) = surface.split_once(marker) {
                surface = head;
            }
        }
        surface.trim().to_string()
    }

    fn source_alignment_blocker_text(query: &str, result: &str) -> String {
        let evidence_preview = Self::compact_lookup_evidence_for_file_artifact(result);
        let preview: String = evidence_preview.chars().take(2_400).collect();
        if Self::query_prefers_chinese(query) {
            format!(
                "status: blocked\nworker: researcher\nerror_kind: source_alignment_mismatch\nblockers: fetched source body did not preserve the user's explicit requested source-material type\nnext_step_hint: continue with a different source/query/provider that preserves the original material constraints, or report that no aligned source could be verified\n\n检索阶段拿到了可读取来源，但来源正文没有保持用户明确要求的素材类型。为了不把不匹配的材料导入知识库并继续生成产物，我已停止这条自动导入路径。\n\n当前证据摘要：\n{}",
                preview
            )
        } else {
            format!(
                "status: blocked\nworker: researcher\nerror_kind: source_alignment_mismatch\nblockers: fetched source body did not preserve the user's explicit requested source-material type\nnext_step_hint: continue with a different source/query/provider that preserves the original material constraints, or report that no aligned source could be verified\n\nThe lookup found a readable source, but the source body does not preserve the explicit material type requested by the user. I stopped the automatic knowledge import instead of grounding the artifact on mismatched material.\n\nCurrent evidence summary:\n{}",
                preview
            )
        }
    }

    fn metadata_surrogate_depth_blocker(query: &str, result: &str) -> String {
        let evidence_preview = Self::compact_lookup_evidence_for_file_artifact(result);
        let preview: String = evidence_preview.chars().take(2_400).collect();
        if Self::query_prefers_chinese(query) {
            format!(
                "检索阶段只取得了可验证的公开元数据/列表证据，没有取得用户要求导入知识库的源正文或可下载内容。为了不把元数据冒充成正文，我已停止后续产物生成。\n\n已取得的证据摘要：\n{}",
                preview
            )
        } else {
            format!(
                "The lookup only produced verifiable public metadata/list evidence, not the source body or downloadable content requested for knowledge import. I stopped before generating the artifact so metadata would not be treated as source content.\n\nEvidence preview:\n{}",
                preview
            )
        }
    }

    fn tool_result_json_blob(result: &str) -> Option<&str> {
        source_selection::tool_result_json_blob(result)
    }

    fn best_lookup_source_url_for_query(query: &str, result: &str) -> Option<String> {
        source_selection::best_lookup_source_url_for_query(query, result)
    }

    fn followup_execution_source_url(query: &str, result: &str) -> Option<String> {
        source_selection::followup_execution_source_url(query, result)
    }

    fn knowledge_import_coordinator_handoff_result(query: &str, url: &str, result: &str) -> String {
        let summary = Self::compact_lookup_summary_for_knowledge_import(result);
        let compact_body = Self::compact_lookup_evidence_for_knowledge_import(result);
        let original_user_request = Self::compact_user_request_for_tool_arg(query);
        format!(
            "status: completed\n\
             worker: researcher\n\
             executed_tool: web_fetch\n\
             handoff_required: knowledge_import\n\
             runtime_effect: source.evidence.ready\n\
             source_url: {url}\n\
             result_summary:\n{summary}\n\n\
             fetched_result:\n{compact_body}\n\n\
             original_user_request:\n{}",
            original_user_request
        )
    }

    fn knowledge_import_delegate_call_with_evidence(
        steps: usize,
        url: &str,
        original_query: &str,
        evidence: Option<&str>,
    ) -> (String, String, serde_json::Value) {
        let evidence_suffix = evidence
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| {
                let compact = Self::compact_lookup_evidence_for_knowledge_import(value);
                format!("\n\nfetched_result:\n{compact}")
            })
            .unwrap_or_default();
        let original_query = Self::compact_user_request_for_tool_arg(original_query);
        (
            format!("orchestrated-knowledge-{}", steps),
            "delegate".to_string(),
            serde_json::json!({
                "role": "knowledge",
                "task": format!(
                    "Import this concrete source URL into the knowledge base exactly once. Do not run another lookup. URL: {}{}\n\nOriginal user request:\n{}",
                    url,
                    evidence_suffix,
                    original_query
                )
            }),
        )
    }

    fn knowledge_create_delegate_call(
        steps: usize,
        query: &str,
        evidence: &str,
    ) -> (String, String, serde_json::Value) {
        let content = format!(
            "原始请求：\n{}\n\n已验证公开证据摘要：\n{}",
            query.trim(),
            Self::compact_lookup_evidence_for_file_artifact(evidence)
        );
        (
            format!("orchestrated-knowledge-create-{}", steps),
            "delegate".to_string(),
            serde_json::json!({
                "role": "knowledge",
                "task": format!(
                    "保存到知识库。内容：{}",
                    content
                )
            }),
        )
    }

    fn toolless_execution_delegate_call(
        steps: usize,
        query: &str,
        route: CapabilityRouteHint,
        continuation_context: Option<String>,
    ) -> (String, String, serde_json::Value) {
        let language_contract = Self::language_contract_suffix_for_query(query);
        let role = match route {
            CapabilityRouteHint::RealtimeLookup(_) => "researcher",
            CapabilityRouteHint::DocumentUnderstanding
            | CapabilityRouteHint::VisualUnderstanding => "document",
            CapabilityRouteHint::VoiceUnderstanding => "voice",
            CapabilityRouteHint::FileOps => {
                if Self::query_requests_file_artifact(query)
                    && !Self::query_requests_code_artifact(query)
                {
                    "writer"
                } else {
                    "coder"
                }
            }
            CapabilityRouteHint::Writing => "writer",
            CapabilityRouteHint::Coding => "coder",
            CapabilityRouteHint::RuntimeSurface | CapabilityRouteHint::ExternalCliTools => {
                "terminal"
            }
            CapabilityRouteHint::Communication => "mailer",
            CapabilityRouteHint::Memory => "knowledge",
            CapabilityRouteHint::CapabilityGap => "skill_manager",
            CapabilityRouteHint::General => "researcher",
        };
        let continuation = continuation_context
            .as_deref()
            .map(str::trim)
            .filter(|context| !context.is_empty())
            .map(|context| {
                format!(
                    "\n\nExisting artifact/work-in-progress context from verified runtime receipts:\n{context}\n\
                     Continue from these existing paths/project and preserve already-created artifact identity. \
                     Do not create a new project/document unless the existing artifact is unusable; if it is unusable, state the concrete blocker."
                )
            })
            .unwrap_or_default();
        (
            format!("orchestrated-toolless-execution-{}", steps),
            "delegate".to_string(),
            serde_json::json!({
                "role": role,
                "fallback_role": role,
                "task": format!(
                    "Execute this routed user task as the specialist because the frontstage model did not emit a tool call after an explicit execution-required prompt. Preserve the full original request and all downstream actions. If the task includes lookup/source discovery before another action, perform the lookup phase first, return item-level evidence and blockers, and do not fabricate unavailable evidence.{} Full user request: {}{}",
                    language_contract,
                    query,
                    continuation
                )
            }),
        )
    }

    fn latest_delegate_artifact_continuation_context(messages: &[Message]) -> Option<String> {
        let result = Self::latest_successful_tool_result_text(messages, "delegate")?;
        Self::artifact_continuation_context_from_result(&result)
    }

    fn latest_artifact_continuation_context(messages: &[Message]) -> Option<String> {
        for message in messages.iter().rev() {
            let text = message.text();
            if let Some(context) = Self::artifact_continuation_context_from_result(&text) {
                return Some(context);
            }
        }
        None
    }

    fn artifact_continuation_context_from_result(result: &str) -> Option<String> {
        let mut entries = Vec::new();
        if let Some(blob) = Self::tool_result_json_blob(result) {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&blob) {
                Self::collect_artifact_path_entries(&value, &mut entries);
            }
        }
        if entries.is_empty() {
            Self::collect_artifact_path_entries_from_text(result, &mut entries);
        }
        Self::dedupe_artifact_entries(&mut entries);
        if entries.is_empty() {
            return None;
        }
        Some(
            entries
                .into_iter()
                .take(10)
                .map(|(key, path)| format!("- {key}: {path}"))
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }

    fn artifact_recovery_context_block(result: &str) -> String {
        Self::artifact_recovery_context_from_result(result)
            .map(|context| {
                format!(
                    "\n\nExisting artifact/work-in-progress context from the failed/recoverable result:\n{context}\n\
                     Recovery rule: continue from these identifiers. Prefer the result's suggested `next_action` or the nearest revise/update/export action. Do not call init/create/new for the same artifact/project unless the user explicitly asked to start over."
                )
            })
            .unwrap_or_default()
    }

    fn artifact_recovery_context_from_result(result: &str) -> Option<String> {
        let mut lines = Vec::new();
        if let Some(context) = Self::artifact_continuation_context_from_result(result) {
            lines.extend(context.lines().map(str::to_string));
        }

        let mut entries = Vec::new();
        if let Some(blob) = Self::tool_result_json_blob(result) {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&blob) {
                Self::collect_artifact_recovery_entries(&value, &mut entries);
            }
        }
        Self::collect_artifact_recovery_entries_from_text(result, &mut entries);
        Self::dedupe_artifact_entries(&mut entries);
        lines.extend(
            entries
                .into_iter()
                .take(16)
                .map(|(key, value)| format!("- {key}: {value}")),
        );

        let lowered = result.to_ascii_lowercase();
        if lowered.contains("artifact.needs_revision") || lowered.contains("needs_revision") {
            lines.push("- recovery_state: needs_revision".to_string());
        }
        if lowered.contains("\"passed\": false") || lowered.contains("\"passed\":false") {
            lines.push("- quality_gate: failed".to_string());
        }

        let mut seen = HashSet::new();
        lines.retain(|line| seen.insert(line.clone()));
        if lines.is_empty() {
            None
        } else {
            Some(lines.join("\n"))
        }
    }

    fn collect_artifact_recovery_entries(
        value: &serde_json::Value,
        entries: &mut Vec<(String, String)>,
    ) {
        match value {
            serde_json::Value::Object(object) => {
                for (key, value) in object {
                    if matches!(
                        key.as_str(),
                        "action"
                            | "next_action"
                            | "runtime_effect"
                            | "status"
                            | "error_kind"
                            | "chapter_number"
                            | "chapter_title"
                            | "section_id"
                            | "section_title"
                            | "recoverable"
                    ) {
                        if let Some(rendered) = Self::render_recovery_scalar(value) {
                            entries.push((key.clone(), rendered));
                        }
                    }
                    if key == "quality_gate" {
                        if let Some(passed) = value.get("passed").and_then(|value| value.as_bool())
                        {
                            entries.push(("quality_gate_passed".to_string(), passed.to_string()));
                        }
                        if let Some(issues) = value.get("issues").and_then(|value| value.as_array())
                        {
                            for issue in issues.iter().filter_map(|value| value.as_str()).take(4) {
                                let issue = issue.trim();
                                if !issue.is_empty() {
                                    entries.push((
                                        "quality_gate_issue".to_string(),
                                        issue.to_string(),
                                    ));
                                }
                            }
                        }
                    }
                    Self::collect_artifact_recovery_entries(value, entries);
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    Self::collect_artifact_recovery_entries(item, entries);
                }
            }
            _ => {}
        }
    }

    fn collect_artifact_recovery_entries_from_text(
        result: &str,
        entries: &mut Vec<(String, String)>,
    ) {
        for line in result.lines() {
            let trimmed = line.trim();
            for key in [
                "action",
                "next_action",
                "runtime_effect",
                "status",
                "error_kind",
                "chapter_number",
                "chapter_title",
                "section_id",
                "section_title",
                "recoverable",
            ] {
                if let Some(value) = trimmed.strip_prefix(&format!("{key}:")) {
                    let value = value.trim().trim_matches('"');
                    if !value.is_empty() {
                        entries.push((key.to_string(), value.to_string()));
                    }
                }
            }
        }
    }

    fn render_recovery_scalar(value: &serde_json::Value) -> Option<String> {
        match value {
            serde_json::Value::String(value) => {
                let value = value.trim();
                (!value.is_empty()).then_some(value.to_string())
            }
            serde_json::Value::Number(value) => Some(value.to_string()),
            serde_json::Value::Bool(value) => Some(value.to_string()),
            _ => None,
        }
    }

    fn collect_artifact_path_entries(
        value: &serde_json::Value,
        entries: &mut Vec<(String, String)>,
    ) {
        match value {
            serde_json::Value::Object(object) => {
                for (key, value) in object {
                    if matches!(
                        key.as_str(),
                        "project_path"
                            | "artifact_path"
                            | "output_path"
                            | "manifest_path"
                            | "path"
                            | "file_path"
                    ) {
                        if let Some(path) = value.as_str().map(str::trim).filter(|path| {
                            !path.is_empty() && Self::path_looks_like_artifact_workspace(path)
                        }) {
                            entries.push((key.clone(), path.to_string()));
                            if let Some(project_path) = Self::infer_artifact_project_path(path) {
                                entries.push(("project_path".to_string(), project_path));
                            }
                        }
                    }
                    Self::collect_artifact_path_entries(value, entries);
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    Self::collect_artifact_path_entries(item, entries);
                }
            }
            _ => {}
        }
    }

    fn collect_artifact_path_entries_from_text(result: &str, entries: &mut Vec<(String, String)>) {
        for token in result.split(|ch: char| {
            ch.is_whitespace()
                || matches!(
                    ch,
                    '"' | '\'' | '`' | ',' | ';' | ')' | '(' | ']' | '[' | '{' | '}'
                )
        }) {
            let path = token.trim_matches(|ch: char| matches!(ch, ':' | '.' | '，' | '。'));
            if Self::path_looks_like_artifact_workspace(path) {
                entries.push(("path".to_string(), path.to_string()));
                if let Some(project_path) = Self::infer_artifact_project_path(path) {
                    entries.push(("project_path".to_string(), project_path));
                }
            }
        }
    }

    fn path_looks_like_artifact_workspace(path: &str) -> bool {
        if path.trim().is_empty() {
            return false;
        }
        let lowered = path.to_ascii_lowercase();
        (path.starts_with('/') || lowered.contains(":\\"))
            && (lowered.contains("/generated/")
                || lowered.contains("\\generated\\")
                || lowered.contains("/novels/")
                || lowered.contains("\\novels\\")
                || lowered.ends_with(".txt")
                || lowered.ends_with(".md")
                || lowered.ends_with(".pdf")
                || lowered.ends_with("project.json"))
    }

    fn infer_artifact_project_path(path: &str) -> Option<String> {
        let normalized = path.replace('\\', "/");
        let path = Path::new(&normalized);
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if file_name == "project.json" {
            return path
                .parent()
                .map(|parent| parent.to_string_lossy().to_string());
        }
        for marker in ["chapters", "plans", "runtime", "truth"] {
            if let Some(index) = normalized.find(&format!("/{marker}/")) {
                return Some(normalized[..index].to_string());
            }
        }
        None
    }

    fn dedupe_artifact_entries(entries: &mut Vec<(String, String)>) {
        let mut seen = HashSet::new();
        entries.retain(|(key, path)| seen.insert((key.clone(), path.clone())));
    }

    fn artifact_execution_delegate_route(query: &str) -> CapabilityRouteHint {
        match classify_query_capability_route(query) {
            Some(
                route @ (CapabilityRouteHint::Writing
                | CapabilityRouteHint::Coding
                | CapabilityRouteHint::FileOps),
            ) => route,
            _ if Self::query_requests_code_artifact(query) => CapabilityRouteHint::Coding,
            _ => CapabilityRouteHint::Writing,
        }
    }

    fn worker_tool_boundary_recovery_delegate_call(
        steps: usize,
        role: &str,
        query: &str,
        error: &str,
    ) -> (String, String, serde_json::Value) {
        let language_contract = Self::language_contract_suffix_for_query(query);
        let available = Self::available_tools_from_not_equipped_error(error)
            .unwrap_or_else(|| "the worker's currently equipped tools".to_string());
        (
            format!("orchestrated-worker-tool-boundary-recovery-{}", steps),
            "delegate".to_string(),
            serde_json::json!({
                "role": role,
                "task": format!(
                    "Continue the same delegated task after a worker tool-boundary recovery. The previous attempt tried to call a tool that is not equipped for this worker. Do not call unavailable orchestration or worker-management tools such as `delegate`. Use only these currently equipped tools: {available}. If the task requests a durable artifact mutation, use an equipped write/update/revise/export/file action that can produce a real runtime write receipt, or return a compact blocker naming the missing equipped capability.{language_contract} Original user request: {query}. Previous boundary detail: {}",
                    Self::compact_tool_result_for_recovery(error)
                )
            }),
        )
    }

    fn worker_tool_contract_recovery_delegate_call(
        steps: usize,
        role: &str,
        query: &str,
        error: &str,
    ) -> (String, String, serde_json::Value) {
        let language_contract = Self::language_contract_suffix_for_query(query);
        let artifact_context = Self::artifact_recovery_context_block(error);
        let recovery_policy = "If a writable artifact or section requires `content`, first generate the actual body/content for the requested artifact, or call an equipped retrieval/read tool directly to obtain it from a URL, imported knowledge receipt, document path, or local path, then call the owning artifact tool again with that content and the required identifiers. If the body is too long or awkward to place inside a tool-call JSON, output only the body text as the next assistant message; the runtime can attach that body to the pending content-required tool call. Tool names are top-level calls, never values for another tool's `action` field or file `content`; if the previous attempt wrote a bare tool invocation as file content, call that tool directly instead of writing the invocation text. Do not call write/edit/file tools to record a recovery note, progress report, status report, execution log, or blocker note; process notes are not completion evidence for a write/update/export request.";
        (
            format!("orchestrated-worker-tool-contract-recovery-{}", steps),
            "delegate".to_string(),
            serde_json::json!({
                "role": role,
                "task": format!(
                    "Continue the same delegated task after a worker tool-contract recovery. The previous worker attempt reached an equipped tool but supplied incomplete or invalid arguments. Do not summarize the previous contract error as success. Do not call orchestration or worker-management tools such as `delegate`, `handover`, `multi_agent_audit`, or `tool_search`; this worker must use only its own equipped specialist/file/content tools. Use the previous tool result's `example_shape`, `required_fields`, and `next_step_hint` to retry with complete arguments. {recovery_policy} If the previous result says `wrong_tool_action`, do not put another equipped tool's name in the failed tool's `action` field; call that separate tool directly, then pass its returned content/path back to the owning artifact tool. If the previous result includes existing artifact/project identifiers or a suggested `next_action`, reuse those identifiers and prefer revise/update/export actions over init/create/new actions. Read or compose context only when needed; do not treat read-only/context-only results as completion for a write/update/save request. If the task cannot be completed with the equipped tools, return a compact blocker naming the missing capability.{language_contract} Original user request: {query}. Previous contract detail: {}{}",
                    Self::compact_tool_result_for_recovery(error),
                    artifact_context
                )
            }),
        )
    }

    fn phase_boundary_recovery_delegate_call(
        steps: usize,
        role: &str,
        query: &str,
        blocker: &str,
    ) -> (String, String, serde_json::Value) {
        (
            format!("orchestrated-phase-boundary-recovery-{}", steps),
            "delegate".to_string(),
            serde_json::json!({
                "role": role,
                "task": format!(
                    "Complete only the prerequisite stage reported by the previous artifact-owner worker before artifact drafting continues. Preserve the original user request constraints. Return concrete source URLs, fetched source/body evidence, collection/path, or a knowledge import receipt when available. Do not write the final artifact in this prerequisite worker. Original user request: {query}. Previous phase-boundary blocker: {}",
                    Self::compact_tool_result_for_recovery(blocker)
                ),
                "full_user_request": query
            }),
        )
    }

    fn route_allows_tooled_delegate_recovery(route: CapabilityRouteHint) -> bool {
        matches!(
            route,
            CapabilityRouteHint::Writing
                | CapabilityRouteHint::FileOps
                | CapabilityRouteHint::Coding
                | CapabilityRouteHint::Communication
                | CapabilityRouteHint::RuntimeSurface
                | CapabilityRouteHint::ExternalCliTools
                | CapabilityRouteHint::DocumentUnderstanding
                | CapabilityRouteHint::VisualUnderstanding
                | CapabilityRouteHint::VoiceUnderstanding
                | CapabilityRouteHint::CapabilityGap
        )
    }

    fn file_artifact_delegate_call(
        steps: usize,
        format_query: &str,
        task_context: &str,
    ) -> (String, String, serde_json::Value) {
        let language_contract = Self::language_contract_suffix_for_query(task_context);
        let lowered_format_query = format_query.to_lowercase();
        let artifact_ext = if lowered_format_query.contains("pdf")
            || lowered_format_query.contains(".pdf")
        {
            "pdf"
        } else if lowered_format_query.contains("markdown") || lowered_format_query.contains(".md")
        {
            "md"
        } else {
            "txt"
        };
        let safe_path = format!(
            "data/generated/tasks/{}/agent-artifact-{steps}.{artifact_ext}",
            Uuid::new_v4(),
        );
        let artifact_kind = match artifact_ext {
            "pdf" => "PDF document",
            "md" => "Markdown document",
            _ => "text document",
        };
        let artifact_worker = Self::file_artifact_delegate_role(format_query, task_context);
        let explicit_scale_instruction =
            Self::requested_text_target_chars(task_context).map(|target_units| {
                format!(
                    " The original user request includes an explicit target size of at least {target_units} text units. Preserve this as a hard artifact constraint: when using governed writing/project tools, initialize or update the project with `target_units: {target_units}`; keep reporting `total_units`/`target_units`; treat chapter drafts, plans, context packages, status notes, and project folders as checkpoints only; do not claim final completion until the exported/saved artifact reports enough units."
                )
            });
        let governed_fiction_artifact = artifact_worker != "coder"
            && Self::query_requests_governed_fiction_project(task_context);
        let artifact_task = if artifact_worker == "coder" {
            format!(
                "Create or continue the requested local code or configuration artifact and save it as a {artifact_kind}. Use the existing researcher evidence and knowledge import receipt already in this conversation when relevant. Write the artifact at `{}` with the available file/artifact tool. For substantial artifacts, satisfy the generic artifact quality contract: minimum structure, grounding when applicable, sufficient depth, and self-review/revision notes. If the requested output is too large for one model response or exceeds a text tool limit, use the generic checkpointed continuation flow instead of stopping at a starter artifact.{}{} Original user request: {}",
                safe_path,
                explicit_scale_instruction.as_deref().unwrap_or(""),
                language_contract,
                task_context
            )
        } else if governed_fiction_artifact {
            format!(
                "Create or continue the requested governed long-form fiction project and export it as a {artifact_kind}. Use the existing researcher evidence and knowledge import receipt already in this conversation. If grounding/source material is represented by a URL, imported knowledge receipt, collection/path/title, or local path, first call an equipped retrieval/read tool directly to obtain source body or usable excerpts before adding it to the governed story project tool; when the receipt shows `collection` and `path`, call `fetch_document` with those exact fields before composing. Do not pass locator-only values as source `content`. Do not invent missing source evidence: if the request requires source-body/full-content grounding but the available evidence is only public metadata or list evidence, return a clear blocker instead of writing the artifact. Use `novel_studio` as the project runtime: initialize the project, set the story contract, add retrieved source material, plan/compose/architect/write/audit/revise chapters, maintain truth/continuity state, and export with `format: \"{artifact_ext}\"` and `output: \"{safe_path}\"` when the requested scope is complete. Do not satisfy this task by calling `write_file` with a starter note, plan, status report, or partial prose. Artifact contract: if the user did not provide a title, infer and persist a fresh, non-hardcoded title before body content; include project metadata, source-use policy, target size, continuity rules, and current progress in the project state. For substantial artifacts, satisfy the generic artifact quality contract: minimum structure, evidence/citation grounding when applicable, sufficient depth, and self-review/revision notes. If the requested output is too large for one model response or exceeds a text tool limit, use the generic checkpointed continuation flow instead of stopping at a starter artifact. Keep writing bounded continuation chunks toward the user's requested size, and only report a smaller result as partial if an execution/runtime cap actually stops the run.{}{} Original user request: {}",
                explicit_scale_instruction.as_deref().unwrap_or(""),
                language_contract,
                task_context
            )
        } else {
            format!(
                "Create or continue the requested written artifact and save it as a {artifact_kind}. Use the existing researcher evidence and knowledge import receipt already in this conversation. If grounding/source material is represented by a URL, imported knowledge receipt, collection/path/title, or local path, first call an equipped retrieval/read tool directly to obtain source body or usable excerpts before adding it to a governed writing/project tool; when the receipt shows `collection` and `path`, call `fetch_document` with those exact fields before composing. Do not pass locator-only values as source `content`. Do not invent missing source evidence: if the request requires source-body/full-content grounding but the available evidence is only public metadata or list evidence, return a clear blocker instead of writing the artifact. Write the artifact at `{}` with the available writing/file artifact tool. Artifact contract: if the user did not provide a title, the worker must infer and write a fresh, non-hardcoded title before body content; include a compact document metadata block with title, artifact type, source-use policy, target size, continuity rules, and current progress. For substantial artifacts, satisfy the generic artifact quality contract: minimum structure, evidence/citation grounding when applicable, sufficient depth, and self-review/revision notes. If the requested output is too large for one model response or exceeds a text tool limit, use the generic checkpointed continuation flow instead of stopping at a starter artifact. Keep writing bounded continuation chunks toward the user's requested size, and only report a smaller result as partial if an execution/runtime cap actually stops the run.{}{} Original user request: {}",
                safe_path,
                explicit_scale_instruction.as_deref().unwrap_or(""),
                language_contract,
                task_context
            )
        };
        (
            format!("orchestrated-file-artifact-{}", steps),
            "delegate".to_string(),
            serde_json::json!({
                "role": artifact_worker,
                "task": artifact_task
            }),
        )
    }

    fn should_prioritize_followup_execution(query: &str, messages: &[Message]) -> bool {
        if !query_requests_followup_execution_after_lookup(query)
            && !Self::query_requests_knowledge_persistence(query)
        {
            return false;
        }

        if Self::has_system_marker(messages, "BENSHU_ORCHESTRATION_CHAIN_KNOWLEDGE") {
            return false;
        }

        if Self::current_turn_has_completed_knowledge_import(messages) {
            return false;
        }

        if Self::latest_successful_tool_name(messages)
            .as_deref()
            .is_some_and(|name| name == "knowledge_import_url")
        {
            return false;
        }

        let Some(result) = Self::latest_lookup_result_for_followup_execution(messages) else {
            return false;
        };
        Self::collection_evidence_gap_for_query(query, &result).is_none()
    }

    fn requested_collection_item_count_from_query(query: &str) -> usize {
        collection_evidence::requested_item_count_or_default(query, 3)
    }

    fn collection_evidence_gap_for_query(
        query: &str,
        result: &str,
    ) -> Option<collection_evidence::CollectionEvidenceGap> {
        collection_evidence::evidence_gap(query, result)
    }

    fn collection_evidence_gap_blocker(
        query: &str,
        gap: collection_evidence::CollectionEvidenceGap,
        result: &str,
    ) -> String {
        let evidence_preview = Self::compact_lookup_evidence_for_file_artifact(result);
        collection_evidence::format_gap_blocker(query, gap, &evidence_preview)
    }

    fn collection_evidence_recovery_instruction(
        query: &str,
        gap: collection_evidence::CollectionEvidenceGap,
        result: &str,
    ) -> String {
        collection_evidence::recovery_instruction(query, gap, result)
    }

    fn latest_structured_lookup_result_for_knowledge_create(
        messages: &[Message],
        query: &str,
    ) -> Option<String> {
        let requested = Self::requested_collection_item_count_from_query(query).max(1);
        turn_state::current_turn_messages(messages)
            .iter()
            .rev()
            .find_map(|message| {
                if !matches!(message.role, Role::Tool) {
                    return None;
                }
                if !message
                    .metadata
                    .get("tool_name")
                    .is_some_and(|name| name == "delegate")
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

                let result = message.text();
                if turn_state::tool_result_is_blocked(&result) {
                    return None;
                }
                let lowered = result.to_ascii_lowercase();
                let is_lookup_worker = lowered.contains("worker: researcher")
                    || lowered.contains("worker: browser")
                    || lowered.contains("executed_tool: web_search")
                    || lowered.contains("executed_tool: web_fetch")
                    || lowered.contains("executed_tool: browser_browse");
                if !is_lookup_worker {
                    return None;
                }
                let items = knowledge_delivery::ranked_metadata_items_from_result(&result);
                (items.len() >= requested
                    && Self::lookup_result_satisfies_requested_knowledge_depth(query, &result))
                .then_some(result)
            })
    }

    fn latest_metadata_surrogate_lookup_for_requested_source_content(
        messages: &[Message],
        query: &str,
    ) -> Option<String> {
        if !Self::query_requests_source_content_for_knowledge(query) {
            return None;
        }
        let requested = Self::requested_collection_item_count_from_query(query).max(1);
        turn_state::current_turn_messages(messages)
            .iter()
            .rev()
            .find_map(|message| {
                if !matches!(message.role, Role::Tool) {
                    return None;
                }
                if !message
                    .metadata
                    .get("tool_name")
                    .is_some_and(|name| name == "delegate")
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

                let result = message.text();
                if turn_state::tool_result_is_blocked(&result)
                    || !Self::lookup_result_is_metadata_surrogate_not_source_content(&result)
                {
                    return None;
                }
                if Self::query_accepts_structured_material_surrogate_for_knowledge(query)
                    && Self::lookup_result_has_structured_material_surrogate(&result)
                {
                    return None;
                }
                let items = knowledge_delivery::ranked_metadata_items_from_result(&result);
                (items.len() >= requested).then_some(result)
            })
    }

    fn latest_lookup_result_requiring_observation_recovery(messages: &[Message]) -> Option<String> {
        turn_state::current_turn_messages(messages)
            .iter()
            .rev()
            .find_map(|message| {
                if !matches!(message.role, Role::Tool) {
                    return None;
                }
                if !message
                    .metadata
                    .get("tool_name")
                    .is_some_and(|name| name == "delegate")
                {
                    return None;
                }
                let result = message.text();
                let lowered = result.to_ascii_lowercase();
                let blocked_lookup = lowered.contains("status: blocked")
                    && (lowered.contains("worker: researcher")
                        || lowered.contains("executed_tool: web_fetch")
                        || lowered.contains("executed_tool: web_search"));
                let observation_relevant = lowered.contains("challenge")
                    || lowered.contains("anti-bot")
                    || lowered.contains("verification")
                    || lowered.contains("low-information")
                    || lowered.contains("directory/search pages")
                    || lowered.contains("insufficient page evidence")
                    || lowered.contains("not provide enough verified evidence");
                (blocked_lookup && observation_relevant).then_some(result)
            })
    }

    #[cfg(test)]
    fn latest_blocked_lookup_result_requiring_browser(messages: &[Message]) -> Option<String> {
        Self::latest_lookup_result_requiring_observation_recovery(messages)
    }

    fn lookup_result_evidence_quality(result: &str, query: &str) -> EvidenceQuality {
        let trimmed = result.trim();
        if trimmed.is_empty() {
            return EvidenceQuality::Empty;
        }

        let lowered = trimmed.to_ascii_lowercase();
        if Self::tool_result_is_blocked(trimmed) {
            if lowered.contains("challenge")
                || lowered.contains("anti-bot")
                || lowered.contains("verification")
                || lowered.contains("captcha")
                || lowered.contains("access denied")
                || lowered.contains("login")
            {
                return EvidenceQuality::BlockedByAccess;
            }
            if lowered.contains("low-information")
                || lowered.contains("low_information")
                || lowered.contains("boilerplate")
                || lowered.contains("empty")
                || lowered.contains("zero usable")
                || lowered.contains("insufficient")
            {
                return EvidenceQuality::LowInformation;
            }
            return EvidenceQuality::Partial;
        }

        if !Self::lookup_result_satisfies_requested_knowledge_depth(query, result) {
            return EvidenceQuality::Partial;
        }

        if !knowledge_delivery::ranked_metadata_items_from_result(result).is_empty()
            || Self::followup_execution_source_url(query, result).is_some()
        {
            return EvidenceQuality::Sufficient;
        }

        if Self::text_contains_url(trimmed) {
            return EvidenceQuality::MissingConcreteSource;
        }

        EvidenceQuality::Irrelevant
    }

    fn lookup_search_attempts(messages: &[Message]) -> usize {
        turn_state::current_turn_messages(messages)
            .iter()
            .filter(|message| matches!(message.role, Role::Tool))
            .filter(|message| {
                let tool_name = message
                    .metadata
                    .get("tool_name")
                    .map(String::as_str)
                    .unwrap_or_default();
                if matches!(tool_name, "tool_search" | "web_search") {
                    return true;
                }
                let lowered = message.text().to_ascii_lowercase();
                lowered.contains("executed_tool: web_search")
                    || lowered.contains("executed_tool: tool_search")
            })
            .count()
    }

    fn latest_repeated_empty_lookup_result(messages: &[Message]) -> Option<String> {
        let mut empty_attempts = 0usize;
        let mut latest_empty = None;
        for message in turn_state::current_turn_messages(messages)
            .iter()
            .filter(|message| matches!(message.role, Role::Tool))
        {
            let tool_name = message
                .metadata
                .get("tool_name")
                .map(String::as_str)
                .unwrap_or_default();
            let text = message.text();
            if Self::lookup_result_is_empty_or_low_information(tool_name, &text) {
                empty_attempts += 1;
                latest_empty = Some(text);
            }
        }
        (empty_attempts >= 2)
            .then_some(latest_empty?)
            .filter(|_| !Self::lookup_observation_already_attempted(messages))
    }

    fn latest_reused_empty_lookup_result(messages: &[Message]) -> Option<(String, String)> {
        turn_state::current_turn_messages(messages)
            .iter()
            .rev()
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
                let tool_name = message
                    .metadata
                    .get("tool_name")
                    .cloned()
                    .unwrap_or_default();
                let text = message.text();
                Self::lookup_result_is_empty_or_low_information(&tool_name, &text)
                    .then_some((tool_name, text))
            })
    }

    fn lookup_result_is_empty_or_low_information(tool_name: &str, text: &str) -> bool {
        let trimmed = text.trim();
        let lowered = trimmed.to_ascii_lowercase();
        let lookup_tool = matches!(tool_name, "web_search" | "web_fetch" | "tool_search")
            || lowered.contains("executed_tool: web_search")
            || lowered.contains("executed_tool: web_fetch")
            || lowered.contains("executed_tool: browser_browse");
        if !lookup_tool {
            return false;
        }

        trimmed.is_empty()
            || trimmed == "[]"
            || trimmed == "{}"
            || lowered.contains("status: blocked")
            || lowered.starts_with("[]\n")
            || lowered.starts_with("{}\n")
            || lowered.contains("] []\n")
            || lowered.contains("] {}\n")
            || lowered.contains("] []\r\n")
            || lowered.contains("] {}\r\n")
            || lowered.ends_with("] []")
            || lowered.ends_with("] {}")
            || lowered.contains("preview=[]")
            || lowered.contains("result: []")
            || lowered.contains("\"results\":[]")
            || lowered.contains("\"results\": []")
            || lowered.contains("results: []")
            || lowered.contains("no candidate search results")
            || lowered.contains("no results")
            || lowered.contains("zero usable")
            || lowered.contains("low-information")
            || lowered.contains("low_information")
    }

    fn lookup_observation_already_attempted(messages: &[Message]) -> bool {
        Self::has_system_marker_after_latest_user(
            messages,
            "BENSHU_ORCHESTRATION_OBSERVATION_RECOVERY",
        ) || turn_state::current_turn_messages(messages)
            .iter()
            .filter(|message| matches!(message.role, Role::Tool))
            .any(|message| {
                let lowered = message.text().to_ascii_lowercase();
                lowered.contains("worker: browser")
                    || lowered.contains("executed_tool: browser_browse")
                    || lowered.contains("observation_trace:")
            })
    }

    fn lookup_specialist_already_attempted(messages: &[Message]) -> bool {
        turn_state::current_turn_messages(messages)
            .iter()
            .filter(|message| matches!(message.role, Role::Tool))
            .any(|message| {
                message
                    .metadata
                    .get("tool_name")
                    .is_some_and(|name| name == "delegate")
            })
    }

    fn observation_recovery_tool_call(
        steps: usize,
        query: &str,
    ) -> (String, String, serde_json::Value) {
        (
            format!("orchestrated-observation-tool-recovery-{}", steps),
            "browser_browse".to_string(),
            serde_json::json!({
                "action": "search",
                "text": query.trim(),
                "max_results": 10,
                "wait_until": "domcontentloaded",
                "format": "semantic",
                "structured": true,
                "compact": true,
            }),
        )
    }

    fn lookup_recovery_action_for_result(
        &self,
        messages: &[Message],
        query: &str,
        result: &str,
        steps: usize,
        max_steps: usize,
    ) -> RecoveryAction {
        decide_lookup_evidence_recovery(LookupEvidenceRecoveryInput {
            current_step: steps,
            max_steps,
            evidence_quality: Self::lookup_result_evidence_quality(result, query),
            has_search_tool: self.tool_is_enabled("tool_search")
                || self.tool_is_enabled("web_search")
                || self.tool_is_enabled("delegate"),
            search_attempts: Self::lookup_search_attempts(messages),
            has_observation_tool: self.tool_is_enabled("browser_browse")
                || self.tool_is_enabled("delegate"),
            observation_already_attempted: Self::lookup_observation_already_attempted(messages),
            has_delegate_tool: self.tool_is_enabled("delegate"),
            specialist_already_attempted: Self::lookup_specialist_already_attempted(messages),
            required_persistence: Self::query_requests_knowledge_persistence(query),
        })
    }

    fn observation_recovery_delegate_call(
        steps: usize,
        query: &str,
        blocked_result: &str,
    ) -> (String, String, serde_json::Value) {
        let lowered_blocker = blocked_result.to_ascii_lowercase();
        let prior_result_was_directory_or_unstructured = lowered_blocker
            .contains("directory/search pages")
            || lowered_blocker.contains("insufficient page evidence")
            || !lowered_blocker.contains("result_summary:");
        let candidate_url = Self::explicit_source_url_in_result(blocked_result).or_else(|| {
            (!prior_result_was_directory_or_unstructured)
                .then(|| Self::best_lookup_source_url_for_query(query, blocked_result))
                .flatten()
        });
        let task = if let Some(url) = candidate_url {
            format!(
                "The prior lookup for this user task could not obtain enough verified observable evidence. Use an observation-capable worker to inspect the best candidate source and return observable item-level content or metadata according to the configured runtime policy. User task: {}\nCandidate URL: {}",
                query.trim(),
                url
            )
        } else {
            format!(
                "The prior lookup for this user task could not obtain enough verified observable evidence. Use an observation-capable worker to inspect sources and return observable item-level content or metadata according to the configured runtime policy. User task: {}",
                query.trim()
            )
        };
        (
            format!("orchestrated-observation-recovery-{}", steps),
            "delegate".to_string(),
            serde_json::json!({
                "role": "browser",
                "task": task
            }),
        )
    }

    #[cfg(test)]
    fn browser_escalation_delegate_call(
        steps: usize,
        query: &str,
        blocked_result: &str,
    ) -> (String, String, serde_json::Value) {
        Self::observation_recovery_delegate_call(steps, query, blocked_result)
    }

    fn content_contains_verification_challenge(content: &str) -> bool {
        if knowledge_delivery::ranked_metadata_items_from_result(content).len() >= 3
            || Self::delegate_result_summary_block(content)
                .map(|summary| {
                    summary
                        .lines()
                        .filter(|line| !line.trim().is_empty())
                        .count()
                        >= 3
                })
                .unwrap_or(false)
        {
            return false;
        }

        let lowered = content.to_ascii_lowercase();
        let strong_challenge = lowered.contains("cloudflare")
            || lowered.contains("enable javascript and cookies to continue")
            || lowered.contains("security verification")
            || lowered.contains("anti-bot")
            || lowered.contains("challenge page");
        let weak_challenge = lowered.contains("正在进行安全验证") || lowered.contains("请稍候");

        strong_challenge || (weak_challenge && content.len() < 1500)
    }

    fn tool_search_result_indicates_external_lookup(content: &str) -> bool {
        let lowered = content.to_ascii_lowercase();
        lowered.contains("\"web_search\"")
            || lowered.contains("\"web_fetch\"")
            || lowered.contains("\"browser_browse\"")
            || lowered.contains("\"realtime_lookup.latest_info\"")
    }

    fn latest_user_media_path(messages: &[Message]) -> Option<String> {
        let message = latest_user_message_with_media(messages)?;
        match &message.content {
            Content::Parts(parts) => {
                for part in parts {
                    if let ContentPart::Image {
                        source: ImageSource::Url { url },
                    } = part
                    {
                        return Some(url.clone());
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn latest_user_media_image_base64(messages: &[Message]) -> Option<(String, String)> {
        let message = latest_user_message_with_media(messages)?;
        let Content::Parts(parts) = &message.content else {
            return None;
        };

        for part in parts {
            if let ContentPart::Image {
                source: ImageSource::Base64 { media_type, data },
            } = part
            {
                return Some((media_type.clone(), data.clone()));
            }
        }

        None
    }

    fn infer_image_extension(media_type: &str) -> &'static str {
        match media_type.to_ascii_lowercase().as_str() {
            "image/png" => "png",
            "image/jpeg" | "image/jpg" => "jpg",
            "image/webp" => "webp",
            "image/gif" => "gif",
            "image/bmp" => "bmp",
            _ => "png",
        }
    }

    async fn materialize_base64_image_to_temp(media_type: &str, data: &str) -> Option<String> {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data.as_bytes())
            .ok()?;
        let mut path = std::env::temp_dir();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_nanos();
        let extension = Self::infer_image_extension(media_type);
        path.push(format!("benshu_mm_fallback_{now}.{extension}"));
        if tokio::fs::write(&path, bytes).await.is_err() {
            return None;
        }
        Some(path.to_string_lossy().to_string())
    }

    fn tool_is_enabled(&self, name: &str) -> bool {
        self.tools.contains(name)
            && self
                .enabled_tools
                .as_ref()
                .is_none_or(|enabled| enabled.read().contains(name))
    }

    fn normalize_local_pseudo_tool_call(
        &self,
        name: String,
        args: serde_json::Value,
    ) -> (String, serde_json::Value) {
        pseudo_tool::normalize_local_call(&self.tools, name, args)
    }

    fn extract_inline_pseudo_tool_calls(text: &str) -> Vec<(String, serde_json::Value)> {
        pseudo_tool::extract_inline_calls(text)
    }

    fn available_execution_tools_for_query(&self, query: &str) -> Vec<String> {
        if Self::query_is_creation_planning_dialogue(query) {
            return Vec::new();
        }
        if query_prefers_session_continuity_answer(query) {
            return Vec::new();
        }

        if query_requests_image_generation(query) && self.tool_is_enabled("generate_image") {
            return vec!["generate_image".to_string()];
        }

        let Some(route) = Self::execution_required_route_for_query(query, false)
            .filter(|route| capability_route_requires_real_tool_call(*route))
        else {
            return Vec::new();
        };

        let candidates: Vec<String> = capability_route_tool_allowlist_for_query(route, Some(query))
            .into_iter()
            .collect();

        let mut available = candidates
            .into_iter()
            .filter(|name| self.tool_is_enabled(name))
            .collect::<Vec<_>>();

        if Self::query_requests_governed_fiction_project(query)
            && self.tool_is_enabled("novel_studio")
        {
            self.prepend_tool_if_enabled(&mut available, "novel_studio");
            available.retain(|name| name != "writing_studio" && name != "write_file");
        }
        self.prepend_retrieval_tools_for_imported_material(&mut available, query);

        if matches!(route, CapabilityRouteHint::RealtimeLookup(_))
            && self.tool_is_enabled("browser_browse")
            && !available.iter().any(|name| name == "browser_browse")
        {
            available.push("browser_browse".to_string());
        }

        available
    }

    fn apply_task_specific_tool_surface_filter(
        tools: &mut Vec<ToolDefinition>,
        query: Option<&str>,
    ) -> usize {
        let Some(query) = query else { return 0 };
        if Self::query_is_creation_planning_dialogue(query) {
            let before = tools.len();
            tools.clear();
            return before;
        }
        if !Self::query_requests_governed_fiction_project(query)
            || !tools.iter().any(|tool| tool.name == "novel_studio")
        {
            return 0;
        }
        let before = tools.len();
        tools.retain(|tool| tool.name != "writing_studio" && tool.name != "write_file");
        before.saturating_sub(tools.len())
    }

    fn query_requests_governed_fiction_project(query: &str) -> bool {
        let lowered = query.to_lowercase();
        let fiction_intent = [
            "novel",
            "fiction",
            "story",
            "multi-chapter",
            "book-length",
            "小说",
            "故事",
            "章节",
            "长篇",
        ]
        .iter()
        .any(|term| lowered.contains(term) || query.contains(term));
        if !fiction_intent {
            return false;
        }

        let governed_scope = Self::requested_text_target_chars(query).is_some()
            || [
                "continuity",
                "truth ledger",
                "chapter",
                "character",
                "plot",
                "drift",
                "连续性",
                "不漂移",
                "角色",
                "剧情",
                "世界观",
                "设定",
            ]
            .iter()
            .any(|term| lowered.contains(term) || query.contains(term));
        governed_scope
    }

    fn query_has_imported_material_locator(query: &str) -> bool {
        let lowered = query.to_ascii_lowercase();
        lowered.contains("knowledge import receipt")
            || lowered.contains("knowledge.imported")
            || lowered.contains("imported web knowledge")
            || lowered.contains("collection:")
                && (lowered.contains("path:") || lowered.contains("document-"))
            || lowered.contains("references/")
            || query.contains("知识库写入")
            || query.contains("已入库")
            || query.contains("入库回执")
    }

    fn prepend_tool_if_enabled(&self, available: &mut Vec<String>, name: &str) {
        if self.tool_is_enabled(name) && !available.iter().any(|tool| tool == name) {
            available.insert(0, name.to_string());
        }
    }

    fn prepend_retrieval_tools_for_imported_material(
        &self,
        available: &mut Vec<String>,
        query: &str,
    ) {
        if !Self::query_has_imported_material_locator(query) {
            return;
        }
        for name in ["knowledge_search", "tiered_search", "fetch_document"] {
            self.prepend_tool_if_enabled(available, name);
        }
    }

    fn prioritize_observation_tools_after_collection_gap(
        available_tools: Vec<String>,
    ) -> Vec<String> {
        let mut prioritized = Vec::new();
        for preferred in ["browser_browse", "web_fetch"] {
            if available_tools.iter().any(|name| name == preferred) {
                prioritized.push(preferred.to_string());
            }
        }
        for tool in available_tools {
            if tool == "web_search"
                && prioritized
                    .iter()
                    .any(|name| name == "browser_browse" || name == "web_fetch")
            {
                continue;
            }
            if !prioritized.iter().any(|name| name == &tool) {
                prioritized.push(tool);
            }
        }
        prioritized
    }

    fn latest_execution_required_route(messages: &[Message]) -> Option<CapabilityRouteHint> {
        let query = Self::latest_user_query(messages)?;
        let has_media_input = latest_user_message_has_media(messages);
        Self::execution_required_route_for_query(&query, has_media_input)
    }

    fn execution_required_route_for_query(
        query: &str,
        has_media_input: bool,
    ) -> Option<CapabilityRouteHint> {
        if Self::query_is_creation_planning_dialogue(query) {
            return None;
        }

        if let Some(route) = classify_query_capability_route(query)
            .filter(|route| route_requires_real_tool_call_for_turn(*route, has_media_input))
        {
            return Some(route);
        }

        if has_media_input {
            return None;
        }

        if Self::query_requests_code_artifact(query) {
            return Some(CapabilityRouteHint::Coding);
        }

        if Self::query_requests_artifact_mutation(query)
            || Self::query_requests_file_artifact(query)
            || Self::query_requests_governed_fiction_project(query)
        {
            return Some(Self::artifact_execution_delegate_route(query));
        }

        None
    }

    fn has_recent_tool_execution_required_prompt(messages: &[Message]) -> bool {
        messages.iter().rev().take(8).any(|message| {
            matches!(message.role, Role::System)
                && message
                    .content
                    .as_text()
                    .contains(reasoner_constants::MARKER_TOOL_EXECUTION_REQUIRED)
        })
    }

    fn query_prefers_chinese(query: &str) -> bool {
        query
            .chars()
            .any(|ch| ('\u{4E00}'..='\u{9FFF}').contains(&ch))
    }

    fn language_contract_for_query(query: Option<&str>) -> LanguageContract {
        resolve_language_contract(query.unwrap_or_default())
    }

    fn language_contract_suffix_for_query(query: &str) -> String {
        Self::language_contract_for_query(Some(query)).delegate_suffix()
    }

    fn user_facing_progress_message(action: &str, query: &str) -> String {
        let prefers_chinese = Self::query_prefers_chinese(query);
        match action {
            "lookup_start" => {
                if prefers_chinese {
                    "我正在搜索相关来源，并先筛选最值得继续读取的结果。".to_string()
                } else {
                    "I’m searching for relevant sources and filtering the best candidates first."
                        .to_string()
                }
            }
            "source_fetch" => {
                if prefers_chinese {
                    "我正在继续读取候选来源，确认哪些内容可以稳定交付。".to_string()
                } else {
                    "I’m reading the best candidate sources now to confirm what can be delivered reliably."
                        .to_string()
                }
            }
            "knowledge_import" => {
                if prefers_chinese {
                    "我正在把已经确认的来源写入知识库。".to_string()
                } else {
                    "I’m saving the confirmed source into the knowledge base now.".to_string()
                }
            }
            "file_artifact" => {
                if prefers_chinese {
                    "我正在把最终内容写成本地文件。".to_string()
                } else {
                    "I’m writing the final content into a local file now.".to_string()
                }
            }
            _ => {
                if prefers_chinese {
                    "我正在继续处理这一步。".to_string()
                } else {
                    "I’m continuing with the next step now.".to_string()
                }
            }
        }
    }

    fn media_answer_needs_text_enrichment(response: &str) -> bool {
        media_delivery::answer_needs_text_enrichment(response)
    }

    fn is_low_value_media_answer(query: &str, response: &str) -> bool {
        media_delivery::is_low_value_answer(query, response)
    }

    fn query_requests_structured_media_output(query: &str) -> bool {
        media_delivery::query_requests_structured_output(query)
    }

    fn normalized_structured_media_output(response: &str) -> Option<String> {
        media_delivery::normalized_structured_output(response)
    }

    fn media_understanding_failure_text(query: &str) -> String {
        media_delivery::understanding_failure_text(query)
    }

    fn latest_successful_tool_result(messages: &[Message]) -> Option<(String, String)> {
        Self::current_turn_messages(messages)
            .iter()
            .rev()
            .find_map(|message| {
                if message.role != Role::Tool {
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

                parts.iter().find_map(|part| match part {
                    crate::agent::message::ContentPart::ToolResult {
                        name: Some(name),
                        content,
                        ..
                    } => {
                        let trimmed = content.trim();
                        let lowered = trimmed.to_ascii_lowercase();
                        if trimmed.is_empty()
                            || Self::tool_result_content_is_runtime_error(trimmed)
                            || lowered.contains("status: blocked")
                            || lowered.contains("status: failed")
                            || lowered.contains("status: needs_confirmation")
                            || lowered.contains("\"status\":\"blocked\"")
                            || lowered.contains("\"status\": \"blocked\"")
                            || lowered.contains("\"status\":\"failed\"")
                            || lowered.contains("\"status\": \"failed\"")
                            || lowered.contains("\"status\":\"needs_confirmation\"")
                            || lowered.contains("\"status\": \"needs_confirmation\"")
                        {
                            None
                        } else {
                            Some((name.clone(), content.clone()))
                        }
                    }
                    _ => None,
                })
            })
    }

    fn direct_tool_display_delivery(messages: &[Message], query: &str) -> Option<String> {
        if Self::query_requests_knowledge_persistence(query)
            || Self::query_requests_file_artifact(query)
            || Self::query_requests_artifact_mutation(query)
        {
            return None;
        }

        let (tool_name, content) = Self::latest_successful_tool_result(messages)?;
        let value = Self::parse_tool_result_json(&content)?;
        if Self::is_realtime_lookup_tool(&tool_name) && !Self::realtime_receipt_is_verified(&value)
        {
            return None;
        }
        let orchestration = value.get("orchestration_decision")?;
        let can_finalize = orchestration
            .get("can_finalize_answer")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let requires_followup = orchestration
            .get("requires_followup")
            .and_then(|value| value.as_bool())
            .unwrap_or(true);
        if !can_finalize || requires_followup {
            return None;
        }

        Self::tool_result_display_text(&content, Self::query_prefers_chinese(query))
    }

    fn direct_tool_trace_display_delivery(
        tool_trace: &[ToolCallData],
        query: &str,
        expected_tool_name: &str,
    ) -> Option<String> {
        if Self::query_requests_knowledge_persistence(query)
            || Self::query_requests_file_artifact(query)
            || Self::query_requests_artifact_mutation(query)
        {
            return None;
        }

        let call = tool_trace
            .iter()
            .rev()
            .find(|call| call.name == expected_tool_name)?;
        let owned_full_content = call
            .outcome
            .as_ref()
            .and_then(|outcome| outcome.full_artifact_ref.as_deref())
            .filter(|_| call.result_truncated)
            .and_then(|path| std::fs::read_to_string(path).ok());
        let content = owned_full_content.as_deref().or(call.result.as_deref())?;
        if call.name == "web_search" || expected_tool_name == "web_search" {
            let prefers_chinese = Self::query_prefers_chinese(query);
            if let Some(snippet) = Self::retrieval_snippet_for_query(query, content) {
                return Some(if prefers_chinese {
                    if Self::requested_search_result_count(query) > 1 {
                        format!("我已经完成初步检索，当前最相关候选是：\n{}", snippet)
                    } else {
                        format!("我已经完成初步检索，当前最相关结果是：{}", snippet)
                    }
                } else if Self::requested_search_result_count(query) > 1 {
                    format!(
                        "The initial search completed. The most relevant current candidates are:\n{}",
                        snippet
                    )
                } else {
                    format!(
                        "The initial search completed. The most relevant current result is: {}",
                        snippet
                    )
                });
            }

            let diagnostics = Self::compact_search_diagnostics(content);
            return Some(if prefers_chinese {
                if diagnostics.is_empty() {
                    "我已经执行了外部检索，但这次没有拿到可交付的可靠候选来源。".to_string()
                } else {
                    format!(
                        "我已经执行了外部检索，但这次没有拿到可交付的可靠候选来源。诊断：{}",
                        diagnostics
                    )
                }
            } else {
                if diagnostics.is_empty() {
                    "I ran the external lookup, but it did not produce a reliable candidate source that can be delivered as an answer.".to_string()
                } else {
                    format!(
                        "I ran the external lookup, but it did not produce a reliable candidate source that can be delivered as an answer. Diagnostics: {}",
                        diagnostics
                    )
                }
            });
        }

        let value = Self::parse_tool_result_json(content)?;
        if Self::is_realtime_lookup_tool(&call.name) && !Self::realtime_receipt_is_verified(&value)
        {
            return None;
        }

        let can_finalize = value
            .get("orchestration_decision")
            .and_then(|orchestration| orchestration.get("can_finalize_answer"))
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let requires_followup = value
            .get("orchestration_decision")
            .and_then(|orchestration| orchestration.get("requires_followup"))
            .and_then(|value| value.as_bool())
            .unwrap_or(true);
        if !can_finalize || requires_followup {
            return None;
        }

        Self::tool_result_display_text(content, Self::query_prefers_chinese(query))
    }

    fn compact_search_diagnostics(content: &str) -> String {
        let Some((_, tail)) = content.split_once("source_diagnostics:") else {
            return String::new();
        };
        tail.lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .take(4)
            .map(|line| line.trim_start_matches("- ").to_string())
            .collect::<Vec<_>>()
            .join("; ")
            .chars()
            .take(500)
            .collect()
    }

    fn latest_realtime_tool_trace_display_delivery(
        tool_trace: &[ToolCallData],
        query: &str,
    ) -> Option<String> {
        if Self::query_requests_knowledge_persistence(query)
            || Self::query_requests_file_artifact(query)
            || Self::query_requests_artifact_mutation(query)
        {
            return None;
        }

        for call in tool_trace.iter().rev() {
            if !Self::is_realtime_lookup_tool(&call.name) {
                continue;
            }
            if let Some(text) =
                Self::direct_tool_trace_display_delivery(tool_trace, query, &call.name)
            {
                return Some(text);
            }
        }
        None
    }

    fn realtime_receipt_is_verified(value: &serde_json::Value) -> bool {
        let Some(receipt) = value.get("realtime_receipt") else {
            return false;
        };
        let status_verified = receipt
            .get("status")
            .and_then(|value| value.as_str())
            .is_some_and(|status| status.eq_ignore_ascii_case("verified"));
        let freshness_ok = receipt
            .get("freshness")
            .and_then(|freshness| freshness.get("ok"))
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let has_timestamped_source = receipt
            .get("sources")
            .and_then(|value| value.as_array())
            .is_some_and(|sources| {
                !sources.is_empty()
                    && sources.iter().all(|source| {
                        source
                            .get("observed_at")
                            .and_then(|value| value.as_str())
                            .is_some_and(|value| !value.trim().is_empty())
                            || source
                                .get("published_at")
                                .and_then(|value| value.as_str())
                                .is_some_and(|value| !value.trim().is_empty())
                    })
            });
        let blockers_empty = receipt
            .get("blockers")
            .and_then(|value| value.as_array())
            .is_none_or(|blockers| blockers.is_empty());
        status_verified && freshness_ok && has_timestamped_source && blockers_empty
    }

    fn realtime_entity_before_any(query: &str, markers: &[&str]) -> Option<String> {
        let mut best: Option<&str> = None;
        for marker in markers {
            if let Some((prefix, _)) = query.split_once(marker) {
                if !prefix.trim().is_empty()
                    && best.is_none_or(|current| prefix.len() < current.len())
                {
                    best = Some(prefix);
                }
            }
        }
        best.map(Self::clean_realtime_entity_candidate)
            .filter(|value| !value.is_empty())
    }

    fn clean_realtime_entity_candidate(value: &str) -> String {
        let mut text = value.trim().to_string();
        for prefix in [
            "帮我查一下",
            "帮我查",
            "查一下",
            "查询",
            "查",
            "看一下",
            "看看",
            "please",
            "can you",
            "could you",
            "what is",
            "what's",
        ] {
            text = text
                .trim_start_matches(prefix)
                .trim()
                .trim_start_matches(['，', ',', ':', '：', ' '])
                .trim()
                .to_string();
        }
        for filler in [
            "今天", "现在", "当前", "实时", "最新", "的", "now", "current", "latest", "today",
        ] {
            text = text.replace(filler, " ");
        }
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    fn direct_realtime_tool_call_for_query(
        &self,
        query: &str,
    ) -> Option<(String, String, serde_json::Value)> {
        if Self::query_is_creation_planning_dialogue(query)
            || query.contains("BENSHU_CREATION_PLANNING_DIALOGUE")
        {
            return None;
        }
        if query_requests_followup_execution_after_lookup(query)
            || Self::query_requests_knowledge_persistence(query)
            || Self::query_requests_file_artifact(query)
            || Self::query_requests_artifact_mutation(query)
            || Self::query_requests_explicit_worker_delegation(query)
        {
            return None;
        }

        let route = classify_query_capability_route(query)?;
        let CapabilityRouteHint::RealtimeLookup(kind) = route else {
            return None;
        };

        let (tool_name, args) = match kind {
            RealtimeLookupKind::PriceLookup => {
                let symbol = Self::realtime_entity_before_any(
                    query,
                    &[
                        "价格",
                        "价钱",
                        "多少钱",
                        "多少",
                        "币价",
                        "股价",
                        "报价",
                        "行情",
                        "点数",
                        "指数",
                        "股票",
                        "price",
                        "quote",
                        "points",
                        "index",
                        "stock",
                    ],
                )?;
                if symbol.chars().count() > 48 {
                    return None;
                }
                if Self::price_query_mentions_stock_equity(query)
                    && symbol.chars().any(|ch| !ch.is_ascii())
                {
                    return None;
                }
                ("price_lookup", serde_json::json!({ "symbol": symbol }))
            }
            RealtimeLookupKind::WeatherLookup => {
                let location = Self::realtime_entity_before_any(
                    query,
                    &[
                        "天气",
                        "气温",
                        "温度",
                        "下雨",
                        "降雨",
                        "预报",
                        "weather",
                        "temperature",
                    ],
                )
                .or_else(|| {
                    let lowered = query.to_ascii_lowercase();
                    lowered
                        .split_once("weather in ")
                        .map(|(_, rest)| Self::clean_realtime_entity_candidate(rest))
                })?;
                if location.chars().count() > 48 {
                    return None;
                }
                (
                    "weather_lookup",
                    serde_json::json!({ "location": location }),
                )
            }
            RealtimeLookupKind::LatestInfoLookup => {
                ("latest_info_lookup", serde_json::json!({ "topic": query }))
            }
            RealtimeLookupKind::FxLookup => {
                let (base_currency, quote_currency) =
                    Self::realtime_currency_pair_for_query(query)?;
                (
                    "fx_lookup",
                    serde_json::json!({
                        "base_currency": base_currency,
                        "quote_currency": quote_currency,
                    }),
                )
            }
            RealtimeLookupKind::WebSearch => ("web_search", serde_json::json!({ "query": query })),
        };

        if !self.tools.contains(tool_name) {
            return None;
        }

        Some((
            format!("orchestrated-direct-realtime-{tool_name}"),
            tool_name.to_string(),
            args,
        ))
    }

    fn direct_realtime_followup_tool_call_for_query(
        &self,
        messages: &[Message],
        query: &str,
    ) -> Option<(String, String, serde_json::Value)> {
        if query_requests_followup_execution_after_lookup(query)
            || Self::query_requests_knowledge_persistence(query)
            || Self::query_requests_file_artifact(query)
            || Self::query_requests_artifact_mutation(query)
            || Self::query_requests_explicit_worker_delegation(query)
            || !Self::looks_like_short_realtime_followup(query)
        {
            return None;
        }

        let prior_tool = Self::latest_realtime_tool_name_from_messages(messages)?;
        let entity = Self::realtime_followup_entity_candidate(query)?;
        let (tool_name, args) = match prior_tool.as_str() {
            "weather_lookup" => ("weather_lookup", serde_json::json!({ "location": entity })),
            "price_lookup" => {
                if Self::price_query_mentions_stock_equity(query)
                    && entity.chars().any(|ch| !ch.is_ascii())
                {
                    return None;
                }
                ("price_lookup", serde_json::json!({ "symbol": entity }))
            }
            "fx_lookup" => {
                let (base_currency, quote_currency) =
                    Self::realtime_currency_pair_for_query(query)?;
                (
                    "fx_lookup",
                    serde_json::json!({
                        "base_currency": base_currency,
                        "quote_currency": quote_currency,
                    }),
                )
            }
            "latest_info_lookup" => ("latest_info_lookup", serde_json::json!({ "topic": query })),
            _ => return None,
        };
        if !self.tools.contains(tool_name) {
            return None;
        }
        Some((
            format!("orchestrated-followup-realtime-{tool_name}"),
            tool_name.to_string(),
            args,
        ))
    }

    fn latest_realtime_tool_name_from_messages(messages: &[Message]) -> Option<String> {
        if let Some((tool_name, _)) = Self::latest_successful_tool_result(messages) {
            if Self::is_realtime_lookup_tool(&tool_name) {
                return Some(tool_name);
            }
        }

        messages.iter().rev().find_map(|message| {
            let text = message.text();
            [
                "weather_lookup",
                "price_lookup",
                "fx_lookup",
                "latest_info_lookup",
            ]
            .into_iter()
            .find(|tool| {
                text.contains(&format!("tool:{tool}"))
                    || text.contains(&format!(":tool:{tool}:"))
                    || text.contains(&format!("agent:benshu:tool:{tool}:"))
            })
            .map(str::to_string)
        })
    }

    fn is_realtime_lookup_tool(tool_name: &str) -> bool {
        matches!(
            tool_name,
            "weather_lookup" | "price_lookup" | "fx_lookup" | "latest_info_lookup"
        )
    }

    fn looks_like_short_realtime_followup(query: &str) -> bool {
        let trimmed = query.trim();
        if trimmed.is_empty() || trimmed.chars().count() > 32 {
            return false;
        }
        let lowered = trimmed.to_ascii_lowercase();
        trimmed.contains('呢')
            || trimmed.starts_with("那")
            || trimmed.starts_with("那么")
            || trimmed.starts_with("还有")
            || lowered.starts_with("what about")
            || lowered.starts_with("how about")
            || lowered.starts_with("and ")
    }

    fn realtime_followup_entity_candidate(query: &str) -> Option<String> {
        let mut text = query.trim().to_string();
        for prefix in ["那么", "那", "还有", "再查", "查一下", "看看"] {
            text = text.trim_start_matches(prefix).trim().to_string();
        }
        loop {
            let before = text.clone();
            for suffix in [
                "怎么样",
                "如何",
                "呢",
                "？",
                "?",
                "。",
                "，",
                ",",
                "现在",
                "今天",
                "当前",
            ] {
                text = text.trim_end_matches(suffix).trim().to_string();
            }
            if text == before {
                break;
            }
        }
        text = text
            .trim_start_matches("what about")
            .trim_start_matches("how about")
            .trim_start_matches("and")
            .trim()
            .to_string();
        if text.is_empty() || text.chars().count() > 48 {
            None
        } else {
            Some(Self::clean_realtime_entity_candidate(&text))
        }
    }

    fn realtime_currency_pair_for_query(query: &str) -> Option<(String, String)> {
        let lowered = query.to_lowercase();
        let mut matches = Vec::new();
        for (marker, code) in Self::realtime_currency_markers() {
            if let Some(position) = lowered.find(marker) {
                matches.push((position, *code));
            }
        }
        matches.sort_by_key(|(position, _)| *position);

        let mut ordered = Vec::new();
        for (_, code) in matches {
            if !ordered.contains(&code) {
                ordered.push(code);
            }
        }

        if ordered.len() >= 2 {
            Some((ordered[0].to_string(), ordered[1].to_string()))
        } else {
            None
        }
    }

    fn price_query_mentions_stock_equity(query: &str) -> bool {
        let lowered = query.to_ascii_lowercase();
        query.contains("股票")
            || query.contains("股价")
            || lowered.contains("stock")
            || lowered.contains("share price")
            || lowered.contains("equity")
    }

    fn query_requests_explicit_worker_delegation(query: &str) -> bool {
        let lowered = query.to_ascii_lowercase();
        if lowered.contains("delegate")
            || lowered.contains("delegate to")
            || lowered.contains(" worker ")
            || lowered.contains(" worker,")
            || lowered.contains(" worker.")
            || lowered.contains(" worker:")
        {
            return true;
        }

        let Some((_, rest)) = query.split_once('让') else {
            return false;
        };
        let target = rest
            .trim_start()
            .split(|ch: char| ch.is_whitespace() || matches!(ch, '，' | ',' | '。' | '：' | ':'))
            .next()
            .unwrap_or_default()
            .trim();
        !target.is_empty()
            && target.len() <= 48
            && target
                .chars()
                .any(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    }

    fn realtime_currency_markers() -> &'static [(&'static str, &'static str)] {
        &[
            ("usd", "USD"),
            ("美元", "USD"),
            ("美金", "USD"),
            ("us dollar", "USD"),
            ("eur", "EUR"),
            ("欧元", "EUR"),
            ("euro", "EUR"),
            ("gbp", "GBP"),
            ("英镑", "GBP"),
            ("pound", "GBP"),
            ("cny", "CNY"),
            ("rmb", "CNY"),
            ("人民币", "CNY"),
            ("yuan", "CNY"),
            ("jpy", "JPY"),
            ("日元", "JPY"),
            ("yen", "JPY"),
            ("hkd", "HKD"),
            ("港币", "HKD"),
            ("港元", "HKD"),
            ("aud", "AUD"),
            ("澳元", "AUD"),
            ("cad", "CAD"),
            ("加元", "CAD"),
            ("chf", "CHF"),
            ("瑞郎", "CHF"),
            ("sgd", "SGD"),
            ("新加坡元", "SGD"),
            ("krw", "KRW"),
            ("韩元", "KRW"),
        ]
    }

    fn image_output_path_from_tool_result(content: &str) -> Option<String> {
        tool_delivery::image_output_path_from_tool_result(content)
    }

    fn first_retrieval_snippet(content: &str) -> Option<String> {
        tool_delivery::first_retrieval_snippet(content)
    }

    fn retrieval_snippet_for_query(query: &str, content: &str) -> Option<String> {
        tool_delivery::retrieval_snippet_for_query(query, content)
    }

    fn requested_search_result_count(query: &str) -> usize {
        tool_delivery::requested_search_result_count(query)
    }

    fn delegate_result_summary_block(content: &str) -> Option<String> {
        tool_delivery::delegate_result_summary_block(content)
    }

    fn strip_tool_runtime_notices(content: &str) -> String {
        tool_delivery::strip_tool_runtime_notices(content)
    }

    fn summarize_knowledge_lookup_delivery(
        query: &str,
        content: &str,
        prefers_chinese: bool,
    ) -> String {
        let result = content
            .split_once("result:")
            .map(|(_, result)| result.trim())
            .unwrap_or(content.trim());
        knowledge_delivery::summarize_lookup_delivery(
            query,
            content,
            prefers_chinese,
            Self::first_retrieval_snippet(content),
            Self::compact_tool_result_for_recovery(result),
        )
    }

    fn extract_direct_retrieval_answer(query: &str, snippet: &str) -> Option<String> {
        tool_delivery::extract_direct_retrieval_answer(query, snippet)
    }

    fn summarize_search_history_delivery(
        query: &str,
        content: &str,
        prefers_chinese: bool,
    ) -> String {
        tool_delivery::summarize_search_history_delivery(query, content, prefers_chinese)
    }

    fn summarize_remember_this_delivery(prefers_chinese: bool) -> String {
        tool_delivery::summarize_remember_this_delivery(prefers_chinese)
    }

    fn artifact_delivery_summary_from_result(
        query: &str,
        content: &str,
        prefers_chinese: bool,
    ) -> Option<String> {
        #[derive(Default)]
        struct ArtifactFacts {
            has_artifact_effect: bool,
            paths: Vec<String>,
            project_path: Option<String>,
            chapter_number: Option<u64>,
            chapter_title: Option<String>,
            unit_count: Option<u64>,
            total_units: Option<u64>,
            target_units: Option<u64>,
            quality_passed: Option<bool>,
            summary: Option<String>,
        }

        fn normalized_key(key: &str) -> String {
            key.chars()
                .filter(|ch| *ch != '_' && *ch != '-' && !ch.is_ascii_whitespace())
                .collect::<String>()
                .to_ascii_lowercase()
        }

        fn push_unique(values: &mut Vec<String>, value: &str) {
            let trimmed = value.trim();
            if trimmed.is_empty() || values.iter().any(|existing| existing == trimmed) {
                return;
            }
            values.push(trimmed.to_string());
        }

        fn short_text(value: &str, max_chars: usize) -> String {
            let collapsed = value.split_whitespace().collect::<Vec<_>>().join(" ");
            let mut out = collapsed.chars().take(max_chars).collect::<String>();
            if collapsed.chars().count() > max_chars {
                out.push_str("...");
            }
            out
        }

        fn value_mentions_artifact_effect(value: &serde_json::Value) -> bool {
            match value {
                serde_json::Value::String(text) => text.contains("artifact."),
                serde_json::Value::Array(items) => items.iter().any(value_mentions_artifact_effect),
                serde_json::Value::Object(map) => map.values().any(value_mentions_artifact_effect),
                _ => false,
            }
        }

        fn path_like(value: &str) -> bool {
            let trimmed = value.trim();
            trimmed.contains('/')
                || trimmed.contains('\\')
                || trimmed.ends_with(".txt")
                || trimmed.ends_with(".md")
                || trimmed.ends_with(".pdf")
                || trimmed.ends_with(".html")
        }

        fn collect(value: &serde_json::Value, facts: &mut ArtifactFacts, parent_key: Option<&str>) {
            match value {
                serde_json::Value::Object(map) => {
                    for (key, child) in map {
                        let normalized = normalized_key(key);
                        match normalized.as_str() {
                            "runtimeeffect" | "runtimeeffects" => {
                                if value_mentions_artifact_effect(child) {
                                    facts.has_artifact_effect = true;
                                }
                            }
                            "artifactpath" | "outputpath" | "filepath" | "fullpath"
                            | "exportpath" => {
                                if let Some(path) = child.as_str() {
                                    push_unique(&mut facts.paths, path);
                                }
                            }
                            "path" => {
                                if let Some(path) = child.as_str().filter(|path| path_like(path)) {
                                    push_unique(&mut facts.paths, path);
                                }
                            }
                            "projectpath" => {
                                if facts.project_path.is_none() {
                                    facts.project_path = child
                                        .as_str()
                                        .map(str::trim)
                                        .filter(|s| !s.is_empty())
                                        .map(ToOwned::to_owned);
                                }
                            }
                            "chapternumber" | "chapterindex" => {
                                facts.chapter_number = facts.chapter_number.or(child.as_u64());
                            }
                            "chaptertitle" | "title" => {
                                if facts.chapter_title.is_none()
                                    && parent_key.is_some_and(|parent| {
                                        matches!(
                                            normalized_key(parent).as_str(),
                                            "chapter" | "plan"
                                        )
                                    })
                                {
                                    facts.chapter_title = child
                                        .as_str()
                                        .map(|text| short_text(text, 80))
                                        .filter(|text| !text.is_empty());
                                }
                            }
                            "unitcount" | "wordcount" | "charcount" | "characters"
                            | "reportedunits" => {
                                facts.unit_count = facts.unit_count.or(child.as_u64());
                            }
                            "totalunits" | "totalwordcount" | "totalcharcount" => {
                                facts.total_units = facts.total_units.or(child.as_u64());
                            }
                            "targetunits" | "targetwordcount" | "targetcharcount" => {
                                facts.target_units = facts.target_units.or(child.as_u64());
                            }
                            "qualitygate" | "review" | "audit" => {
                                if let Some(passed) =
                                    child.get("passed").and_then(|value| value.as_bool())
                                {
                                    facts.quality_passed = Some(passed);
                                }
                            }
                            "passed" => {
                                if parent_key.is_some_and(|parent| {
                                    matches!(
                                        normalized_key(parent).as_str(),
                                        "qualitygate" | "review" | "audit"
                                    )
                                }) {
                                    facts.quality_passed = facts.quality_passed.or(child.as_bool());
                                }
                            }
                            "summary" | "briefsummary" | "chaptersummary" | "abstract" => {
                                if facts.summary.is_none() {
                                    facts.summary = child
                                        .as_str()
                                        .map(|text| short_text(text, 260))
                                        .filter(|text| !text.is_empty());
                                }
                            }
                            "number" => {
                                if parent_key.is_some_and(|parent| {
                                    normalized_key(parent).as_str() == "chapter"
                                }) {
                                    facts.chapter_number = facts.chapter_number.or(child.as_u64());
                                }
                            }
                            "content" | "body" | "text" | "fulltext" | "draft" => {
                                continue;
                            }
                            _ => {}
                        }
                        collect(child, facts, Some(key));
                    }
                }
                serde_json::Value::Array(items) => {
                    for item in items {
                        collect(item, facts, parent_key);
                    }
                }
                _ => {}
            }
        }

        let value = Self::parse_tool_result_json(content).or_else(|| {
            Self::tool_result_json_blob(content)
                .and_then(|blob| serde_json::from_str::<serde_json::Value>(blob).ok())
        })?;
        let mut facts = ArtifactFacts::default();
        collect(&value, &mut facts, None);

        let artifact_requested = Self::query_requests_artifact_mutation(query)
            || Self::query_requests_file_artifact(query);
        if !facts.has_artifact_effect && facts.paths.is_empty() && !artifact_requested {
            return None;
        }
        if facts.paths.is_empty() && !facts.has_artifact_effect {
            return None;
        }

        let path = facts.paths.first().cloned();
        let review = facts.quality_passed.map(|passed| {
            if prefers_chinese {
                if passed {
                    "通过".to_string()
                } else {
                    "需要修订".to_string()
                }
            } else if passed {
                "passed".to_string()
            } else {
                "needs revision".to_string()
            }
        });

        if prefers_chinese {
            let mut lines = vec!["已保存写作/文件产物检查点。".to_string()];
            if let Some(chapter) = facts.chapter_number {
                let title = facts
                    .chapter_title
                    .as_deref()
                    .map(|title| format!("：{title}"))
                    .unwrap_or_default();
                lines.push(format!("- 章节：第 {chapter} 章{title}"));
            }
            let mut counts = Vec::new();
            if let Some(unit_count) = facts.unit_count {
                counts.push(format!("本次 {unit_count}"));
            }
            if let Some(total_units) = facts.total_units {
                counts.push(format!("累计 {total_units}"));
            }
            if let Some(target_units) = facts.target_units {
                counts.push(format!("目标 {target_units}"));
            }
            if !counts.is_empty() {
                lines.push(format!("- 字数/单位：{}", counts.join(" / ")));
            }
            if let Some(path) = path {
                lines.push(format!("- 文件：{path}"));
            }
            if let Some(project_path) = facts.project_path {
                lines.push(format!("- 项目：{project_path}"));
            }
            lines.push(format!(
                "- 审查：{}",
                review.unwrap_or_else(|| "未提供".to_string())
            ));
            if let Some(summary) = facts.summary {
                lines.push(format!("- 摘要：{summary}"));
            }
            return Some(lines.join("\n"));
        }

        let mut lines = vec!["Saved a writing/file artifact checkpoint.".to_string()];
        if let Some(chapter) = facts.chapter_number {
            let title = facts
                .chapter_title
                .as_deref()
                .map(|title| format!(": {title}"))
                .unwrap_or_default();
            lines.push(format!("- Chapter: {chapter}{title}"));
        }
        let mut counts = Vec::new();
        if let Some(unit_count) = facts.unit_count {
            counts.push(format!("this step {unit_count}"));
        }
        if let Some(total_units) = facts.total_units {
            counts.push(format!("total {total_units}"));
        }
        if let Some(target_units) = facts.target_units {
            counts.push(format!("target {target_units}"));
        }
        if !counts.is_empty() {
            lines.push(format!("- Units: {}", counts.join(" / ")));
        }
        if let Some(path) = path {
            lines.push(format!("- File: {path}"));
        }
        if let Some(project_path) = facts.project_path {
            lines.push(format!("- Project: {project_path}"));
        }
        lines.push(format!(
            "- Review: {}",
            review.unwrap_or_else(|| "not provided".to_string())
        ));
        if let Some(summary) = facts.summary {
            lines.push(format!("- Summary: {summary}"));
        }
        Some(lines.join("\n"))
    }

    fn summarize_delegate_delivery(query: &str, content: &str, prefers_chinese: bool) -> String {
        let cleaned_content = Self::strip_tool_runtime_notices(content);
        let content = cleaned_content.as_str();
        let lowered = content.to_ascii_lowercase();
        if Self::tool_result_content_is_runtime_error(content) {
            return if prefers_chinese {
                if Self::query_requests_knowledge_persistence(query) {
                    format!(
                        "我已经尝试把任务交给 specialist 执行，但这次委派链路失败了，所以没有把不可靠结果写入知识库。\n\n当前具体卡点：{}",
                        Self::compact_tool_result_for_recovery(content)
                    )
                } else {
                    format!(
                        "我已经尝试把任务交给 specialist 执行，但这次委派链路失败了，所以没有拿到可靠结果。\n\n当前具体卡点：{}",
                        Self::compact_tool_result_for_recovery(content)
                    )
                }
            } else {
                if Self::query_requests_knowledge_persistence(query) {
                    format!(
                        "I attempted to delegate the task to a specialist, but the delegated execution failed, so I did not save unreliable results into the knowledge base.\n\nCurrent blocker: {}",
                        Self::compact_tool_result_for_recovery(content)
                    )
                } else {
                    format!(
                        "I attempted to delegate the task to a specialist, but the delegated execution failed, so no reliable result was obtained.\n\nCurrent blocker: {}",
                        Self::compact_tool_result_for_recovery(content)
                    )
                }
            };
        }
        if Self::is_pseudo_tool_call_leak(content) {
            return if prefers_chinese {
                "委派 worker 返回了未执行的工具调用标签，系统已经阻止把它当作成功结果交付。当前没有拿到真实工具执行结果，所以这一步还没有完成。".to_string()
            } else {
                "The delegated worker returned an unexecuted tool-call tag. The runtime blocked it from being accepted as a successful result, and no real tool execution result was received yet.".to_string()
            };
        }
        let is_incomplete = lowered.contains("status: incomplete")
            || lowered.contains("\"status\":\"incomplete\"")
            || lowered.contains("\"status\": \"incomplete\"");
        let is_blocked = lowered.contains("status: blocked");
        let executed_tool = if lowered.contains("executed_tool: web_search") {
            Some("web_search")
        } else if lowered.contains("executed_tool: web_fetch") {
            Some("web_fetch")
        } else if lowered.contains("executed_tool: knowledge_import_url") {
            Some("knowledge_import_url")
        } else if lowered.contains("executed_tool: tiered_search")
            || lowered.contains("executed_tool: knowledge_search")
            || lowered.contains("executed_tool: fetch_document")
        {
            Some("knowledge_lookup")
        } else {
            None
        };

        if is_incomplete {
            if matches!(executed_tool, Some("web_search" | "web_fetch"))
                || lowered.contains("worker: researcher")
            {
                return if prefers_chinese {
                    if Self::query_requests_knowledge_persistence(query) {
                        "我已经尝试执行外部检索，但当前只拿到不完整或未验证的检索证据，不能把它当作可靠结果，也不能继续写入知识库。下一步需要换来源或重新检索。".to_string()
                    } else {
                        "我已经尝试执行外部检索，但当前只拿到不完整或未验证的检索证据，不能把它当作可靠答案交付。下一步需要换来源或重新检索。".to_string()
                    }
                } else if Self::query_requests_knowledge_persistence(query) {
                    "I attempted the external lookup, but the current evidence is incomplete or unverified, so it cannot be treated as a reliable result or imported into the knowledge base. The next step should use a different source or retry the lookup.".to_string()
                } else {
                    "I attempted the external lookup, but the current evidence is incomplete or unverified, so it cannot be delivered as a reliable answer. The next step should use a different source or retry the lookup.".to_string()
                };
            }

            return if prefers_chinese {
                "委派执行只返回了不完整结果，当前还不能声明任务完成。".to_string()
            } else {
                "The delegated execution only returned an incomplete result, so this task cannot be claimed as complete yet.".to_string()
            };
        }

        if is_blocked {
            if lowered.contains("worker: researcher") {
                return if prefers_chinese {
                    if Self::query_requests_knowledge_persistence(query) {
                        "我已经尝试执行外部检索，但当前搜索引擎或目标站点返回了反爬虫/验证页面，暂时无法稳定拿到可用来源，所以这一步还不能安全继续写入知识库。".to_string()
                    } else {
                        "我已经尝试执行外部检索，但当前搜索引擎或目标站点返回了反爬虫/验证页面，暂时无法稳定拿到可用来源，所以这一步不能作为可靠搜索结果交付。".to_string()
                    }
                } else {
                    if Self::query_requests_knowledge_persistence(query) {
                        "I attempted the external lookup, but the search engine or target site returned an anti-bot or verification page, so I could not reliably obtain a usable source and cannot safely continue the knowledge-base import yet.".to_string()
                    } else {
                        "I attempted the external lookup, but the search engine or target site returned an anti-bot or verification page, so I could not reliably obtain a usable source for this search result.".to_string()
                    }
                };
            }

            if lowered.contains("worker: knowledge") {
                return if prefers_chinese {
                    "我已经尝试执行知识库写入，但当前还没有拿到可用的具体来源链接，所以暂时不能继续导入。".to_string()
                } else {
                    "I attempted the knowledge-base import, but there is still no usable concrete source URL, so the import cannot continue yet.".to_string()
                };
            }

            return if prefers_chinese {
                "委派执行遇到了外部阻塞，当前还不能继续完成这一步。".to_string()
            } else {
                "The delegated execution hit an external blocker and cannot safely continue this step yet.".to_string()
            };
        }

        if matches!(executed_tool, Some("web_search")) {
            if Self::query_requests_knowledge_persistence(query) {
                if let Some(url) = Self::best_lookup_source_url_for_query(query, content) {
                    return if prefers_chinese {
                        format!(
                            "我已经完成初步检索，并找到了可继续处理的来源：{}。下一步应继续抓取正文并写入知识库。",
                            url
                        )
                    } else {
                        format!(
                            "The initial search completed and found a usable source URL: {}. The next step should fetch the source content and save it into the knowledge base.",
                            url
                        )
                    };
                }

                return if prefers_chinese {
                    "我已经完成初步检索，但当前结果里还没有拿到足够可靠的目标来源，暂时不能安全写入知识库。下一步应该改写检索词或继续抓取更可靠的来源。".to_string()
                } else {
                    "The initial search completed, but the current results do not yet contain a reliable target source that can be safely imported into the knowledge base. The next step should refine the lookup or fetch a more reliable source.".to_string()
                };
            }

            if let Some(snippet) = Self::retrieval_snippet_for_query(query, content) {
                return if prefers_chinese {
                    if Self::requested_search_result_count(query) > 1 {
                        format!("我已经完成初步检索，当前最相关候选是：\n{}", snippet)
                    } else {
                        format!("我已经完成初步检索，当前最相关结果是：{}", snippet)
                    }
                } else {
                    if Self::requested_search_result_count(query) > 1 {
                        format!(
                            "The initial search completed. The most relevant current candidates are:\n{}",
                            snippet
                        )
                    } else {
                        format!(
                            "The initial search completed. The most relevant current result is: {}",
                            snippet
                        )
                    }
                };
            }

            return if prefers_chinese {
                "我已经执行了外部检索，但这次没有拿到可交付的可靠候选来源。当前不应该把空结果或低质量页面当作答案。".to_string()
            } else {
                "I ran the external lookup, but it did not produce a reliable candidate source that can be delivered as an answer.".to_string()
            };
        }

        if matches!(executed_tool, Some("web_fetch")) {
            let source_url = Self::explicit_source_url_in_result(content)
                .or_else(|| Self::best_lookup_source_url_for_query(query, content));
            if Self::query_requests_knowledge_persistence(query)
                && Self::content_contains_verification_challenge(content)
            {
                return if prefers_chinese {
                    if let Some(url) = source_url {
                        format!(
                            "我已经找到候选来源并继续抓取了 `{}`，但目标站点返回的是安全验证/反爬页面，不是真正的正文，所以这一步不能安全写入知识库。",
                            url
                        )
                    } else {
                        "我已经继续抓取候选来源，但目标站点返回的是安全验证/反爬页面，不是真正的正文，所以这一步不能安全写入知识库。".to_string()
                    }
                } else if let Some(url) = source_url {
                    format!(
                        "I fetched the candidate source `{}`, but the target site returned an anti-bot or verification page instead of the actual content, so this cannot be safely imported into the knowledge base.",
                        url
                    )
                } else {
                    "I fetched the candidate source, but the target site returned an anti-bot or verification page instead of the actual content, so this cannot be safely imported into the knowledge base.".to_string()
                };
            }

            if let Some(summary) = Self::delegate_result_summary_block(content) {
                return if prefers_chinese {
                    if Self::requested_search_result_count(query) > 1
                        || summary
                            .lines()
                            .filter(|line| line.trim_start().starts_with('-'))
                            .count()
                            > 1
                    {
                        format!("我已经完成检索，当前候选是：\n{}", summary)
                    } else {
                        format!("我已经完成检索，当前结果是：{}", summary)
                    }
                } else if Self::requested_search_result_count(query) > 1 {
                    format!(
                        "The lookup completed. The current candidates are:\n{}",
                        summary
                    )
                } else {
                    format!("The lookup completed. Current result: {}", summary)
                };
            }

            if let Some(snippet) = Self::retrieval_snippet_for_query(query, content) {
                let snippet = if let Some(url) =
                    source_url.filter(|url| !snippet.contains(url)).filter(|_| {
                        query.contains("来源") || query.to_ascii_lowercase().contains("source")
                    }) {
                    if prefers_chinese {
                        format!("{}\n来源：{}", snippet, url)
                    } else {
                        format!("{}\nSource: {}", snippet, url)
                    }
                } else {
                    snippet
                };
                return if prefers_chinese {
                    format!("我已经完成检索，当前结果是：{}", snippet)
                } else {
                    format!("The lookup completed. Current result: {}", snippet)
                };
            }
        }

        if matches!(executed_tool, Some("knowledge_import_url")) {
            if let Some(summary) = Self::extract_knowledge_import_summary(content) {
                return if prefers_chinese {
                    format!("我已经完成知识库写入：{}。", summary)
                } else {
                    format!(
                        "The knowledge-base import completed successfully: {}.",
                        summary
                    )
                };
            }

            return if prefers_chinese {
                "我已经完成知识库写入。".to_string()
            } else {
                "The knowledge-base import completed successfully.".to_string()
            };
        }

        if matches!(executed_tool, Some("knowledge_lookup")) {
            return Self::summarize_knowledge_lookup_delivery(query, content, prefers_chinese);
        }

        if let Some(summary) =
            Self::artifact_delivery_summary_from_result(query, content, prefers_chinese)
        {
            return summary;
        }

        if Self::query_requests_artifact_mutation(query)
            && !Self::tool_result_satisfies_artifact_request(query, content)
        {
            let compact = Self::strip_spurious_completion_claims(
                &Self::compact_tool_result_for_recovery(content),
            );
            return if prefers_chinese {
                format!(
                    "委派 worker 已返回中间结果，但还没有产生可验证的本地产物写入回执，所以这一步不能声明完成。\n\n当前具体卡点：{}",
                    compact.trim()
                )
            } else {
                format!(
                    "The delegated worker returned an intermediate result, but no verifiable local artifact write receipt was produced, so this step cannot be claimed as complete yet.\n\nCurrent blocker: {}",
                    compact.trim()
                )
            };
        }

        if matches!(executed_tool, Some("web_fetch")) {
            let source_url = Self::explicit_source_url_in_result(content)
                .or_else(|| Self::best_lookup_source_url_for_query(query, content));
            if Self::query_requests_knowledge_persistence(query) {
                if Self::content_contains_verification_challenge(content) {
                    return if prefers_chinese {
                        if let Some(url) = source_url {
                            format!(
                                "我已经找到候选来源并继续抓取了 `{}`，但目标站点返回的是安全验证/反爬页面，不是真正的论文正文，所以这一步不能安全写入知识库。",
                                url
                            )
                        } else {
                            "我已经继续抓取候选来源，但目标站点返回的是安全验证/反爬页面，不是真正的正文，所以这一步不能安全写入知识库。".to_string()
                        }
                    } else if let Some(url) = source_url {
                        format!(
                            "I fetched the candidate source `{}`, but the target site returned an anti-bot or verification page instead of the actual article body, so this cannot be safely imported into the knowledge base.",
                            url
                        )
                    } else {
                        "I fetched the candidate source, but the target site returned an anti-bot or verification page instead of the actual content, so this cannot be safely imported into the knowledge base.".to_string()
                    };
                }

                if let Some(url) = source_url {
                    return if prefers_chinese {
                        format!(
                            "我已经完成来源抓取，并拿到了可继续写入知识库的候选来源：{}。下一步应执行一次正式入库，而不是重新搜索。",
                            url
                        )
                    } else {
                        format!(
                            "The source fetch completed and produced an importable candidate URL: {}. The next step should perform one bounded knowledge-base import instead of re-running the search.",
                            url
                        )
                    };
                }
            }
        }

        let compact_result = Self::compact_tool_result_for_recovery(content);
        if prefers_chinese {
            format!(
                "已完成委派执行，specialist 返回结果如下：{}",
                compact_result.trim()
            )
        } else {
            format!(
                "Delegation completed successfully. Specialist result: {}",
                compact_result.trim()
            )
        }
    }

    fn synthesize_successful_tool_delivery(messages: &[Message]) -> Option<String> {
        let query = Self::latest_user_query(messages)?;
        let persistence_query = Self::latest_knowledge_persistence_query(messages);
        let followup_query = persistence_query.as_deref().unwrap_or(&query);
        let prefers_chinese = Self::query_prefers_chinese(&query);
        let (tool_name, raw_content) = Self::latest_successful_tool_result(messages)?;
        let content = Self::strip_tool_runtime_notices(&raw_content);

        if turn_state::tool_result_is_blocked(&content) {
            return Some(Self::summarize_blocked_tool_delivery(
                &query,
                &tool_name,
                &content,
                prefers_chinese,
            ));
        }

        if tool_name == "generate_image" {
            if let Some(path) = Self::image_output_path_from_tool_result(&content) {
                return Some(if prefers_chinese {
                    format!("图片已经生成完成，文件已保存到：{}", path)
                } else {
                    format!(
                        "The image has been generated successfully and saved to: {}",
                        path
                    )
                });
            }

            return Some(if prefers_chinese {
                format!("图片已经生成完成。{}", content.trim())
            } else {
                format!("The image was generated successfully. {}", content.trim())
            });
        }

        if tool_name == "search_history" {
            return Some(Self::summarize_search_history_delivery(
                &query,
                &content,
                prefers_chinese,
            ));
        }

        if tool_name == "remember_this" {
            return Some(Self::summarize_remember_this_delivery(prefers_chinese));
        }

        if tool_name == "manage_facts" {
            let lowered = content.to_ascii_lowercase();
            if lowered.contains("deleted") || content.contains("删除") {
                return Some(if prefers_chinese {
                    "我已经删除了。".to_string()
                } else {
                    "I have deleted it.".to_string()
                });
            }
            if lowered.contains("upserted") || lowered.contains("saved") || content.contains("记住")
            {
                return Some(if prefers_chinese {
                    "我已经更新了。".to_string()
                } else {
                    "I have updated it.".to_string()
                });
            }
        }

        if matches!(tool_name.as_str(), "knowledge_search" | "tiered_search") {
            if let Some(snippet) = Self::first_retrieval_snippet(&content) {
                if let Some(answer) = Self::extract_direct_retrieval_answer(&query, &snippet) {
                    return Some(if prefers_chinese {
                        format!("根据知识库，答案是：{}", answer)
                    } else {
                        format!("According to the knowledge base, the answer is: {}", answer)
                    });
                }
                return Some(if prefers_chinese {
                    format!("我在知识库里找到的最相关内容是：{}", snippet)
                } else {
                    format!("The most relevant knowledge-base result says: {}", snippet)
                });
            }
        }

        if let Some(display) = Self::tool_result_display_text(&content, prefers_chinese) {
            return Some(display);
        }

        if let Some(summary) =
            Self::artifact_delivery_summary_from_result(&query, &content, prefers_chinese)
        {
            return Some(summary);
        }

        if Self::observation_tool_result_cannot_satisfy_durable_goal(&tool_name, followup_query) {
            return None;
        }

        if tool_name == "delegate" {
            if Self::query_requests_knowledge_persistence(followup_query)
                && Self::latest_delegate_role(messages)
                    .as_deref()
                    .is_none_or(|role| role != "knowledge")
                && Self::should_prioritize_followup_execution(followup_query, messages)
            {
                return None;
            }
            return Some(Self::summarize_delegate_delivery(
                &query,
                &content,
                prefers_chinese,
            ));
        }

        Some(if prefers_chinese {
            format!(
                "请求已执行完成。工具 `{}` 的结果如下：{}",
                tool_name,
                content.trim()
            )
        } else {
            format!(
                "The requested action completed successfully. Tool `{}` returned: {}",
                tool_name,
                content.trim()
            )
        })
    }

    fn tool_result_display_text(content: &str, prefers_chinese: bool) -> Option<String> {
        let value = Self::parse_tool_result_json(content)?;
        let display = value.get("display")?;
        if let Some(text) = display
            .as_str()
            .map(str::trim)
            .filter(|text| !text.is_empty())
        {
            return Some(text.to_string());
        }

        let locale_key = if prefers_chinese { "zh" } else { "en" };
        display
            .get(locale_key)
            .and_then(|value| value.as_str())
            .or_else(|| display.get("default").and_then(|value| value.as_str()))
            .or_else(|| display.get("text").and_then(|value| value.as_str()))
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(ToOwned::to_owned)
    }

    fn parse_tool_result_json(content: &str) -> Option<serde_json::Value> {
        let trimmed = content.trim();
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
            return Some(value);
        }

        let prefix = trimmed
            .split_once("\n\n---\n")
            .map(|(prefix, _)| prefix.trim())
            .or_else(|| {
                trimmed
                    .split_once("\n---\n")
                    .map(|(prefix, _)| prefix.trim())
            })?;
        serde_json::from_str::<serde_json::Value>(prefix).ok()
    }

    fn strip_spurious_completion_claims(text: &str) -> String {
        text.replace("请求已执行完成。", "")
            .replace("已完成委派执行，", "")
            .replace("已完成委派执行。", "")
            .replace("Delegation completed successfully.", "")
            .trim()
            .to_string()
    }

    fn strip_model_channel_markers(text: &str) -> String {
        text.replace("<|channel>thought\n<channel|>", "")
            .replace("<|channel>thought\r\n<channel|>", "")
            .replace("<|channel>final\n<channel|>", "")
            .replace("<|channel>final\r\n<channel|>", "")
            .replace("<|channel>analysis\n<channel|>", "")
            .replace("<|channel>analysis\r\n<channel|>", "")
            .replace("<|channel>commentary\n<channel|>", "")
            .replace("<|channel>commentary\r\n<channel|>", "")
            .replace("<|channel>thought", "")
            .replace("<|channel>final", "")
            .replace("<|channel>analysis", "")
            .replace("<|channel>commentary", "")
            .replace("<channel|>", "")
            .trim()
            .to_string()
    }

    fn model_output_is_empty_or_non_deliverable(text: &str) -> bool {
        let stripped = Self::strip_model_channel_markers(text);
        if stripped.trim().is_empty() {
            return true;
        }

        let trimmed = text.trim();
        let lower = trimmed.to_ascii_lowercase();
        let has_channel_marker = lower.contains("<|channel>");
        let has_deliverable_channel = lower.contains("<|channel>final")
            || lower.contains("<|channel>commentary")
            || lower.contains("<|channel>assistant");
        let has_only_internal_channel = has_channel_marker
            && !has_deliverable_channel
            && (lower.starts_with("<|channel>thought")
                || lower.starts_with("<|channel>analysis")
                || lower.starts_with("<|channel>reasoning"));

        if !has_only_internal_channel {
            return false;
        }

        let meaningful = stripped
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .filter(|line| {
                let lower = line.to_ascii_lowercase();
                !matches!(
                    lower.as_str(),
                    "thought"
                        | "analysis"
                        | "reasoning"
                        | "we need answer"
                        | "need answer"
                        | "i need answer"
                        | "we need to answer"
                )
            })
            .take(2)
            .count();

        meaningful == 0
    }

    fn reflexion_critique_reports_missing_response(reason: &str) -> bool {
        let lower = reason.to_ascii_lowercase();
        let compact = lower
            .chars()
            .map(|ch| if ch.is_whitespace() { ' ' } else { ch })
            .collect::<String>();
        [
            "response is empty",
            "response itself is missing",
            "no content to critique",
            "no last response",
            "no \"last response\"",
            "last response\" being critiqued is not provided",
            "last response\" provided",
            "target response\" provided",
            "there must be a \"target response\"",
            "without the original context",
            "non-existent response",
            "missing response",
            "empty/missing",
        ]
        .iter()
        .any(|needle| compact.contains(needle))
    }

    fn empty_worker_response_blocker(messages: &[Message]) -> String {
        let latest_query = Self::latest_user_query(messages).unwrap_or_default();
        let blocker = if Self::query_prefers_chinese(&latest_query) {
            "当前 worker 连续返回空响应，未产生可执行工具调用、可验证证据或可交付内容。系统已停止本轮空转，避免继续消耗资源。请稍后重试，或调整 worker/模型配置后继续。"
        } else {
            "The current worker returned empty responses repeatedly and produced no executable tool call, verifiable evidence, or deliverable content. The runtime stopped this loop to avoid wasting resources. Retry later or adjust the worker/model configuration before continuing."
        };
        format!(
            "status: blocked\nerror_kind: empty_worker_response\nblockers: {blocker}\nnext_step_hint: retry with a configured worker/tool path that can produce observable evidence, a durable artifact receipt, or a concise blocker"
        )
    }

    fn observation_tool_result_cannot_satisfy_durable_goal(tool_name: &str, query: &str) -> bool {
        matches!(
            tool_name,
            "web_search" | "web_fetch" | "browser_browse" | "browser_observe"
        ) && (Self::query_requests_knowledge_persistence(query)
            || Self::query_requests_file_artifact(query))
    }

    fn synthesize_incomplete_tool_delivery_for_recovery(
        messages: &[Message],
        query: &str,
    ) -> Option<String> {
        let (tool_name, raw_content) = Self::latest_successful_tool_result(messages)?;
        let content = Self::strip_tool_runtime_notices(&raw_content);
        let prefers_chinese = Self::query_prefers_chinese(query);

        if matches!(
            tool_name.as_str(),
            "web_search" | "web_fetch" | "browser_browse" | "browser_observe"
        ) {
            let compact = Self::compact_tool_result_for_recovery(&content);
            let evidence_header =
                Self::incomplete_observation_recovery_header(query, &tool_name, &content);
            if Self::query_requests_knowledge_persistence(query) {
                return Some(if prefers_chinese {
                    format!(
                        "{evidence_header}我已经执行了检索/抓取，但这一步还没有产生知识库写入回执，所以不能声明入库完成。\n\n当前可用证据：{}",
                        compact.trim()
                    )
                } else {
                    format!(
                        "{evidence_header}I ran the lookup/fetch, but this step did not produce a knowledge-base import receipt yet, so it cannot be treated as completed.\n\nCurrent evidence: {}",
                        compact.trim()
                    )
                });
            }
            if Self::query_requests_file_artifact(query) {
                return Some(if prefers_chinese {
                    format!(
                        "{evidence_header}我已经执行了检索/抓取，但这一步还没有产生本地产物写入回执，所以不能声明文件已完成。\n\n当前可用证据：{}",
                        compact.trim()
                    )
                } else {
                    format!(
                        "{evidence_header}I ran the lookup/fetch, but this step did not produce a local artifact write receipt yet, so it cannot be treated as completed.\n\nCurrent evidence: {}",
                        compact.trim()
                    )
                });
            }
        }

        None
    }

    fn incomplete_observation_recovery_header(
        query: &str,
        tool_name: &str,
        content: &str,
    ) -> String {
        let mut lines = vec![
            "status: incomplete".to_string(),
            format!("executed_tool: {tool_name}"),
        ];
        if let Some(url) = Self::explicit_source_url_in_result(content)
            .or_else(|| Self::best_lookup_source_url_for_query(query, content))
        {
            lines.push(format!("source_url: {url}"));
        }
        lines.join("\n") + "\n\n"
    }

    fn summarize_blocked_tool_delivery(
        query: &str,
        tool_name: &str,
        content: &str,
        prefers_chinese: bool,
    ) -> String {
        let compact = Self::compact_tool_result_for_recovery(content);
        let lookup_tool = matches!(
            tool_name,
            "web_search" | "web_fetch" | "tool_search" | "browser_browse"
        );
        if prefers_chinese {
            if lookup_tool {
                if Self::query_requests_knowledge_persistence(query) {
                    format!(
                        "我已经尝试执行检索，但当前没有拿到可用于继续入库的可靠来源，所以还不能声明知识库写入完成。\n\n当前具体卡点：{}",
                        compact.trim()
                    )
                } else {
                    format!(
                        "我已经尝试执行检索，但当前没有拿到可靠结果，不能把这一步当作完成。\n\n当前具体卡点：{}",
                        compact.trim()
                    )
                }
            } else {
                format!(
                    "工具 `{}` 返回了阻塞状态，当前不能把这一步当作完成。\n\n当前具体卡点：{}",
                    tool_name,
                    compact.trim()
                )
            }
        } else if lookup_tool {
            if Self::query_requests_knowledge_persistence(query) {
                format!(
                    "I attempted the lookup, but it did not produce a reliable source that can be imported into the knowledge base yet.\n\nCurrent blocker: {}",
                    compact.trim()
                )
            } else {
                format!(
                    "I attempted the lookup, but it did not produce a reliable result, so this step cannot be treated as complete.\n\nCurrent blocker: {}",
                    compact.trim()
                )
            }
        } else {
            format!(
                "Tool `{}` returned a blocked status, so this step cannot be treated as complete yet.\n\nCurrent blocker: {}",
                tool_name,
                compact.trim()
            )
        }
    }

    fn max_step_tokens(&self) -> usize {
        if self.provider.is_local() {
            reasoner_constants::LOCAL_MAX_STEP_TOKENS
        } else {
            reasoner_constants::API_MAX_STEP_TOKENS
        }
    }

    fn effective_llm_timeout(&self) -> Duration {
        self.config.llm_timeout
    }

    fn effective_llm_timeout_for_request(
        &self,
        request: &crate::agent::provider::ChatRequest,
    ) -> Duration {
        let configured = self.effective_llm_timeout();
        if !self.provider.is_local() {
            return configured;
        }

        if let Some(stage) = Self::creation_planning_stage_for_request(request) {
            let (floor_secs, cap_secs) = match stage {
                CreationPlanningStage::Skeleton => (45, 90),
                CreationPlanningStage::Characters => (60, 120),
                CreationPlanningStage::Plot => (75, 150),
                CreationPlanningStage::Governance => (60, 120),
                CreationPlanningStage::Generic => (60, 150),
            };
            return configured
                .max(Duration::from_secs(floor_secs))
                .min(Duration::from_secs(cap_secs));
        }

        let requested_output_tokens = request
            .max_tokens
            .unwrap_or(reasoner_constants::LOCAL_MAX_STEP_TOKENS as u64)
            .max(1);
        let timeout_floor_secs =
            Self::local_timeout_floor_secs_for_output_budget(requested_output_tokens);
        let scaled_secs = (requested_output_tokens
            / reasoner_constants::LOCAL_OUTPUT_TOKENS_PER_TIMEOUT_SEC)
            .clamp(
                timeout_floor_secs,
                reasoner_constants::LOCAL_MAX_LLM_TIMEOUT_SECS,
            );

        configured.max(Duration::from_secs(scaled_secs))
    }

    fn creation_planning_stage_for_request(
        request: &crate::agent::provider::ChatRequest,
    ) -> Option<CreationPlanningStage> {
        request
            .messages
            .iter()
            .rev()
            .find(|message| message.role == Role::User)
            .and_then(|message| Self::creation_planning_stage_for_query(&message.text()))
    }

    fn local_timeout_floor_secs_for_output_budget(max_tokens: u64) -> u64 {
        if max_tokens <= reasoner_constants::SHORT_ANSWER_MAX_TOKENS {
            reasoner_constants::LOCAL_SHORT_LLM_TIMEOUT_SECS
        } else if max_tokens <= reasoner_constants::EXPLANATION_MAX_TOKENS {
            reasoner_constants::LOCAL_MEDIUM_LLM_TIMEOUT_SECS
        } else if max_tokens <= reasoner_constants::ARTIFACT_STEP_MAX_TOKENS {
            reasoner_constants::LOCAL_ARTIFACT_LLM_TIMEOUT_SECS
        } else {
            reasoner_constants::LOCAL_MIN_LLM_TIMEOUT_SECS
        }
    }

    fn context_limit_blocker_text(
        context_error: &ContextLimitError,
        latest_user_input: Option<&str>,
    ) -> String {
        let prefers_chinese = Self::query_prefers_chinese(latest_user_input.unwrap_or_default());
        context_error.as_user_blocker(prefers_chinese)
    }

    fn output_contract_for_turn(
        &self,
        tools_visible: bool,
        direct_capability_route: Option<CapabilityRouteHint>,
        coordinator_task_mode: CoordinatorTaskMode,
        messages: &[Message],
    ) -> output_contract::OutputContract {
        if Self::latest_user_query(messages)
            .as_deref()
            .is_some_and(Self::query_is_creation_planning_dialogue)
        {
            let max_tokens = Self::creation_planning_dialogue_max_tokens(
                Self::latest_user_query(messages)
                    .as_deref()
                    .unwrap_or_default(),
                self.config
                    .max_tokens
                    .map(|tokens| tokens.min(self.max_step_tokens() as u64)),
            );
            return output_contract::OutputContract {
                kind: output_contract::OutputContractKind::Explanation,
                surface: output_contract::OutputSurface::Chat,
                max_tokens,
                requires_background: false,
                requires_artifact: false,
            };
        }

        if Self::turn_requires_generated_artifact_content(messages) {
            let latest_user_text = Self::latest_user_query(messages);
            let configured = self
                .config
                .max_tokens
                .map(|tokens| tokens.min(self.max_step_tokens() as u64))
                .unwrap_or(self.max_step_tokens() as u64);
            let kind = if latest_user_text
                .as_deref()
                .is_some_and(|text| output_contract::text_looks_like_longform(text))
            {
                output_contract::OutputContractKind::Longform
            } else {
                output_contract::OutputContractKind::Artifact
            };
            return output_contract::OutputContract {
                kind,
                surface: output_contract::OutputSurface::Artifact,
                max_tokens: reasoner_constants::LONGFORM_STEP_MAX_TOKENS
                    .min(configured)
                    .max(1),
                requires_background: true,
                requires_artifact: true,
            };
        }

        let configured = self
            .config
            .max_tokens
            .map(|tokens| tokens.min(self.max_step_tokens() as u64));
        let latest_user_text = Self::latest_user_query(messages);
        let execution_turn = tools_visible
            && (direct_capability_route.is_some()
                || Self::has_recent_tool_execution_required_prompt(messages)
                || matches!(
                    coordinator_task_mode,
                    CoordinatorTaskMode::ToolAgent
                        | CoordinatorTaskMode::DocumentLite
                        | CoordinatorTaskMode::VisionLite
                ));
        output_contract::resolve_output_contract(output_contract::OutputContractInput {
            latest_user_text: latest_user_text.as_deref(),
            tools_visible,
            execution_turn,
            direct_capability_route,
            coordinator_task_mode,
            configured_ceiling: configured,
            max_step_tokens: self.max_step_tokens() as u64,
        })
    }

    fn request_max_tokens_for_turn(
        &self,
        tools_visible: bool,
        direct_capability_route: Option<CapabilityRouteHint>,
        coordinator_task_mode: CoordinatorTaskMode,
        messages: &[Message],
    ) -> Option<u64> {
        Some(
            self.output_contract_for_turn(
                tools_visible,
                direct_capability_route,
                coordinator_task_mode,
                messages,
            )
            .max_tokens,
        )
    }

    fn creation_planning_dialogue_max_tokens(query: &str, configured_ceiling: Option<u64>) -> u64 {
        if let Some(stage) = Self::creation_planning_stage_for_query(query) {
            let requested = match stage {
                CreationPlanningStage::Skeleton => 1024,
                CreationPlanningStage::Characters => 1536,
                CreationPlanningStage::Plot => 2048,
                CreationPlanningStage::Governance => 1536,
                CreationPlanningStage::Generic => 4096,
            };
            return configured_ceiling
                .unwrap_or(reasoner_constants::LOCAL_MAX_STEP_TOKENS as u64)
                .min(requested)
                .max(1);
        }

        let lower = query.to_ascii_lowercase();
        let requests_structured_plan = query.contains("大纲")
            || query.contains("框架")
            || query.contains("章节")
            || query.contains("章左右")
            || lower.contains("outline")
            || lower.contains("chapter")
            || lower.contains("framework")
            || lower.contains("plan");
        let expected_items = Self::creation_planning_expected_item_count(query);
        let requested =
            if requests_structured_plan || expected_items.is_some_and(|items| items >= 10) {
                3072
            } else {
                2048
            };
        configured_ceiling
            .unwrap_or(reasoner_constants::LOCAL_MAX_STEP_TOKENS as u64)
            .min(requested)
            .max(1)
    }

    fn creation_planning_stage_for_query(query: &str) -> Option<CreationPlanningStage> {
        if !query.contains(CREATION_PLANNING_DIALOGUE_MARKER)
            && !query.contains("BENSHU_CREATION_PLANNING_DIALOGUE")
        {
            return None;
        }
        if query.contains("合同分段补齐阶段：Skeleton") {
            Some(CreationPlanningStage::Skeleton)
        } else if query.contains("合同分段补齐阶段：Characters") {
            Some(CreationPlanningStage::Characters)
        } else if query.contains("合同分段补齐阶段：Plot") {
            Some(CreationPlanningStage::Plot)
        } else if query.contains("合同分段补齐阶段：Governance") {
            Some(CreationPlanningStage::Governance)
        } else {
            Some(CreationPlanningStage::Generic)
        }
    }

    fn creation_planning_expected_item_count(query: &str) -> Option<u64> {
        for marker in [
            "预计章节数：",
            "预计章节数:",
            "expected_chapters:",
            "expected chapters:",
        ] {
            if let Some(value) = Self::unsigned_number_after_marker(query, marker) {
                return Some(value);
            }
        }
        if let Some((total, per_unit)) = Self::creation_planning_total_and_unit_targets(query) {
            if per_unit > 0 {
                return Some(total.div_ceil(per_unit));
            }
        }
        None
    }

    fn creation_planning_total_and_unit_targets(query: &str) -> Option<(u64, u64)> {
        let total = ["总目标字数：", "总目标字数:", "总字数=", "target_units:"]
            .iter()
            .find_map(|marker| Self::unsigned_number_after_marker(query, marker))?;
        let per_unit = [
            "每章目标档位：",
            "每章目标档位:",
            "每章档位=",
            "chapter_unit_target:",
        ]
        .iter()
        .find_map(|marker| Self::unsigned_number_after_marker(query, marker))?;
        Some((total, per_unit))
    }

    fn unsigned_number_after_marker(text: &str, marker: &str) -> Option<u64> {
        let start = text.find(marker)? + marker.len();
        let tail = &text[start..];
        let mut digits = String::new();
        for ch in tail.chars() {
            if ch.is_ascii_digit() {
                digits.push(ch);
            } else if !digits.is_empty() {
                break;
            } else if !matches!(ch, ' ' | '\t' | '=' | ':' | '：') {
                break;
            }
        }
        digits.parse::<u64>().ok().filter(|value| *value > 0)
    }

    fn should_use_latest_turn_context_only_for_query(query: &str) -> bool {
        if Self::query_is_creation_planning_dialogue(query) {
            return true;
        }
        if query_prefers_session_continuity_answer(query) {
            return false;
        }

        let raw_route = classify_query_capability_route(query);
        if !matches!(
            select_coordinator_task_mode(raw_route, false),
            CoordinatorTaskMode::ChatLite
        ) {
            return false;
        }

        coordinator_chat_lite_tool_names_for_query(Some(query)).is_empty()
    }

    fn output_contract_system_message(
        contract: output_contract::OutputContract,
        latest_user_text: Option<&str>,
    ) -> Option<String> {
        let zh = latest_user_text.is_some_and(Self::query_prefers_chinese);
        let message = match contract.kind {
            output_contract::OutputContractKind::ShortAnswer => {
                if zh {
                    "### OUTPUT_CONTRACT\n用用户的语言直接回答。保持短答，通常 1-3 句；不要输出内部流程、工具说明或冗长免责声明。"
                } else {
                    "### OUTPUT_CONTRACT\nAnswer directly in the user's language. Keep it short, usually 1-3 sentences; do not include internal workflow, tool narration, or long disclaimers."
                }
            }
            output_contract::OutputContractKind::Explanation if contract.max_tokens <= 512 => {
                if zh {
                    "### OUTPUT_CONTRACT\n用用户的语言给出简明科普解释。默认 1 个结论句 + 2 个关键原因；除非用户要求详细展开，不要写长篇。"
                } else {
                    "### OUTPUT_CONTRACT\nGive a concise educational explanation in the user's language. Default to 1 answer sentence plus 2 key reasons; do not write a longform answer unless the user asks for depth."
                }
            }
            output_contract::OutputContractKind::Explanation => {
                if zh {
                    "### OUTPUT_CONTRACT\n用用户的语言解释清楚，但保持结构化。用户要求深入时可以展开；仍避免无关铺陈。"
                } else {
                    "### OUTPUT_CONTRACT\nExplain clearly in the user's language with structure. Expand when the user asks for depth, while avoiding unrelated padding."
                }
            }
            output_contract::OutputContractKind::Longform
            | output_contract::OutputContractKind::Artifact => {
                if zh {
                    "### OUTPUT_CONTRACT\n这是产物/长任务导向请求。聊天中交付进度、摘要、路径和验收状态；大正文应写入 artifact，不要默认塞满聊天框。"
                } else {
                    "### OUTPUT_CONTRACT\nThis is an artifact/long-task request. In chat, return progress, summaries, paths, and acceptance status; large bodies should go to artifacts instead of filling chat history."
                }
            }
            _ => return None,
        };
        Some(message.to_string())
    }

    fn pending_content_generation_system_message(latest_user_text: Option<&str>) -> String {
        if latest_user_text.is_some_and(Self::query_prefers_chinese) {
            "### PENDING_CONTENT_BODY_GENERATION\n最近的工具结果/错误要求为待保存产物补齐正文内容。本轮不要再调用工具，也不要输出计划、状态报告或工具说明；只输出应写入产物的正文。运行时会把这段正文自动附加到待执行的写入动作。正文语言、体裁、人物和连续性必须继承用户请求与已有上下文。".to_string()
        } else {
            "### PENDING_CONTENT_BODY_GENERATION\nThe latest tool result/error requires body content for a pending artifact write. Do not call another tool this turn, and do not output a plan, status report, or tool narration; output only the body/content that should be saved. The runtime will attach that text to the pending write action. Preserve the requested language, genre, entities, and continuity from the current task context.".to_string()
        }
    }

    fn routing_judgment_fallback_text(route: Option<CapabilityRouteHint>) -> String {
        fallback_text::routing_judgment_fallback_text(route)
    }

    pub(crate) fn tool_surface_has_generate_image(tools: &[ToolDefinition]) -> bool {
        tools.iter().any(|tool| tool.name == "generate_image")
    }

    pub(crate) fn image_generation_unavailable_fallback_text(query: &str) -> String {
        fallback_text::image_generation_unavailable_fallback_text(query)
    }

    fn classified_finalization_fallback_text(
        kind: FinalizationFallbackKind,
        query: &str,
    ) -> String {
        fallback_text::classified_finalization_fallback_text(
            kind,
            Self::query_prefers_chinese(query),
        )
    }

    fn is_pseudo_tool_call_leak(text: &str) -> bool {
        fallback_text::is_pseudo_tool_call_leak(text)
    }

    fn is_multimodal_procedural_placeholder(text: &str) -> bool {
        fallback_text::is_multimodal_procedural_placeholder(text)
    }

    fn extract_latest_parsed_attachment_summary(messages: &[Message]) -> Option<String> {
        let latest_user = messages
            .iter()
            .rev()
            .find(|message| message.role == Role::User)?;
        let parts = match &latest_user.content {
            Content::Parts(parts) => parts,
            _ => return None,
        };

        parts.iter().find_map(|part| {
            let text = match part {
                crate::agent::message::ContentPart::Text { text } => text.trim(),
                _ => return None,
            };

            if !text.starts_with("[Parsed ") && !text.starts_with("\n[Parsed ") {
                return None;
            }

            let mut lines = text.lines();
            let _header = lines.next();
            let mut summary_lines = Vec::new();
            for line in lines {
                let trimmed = line.trim();
                if trimmed.is_empty()
                    || trimmed.starts_with("source:")
                    || trimmed.starts_with("parser_mode:")
                {
                    continue;
                }
                summary_lines.push(trimmed);
            }

            let summary = summary_lines.join("\n").trim().to_string();
            if summary.is_empty() {
                None
            } else {
                Some(summary)
            }
        })
    }

    async fn retry_simple_media_answer(
        &self,
        request_messages: &[Message],
        request_model: &str,
    ) -> Option<String> {
        let latest_user = request_messages
            .iter()
            .rev()
            .find(|message| message.role == Role::User)?;
        if !latest_user_message_has_media(request_messages) {
            return None;
        }

        let user_query = Self::latest_user_query(request_messages)
            .filter(|query| !query.trim().is_empty())
            .unwrap_or_else(|| {
                "请只根据图片中真实可见的内容，用中文直接描述你看到的主要对象；如果确实无法判断，再回答“不确定”。"
                    .to_string()
            });
        let retry_request = crate::agent::provider::ChatRequest {
            model: request_model.to_string(),
            system_prompt: None,
            messages: vec![
                Message::new(
                    Role::System,
                    "请只输出图片内容本身，不要复述规则、不要解释你的流程、不要重复用户问题。"
                        .to_string(),
                ),
                Message::new(Role::User, latest_user.content.clone()),
                Message::new(Role::User, user_query),
            ],
            max_tokens: Some(128),
            temperature: Some(0.2),
            ..Default::default()
        };

        let stream = match tokio::time::timeout(
            self.effective_llm_timeout_for_request(&retry_request),
            self.provider.stream_completion(retry_request),
        )
        .await
        {
            Ok(Ok(stream)) => stream,
            Ok(Err(error)) => {
                warn!("Reasoner: multimodal retry failed to start: {}", error);
                return None;
            }
            Err(_) => {
                warn!("Reasoner: multimodal retry timed out before start");
                return None;
            }
        };

        match stream.collect_text().await {
            Ok(text) => {
                info!(
                    "Reasoner: multimodal retry returned preview=\"{}\"",
                    text.chars().take(160).collect::<String>()
                );
                Some(text)
            }
            Err(error) => {
                warn!("Reasoner: multimodal retry failed: {}", error);
                None
            }
        }
    }

    async fn document_understand_fallback_summary(
        &self,
        request_messages: &[Message],
    ) -> Option<String> {
        let tool = match self.tools.get("document_understand") {
            Some(tool) => tool,
            None => {
                warn!(
                    "Reasoner: document_understand fallback unavailable because tool is not registered in this agent toolset."
                );
                return None;
            }
        };
        let path = if let Some(url) = Self::latest_user_media_path(request_messages) {
            url
        } else if let Some((media_type, data)) =
            Self::latest_user_media_image_base64(request_messages)
        {
            Self::materialize_base64_image_to_temp(&media_type, &data).await?
        } else {
            return None;
        };
        let prompt = Self::latest_user_query(request_messages);
        let args = serde_json::json!({
            "action": "analyze",
            "path": path,
            "goal": "understand",
            "prompt": prompt,
            "local_only": true
        });
        let args_str = serde_json::to_string(&args).ok()?;
        info!(
            "Reasoner: document_understand fallback calling with args: {}",
            args_str
        );
        if let Err(error) = tool.pre_call(&args_str).await {
            warn!(
                "Reasoner: document_understand fallback pre_call failed: {}",
                error
            );
            return None;
        }
        let output = match tool.call(&args_str).await {
            Ok(output) => output,
            Err(error) => {
                warn!(
                    "Reasoner: document_understand fallback call failed: {}",
                    error
                );
                return None;
            }
        };
        info!(
            "Reasoner: document_understand fallback output_len={} preview=\"{}\"",
            output.len(),
            output.chars().take(240).collect::<String>()
        );
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&output) {
            if let Some(result) = parsed.get("result").and_then(|value| value.as_str()) {
                let trimmed = result.trim();
                info!(
                    "Reasoner: document_understand fallback parsed result preview=\"{}\"",
                    trimmed.chars().take(240).collect::<String>()
                );
                if !trimmed.is_empty() {
                    if let Some(query) = Self::latest_user_query(request_messages) {
                        if Self::is_low_value_media_answer(&query, trimmed) {
                            warn!(
                                "Reasoner: document_understand fallback result was classified as low-value media output and suppressed."
                            );
                            return None;
                        }
                    }
                    return Some(trimmed.to_string());
                }
                warn!(
                    "Reasoner: document_understand fallback returned structured payload with empty result; suppressing raw JSON leak."
                );
                return None;
            }
            if let Some(error_message) = parsed.get("error").and_then(|value| value.as_str()) {
                let trimmed = error_message.trim();
                if !trimmed.is_empty() {
                    warn!(
                        "Reasoner: document_understand fallback returned structured error: {}",
                        trimmed
                    );
                    return Some(format!(
                        "我收到了这张图片，但本地视觉理解这次没有产出可用描述：{}",
                        trimmed
                    ));
                }
            }
            warn!(
                "Reasoner: document_understand fallback returned structured payload without a usable result; suppressing raw JSON leak."
            );
            return None;
        }
        let trimmed = output.trim();
        if trimmed.is_empty() {
            warn!("Reasoner: document_understand fallback returned empty output.");
            None
        } else {
            Some(trimmed.to_string())
        }
    }
}

impl<P: Provider> Reasoner<P> {
    fn explicit_image_generation_turn(messages: &[Message]) -> bool {
        messages
            .iter()
            .rev()
            .find(|message| message.role == Role::User)
            .is_some_and(|message| query_requests_image_generation(&message.content.as_text()))
    }

    pub fn new(
        provider: Arc<P>,
        config: ReasonerConfig,
        tools: ToolSet,
        enabled_tools: Option<Arc<RwLock<std::collections::HashSet<String>>>>,
        tactical_orchestrator: Arc<dyn TacticalOrchestrator>,
    ) -> Self {
        // Validation loop for production defaults
        let validated_config = Self::validate_config(config);

        let distillation_cache = Cache::builder()
            .max_capacity(reasoner_constants::DISTILLATION_CACHE_MAX_SIZE)
            .time_to_live(Duration::from_secs(
                reasoner_constants::DISTILLATION_CACHE_TTL * 60,
            ))
            .build();

        Self {
            provider,
            config: validated_config,
            tools,
            enabled_tools,
            tactical_orchestrator,
            complexity_estimator: ComplexityEstimator::new(),
            distillation_cache,
            rate_limiter: Arc::new(tokio::sync::Semaphore::new(12)), // Max 12 concurrent reasoning loops
        }
    }

    /// Validates and corrects reasoner configuration for stability
    fn validate_config(mut config: ReasonerConfig) -> ReasonerConfig {
        if config.max_history_messages == 0 {
            config.max_history_messages = 50;
        }

        if config.temperature.is_none()
            || config.temperature.unwrap() < 0.0
            || config.temperature.unwrap() > 2.0
        {
            config.temperature = Some(0.7);
        }

        if config.max_tokens.is_none() || config.max_tokens.unwrap() == 0 {
            config.max_tokens = Some(2048);
        }

        if config.llm_timeout == Duration::ZERO {
            config.llm_timeout = Duration::from_secs(45);
        }

        if config.max_reflexion_retries == 0 {
            config.max_reflexion_retries =
                crate::agent::protocol::constants::DEFAULT_MAX_REFLEXION_RETRIES;
        }

        // Production Hardening: Tighten temperature for JSON Mode to prevent formatting erraticness
        if config.json_mode && (config.temperature.unwrap_or(0.7) > 0.3) {
            config.temperature = Some(0.2);
        }

        config
    }

    /// Phase 12.1: The Reasoning Loop (The Parietal Lobe)
    /// Drives the Observe-Think-Act-Verify cycle.
    pub async fn execute_loop(
        &self,
        bridge: &dyn AgentLiaison,
        _agent: &dyn MultiAgent,
        messages: &mut Vec<Message>,
        attempt: &Attempt,
        reasoning_strategy: &ReasoningStrategy,
        max_steps: usize,
        last_error: Option<&str>,
        risk_score: f32,
        model_override: Option<String>,
    ) -> Result<ChatOutcome> {
        let mut steps = 0;
        let mut audit_rejections = 0;
        let mut thoughts = Vec::new();
        let mut tool_trace = Vec::new();
        let mut history = QueryHistory::new();

        let mut total_tokens_used = 0;

        // --- High-Level Logic: Provider-owned Runtime Policy ---
        let runtime_policy = self.provider.runtime_policy();
        let quota = runtime_policy.session_token_quota;

        loop {
            // Concurrent execution control
            let _permit = self
                .rate_limiter
                .acquire()
                .await
                .map_err(|_| Error::agent_config("Rate limiter failed"))?;

            // Session-level token quota check (Prevents infinite loops and money drain)
            if total_tokens_used > quota {
                bridge.emit(AgentEventData::Error {
                    message: format!(
                        "Session token quota exceeded: {} > {} (Local: {})",
                        total_tokens_used, quota, runtime_policy.unlocks_full_context_window
                    ),
                });
                return Err(Error::agent_config("Session token quota exhausted"));
            }

            // --- 0. Control & Boundary ---
            if steps >= max_steps {
                bridge.emit(AgentEventData::Error {
                    message: format!("Max steps reached: {}", max_steps),
                });
                return Err(Error::agent_config("Max agent steps exceeded"));
            }

            if bridge.cancel_token().is_cancelled() {
                bridge.emit(AgentEventData::Cancelled {
                    reason: "Cancelled by user".to_string(),
                });
                return Err(Error::agent_config("Task cancelled by user"));
            }

            let resumed_inputs = bridge.wait_if_paused().await?;
            if !resumed_inputs.is_empty() {
                let joined = resumed_inputs.join("\n");
                messages.push(Message::user(format!(
                    "User resumed the paused task with additional instructions. Treat these instructions as part of the same in-progress task, then continue from the latest valid checkpoint instead of restarting from scratch.\n\n{joined}"
                )));
                bridge.emit(AgentEventData::Thought {
                    content: "Paused task resumed with user instructions".to_string(),
                });
            }

            steps += 1;
            bridge.emit(AgentEventData::StepStart { step: steps });

            // --- 1. Observe (Sensory & Intervention) ---
            // Frontstage routing no longer uses the heavier L2 swarm/fission probe.
            let complexity = self.complexity_estimator.estimate(messages, None).await;

            let metabolic = bridge.current_metabolic_pressure();
            let total_chars: usize = messages.iter().map(|m| m.content.as_text().len()).sum();
            let provider_unlocks_full_context_window = runtime_policy.unlocks_full_context_window;

            // SENSORY REPAIR: Bridge between metabolic pressure and local model overrides
            let vram_pressure = metabolic.vram_pressure;

            bridge
                .intervention()
                .handle_all_interventions(
                    messages,
                    steps,
                    if steps == 1 { last_error } else { None },
                    complexity.clone(),
                    max_steps,
                    metabolic,
                    total_chars,
                    provider_unlocks_full_context_window,
                    bridge.token_budget(),
                )
                .await?;

            if let Some(outcome) = bridge.prepare_for_step(messages, steps).await? {
                return Ok(outcome);
            }

            match self
                .try_pre_llm_local_file_continuation(
                    bridge,
                    messages,
                    steps,
                    &mut history,
                    &mut tool_trace,
                )
                .await?
            {
                orchestration_chain::StepDisposition::Finalized(outcome) => return Ok(outcome),
                orchestration_chain::StepDisposition::ContinueLoop => continue,
                orchestration_chain::StepDisposition::NotApplicable => {}
            }

            match self
                .try_pre_llm_knowledge_followup(
                    bridge,
                    messages,
                    steps,
                    &mut history,
                    &mut tool_trace,
                )
                .await?
            {
                orchestration_chain::StepDisposition::Finalized(outcome) => return Ok(outcome),
                orchestration_chain::StepDisposition::ContinueLoop => continue,
                orchestration_chain::StepDisposition::NotApplicable => {}
            }

            if steps == 1 {
                if let Some(query) = Self::latest_user_query(messages) {
                    if !Self::has_system_marker_after_latest_user(
                        messages,
                        "BENSHU_DIRECT_REALTIME_LOOKUP_EXECUTED",
                    ) {
                        if let Some(tool_call) = self
                            .direct_realtime_followup_tool_call_for_query(messages, &query)
                            .or_else(|| self.direct_realtime_tool_call_for_query(&query))
                        {
                            let direct_tool_name = tool_call.1.clone();
                            bridge.emit(AgentEventData::Thought {
                                content: "ORCHESTRATION CHAIN: simple realtime lookup route has a structured tool and no durable follow-up; executing it directly instead of spending extra model rounds deciding the same tool.".to_string(),
                            });
                            messages.push(Message::system(
                                "BENSHU_DIRECT_REALTIME_LOOKUP_EXECUTED".to_string(),
                            ));
                            bridge
                                .executor()
                                .coordinate(
                                    messages,
                                    String::new(),
                                    vec![tool_call],
                                    steps,
                                    &mut history,
                                    &mut tool_trace,
                                )
                                .await?;

                            if let Some(final_text) =
                                Self::direct_tool_display_delivery(messages, &query)
                                    .or_else(|| {
                                        Self::direct_tool_trace_display_delivery(
                                            &tool_trace,
                                            &query,
                                            &direct_tool_name,
                                        )
                                    })
                                    .or_else(|| {
                                        Self::latest_realtime_tool_trace_display_delivery(
                                            &tool_trace,
                                            &query,
                                        )
                                    })
                            {
                                return bridge
                                    .finalize_outcome(
                                        messages, final_text, None, thoughts, tool_trace, steps,
                                    )
                                    .await;
                            }
                            if let Some((failed_tool_name, error)) =
                                Self::latest_tool_error_result(messages)
                            {
                                let final_text = Self::tool_failure_delivery_text(
                                    &query,
                                    &failed_tool_name,
                                    &error,
                                );
                                bridge.emit(AgentEventData::Thought {
                                    content: format!(
                                        "ORCHESTRATION FINALIZE: direct realtime lookup `{}` returned a runtime error; returning a clear blocker instead of spending another model round on parameter retries.",
                                        failed_tool_name
                                    ),
                                });
                                return bridge
                                    .finalize_outcome(
                                        messages, final_text, None, thoughts, tool_trace, steps,
                                    )
                                    .await;
                            }
                            continue;
                        }
                    }
                }
            }

            // --- 2. Think (Reasoning Strategy & Metabolic Adaptation) ---
            let throttle = bridge.suggest_resource_throttle();
            let mut adapted_strategy = reasoning_strategy.clone();
            let mut context_strategy = attempt.strategy.clone();
            let explicit_image_generation_turn = Self::explicit_image_generation_turn(messages);
            let simple_media_understanding =
                latest_turn_simple_media_understanding(messages, &complexity, steps, total_chars);
            let creation_planning_dialogue = Self::latest_user_query(messages)
                .as_deref()
                .is_some_and(Self::query_is_creation_planning_dialogue);

            // Phase 8.2: Dynamic Strategy Adaptation (Non-mutating state approach)
            match throttle {
                ThrottleLevel::Low => {
                    if *reasoning_strategy != ReasoningStrategy::ReAct {
                        bridge.emit(AgentEventData::Thought {
                            content: "SYSTEM METABOLIC GUARD: CRITICAL resource pressure. Using minimal ReAct strategy for conservation.".to_string()
                        });
                    }
                    adapted_strategy = ReasoningStrategy::ReAct;
                    context_strategy = crate::agent::attempt::Strategy::Fallback;
                }
                ThrottleLevel::Medium => {
                    if *reasoning_strategy == ReasoningStrategy::TreeOfThoughts {
                        bridge.emit(AgentEventData::Thought {
                            content: "SYSTEM METABOLIC GUARD: Moderate pressure. Complexity-downscaling: ToT -> Reflexion.".to_string()
                        });
                        adapted_strategy = ReasoningStrategy::Reflexion;
                    }
                    context_strategy = crate::agent::attempt::Strategy::Compressed;
                }
                ThrottleLevel::High => {
                    // Normal operation - check for complexity-driven upgrades
                    let reflexion_upgrade = if creation_planning_dialogue {
                        ReflexionUpgradeDecision {
                            should_upgrade: false,
                            reason: None,
                        }
                    } else {
                        decide_reflexion_strategy_upgrade(ReflexionUpgradeInput {
                            current_strategy_is_react: adapted_strategy == ReasoningStrategy::ReAct,
                            complexity_score: (complexity.score.clamp(0.0, 1.0) * 100.0).round()
                                as u16,
                            retry_count: attempt.retry_count as usize,
                            max_reflexion_retries: self.config.max_reflexion_retries,
                            retry_recovery_eligible: retry_allows_reflexion_upgrade(
                                attempt.retry_count as usize,
                                self.config.max_reflexion_retries,
                                classify_failure(last_error.unwrap_or_default()),
                            ),
                            explicit_image_generation_turn,
                            has_media_input: latest_user_message_has_media(messages),
                            simple_media_understanding,
                        })
                    };
                    if reflexion_upgrade.should_upgrade {
                        let upgrade_reason = match reflexion_upgrade.reason {
                            Some(ReflexionUpgradeReason::RetryRecovery) => {
                                format!("retry recovery (attempt #{})", attempt.retry_count)
                            }
                            Some(ReflexionUpgradeReason::HighComplexity) => {
                                format!("task complexity ({:.1})", complexity.score)
                            }
                            None => "policy gate".to_string(),
                        };
                        bridge.emit(AgentEventData::Thought {
                            content: format!(
                                "COMPLEXITY UPGRADE: {} detected. Activating System 2 Reflexion.",
                                upgrade_reason
                            ),
                        });
                        adapted_strategy = ReasoningStrategy::Reflexion;
                    }
                }
            }

            if let Some(query) = Self::latest_user_query(messages) {
                if Self::should_use_latest_turn_context_only_for_query(&query) {
                    context_strategy = crate::agent::attempt::Strategy::Fallback;
                }
            }

            let mut context_manager = bridge.context_manager().clone();
            if let Some(background_envelope) = bridge.current_background_envelope() {
                context_manager.set_background_envelope(background_envelope);
            } else {
                context_manager.clear_background_envelope();
            }

            let context_messages = context_manager
                .build_context(
                    messages,
                    &context_strategy,
                    runtime_policy.unlocks_full_context_window,
                )
                .await
                .map_err(|e| Error::agent_config(format!("Failed to build context: {}", e)))?;

            let active_model_override = model_override
                .clone()
                .or_else(|| bridge.current_model_override())
                .or_else(|| {
                    bridge
                        .intervention()
                        .get_metabolic_model_override(vram_pressure)
                });

            let step_result = self
                .think(
                    context_messages,
                    &adapted_strategy,
                    |data| bridge.emit(data),
                    bridge.cancel_token(),
                    active_model_override,
                    Some(bridge),
                )
                .await?;

            let mut full_text = step_result.text;
            let mut tool_calls = step_result.tool_calls.clone();
            let thoughts_snapshot = step_result.thoughts.clone();
            let usage = step_result.usage;
            let finalize_instead_of_recovering_pseudo_tool = tool_calls.is_empty()
                && Self::should_finalize_instead_of_recovering_pseudo_tool(messages, &full_text);

            if finalize_instead_of_recovering_pseudo_tool {
                bridge.emit(AgentEventData::Thought {
                    content: "FINALIZATION GUARD: current-turn durable evidence already satisfies the request; ignoring leaked pseudo-tool text and finalizing from runtime receipts."
                        .to_string(),
                });
            } else if tool_calls.is_empty() && Self::is_pseudo_tool_call_leak(&full_text) {
                let parsed_pseudo_calls = Self::extract_inline_pseudo_tool_calls(&full_text);
                if !parsed_pseudo_calls.is_empty() {
                    info!(
                        "Reasoner: recovered {} inline pseudo tool call(s) from local model text output.",
                        parsed_pseudo_calls.len()
                    );
                    bridge.emit(AgentEventData::Thought {
                        content: "TOOL RECOVERY: detected local pseudo tool output and converted it into executable tool calls."
                            .to_string(),
                    });
                    tool_calls = parsed_pseudo_calls
                        .into_iter()
                        .enumerate()
                        .map(|(idx, (name, args))| {
                            let (name, args) = self.normalize_local_pseudo_tool_call(name, args);
                            (format!("pseudo-inline-tool-call-{}", idx + 1), name, args)
                        })
                        .collect();
                    full_text.clear();
                }
            }

            if tool_calls.is_empty() {
                if let Some(pending_call) =
                    Self::pending_content_action_tool_call_from_generated_text(messages, &full_text)
                {
                    bridge.emit(AgentEventData::Thought {
                        content: "TOOL CONTRACT RECOVERY: generated body text was attached to the pending content action."
                            .to_string(),
                    });
                    tool_calls = vec![pending_call];
                    full_text.clear();
                } else if let Some(repaired_call) =
                    Self::content_required_tool_call_from_generated_text(messages, &full_text)
                {
                    bridge.emit(AgentEventData::Thought {
                        content: "TOOL CONTRACT RECOVERY: generated body text was attached to the pending content-required tool call."
                            .to_string(),
                    });
                    tool_calls = vec![repaired_call];
                    full_text.clear();
                }
            }

            // Track and update session token usage
            if let Some(ref u) = usage {
                total_tokens_used += u.total_tokens as usize;

                let budget = bridge.register_token_usage(u);
                bridge.emit(AgentEventData::GovernanceBudget {
                    budget_kind: "session_tokens".to_string(),
                    limit: budget.limit,
                    used: budget.used,
                    remaining: budget.remaining,
                    exceeded: budget.exceeded,
                    detail: Some(format!(
                        "prompt_tokens={} completion_tokens={} total_tokens={}",
                        u.prompt_tokens, u.completion_tokens, u.total_tokens
                    )),
                });

                if budget.exceeded {
                    bridge.emit(AgentEventData::Error {
                        message: format!(
                            "Governance token budget exceeded: used={} limit={}",
                            budget.used,
                            budget.limit.unwrap_or_default()
                        ),
                    });
                    return Err(Error::agent_config("Governance token budget exhausted"));
                }
            }
            thoughts.extend(thoughts_snapshot.clone());

            if finalize_instead_of_recovering_pseudo_tool {
                let final_text = Self::synthesize_successful_tool_delivery(messages)
                    .unwrap_or_else(|| {
                        Self::latest_successful_tool_result(messages)
                            .map(|(_tool_name, result)| {
                                Self::compact_tool_result_for_recovery(&result)
                            })
                            .unwrap_or_else(|| full_text.trim().to_string())
                    });
                return bridge
                    .finalize_outcome(messages, final_text, usage, thoughts, tool_trace, steps)
                    .await;
            }

            // --- 3. Act (Tactical Validation & Execution) ---
            if matches!(
                self.run_tactical_precheck(bridge, messages, &tool_calls)
                    .await?,
                execution_guard::GuardDecision::ContinueLoop
            ) {
                continue;
            }

            if tool_calls.is_empty() {
                if Self::model_output_is_empty_or_non_deliverable(&full_text) {
                    let retry_marker = "BENSHU_EMPTY_MODEL_RESPONSE_RETRY";
                    if Self::has_system_marker_after_latest_user(messages, retry_marker) {
                        let final_text = Self::empty_worker_response_blocker(messages);
                        bridge.emit(AgentEventData::Thought {
                            content: "EMPTY RESPONSE GUARD: repeated empty model output; returning a structured blocker instead of entering reflexion churn.".to_string(),
                        });
                        return bridge
                            .finalize_outcome(
                                messages, final_text, usage, thoughts, tool_trace, steps,
                            )
                            .await;
                    }

                    messages.push(Message::system(format!(
                        "{retry_marker}\n\nThe previous model turn was empty. Continue exactly once by either calling the best equipped tool for the current task or returning a concise blocker with the missing runtime condition. Do not run reflexion on an empty response."
                    )));
                    bridge.emit(AgentEventData::Thought {
                        content: "EMPTY RESPONSE GUARD: model returned no text and no tool call; giving it one actionable retry before stopping this worker loop.".to_string(),
                    });
                    continue;
                }

                if let Some(route) = Self::latest_execution_required_route(messages) {
                    if Self::has_recent_tool_execution_required_prompt(messages) {
                        if let Some(query) = Self::latest_knowledge_persistence_query(messages) {
                            if Self::should_prioritize_followup_execution(&query, messages) {
                                if let Some(result) =
                                    Self::latest_lookup_result_for_followup_execution(messages)
                                {
                                    if let Some(url) =
                                        Self::followup_execution_source_url(&query, &result)
                                    {
                                        bridge.emit(AgentEventData::Thought {
                                            content: Self::user_facing_progress_message(
                                                "knowledge_import",
                                                &query,
                                            ),
                                        });
                                        bridge.emit(AgentEventData::Thought {
                                            content: "ORCHESTRATION CHAIN: tool-first recovery found verified lookup evidence for a knowledge-persistence task; executing the bounded knowledge worker instead of finalizing from researcher output.".to_string(),
                                        });

                                        bridge
                                            .executor()
                                            .coordinate(
                                                messages,
                                                String::new(),
                                                vec![Self::knowledge_import_delegate_call_with_evidence(
                                                    steps,
                                                    &url,
                                                    &query,
                                                    Some(&Self::lookup_result_source_body_surface(&result)),
                                                )],
                                                steps,
                                                &mut history,
                                                &mut tool_trace,
                                            )
                                            .await?;

                                        if Self::query_requests_post_import_delivery(&query) {
                                            if let Some(final_text) =
                                                Self::try_file_artifact_followup_after_import(
                                                    bridge,
                                                    messages,
                                                    &query,
                                                    steps,
                                                    &mut history,
                                                    &mut tool_trace,
                                                )
                                                .await?
                                            {
                                                bridge.emit(AgentEventData::Thought {
                                                    content: Self::file_artifact_followup_finalize_thought(messages, &query),
                                                });
                                                return bridge
                                                    .finalize_outcome(
                                                        messages, final_text, usage, thoughts,
                                                        tool_trace, steps,
                                                    )
                                                    .await;
                                            }
                                            if let Some(final_text) =
                                                Self::synthesize_post_import_delivery(
                                                    &query, messages,
                                                )
                                            {
                                                bridge.emit(AgentEventData::Thought {
                                                    content: "ORCHESTRATION FINALIZE: knowledge import completed during tool-first recovery; returning the requested final delivery.".to_string(),
                                                });
                                                return bridge
                                                    .finalize_outcome(
                                                        messages, final_text, usage, thoughts,
                                                        tool_trace, steps,
                                                    )
                                                    .await;
                                            }
                                        }

                                        if let Some(knowledge_result) =
                                            Self::latest_successful_tool_result_text(
                                                messages, "delegate",
                                            )
                                        {
                                            let final_text = Self::summarize_delegate_delivery(
                                                &query,
                                                &knowledge_result,
                                                Self::query_prefers_chinese(&query),
                                            );
                                            return bridge
                                                .finalize_outcome(
                                                    messages, final_text, usage, thoughts,
                                                    tool_trace, steps,
                                                )
                                                .await;
                                        }
                                    }
                                }

                                messages.push(Message::system(format!(
                                    "{}\n\nA verified lookup result is already available and the user requested knowledge-base persistence. Do not finalize from the researcher result yet. Continue so the bounded orchestration chain can delegate exactly once to the knowledge worker.",
                                    reasoner_constants::MARKER_TOOL_EXECUTION_REQUIRED
                                )));
                                continue;
                            }
                        }
                        if let Some(query) = Self::latest_user_query(messages) {
                            if let Some(empty_lookup) =
                                Self::latest_repeated_empty_lookup_result(messages)
                            {
                                if self.tool_is_enabled("browser_browse") {
                                    bridge.emit(AgentEventData::Thought {
                                        content: "ORCHESTRATION RECOVERY: the model produced no next tool call after repeated blocked/empty lookup results; switching once to an observation-capable tool surface before finalizing.".to_string(),
                                    });
                                    messages.push(Message::system(
                                        "BENSHU_ORCHESTRATION_OBSERVATION_RECOVERY".to_string(),
                                    ));
                                    bridge
                                        .executor()
                                        .coordinate(
                                            messages,
                                            String::new(),
                                            vec![Self::observation_recovery_tool_call(
                                                steps, &query,
                                            )],
                                            steps,
                                            &mut history,
                                            &mut tool_trace,
                                        )
                                        .await?;
                                    continue;
                                }
                                if self.tool_is_enabled("delegate") {
                                    bridge.emit(AgentEventData::Thought {
                                        content: "ORCHESTRATION RECOVERY: the model produced no next tool call after repeated blocked/empty lookup results; delegating once to an observation-capable worker before finalizing.".to_string(),
                                    });
                                    messages.push(Message::system(
                                        "BENSHU_ORCHESTRATION_OBSERVATION_RECOVERY".to_string(),
                                    ));
                                    bridge
                                        .executor()
                                        .coordinate(
                                            messages,
                                            String::new(),
                                            vec![Self::observation_recovery_delegate_call(
                                                steps,
                                                &query,
                                                &empty_lookup,
                                            )],
                                            steps,
                                            &mut history,
                                            &mut tool_trace,
                                        )
                                        .await?;
                                    continue;
                                }
                            }

                            if !Self::query_is_creation_planning_dialogue(&query)
                                && self.tool_is_enabled("delegate")
                                && (query_requests_followup_execution_after_lookup(&query)
                                    || Self::route_allows_tooled_delegate_recovery(route))
                                && !Self::latest_successful_result_satisfies_execution_request(
                                    messages, &query,
                                )
                                && !Self::has_system_marker_after_latest_user(
                                    messages,
                                    "BENSHU_ORCHESTRATION_TOOLLESS_EXECUTION_DELEGATE",
                                )
                            {
                                bridge.emit(AgentEventData::Thought {
                                    content: "TOOL-FIRST RECOVERY: the frontstage model still produced no tool call for a compound execution request; executing one bounded specialist delegate instead of finalizing as a false negative.".to_string(),
                                });
                                messages.push(Message::system(
                                    "BENSHU_ORCHESTRATION_TOOLLESS_EXECUTION_DELEGATE".to_string(),
                                ));
                                let delegate_route =
                                    if Self::query_requests_artifact_mutation(&query)
                                        || Self::query_requests_file_artifact(&query)
                                    {
                                        Self::artifact_execution_delegate_route(&query)
                                    } else {
                                        route
                                    };
                                let continuation_context =
                                    Self::latest_delegate_artifact_continuation_context(messages);
                                bridge
                                    .executor()
                                    .coordinate(
                                        messages,
                                        String::new(),
                                        vec![Self::toolless_execution_delegate_call(
                                            steps,
                                            &query,
                                            delegate_route,
                                            continuation_context,
                                        )],
                                        steps,
                                        &mut history,
                                        &mut tool_trace,
                                    )
                                    .await?;
                                continue;
                            }
                        }
                        let latest_query = Self::latest_user_query(messages).unwrap_or_default();
                        let failure_route = classify_query_capability_route(&latest_query)
                            .filter(|route| capability_route_requires_real_tool_call(*route))
                            .unwrap_or(route);
                        let artifact_requested =
                            Self::query_requests_artifact_mutation(&latest_query)
                                || Self::query_requests_file_artifact(&latest_query);
                        if artifact_requested {
                            if let Some((_tool_name, latest_result)) =
                                Self::latest_successful_tool_result(messages)
                            {
                                if !Self::tool_result_satisfies_artifact_request(
                                    &latest_query,
                                    &latest_result,
                                ) && self.tool_is_enabled(&_tool_name)
                                {
                                    if let Some((marker, tool_name, args)) =
                                        Self::declared_next_action_tool_call_from_result(
                                            &_tool_name,
                                            &latest_result,
                                        )
                                    {
                                        if !Self::has_system_marker_after_latest_user(
                                            messages, &marker,
                                        ) && steps < max_steps
                                        {
                                            bridge.emit(AgentEventData::Thought {
                                                content: format!(
                                                    "TOOL-FIRST RECOVERY: `{}` declared a concrete next_action; continuing that same tool before treating an intermediate checkpoint as final evidence.",
                                                    tool_name
                                                ),
                                            });
                                            messages.push(Message::system(marker));
                                            bridge
                                                .executor()
                                                .coordinate(
                                                    messages,
                                                    String::new(),
                                                    vec![(
                                                        "declared-next-action-continuation"
                                                            .to_string(),
                                                        tool_name,
                                                        args,
                                                    )],
                                                    steps,
                                                    &mut history,
                                                    &mut tool_trace,
                                                )
                                                .await?;
                                            continue;
                                        }
                                    }
                                }
                                let explicit_scale_continuation =
                                    Self::tool_result_is_scaled_artifact_continuation(
                                        &latest_query,
                                        &latest_result,
                                    );
                                let already_escalated = Self::has_system_marker_after_latest_user(
                                    messages,
                                    "BENSHU_ARTIFACT_CONTEXT_ONLY_ESCALATED_TO_WORKER",
                                );
                                if !Self::tool_result_satisfies_artifact_request(
                                    &latest_query,
                                    &latest_result,
                                ) && self.tool_is_enabled("delegate")
                                    && (explicit_scale_continuation || !already_escalated)
                                    && steps < max_steps
                                {
                                    bridge.emit(AgentEventData::Thought {
                                        content: if explicit_scale_continuation {
                                            "TOOL-FIRST RECOVERY: the latest artifact checkpoint is valid but below the requested explicit scale; delegating another bounded continuation instead of finalizing from a partial artifact.".to_string()
                                        } else {
                                            "TOOL-FIRST RECOVERY: best verified artifact evidence is still incomplete for the requested final format; delegating continuation instead of finalizing from a partial artifact.".to_string()
                                        },
                                    });
                                    messages.push(Message::system(
                                        "BENSHU_ARTIFACT_CONTEXT_ONLY_ESCALATED_TO_WORKER"
                                            .to_string(),
                                    ));
                                    let continuation_context =
                                        Self::latest_delegate_artifact_continuation_context(
                                            messages,
                                        )
                                        .or_else(|| {
                                            Self::artifact_continuation_context_from_result(
                                                &latest_result,
                                            )
                                        });
                                    let artifact_route =
                                        Self::artifact_execution_delegate_route(&latest_query);
                                    bridge
                                        .executor()
                                        .coordinate(
                                            messages,
                                            String::new(),
                                            vec![Self::toolless_execution_delegate_call(
                                                steps,
                                                &latest_query,
                                                artifact_route,
                                                continuation_context,
                                            )],
                                            steps,
                                            &mut history,
                                            &mut tool_trace,
                                        )
                                        .await?;
                                    continue;
                                }
                            }
                        }
                        let final_text = Self::synthesize_successful_tool_delivery(messages)
                            .or_else(|| {
                                Self::synthesize_incomplete_tool_delivery_for_recovery(
                                    messages,
                                    &latest_query,
                                )
                            })
                            .or_else(|| {
                                Self::latest_blocked_tool_result(messages).map(
                                    |(tool_name, content)| {
                                        Self::summarize_blocked_tool_delivery(
                                            &latest_query,
                                            &tool_name,
                                            &content,
                                            Self::query_prefers_chinese(&latest_query),
                                        )
                                    },
                                )
                            })
                            .unwrap_or_else(|| {
                                capability_route_tool_required_failure_message(failure_route)
                            });
                        bridge.emit(AgentEventData::Thought {
                            content: "TOOL-FIRST RECOVERY: the model still produced no tool call after an explicit execution-required prompt; finalizing from the best verified execution evidence."
                                .to_string(),
                        });

                        let outcome = bridge
                            .finalize_outcome(
                                messages, final_text, usage, thoughts, tool_trace, steps,
                            )
                            .await?;

                        return Ok(outcome);
                    }
                }

                if let Some(latest_query) = Self::latest_knowledge_persistence_query(messages)
                    .or_else(|| Self::latest_user_query(messages))
                {
                    if Self::query_requests_knowledge_persistence(&latest_query)
                        && Self::latest_delegate_role(messages)
                            .as_deref()
                            .is_none_or(|role| role != "knowledge")
                        && Self::should_prioritize_followup_execution(&latest_query, messages)
                    {
                        if let Some(result) =
                            Self::latest_lookup_result_for_followup_execution(messages)
                        {
                            if Self::content_contains_verification_challenge(&result) {
                                let final_text = Self::summarize_delegate_delivery(
                                    &latest_query,
                                    &result,
                                    Self::query_prefers_chinese(&latest_query),
                                );
                                bridge.emit(AgentEventData::Thought {
                                    content: "ORCHESTRATION FINALIZE: lookup reached a source fetch, but the fetched page is an anti-bot/security verification gate rather than real source content; stopping without importing to knowledge.".to_string(),
                                });
                                return bridge
                                    .finalize_outcome(
                                        messages, final_text, usage, thoughts, tool_trace, steps,
                                    )
                                    .await;
                            }

                            if !Self::lookup_result_satisfies_requested_material_alignment(
                                &latest_query,
                                &result,
                            ) {
                                let marker = "BENSHU_ORCHESTRATION_SOURCE_ALIGNMENT_GAP";
                                let retry_marker =
                                    "BENSHU_ORCHESTRATION_SOURCE_ALIGNMENT_RECOVERY_DELEGATED";
                                if self.tool_is_enabled("delegate")
                                    && !Self::has_system_marker_after_latest_user(
                                        messages,
                                        retry_marker,
                                    )
                                    && steps < max_steps
                                {
                                    bridge.emit(AgentEventData::Thought {
                                        content: Self::user_facing_progress_message(
                                            "source_fetch",
                                            &latest_query,
                                        ),
                                    });
                                    bridge.emit(AgentEventData::Thought {
                                        content: "ORCHESTRATION RECOVERY: the current lookup evidence is readable but mismatched with the requested source material; delegating one bounded alternative-source search instead of importing the mismatched URL.".to_string(),
                                    });
                                    if !Self::has_system_marker_after_latest_user(messages, marker)
                                    {
                                        messages.push(Message::system(marker.to_string()));
                                    }
                                    messages.push(Message::system(retry_marker.to_string()));
                                    bridge
                                        .executor()
                                        .coordinate(
                                            messages,
                                            String::new(),
                                            vec![(
                                                format!(
                                                    "orchestrated-source-alignment-recovery-{}",
                                                    steps
                                                ),
                                                "delegate".to_string(),
                                                serde_json::json!({
                                                    "role": "researcher",
                                                    "full_user_request": latest_query,
                                                    "task": format!(
                                                        "Continue the same user request by finding and fetching an alternative readable source body that matches the user's explicit source-material type. The latest fetched body was readable but mismatched, so do not import it and do not write the artifact from it. Return status, worker: researcher, executed_tool, source_url, and fetched_result/body evidence for the next candidate, or a clear blocker if no aligned source can be verified.\n\nOriginal user request:\n{}\n\nRejected lookup preview:\n{}",
                                                        latest_query,
                                                        Self::compact_lookup_evidence_for_file_artifact(&result)
                                                    )
                                                }),
                                            )],
                                            steps,
                                            &mut history,
                                            &mut tool_trace,
                                        )
                                        .await?;
                                    continue;
                                }

                                let final_text =
                                    Self::source_alignment_blocker_text(&latest_query, &result);
                                bridge.emit(AgentEventData::Thought {
                                    content: "ORCHESTRATION FINALIZE: source alignment recovery did not produce an aligned source; stopping before knowledge import instead of treating the mismatched lookup as usable evidence.".to_string(),
                                });
                                return bridge
                                    .finalize_outcome(
                                        messages, final_text, usage, thoughts, tool_trace, steps,
                                    )
                                    .await;
                            }

                            if let Some(url) =
                                Self::followup_execution_source_url(&latest_query, &result)
                            {
                                if !self.tool_is_enabled("delegate") {
                                    let final_text =
                                        Self::knowledge_import_coordinator_handoff_result(
                                            &latest_query,
                                            &url,
                                            &result,
                                        );
                                    bridge.emit(AgentEventData::Thought {
                                        content: "ORCHESTRATION CHAIN: current worker found source evidence for a knowledge-persistence task but is not equipped with cross-worker delegation; returning a coordinator handoff instead of calling unavailable tools.".to_string(),
                                    });
                                    return bridge
                                        .finalize_outcome(
                                            messages, final_text, usage, thoughts, tool_trace,
                                            steps,
                                        )
                                        .await;
                                }

                                bridge.emit(AgentEventData::Thought {
                                    content: Self::user_facing_progress_message(
                                        "knowledge_import",
                                        &latest_query,
                                    ),
                                });
                                bridge.emit(AgentEventData::Thought {
                                    content: "ORCHESTRATION CHAIN: empty/no-tool model turn still has verified lookup evidence and the user requested knowledge persistence; executing one bounded `knowledge` import before final delivery.".to_string(),
                                });

                                bridge
                                    .executor()
                                    .coordinate(
                                        messages,
                                        String::new(),
                                        vec![Self::knowledge_import_delegate_call_with_evidence(
                                            steps,
                                            &url,
                                            &latest_query,
                                            Some(&Self::lookup_result_source_body_surface(&result)),
                                        )],
                                        steps,
                                        &mut history,
                                        &mut tool_trace,
                                    )
                                    .await?;

                                if let Some((failed_tool_name, error)) =
                                    Self::latest_tool_error_result(messages)
                                {
                                    let final_text = Self::tool_failure_delivery_text(
                                        &latest_query,
                                        &failed_tool_name,
                                        &error,
                                    );
                                    bridge.emit(AgentEventData::Thought {
                                        content: format!(
                                            "ORCHESTRATION FINALIZE: bounded knowledge-import follow-up failed inside `{}`; returning the blocker directly.",
                                            failed_tool_name
                                        ),
                                    });
                                    return bridge
                                        .finalize_outcome(
                                            messages, final_text, usage, thoughts, tool_trace,
                                            steps,
                                        )
                                        .await;
                                }

                                let knowledge_import_completed =
                                    Self::latest_successful_tool_result_text(messages, "delegate")
                                        .as_deref()
                                        .is_some_and(|result| {
                                            Self::tool_result_has_knowledge_persistence_effect(
                                                result,
                                            )
                                        });
                                if !knowledge_import_completed {
                                    bridge.emit(AgentEventData::Thought {
                                        content: "ORCHESTRATION RECOVERY: bounded knowledge import returned without a durable knowledge.imported receipt; continuing instead of treating it as completed input for artifact writing.".to_string(),
                                    });
                                    continue;
                                }

                                if Self::query_requests_post_import_delivery(&latest_query) {
                                    if let Some(final_text) =
                                        Self::try_file_artifact_followup_after_import(
                                            bridge,
                                            messages,
                                            &latest_query,
                                            steps,
                                            &mut history,
                                            &mut tool_trace,
                                        )
                                        .await?
                                    {
                                        bridge.emit(AgentEventData::Thought {
                                            content: Self::file_artifact_followup_finalize_thought(
                                                messages,
                                                &latest_query,
                                            ),
                                        });
                                        return bridge
                                            .finalize_outcome(
                                                messages, final_text, usage, thoughts, tool_trace,
                                                steps,
                                            )
                                            .await;
                                    }
                                    if let Some(final_text) = Self::synthesize_post_import_delivery(
                                        &latest_query,
                                        messages,
                                    ) {
                                        bridge.emit(AgentEventData::Thought {
                                            content: "ORCHESTRATION FINALIZE: knowledge import completed from an empty/no-tool turn and verified researcher data is already available; returning the requested final delivery immediately.".to_string(),
                                        });
                                        return bridge
                                            .finalize_outcome(
                                                messages, final_text, usage, thoughts, tool_trace,
                                                steps,
                                            )
                                            .await;
                                    }
                                    Self::push_post_import_delivery_instruction(
                                        messages,
                                        &latest_query,
                                    );
                                    continue;
                                }

                                if let Some(knowledge_result) =
                                    Self::latest_successful_tool_result_text(messages, "delegate")
                                {
                                    let final_text = Self::summarize_delegate_delivery(
                                        &latest_query,
                                        &knowledge_result,
                                        Self::query_prefers_chinese(&latest_query),
                                    );
                                    bridge.emit(AgentEventData::Thought {
                                        content: "ORCHESTRATION FINALIZE: lookup -> knowledge chain completed after an empty/no-tool model turn; returning the import result.".to_string(),
                                    });
                                    return bridge
                                        .finalize_outcome(
                                            messages, final_text, usage, thoughts, tool_trace,
                                            steps,
                                        )
                                        .await;
                                }
                            }
                        }
                    }
                }

                if steps < max_steps {
                    if let Some(query) = Self::latest_user_query(messages) {
                        if let Some(empty_lookup) =
                            Self::latest_repeated_empty_lookup_result(messages)
                        {
                            if self.tool_is_enabled("browser_browse") {
                                bridge.emit(AgentEventData::Thought {
                                    content: "ORCHESTRATION RECOVERY: repeated lookup attempts returned empty/low-information results; switching once to an observation-capable tool surface.".to_string(),
                                });
                                messages.push(Message::system(
                                    "BENSHU_ORCHESTRATION_OBSERVATION_RECOVERY".to_string(),
                                ));
                                bridge
                                    .executor()
                                    .coordinate(
                                        messages,
                                        String::new(),
                                        vec![Self::observation_recovery_tool_call(steps, &query)],
                                        steps,
                                        &mut history,
                                        &mut tool_trace,
                                    )
                                    .await?;
                                continue;
                            }
                            if self.tool_is_enabled("delegate") {
                                bridge.emit(AgentEventData::Thought {
                                    content: "ORCHESTRATION RECOVERY: repeated lookup attempts returned empty/low-information results; delegating once to an observation-capable worker.".to_string(),
                                });
                                messages.push(Message::system(
                                    "BENSHU_ORCHESTRATION_OBSERVATION_RECOVERY".to_string(),
                                ));
                                bridge
                                    .executor()
                                    .coordinate(
                                        messages,
                                        String::new(),
                                        vec![Self::observation_recovery_delegate_call(
                                            steps,
                                            &query,
                                            &empty_lookup,
                                        )],
                                        steps,
                                        &mut history,
                                        &mut tool_trace,
                                    )
                                    .await?;
                                continue;
                            }
                        }

                        let collection_gap_recovery = Self::has_system_marker_after_latest_user(
                            messages,
                            "BENSHU_ORCHESTRATION_COLLECTION_EVIDENCE_GAP",
                        );
                        if self.tool_is_enabled("delegate")
                            && Self::recent_context_only_artifact_progress_stalled(messages, &query)
                            && !Self::has_system_marker_after_latest_user(
                                messages,
                                "BENSHU_ARTIFACT_CONTEXT_ONLY_ESCALATED_TO_WORKER",
                            )
                        {
                            bridge.emit(AgentEventData::Thought {
                                content: "ORCHESTRATION RECOVERY: context-only tools were used for an artifact task but no durable write receipt exists; delegating once to the owning artifact worker instead of continuing memory/context reads.".to_string(),
                            });
                            messages.push(Message::system(
                                "BENSHU_ARTIFACT_CONTEXT_ONLY_ESCALATED_TO_WORKER".to_string(),
                            ));
                            let artifact_route = Self::artifact_execution_delegate_route(&query);
                            let continuation_context =
                                Self::latest_delegate_artifact_continuation_context(messages);
                            bridge
                                .executor()
                                .coordinate(
                                    messages,
                                    String::new(),
                                    vec![Self::toolless_execution_delegate_call(
                                        steps,
                                        &query,
                                        artifact_route,
                                        continuation_context,
                                    )],
                                    steps,
                                    &mut history,
                                    &mut tool_trace,
                                )
                                .await?;
                            continue;
                        }
                        let mut available_tools = self.available_execution_tools_for_query(&query);
                        if collection_gap_recovery {
                            available_tools =
                                Self::prioritize_observation_tools_after_collection_gap(
                                    available_tools,
                                );
                        }
                        let latest_result_already_satisfies_request =
                            Self::latest_successful_result_satisfies_execution_request(
                                messages, &query,
                            );
                        if latest_result_already_satisfies_request {
                            bridge.emit(AgentEventData::Thought {
                                content: "TOOL-FIRST RECOVERY: latest verified tool evidence already satisfies the execution request; finalizing from the durable receipt instead of forcing another tool call.".to_string(),
                            });
                        } else if decide_tool_first_recovery(ToolFirstRecoveryInput {
                            current_step: steps,
                            max_steps,
                            available_tool_count: available_tools.len(),
                            has_recent_tool_execution_required_prompt:
                                Self::has_recent_tool_execution_required_prompt(messages),
                            simple_media_understanding,
                        }) {
                            let listed_tools = available_tools.join(", ");
                            bridge.emit(AgentEventData::Thought {
                                content: format!(
                                    "TOOL-FIRST RECOVERY: explicit execution request detected; surfacing actionable tools for the model to choose from [{listed_tools}]."
                                ),
                            });
                            let collection_gap_instruction = if collection_gap_recovery {
                                "\nThe prior search did not satisfy the requested item-level collection evidence. Switch observation surface now: open/fetch concrete candidate URLs or use the browser observation tool. Do not repeat the same broad search unless no observation tool is available."
                            } else {
                                ""
                            };
                            messages.push(Message::system(format!(
                                "{}\n\nThe latest user turn is an explicit execution request, and these tools are available right now: {}.\nChoose the next concrete action: call the best matching equipped tool, or return a concise blocker if execution is impossible in this runtime.\nDo not continue open-ended analysis without making progress toward observable evidence or a clear blocker.",
                                reasoner_constants::MARKER_TOOL_EXECUTION_REQUIRED,
                                listed_tools
                            ) + collection_gap_instruction));
                            continue;
                        }
                    }
                }

                // Phase 12.3: Reflexion - Autonomous Self-Critique (会复盘 + 会改错)
                let durable_effect_result =
                    Self::latest_successful_durable_effect_tool_result(messages);
                if durable_effect_result.is_some() {
                    bridge.emit(AgentEventData::Thought {
                        content: "FINALIZATION GUARD: durable tool side-effect receipt detected; skipping autonomous reflexion on tool logs and finalizing from runtime evidence.".to_string(),
                    });
                }

                if durable_effect_result.is_none()
                    && !Self::latest_user_query(messages)
                        .as_deref()
                        .is_some_and(Self::query_is_creation_planning_dialogue)
                    && !Self::model_output_is_empty_or_non_deliverable(&full_text)
                    && should_run_reflexion_review(ReflexionReviewInput {
                        strategy_is_reflexion: adapted_strategy == ReasoningStrategy::Reflexion,
                        current_step: steps,
                        max_steps,
                        has_media_input: latest_user_message_has_media(messages),
                        simple_media_understanding,
                    })
                {
                    bridge.emit(AgentEventData::Thought {
                        content: "REFLEXION: Performing autonomous self-critique...".to_string(),
                    });

                    let critique_request = crate::agent::provider::ChatRequest {
                        model: self.config.model.clone(),
                        system_prompt: Some("You are a rigorous self-critic. Review the last response for accuracy and missing steps. Respond only with '[CRITIQUE] <reason>' or '[PASSED]'.".to_string()),
                        messages: vec![Message::user(format!("Critique this response:\n\n{}", full_text))],
                        temperature: Some(0.1),
                        max_tokens: Some(256),
                        session_id: self.auxiliary_session_id("reflexion"),
                        ..Default::default()
                    };

                    // Use streaming and collect for critique
                    if let Ok(stream) = self.provider.stream_completion(critique_request).await {
                        let critique_text = stream.collect_text().await.unwrap_or_default();
                        if let Some(reason) = extract_reflexion_critique_reason(&critique_text) {
                            if Self::reflexion_critique_reports_missing_response(&reason) {
                                let retry_marker = "BENSHU_EMPTY_MODEL_RESPONSE_RETRY";
                                if Self::has_system_marker_after_latest_user(messages, retry_marker)
                                {
                                    let final_text = Self::empty_worker_response_blocker(messages);
                                    bridge.emit(AgentEventData::Thought {
                                        content: "EMPTY RESPONSE GUARD: reflexion reported a missing response twice; returning a structured blocker instead of continuing critique churn.".to_string(),
                                    });
                                    return bridge
                                        .finalize_outcome(
                                            messages, final_text, usage, thoughts, tool_trace,
                                            steps,
                                        )
                                        .await;
                                }

                                messages.push(Message::system(format!(
                                    "{retry_marker}\n\nThe reflexion reviewer reported that the previous model turn was empty or missing. Continue exactly once by either calling the best equipped tool for the current task or returning a concise blocker with the missing runtime condition. Do not run reflexion on an empty response."
                                )));
                                bridge.emit(AgentEventData::Thought {
                                    content: "EMPTY RESPONSE GUARD: reflexion reported a missing response; giving the worker one actionable retry before stopping this loop.".to_string(),
                                });
                                continue;
                            }

                            info!("Reasoner: Reflexion found error: {}", reason);
                            bridge.emit(AgentEventData::Thought {
                                content: format!("REFLEXION CRITIQUE DETECTED: {}", reason),
                            });

                            messages.push(Message::system(format!(
                                "{}\n\nYou missed something or made an error: {}\n\nPlease correct your response and provide the final answer.",
                                reasoner_constants::MARKER_REFLEXION_CRITIQUE,
                                reason
                            )));
                            continue; // Re-enter loop to fix it
                        }
                    }
                }

                let final_text = if let Some((tool_name, receipt)) = durable_effect_result {
                    if full_text.contains("runtime_effect:")
                        || full_text.contains("runtime_effects:")
                    {
                        full_text
                    } else {
                        format!(
                            "runtime_effect_receipt_tool: {tool_name}\n{receipt}\n\n{full_text}"
                        )
                    }
                } else {
                    full_text
                };
                let final_text = Self::strip_model_channel_markers(&final_text);

                // --- 4. Verify (Finalize) ---
                let outcome = bridge
                    .finalize_outcome(messages, final_text, usage, thoughts, tool_trace, steps)
                    .await?;

                return Ok(outcome);
            }

            // Priority 3: Shadow Red-Team Audit (Pre-Execution Guard)
            if matches!(
                self.run_red_team_audit_if_needed(
                    bridge,
                    messages,
                    &thoughts_snapshot,
                    &tool_calls,
                    risk_score,
                    max_steps,
                    &mut audit_rejections,
                )
                .await?,
                execution_guard::GuardDecision::ContinueLoop
            ) {
                continue;
            }

            bridge
                .executor()
                .coordinate(
                    messages,
                    full_text,
                    tool_calls.clone(),
                    steps,
                    &mut history,
                    &mut tool_trace,
                )
                .await?;

            let latest_query = Self::latest_user_query(messages).unwrap_or_default();
            if let Some(final_text) = Self::direct_tool_display_delivery(messages, &latest_query) {
                bridge.emit(AgentEventData::Thought {
                    content: "ORCHESTRATION FINALIZE: latest tool result declared itself finalizable and provided a user-facing display payload; returning it without another model round.".to_string(),
                });
                return bridge
                    .finalize_outcome(messages, final_text, usage, thoughts, tool_trace, steps)
                    .await;
            }
            if let Some((tool_name, content)) = Self::latest_blocked_tool_result(messages) {
                let final_text = Self::summarize_blocked_tool_delivery(
                    &latest_query,
                    &tool_name,
                    &content,
                    Self::query_prefers_chinese(&latest_query),
                );
                bridge.emit(AgentEventData::Thought {
                    content: "ORCHESTRATION FINALIZE: latest tool result is blocked or needs revision; returning the blocker instead of entering another model round.".to_string(),
                });
                return bridge
                    .finalize_outcome(messages, final_text, usage, thoughts, tool_trace, steps)
                    .await;
            }
            let persistence_query =
                Self::latest_knowledge_persistence_query(messages).unwrap_or(latest_query.clone());
            if let Some((failed_tool_name, error)) = Self::latest_tool_error_result(messages) {
                let delegated_contract_recovery_marker = Self::tool_contract_recovery_marker(
                    "BENSHU_DELEGATED_WORKER_TOOL_CONTRACT_RECOVERY",
                    &failed_tool_name,
                    &error,
                );
                if failed_tool_name == "delegate"
                    && Self::tool_error_is_recoverable_contract(&error)
                    && !Self::has_system_marker_after_latest_user(
                        messages,
                        &delegated_contract_recovery_marker,
                    )
                {
                    if let Some(role) = Self::delegate_worker_role_from_error(&error)
                        .or_else(|| Self::latest_delegate_role(messages))
                    {
                        bridge.emit(AgentEventData::Thought {
                            content: format!(
                                "DELEGATED WORKER TOOL CONTRACT RECOVERY: worker `{}` returned a structured argument/content error; retrying once with complete tool arguments instead of failing the top-level task.",
                                role
                            ),
                        });
                        messages.push(Message::system(delegated_contract_recovery_marker));
                        bridge
                            .executor()
                            .coordinate(
                                messages,
                                String::new(),
                                vec![Self::worker_tool_contract_recovery_delegate_call(
                                    steps,
                                    &role,
                                    &latest_query,
                                    &error,
                                )],
                                steps,
                                &mut history,
                                &mut tool_trace,
                            )
                            .await?;
                        continue;
                    }
                }
                if Self::tool_error_is_loop_prevention(&error)
                    && !Self::has_system_marker_after_latest_user(
                        messages,
                        "BENSHU_LOOP_GUARD_RECOVERY",
                    )
                {
                    let available_tools = self.available_execution_tools_for_query(&latest_query);
                    if let Some(prompt) = Self::loop_guard_recovery_prompt(
                        &failed_tool_name,
                        &error,
                        &available_tools,
                    ) {
                        bridge.emit(AgentEventData::Thought {
                            content: format!(
                                "LOOP GUARD RECOVERY: `{}` repeated without progress; retrying once with alternative tool surface instead of failing immediately.",
                                failed_tool_name
                            ),
                        });
                        messages.push(Message::system(prompt));
                        continue;
                    }
                }
                let tool_contract_recovery_marker = Self::tool_contract_recovery_marker(
                    "BENSHU_TOOL_CONTRACT_RECOVERY",
                    &failed_tool_name,
                    &error,
                );
                if Self::tool_error_is_recoverable_contract(&error)
                    && !Self::has_system_marker_after_latest_user(
                        messages,
                        &tool_contract_recovery_marker,
                    )
                {
                    let available_tools = self.available_execution_tools_for_query(&latest_query);
                    bridge.emit(AgentEventData::Thought {
                        content: format!(
                            "TOOL CONTRACT RECOVERY: `{}` returned a structured argument/content error; retrying once with corrected tool arguments instead of finalizing from the error.",
                            failed_tool_name
                        ),
                    });
                    messages.push(Message::system(tool_contract_recovery_marker));
                    messages.push(Message::system(Self::tool_contract_recovery_prompt(
                        &failed_tool_name,
                        &error,
                        &available_tools,
                    )));
                    continue;
                }
                if self.should_retry_tool_boundary_recovery(&failed_tool_name, &error)
                    && !Self::has_system_marker_after_latest_user(
                        messages,
                        "BENSHU_TOOL_BOUNDARY_RECOVERY",
                    )
                {
                    let available_tools = self.available_execution_tools_for_query(&latest_query);
                    bridge.emit(AgentEventData::Thought {
                        content: format!(
                            "TOOL BOUNDARY RECOVERY: `{}` is not equipped for this agent; retrying once with the current available tool surface instead of finalizing.",
                            failed_tool_name
                        ),
                    });
                    messages.push(Message::system(Self::tool_boundary_recovery_prompt(
                        &failed_tool_name,
                        &error,
                        &available_tools,
                    )));
                    continue;
                }
                if failed_tool_name == "delegate"
                    && Self::delegate_worker_tool_boundary_error(&error)
                    && !Self::has_system_marker_after_latest_user(
                        messages,
                        "BENSHU_DELEGATED_WORKER_TOOL_BOUNDARY_RECOVERY",
                    )
                {
                    if let Some(role) = Self::delegate_worker_role_from_error(&error)
                        .or_else(|| Self::latest_delegate_role(messages))
                    {
                        bridge.emit(AgentEventData::Thought {
                            content: format!(
                                "DELEGATED WORKER TOOL BOUNDARY RECOVERY: worker `{}` called an unavailable tool; retrying once with its equipped tool surface instead of failing the top-level task.",
                                role
                            ),
                        });
                        messages.push(Message::system(
                            "BENSHU_DELEGATED_WORKER_TOOL_BOUNDARY_RECOVERY".to_string(),
                        ));
                        bridge
                            .executor()
                            .coordinate(
                                messages,
                                String::new(),
                                vec![Self::worker_tool_boundary_recovery_delegate_call(
                                    steps,
                                    &role,
                                    &latest_query,
                                    &error,
                                )],
                                steps,
                                &mut history,
                                &mut tool_trace,
                            )
                            .await?;
                        continue;
                    }
                }
                let final_text =
                    Self::tool_failure_delivery_text(&persistence_query, &failed_tool_name, &error);
                bridge.emit(AgentEventData::Thought {
                    content: format!(
                        "ORCHESTRATION FINALIZE: `{}` returned a runtime error; returning a clear blocker instead of continuing until NoResponse.",
                        failed_tool_name
                    ),
                });
                return bridge
                    .finalize_outcome(messages, final_text, usage, thoughts, tool_trace, steps)
                    .await;
            }

            if Self::query_requests_knowledge_persistence(&persistence_query)
                && !Self::has_system_marker_after_latest_user(
                    messages,
                    "BENSHU_ORCHESTRATION_STRUCTURED_KNOWLEDGE_CREATE",
                )
            {
                if let Some(evidence) =
                    Self::latest_metadata_surrogate_lookup_for_requested_source_content(
                        messages,
                        &persistence_query,
                    )
                {
                    let final_text =
                        Self::metadata_surrogate_depth_blocker(&persistence_query, &evidence);
                    bridge.emit(AgentEventData::Thought {
                        content: "ORCHESTRATION FINALIZE: lookup satisfied item metadata count but not the requested source-content depth; returning a blocker instead of importing metadata as source content.".to_string(),
                    });
                    return bridge
                        .finalize_outcome(messages, final_text, usage, thoughts, tool_trace, steps)
                        .await;
                }

                if let Some(evidence) = Self::latest_structured_lookup_result_for_knowledge_create(
                    messages,
                    &persistence_query,
                ) {
                    bridge.emit(AgentEventData::Thought {
                        content: Self::user_facing_progress_message(
                            "knowledge_import",
                            &persistence_query,
                        ),
                    });
                    bridge.emit(AgentEventData::Thought {
                        content: "ORCHESTRATION CHAIN: lookup produced enough structured item-level evidence; creating a knowledge document from the verified metadata instead of importing a challenge-prone source URL.".to_string(),
                    });
                    messages.push(Message::system(
                        "BENSHU_ORCHESTRATION_STRUCTURED_KNOWLEDGE_CREATE".to_string(),
                    ));

                    bridge
                        .executor()
                        .coordinate(
                            messages,
                            String::new(),
                            vec![Self::knowledge_create_delegate_call(
                                steps,
                                &persistence_query,
                                &evidence,
                            )],
                            steps,
                            &mut history,
                            &mut tool_trace,
                        )
                        .await?;

                    if let Some((failed_tool_name, error)) =
                        Self::latest_tool_error_result(messages)
                    {
                        let final_text = Self::tool_failure_delivery_text(
                            &persistence_query,
                            &failed_tool_name,
                            &error,
                        );
                        bridge.emit(AgentEventData::Thought {
                            content: format!(
                                "ORCHESTRATION FINALIZE: structured knowledge-create follow-up failed inside `{}`; returning the blocker directly.",
                                failed_tool_name
                            ),
                        });
                        return bridge
                            .finalize_outcome(
                                messages, final_text, usage, thoughts, tool_trace, steps,
                            )
                            .await;
                    }

                    if Self::query_requests_post_import_delivery(&persistence_query) {
                        if let Some(final_text) = Self::try_file_artifact_followup_after_import(
                            bridge,
                            messages,
                            &persistence_query,
                            steps,
                            &mut history,
                            &mut tool_trace,
                        )
                        .await?
                        {
                            bridge.emit(AgentEventData::Thought {
                                content: "ORCHESTRATION FINALIZE: structured knowledge document was created, and the requested file artifact step ran before delivery.".to_string(),
                            });
                            return bridge
                                .finalize_outcome(
                                    messages, final_text, usage, thoughts, tool_trace, steps,
                                )
                                .await;
                        }
                        if let Some(final_text) =
                            Self::synthesize_post_import_delivery(&persistence_query, messages)
                        {
                            bridge.emit(AgentEventData::Thought {
                                content: "ORCHESTRATION FINALIZE: structured knowledge document was created and final delivery was synthesized from verified evidence.".to_string(),
                            });
                            return bridge
                                .finalize_outcome(
                                    messages, final_text, usage, thoughts, tool_trace, steps,
                                )
                                .await;
                        }
                    }
                }
            }

            if Self::query_requests_knowledge_persistence(&persistence_query)
                && !Self::has_system_marker_after_latest_user(
                    messages,
                    "BENSHU_ORCHESTRATION_OBSERVATION_RECOVERY",
                )
            {
                if let Some(blocked_lookup) =
                    Self::latest_lookup_result_requiring_observation_recovery(messages)
                {
                    let recovery_action = self.lookup_recovery_action_for_result(
                        messages,
                        &persistence_query,
                        &blocked_lookup,
                        steps,
                        max_steps,
                    );
                    if !matches!(
                        recovery_action,
                        RecoveryAction::SwitchObservationSurface
                            | RecoveryAction::DelegateSpecialist
                    ) {
                        let final_text = Self::summarize_delegate_delivery(
                            &persistence_query,
                            &blocked_lookup,
                            Self::query_prefers_chinese(&persistence_query),
                        );
                        bridge.emit(AgentEventData::Thought {
                            content: format!(
                                "ORCHESTRATION FINALIZE: lookup evidence recovery decision was {:?}; returning the current blocker instead of opening another loop.",
                                recovery_action
                            ),
                        });
                        return bridge
                            .finalize_outcome(
                                messages, final_text, usage, thoughts, tool_trace, steps,
                            )
                            .await;
                    }
                    bridge.emit(AgentEventData::Thought {
                        content: Self::user_facing_progress_message(
                            "source_fetch",
                            &persistence_query,
                        ),
                    });
                    bridge.emit(AgentEventData::Thought {
                        content: "ORCHESTRATION CHAIN: lookup evidence quality is not sufficient for persistence; running one generic observation recovery without changing the user's task.".to_string(),
                    });
                    messages.push(Message::system(
                        "BENSHU_ORCHESTRATION_OBSERVATION_RECOVERY".to_string(),
                    ));
                    bridge
                        .executor()
                        .coordinate(
                            messages,
                            String::new(),
                            vec![Self::observation_recovery_delegate_call(
                                steps,
                                &persistence_query,
                                &blocked_lookup,
                            )],
                            steps,
                            &mut history,
                            &mut tool_trace,
                        )
                        .await?;

                    if let Some(evidence) =
                        Self::latest_structured_lookup_result_for_knowledge_create(
                            messages,
                            &persistence_query,
                        )
                    {
                        bridge
                            .executor()
                            .coordinate(
                                messages,
                                String::new(),
                                vec![Self::knowledge_create_delegate_call(
                                    steps,
                                    &persistence_query,
                                    &evidence,
                                )],
                                steps,
                                &mut history,
                                &mut tool_trace,
                            )
                            .await?;

                        if Self::query_requests_post_import_delivery(&persistence_query) {
                            if let Some(final_text) = Self::try_file_artifact_followup_after_import(
                                bridge,
                                messages,
                                &persistence_query,
                                steps,
                                &mut history,
                                &mut tool_trace,
                            )
                            .await?
                            {
                                return bridge
                                    .finalize_outcome(
                                        messages, final_text, usage, thoughts, tool_trace, steps,
                                    )
                                    .await;
                            }
                        }
                    }
                    if let Some(blocked_result) =
                        Self::latest_successful_tool_result_text(messages, "delegate")
                            .filter(|result| Self::tool_result_is_blocked(result))
                    {
                        let final_text = Self::summarize_delegate_delivery(
                            &persistence_query,
                            &blocked_result,
                            Self::query_prefers_chinese(&persistence_query),
                        );
                        bridge.emit(AgentEventData::Thought {
                            content: "ORCHESTRATION FINALIZE: browser escalation returned a concrete blocker without enough item-level evidence; returning the blocker instead of re-entering open-ended tool recovery.".to_string(),
                        });
                        return bridge
                            .finalize_outcome(
                                messages, final_text, usage, thoughts, tool_trace, steps,
                            )
                            .await;
                    }
                    continue;
                }
            }

            if Self::has_system_marker(messages, "BENSHU_ORCHESTRATION_CHAIN_FINAL_DELIVERY") {
                if let Some(final_text) =
                    Self::synthesize_post_import_delivery(&persistence_query, messages)
                {
                    bridge.emit(AgentEventData::Thought {
                        content: "ORCHESTRATION FINALIZE: post-import delivery was requested; synthesizing the final user-facing answer from verified researcher output and knowledge import receipt instead of looping on another specialist call.".to_string(),
                    });
                    return bridge
                        .finalize_outcome(messages, final_text, usage, thoughts, tool_trace, steps)
                        .await;
                }
            }

            if Self::latest_knowledge_persistence_query(messages).is_none()
                && !Self::query_requests_knowledge_persistence(&latest_query)
            {
                if let Some(delegate_result) =
                    Self::latest_successful_tool_result_text(messages, "delegate")
                {
                    let artifact_requested = Self::query_requests_artifact_mutation(&latest_query)
                        || Self::query_requests_file_artifact(&latest_query);
                    let artifact_satisfied = Self::tool_result_satisfies_artifact_request(
                        &latest_query,
                        &delegate_result,
                    );
                    const INTERMEDIATE_ARTIFACT_MARKER: &str =
                        "BENSHU_DELEGATE_RESULT_INTERMEDIATE_ARTIFACT_CONTINUE";
                    if artifact_requested && !artifact_satisfied {
                        if !Self::has_system_marker_after_latest_successful_tool_result(
                            messages,
                            "delegate",
                            INTERMEDIATE_ARTIFACT_MARKER,
                        ) {
                            bridge.emit(AgentEventData::Thought {
                                content: "ORCHESTRATION RECOVERY: delegated worker produced artifact progress, but the requested final artifact scope or format is still missing; continuing instead of finalizing an intermediate draft.".to_string(),
                            });
                            messages.push(Message::system(format!(
                                "{}\n\n{INTERMEDIATE_ARTIFACT_MARKER}\n\
                                 The latest delegated worker result is progress, not final completion for the user's artifact request.\n\
                                 Continue the same task. If the user requested a specific saved/exported format, do not finalize until a matching artifact receipt exists for that format. If the user requested a complete document, do not treat an individual chapter, draft chunk, status report, plan, or project directory as the final artifact. Use the equipped writing/file/export tools to continue, audit/revise when applicable, and export/save the requested final artifact. If a real runtime/tool blocker prevents completion, return that blocker explicitly instead of claiming completion.",
                                reasoner_constants::MARKER_TOOL_EXECUTION_REQUIRED
                            )));
                            continue;
                        }
                    } else {
                        let final_text = Self::summarize_delegate_delivery(
                            &latest_query,
                            &delegate_result,
                            Self::query_prefers_chinese(&latest_query),
                        );
                        bridge.emit(AgentEventData::Thought {
                            content: "ORCHESTRATION FINALIZE: delegated worker returned a concrete result that satisfies the requested artifact/answer contract; summarizing and returning it.".to_string(),
                        });
                        return bridge
                            .finalize_outcome(
                                messages, final_text, usage, thoughts, tool_trace, steps,
                            )
                            .await;
                    }
                }
            }

            if Self::query_requests_knowledge_persistence(&persistence_query)
                && Self::latest_delegate_role(messages)
                    .as_deref()
                    .is_none_or(|role| role != "knowledge")
                && Self::should_prioritize_followup_execution(&persistence_query, messages)
            {
                if let Some(result) = Self::latest_lookup_result_for_followup_execution(messages) {
                    if Self::content_contains_verification_challenge(&result) {
                        let final_text = Self::summarize_delegate_delivery(
                            &persistence_query,
                            &result,
                            Self::query_prefers_chinese(&persistence_query),
                        );
                        bridge.emit(AgentEventData::Thought {
                            content: "ORCHESTRATION FINALIZE: lookup reached a source fetch, but the fetched page is an anti-bot/security verification gate rather than real source content; stopping without importing to knowledge.".to_string(),
                        });
                        return bridge
                            .finalize_outcome(
                                messages, final_text, usage, thoughts, tool_trace, steps,
                            )
                            .await;
                    }

                    if !Self::lookup_result_satisfies_requested_material_alignment(
                        &persistence_query,
                        &result,
                    ) {
                        let marker = "BENSHU_ORCHESTRATION_SOURCE_ALIGNMENT_GAP";
                        if Self::has_system_marker_after_latest_user(messages, marker) {
                            let final_text =
                                Self::source_alignment_blocker_text(&persistence_query, &result);
                            bridge.emit(AgentEventData::Thought {
                                content: "ORCHESTRATION FINALIZE: source alignment recovery still points at the same mismatched source; stopping before knowledge import instead of treating the marker as an import bypass.".to_string(),
                            });
                            return bridge
                                .finalize_outcome(
                                    messages, final_text, usage, thoughts, tool_trace, steps,
                                )
                                .await;
                        }
                        bridge.emit(AgentEventData::Thought {
                            content: Self::user_facing_progress_message(
                                "source_fetch",
                                &persistence_query,
                            ),
                        });
                        bridge.emit(AgentEventData::Thought {
                            content: "ORCHESTRATION CHAIN: source alignment gate paused automatic knowledge import because the fetched body did not preserve the user's requested source-material intent.".to_string(),
                        });
                        messages.push(Message::system(marker.to_string()));
                        messages.push(Message::system(format!(
                            "BENSHU_SOURCE_ALIGNMENT_RECOVERY_REQUIRED\n\
                             The latest fetched body is readable, but it does not satisfy the user's original source-material intent. Do not import it into the knowledge base and do not use it as grounding for the requested artifact. Continue the same task by obtaining a source body/detail content that matches the user's explicit material type, or return a clear runtime blocker if no aligned source can be verified.\n\n\
                             User request:\n{}\n\nLatest lookup preview:\n{}",
                            persistence_query,
                            Self::compact_lookup_evidence_for_file_artifact(&result)
                        )));
                        continue;
                    }

                    let best_url = Self::followup_execution_source_url(&persistence_query, &result);
                    if let Some(url) = best_url {
                        if !self.tool_is_enabled("delegate") {
                            let final_text = Self::knowledge_import_coordinator_handoff_result(
                                &persistence_query,
                                &url,
                                &result,
                            );
                            bridge.emit(AgentEventData::Thought {
                                content: "ORCHESTRATION CHAIN: current worker found source evidence for a knowledge-persistence task but is not equipped with cross-worker delegation; returning a coordinator handoff instead of calling unavailable tools.".to_string(),
                            });
                            return bridge
                                .finalize_outcome(
                                    messages, final_text, usage, thoughts, tool_trace, steps,
                                )
                                .await;
                        }

                        bridge.emit(AgentEventData::Thought {
                            content: Self::user_facing_progress_message(
                                "knowledge_import",
                                &persistence_query,
                            ),
                        });
                        bridge.emit(AgentEventData::Thought {
                            content: "ORCHESTRATION CHAIN: lookup already produced a concrete source URL; executing a bounded `knowledge` import immediately instead of asking the model to plan the next step.".to_string(),
                        });

                        bridge
                            .executor()
                            .coordinate(
                                messages,
                                String::new(),
                                vec![(
                                    format!("orchestrated-knowledge-{}", steps),
                                    "delegate".to_string(),
                                    serde_json::json!({
                                        "role": "knowledge",
                                        "task": format!(
                                            "Import this concrete source URL into the knowledge base exactly once. Do not run another lookup. URL: {}\n\nfetched_result:\n{}",
                                            url,
                                            Self::compact_lookup_evidence_for_knowledge_import(&result)
                                        )
                                    }),
                                )],
                                steps,
                                &mut history,
                                &mut tool_trace,
                            )
                            .await?;

                        if let Some((failed_tool_name, error)) =
                            Self::latest_tool_error_result(messages)
                        {
                            let final_text = Self::tool_failure_delivery_text(
                                &persistence_query,
                                &failed_tool_name,
                                &error,
                            );
                            bridge.emit(AgentEventData::Thought {
                                content: format!(
                                    "ORCHESTRATION FINALIZE: bounded knowledge-import follow-up failed inside `{}`; returning the blocker directly.",
                                    failed_tool_name
                                ),
                            });
                            return bridge
                                .finalize_outcome(
                                    messages, final_text, usage, thoughts, tool_trace, steps,
                                )
                                .await;
                        }

                        if let Some(knowledge_result) =
                            Self::latest_successful_tool_result_text(messages, "delegate")
                        {
                            if Self::query_requests_post_import_delivery(&persistence_query)
                                && !Self::has_system_marker(
                                    messages,
                                    "BENSHU_ORCHESTRATION_CHAIN_FINAL_DELIVERY",
                                )
                            {
                                if let Some(final_text) =
                                    Self::try_file_artifact_followup_after_import(
                                        bridge,
                                        messages,
                                        &persistence_query,
                                        steps,
                                        &mut history,
                                        &mut tool_trace,
                                    )
                                    .await?
                                {
                                    bridge.emit(AgentEventData::Thought {
                                        content: Self::file_artifact_followup_finalize_thought(
                                            messages,
                                            &persistence_query,
                                        ),
                                    });
                                    return bridge
                                        .finalize_outcome(
                                            messages, final_text, usage, thoughts, tool_trace,
                                            steps,
                                        )
                                        .await;
                                }
                                if let Some(final_text) = Self::synthesize_post_import_delivery(
                                    &persistence_query,
                                    messages,
                                ) {
                                    bridge.emit(AgentEventData::Thought {
                                        content: "ORCHESTRATION FINALIZE: bounded knowledge import completed and verified researcher data is already available; returning the final requested delivery immediately instead of spending another model round.".to_string(),
                                    });
                                    return bridge
                                        .finalize_outcome(
                                            messages, final_text, usage, thoughts, tool_trace,
                                            steps,
                                        )
                                        .await;
                                }
                                Self::push_post_import_delivery_instruction(
                                    messages,
                                    &persistence_query,
                                );
                                bridge.emit(AgentEventData::Thought {
                                    content: "ORCHESTRATION CHAIN: bounded knowledge import completed, but the user also requested final analysis/delivery; continuing to one bounded final synthesis round instead of returning only the import receipt.".to_string(),
                                });
                                continue;
                            }
                            let final_text = Self::summarize_delegate_delivery(
                                &persistence_query,
                                &knowledge_result,
                                Self::query_prefers_chinese(&persistence_query),
                            );
                            bridge.emit(AgentEventData::Thought {
                                content: "ORCHESTRATION FINALIZE: bounded lookup -> knowledge chain completed in-code; returning the result without another free-form reasoning round.".to_string(),
                            });
                            return bridge
                                .finalize_outcome(
                                    messages, final_text, usage, thoughts, tool_trace, steps,
                                )
                                .await;
                        }

                        let final_text = if Self::query_prefers_chinese(&latest_query) {
                            "我已经拿到可导入的来源链接并执行了知识库写入步骤，但当前没有收到稳定的最终回执，所以先停止继续空转。".to_string()
                        } else {
                            "I obtained an importable source URL and executed the knowledge-base import step, but the runtime did not return a stable final receipt, so I stopped instead of continuing to loop.".to_string()
                        };
                        bridge.emit(AgentEventData::Thought {
                            content: "ORCHESTRATION FINALIZE: bounded knowledge-import step produced no stable receipt; stopping here instead of re-entering open-ended planning.".to_string(),
                        });
                        return bridge
                            .finalize_outcome(
                                messages, final_text, usage, thoughts, tool_trace, steps,
                            )
                            .await;
                    }

                    let recovery_action = self.lookup_recovery_action_for_result(
                        messages,
                        &persistence_query,
                        &result,
                        steps,
                        max_steps,
                    );
                    if matches!(
                        recovery_action,
                        RecoveryAction::SwitchObservationSurface
                            | RecoveryAction::DelegateSpecialist
                    ) && !Self::has_system_marker_after_latest_user(
                        messages,
                        "BENSHU_ORCHESTRATION_OBSERVATION_RECOVERY",
                    ) {
                        bridge.emit(AgentEventData::Thought {
                            content: format!(
                                "ORCHESTRATION CHAIN: lookup returned weak evidence ({:?}); running one generic observation recovery before deciding whether to import or return a blocker.",
                                Self::lookup_result_evidence_quality(&result, &persistence_query)
                            ),
                        });
                        messages.push(Message::system(
                            "BENSHU_ORCHESTRATION_OBSERVATION_RECOVERY".to_string(),
                        ));
                        bridge
                            .executor()
                            .coordinate(
                                messages,
                                String::new(),
                                vec![Self::observation_recovery_delegate_call(
                                    steps,
                                    &persistence_query,
                                    &result,
                                )],
                                steps,
                                &mut history,
                                &mut tool_trace,
                            )
                            .await?;
                        continue;
                    }

                    let final_text = Self::summarize_delegate_delivery(
                        &persistence_query,
                        &result,
                        Self::query_prefers_chinese(&persistence_query),
                    );
                    bridge.emit(AgentEventData::Thought {
                        content: format!(
                            "ORCHESTRATION FINALIZE: lookup evidence recovery decision was {:?}; returning the current blocker instead of launching another open-ended follow-up search.",
                            recovery_action
                        ),
                    });
                    return bridge
                        .finalize_outcome(messages, final_text, usage, thoughts, tool_trace, steps)
                        .await;
                }
            }

            if Self::query_requests_knowledge_persistence(&persistence_query)
                && Self::latest_successful_tool_name(messages)
                    .as_deref()
                    .is_some_and(|name| name == "tool_search")
            {
                if let Some(tool_search_result) =
                    Self::latest_successful_tool_result_text(messages, "tool_search")
                {
                    if Self::tool_search_result_indicates_external_lookup(&tool_search_result) {
                        bridge.emit(AgentEventData::Thought {
                            content: Self::user_facing_progress_message(
                                "lookup_start",
                                &persistence_query,
                            ),
                        });
                        bridge.emit(AgentEventData::Thought {
                            content: "ORCHESTRATION CHAIN: tool_search already identified an external lookup route for a knowledge-persistence task; executing one bounded `delegate(researcher, ...)` step instead of letting the model re-enter tool discovery.".to_string(),
                        });
                        bridge
                            .executor()
                            .coordinate(
                                messages,
                                String::new(),
                                vec![(
                                    format!("orchestrated-researcher-{}", steps),
                                    "delegate".to_string(),
                                    serde_json::json!({
                                        "role": "researcher",
                                        "task": format!(
                                            "Search the requested sources for this user task, return titles, concise findings, and source URLs, and if needed fetch at most one best candidate source page. Do not import into the knowledge base yourself. User task: {}",
                                            persistence_query
                                        )
                                    }),
                                )],
                                steps,
                                &mut history,
                                &mut tool_trace,
                            )
                            .await?;

                        if let Some((failed_tool_name, error)) =
                            Self::latest_tool_error_result(messages)
                        {
                            let final_text = Self::tool_failure_delivery_text(
                                &persistence_query,
                                &failed_tool_name,
                                &error,
                            );
                            bridge.emit(AgentEventData::Thought {
                                content: format!(
                                    "ORCHESTRATION FINALIZE: bounded researcher follow-up after tool_search failed inside `{}`; returning the blocker directly.",
                                    failed_tool_name
                                ),
                            });
                            return bridge
                                .finalize_outcome(
                                    messages, final_text, usage, thoughts, tool_trace, steps,
                                )
                                .await;
                        }
                    }
                }
            }

            if Self::latest_loop_guard_abort_for_tool(messages, "web_search")
                || Self::latest_loop_guard_abort_for_tool(messages, "web_fetch")
                || Self::latest_loop_guard_abort_for_tool(messages, "browser")
            {
                let final_text = Self::lookup_loop_guard_failure_message(&latest_query);
                bridge.emit(AgentEventData::Thought {
                    content: "ORCHESTRATION FINALIZE: external lookup loop guard stopped repeated browser/search attempts; returning a clear blocker instead of waiting for gateway timeout.".to_string(),
                });
                return bridge
                    .finalize_outcome(messages, final_text, usage, thoughts, tool_trace, steps)
                    .await;
            }

            if Self::query_requests_knowledge_persistence(&persistence_query)
                && !Self::has_system_marker(messages, "BENSHU_ORCHESTRATION_CHAIN_KNOWLEDGE")
            {
                if let Some(result) = Self::latest_lookup_result_for_followup_execution(messages) {
                    if let Some(url) = Self::explicit_source_url_in_result(&result).or_else(|| {
                        Self::best_lookup_source_url_for_query(&persistence_query, &result)
                    }) {
                        if !self.tool_is_enabled("delegate") {
                            let final_text = Self::knowledge_import_coordinator_handoff_result(
                                &persistence_query,
                                &url,
                                &result,
                            );
                            bridge.emit(AgentEventData::Thought {
                                content: "ORCHESTRATION CHAIN (fallback): current worker found source evidence for a knowledge-persistence task but is not equipped with cross-worker delegation; returning a coordinator handoff instead of calling unavailable tools.".to_string(),
                            });
                            return bridge
                                .finalize_outcome(
                                    messages, final_text, usage, thoughts, tool_trace, steps,
                                )
                                .await;
                        }

                        bridge.emit(AgentEventData::Thought {
                            content: Self::user_facing_progress_message(
                                "knowledge_import",
                                &persistence_query,
                            ),
                        });
                        bridge.emit(AgentEventData::Thought {
                            content: "ORCHESTRATION CHAIN (fallback): researcher already returned a concrete source URL for a knowledge-persistence task; forcing one bounded `delegate(knowledge, ...)` follow-up instead of stopping at a partial handoff.".to_string(),
                        });

                        messages.push(Message::system(
                            "BENSHU_ORCHESTRATION_CHAIN_KNOWLEDGE".to_string(),
                        ));

                        bridge
                            .executor()
                            .coordinate(
                                messages,
                                String::new(),
                                vec![(
                                    format!("orchestrated-knowledge-fallback-{}", steps),
                                    "delegate".to_string(),
                                    serde_json::json!({
                                        "role": "knowledge",
                                        "task": format!(
                                            "Import this concrete source URL into the knowledge base exactly once. Do not run another lookup. URL: {}",
                                            url
                                        )
                                    }),
                                )],
                                steps,
                                &mut history,
                                &mut tool_trace,
                            )
                            .await?;

                        if let Some((failed_tool_name, error)) =
                            Self::latest_tool_error_result(messages)
                        {
                            let final_text = Self::tool_failure_delivery_text(
                                &persistence_query,
                                &failed_tool_name,
                                &error,
                            );
                            bridge.emit(AgentEventData::Thought {
                                content: format!(
                                    "ORCHESTRATION FINALIZE: fallback bounded knowledge-import follow-up failed inside `{}`; returning the blocker directly.",
                                    failed_tool_name
                                ),
                            });
                            return bridge
                                .finalize_outcome(
                                    messages, final_text, usage, thoughts, tool_trace, steps,
                                )
                                .await;
                        }

                        if let Some(knowledge_result) =
                            Self::latest_successful_tool_result_text(messages, "delegate")
                        {
                            if Self::query_requests_post_import_delivery(&persistence_query)
                                && !Self::has_system_marker(
                                    messages,
                                    "BENSHU_ORCHESTRATION_CHAIN_FINAL_DELIVERY",
                                )
                            {
                                if let Some(final_text) =
                                    Self::try_file_artifact_followup_after_import(
                                        bridge,
                                        messages,
                                        &persistence_query,
                                        steps,
                                        &mut history,
                                        &mut tool_trace,
                                    )
                                    .await?
                                {
                                    bridge.emit(AgentEventData::Thought {
                                        content: Self::file_artifact_followup_finalize_thought(
                                            messages,
                                            &persistence_query,
                                        ),
                                    });
                                    return bridge
                                        .finalize_outcome(
                                            messages, final_text, usage, thoughts, tool_trace,
                                            steps,
                                        )
                                        .await;
                                }
                                if let Some(final_text) = Self::synthesize_post_import_delivery(
                                    &persistence_query,
                                    messages,
                                ) {
                                    bridge.emit(AgentEventData::Thought {
                                        content: "ORCHESTRATION FINALIZE: fallback knowledge import completed and verified researcher data is already available; returning the final requested delivery immediately instead of spending another model round.".to_string(),
                                    });
                                    return bridge
                                        .finalize_outcome(
                                            messages, final_text, usage, thoughts, tool_trace,
                                            steps,
                                        )
                                        .await;
                                }
                                Self::push_post_import_delivery_instruction(
                                    messages,
                                    &persistence_query,
                                );
                                bridge.emit(AgentEventData::Thought {
                                    content: "ORCHESTRATION CHAIN: fallback knowledge import completed, but the user also requested final analysis/delivery; continuing to one bounded final synthesis round instead of returning only the import receipt.".to_string(),
                                });
                                continue;
                            }
                            let final_text = Self::summarize_delegate_delivery(
                                &persistence_query,
                                &knowledge_result,
                                Self::query_prefers_chinese(&persistence_query),
                            );
                            bridge.emit(AgentEventData::Thought {
                                content: "ORCHESTRATION FINALIZE: fallback lookup -> knowledge chain completed in-code; returning the final import result.".to_string(),
                            });
                            return bridge
                                .finalize_outcome(
                                    messages, final_text, usage, thoughts, tool_trace, steps,
                                )
                                .await;
                        }
                    }
                }
            }

            if Self::latest_loop_guard_abort_for_tool(messages, "tool_search") {
                let final_text = Self::tool_discovery_loop_guard_failure_message(&latest_query);
                bridge.emit(AgentEventData::Thought {
                    content: "ORCHESTRATION FINALIZE: tool discovery loop guard stopped repeated tool_search calls; returning a clear blocker instead of waiting for gateway timeout.".to_string(),
                });
                return bridge
                    .finalize_outcome(messages, final_text, usage, thoughts, tool_trace, steps)
                    .await;
            }

            if Self::latest_loop_guard_reuse_for_tool(messages, "tool_search") {
                if let Some(delegate_error) =
                    Self::latest_runtime_tool_error_for_tool(messages, "delegate")
                {
                    let mut final_text =
                        Self::tool_discovery_loop_guard_failure_message(&latest_query);
                    final_text.push_str("\n\n当前具体卡点：");
                    final_text.push_str(&Self::compact_tool_result_for_recovery(&delegate_error));
                    bridge.emit(AgentEventData::Thought {
                        content: "ORCHESTRATION FINALIZE: repeated tool_search reused cached result after delegate failure; returning current blocker instead of re-querying tools.".to_string(),
                    });
                    return bridge
                        .finalize_outcome(messages, final_text, usage, thoughts, tool_trace, steps)
                        .await;
                }

                messages.push(Message::system(format!(
                    "{}\n\nBENSHU_LOOP_GUARD_REUSE\n\
                     The latest `tool_search` call exactly repeated an earlier query, so the runtime reused the existing result instead of calling the tool again.\n\
                     Do not call `tool_search` with the same arguments again. Use the existing result, choose a concrete tool, or provide the final blocker if no tool is available.",
                    reasoner_constants::MARKER_TOOL_EXECUTION_REQUIRED
                )));
                continue;
            }

            if let Some(reused_tool_name) = Self::latest_loop_guard_reuse_tool_name(messages) {
                if reused_tool_name != "tool_search" {
                    if let Some((_, empty_lookup)) =
                        Self::latest_reused_empty_lookup_result(messages)
                    {
                        if !Self::lookup_observation_already_attempted(messages) {
                            if let Some(query) = Self::latest_user_query(messages) {
                                if self.tool_is_enabled("browser_browse") {
                                    bridge.emit(AgentEventData::Thought {
                                        content: format!(
                                            "ORCHESTRATION RECOVERY: repeated `{}` reused an empty/low-information lookup result; switching once to an observation-capable tool surface instead of treating the cache reuse as success.",
                                            reused_tool_name
                                        ),
                                    });
                                    messages.push(Message::system(
                                        "BENSHU_ORCHESTRATION_OBSERVATION_RECOVERY".to_string(),
                                    ));
                                    bridge
                                        .executor()
                                        .coordinate(
                                            messages,
                                            String::new(),
                                            vec![Self::observation_recovery_tool_call(
                                                steps, &query,
                                            )],
                                            steps,
                                            &mut history,
                                            &mut tool_trace,
                                        )
                                        .await?;
                                    continue;
                                }
                                if self.tool_is_enabled("delegate") {
                                    bridge.emit(AgentEventData::Thought {
                                        content: format!(
                                            "ORCHESTRATION RECOVERY: repeated `{}` reused an empty/low-information lookup result; delegating once to an observation-capable worker instead of treating the cache reuse as success.",
                                            reused_tool_name
                                        ),
                                    });
                                    messages.push(Message::system(
                                        "BENSHU_ORCHESTRATION_OBSERVATION_RECOVERY".to_string(),
                                    ));
                                    bridge
                                        .executor()
                                        .coordinate(
                                            messages,
                                            String::new(),
                                            vec![Self::observation_recovery_delegate_call(
                                                steps,
                                                &query,
                                                &empty_lookup,
                                            )],
                                            steps,
                                            &mut history,
                                            &mut tool_trace,
                                        )
                                        .await?;
                                    continue;
                                }
                            }
                        }
                        let final_text = Self::lookup_loop_guard_failure_message(&latest_query);
                        bridge.emit(AgentEventData::Thought {
                            content: format!(
                                "ORCHESTRATION FINALIZE: repeated `{}` reused an empty/low-information lookup result; returning a clear blocker instead of presenting the empty result as success.",
                                reused_tool_name
                            ),
                        });
                        return bridge
                            .finalize_outcome(
                                messages, final_text, usage, thoughts, tool_trace, steps,
                            )
                            .await;
                    }

                    let reused_result =
                        Self::latest_successful_tool_result_text(messages, &reused_tool_name);
                    let artifact_write_requested =
                        Self::query_requests_artifact_mutation(&latest_query)
                            || Self::query_requests_file_artifact(&latest_query);
                    if artifact_write_requested
                        && reused_result.as_deref().is_none_or(|result| {
                            !Self::tool_result_satisfies_artifact_request(&latest_query, result)
                        })
                        && !Self::has_system_marker_after_latest_user(
                            messages,
                            "BENSHU_ARTIFACT_WRITE_STILL_REQUIRED_AFTER_LOOP_REUSE",
                        )
                    {
                        bridge.emit(AgentEventData::Thought {
                            content: format!(
                                "ORCHESTRATION RECOVERY: repeated `{}` reused a read-only result, but the user requested a durable artifact mutation; continuing once instead of finalizing without artifact evidence.",
                                reused_tool_name
                            ),
                        });
                        messages.push(Message::system(format!(
                            "{}\n\nBENSHU_ARTIFACT_WRITE_STILL_REQUIRED_AFTER_LOOP_REUSE\n\
                             The latest repeated `{}` call reused an existing read-only result. The user request still requires a durable artifact mutation.\n\
                             Do not finalize from list/read/search/ledger state alone. Continue the same task by choosing an available write, update, revise, export, or file-artifact action that can produce `runtime_effect: artifact.written`.\n\
                             If no equipped tool can write or update the requested artifact, return a compact blocker that names the missing capability instead of claiming completion.",
                            reasoner_constants::MARKER_TOOL_EXECUTION_REQUIRED,
                            reused_tool_name
                        )));
                        continue;
                    }

                    if artifact_write_requested
                        && reused_result.as_deref().is_none_or(|result| {
                            !Self::tool_result_satisfies_artifact_request(&latest_query, result)
                        })
                        && self.tool_is_enabled("delegate")
                        && !Self::has_system_marker_after_latest_user(
                            messages,
                            "BENSHU_ARTIFACT_CONTEXT_ONLY_ESCALATED_TO_WORKER",
                        )
                    {
                        bridge.emit(AgentEventData::Thought {
                            content: format!(
                                "ORCHESTRATION RECOVERY: repeated `{}` still has only read/context evidence for a durable artifact task; delegating once to the owning artifact worker instead of finalizing without a write receipt.",
                                reused_tool_name
                            ),
                        });
                        messages.push(Message::system(
                            "BENSHU_ARTIFACT_CONTEXT_ONLY_ESCALATED_TO_WORKER".to_string(),
                        ));
                        let artifact_route = Self::artifact_execution_delegate_route(&latest_query);
                        let continuation_context =
                            Self::latest_delegate_artifact_continuation_context(messages);
                        bridge
                            .executor()
                            .coordinate(
                                messages,
                                String::new(),
                                vec![Self::toolless_execution_delegate_call(
                                    steps,
                                    &latest_query,
                                    artifact_route,
                                    continuation_context,
                                )],
                                steps,
                                &mut history,
                                &mut tool_trace,
                            )
                            .await?;
                        continue;
                    }

                    let final_text = Self::synthesize_successful_tool_delivery(messages)
                        .unwrap_or_else(|| {
                            if Self::query_prefers_chinese(&latest_query) {
                                format!(
                                    "工具 `{}` 的相同调用已经执行过，系统复用了已有结果并停止继续重复调用。",
                                    reused_tool_name
                                )
                            } else {
                                format!(
                                    "The repeated `{}` tool call reused the existing result, so I stopped calling it again.",
                                    reused_tool_name
                                )
                            }
                        });
                    bridge.emit(AgentEventData::Thought {
                        content: format!(
                            "ORCHESTRATION FINALIZE: repeated `{}` call reused an existing result; returning that result instead of continuing the loop.",
                            reused_tool_name
                        ),
                    });
                    return bridge
                        .finalize_outcome(messages, final_text, usage, thoughts, tool_trace, steps)
                        .await;
                }
            }

            if Self::latest_loop_guard_abort_for_tool(messages, "delegate") {
                let prior_delegate_result =
                    Self::latest_successful_tool_result_text(messages, "delegate");

                if Self::query_requests_knowledge_persistence(&persistence_query) {
                    if let Some(result) = prior_delegate_result {
                        let compact_result = Self::compact_tool_result_for_recovery(&result);
                        bridge.emit(AgentEventData::Thought {
                            content: "ORCHESTRATION RECOVERY: repeated delegation was blocked; routing the existing specialist result toward knowledge persistence instead of re-querying the same worker.".to_string(),
                        });
                        messages.push(Message::system(format!(
                            "{}\n\nA repeated `delegate` call was blocked by the loop guard. Do not call the same specialist with the same research task again.\n\nThe latest user task requested knowledge persistence. You already have this specialist result:\n\n{}\n\nNext action:\n1. If the result contains a concrete source URL, call `delegate` exactly once with role `knowledge` and ask it to import that URL into the knowledge base.\n2. If no concrete source URL is present, stop and answer with the blocker clearly.\n3. Do not delegate back to `researcher` unless the previous result contains no usable source at all.",
                            reasoner_constants::MARKER_TOOL_EXECUTION_REQUIRED,
                            compact_result
                        )));
                        continue;
                    }
                }

                if let Some(result) = prior_delegate_result {
                    let final_text = Self::summarize_delegate_delivery(
                        &latest_query,
                        &result,
                        Self::query_prefers_chinese(&latest_query),
                    );
                    bridge.emit(AgentEventData::Thought {
                        content: "ORCHESTRATION RECOVERY: repeated delegation was blocked; finalizing from the existing specialist result.".to_string(),
                    });
                    return bridge
                        .finalize_outcome(messages, final_text, usage, thoughts, tool_trace, steps)
                        .await;
                }
            }

            if let Some(result) = Self::latest_successful_tool_result_text(messages, "delegate") {
                if Self::tool_result_is_blocked(&result) {
                    if !Self::has_system_marker_after_latest_user(
                        messages,
                        "BENSHU_PHASE_BOUNDARY_RECOVERY",
                    ) {
                        if let Some(role) = Self::delegate_phase_boundary_suggested_role(&result) {
                            bridge.emit(AgentEventData::Thought {
                                content: format!(
                                    "PHASE BOUNDARY RECOVERY: artifact worker reported a prerequisite stage boundary; delegating the prerequisite stage to `{}` before returning to artifact work.",
                                    role
                                ),
                            });
                            messages.push(Message::system(
                                "BENSHU_PHASE_BOUNDARY_RECOVERY".to_string(),
                            ));
                            bridge
                                .executor()
                                .coordinate(
                                    messages,
                                    String::new(),
                                    vec![Self::phase_boundary_recovery_delegate_call(
                                        steps,
                                        &role,
                                        &latest_query,
                                        &result,
                                    )],
                                    steps,
                                    &mut history,
                                    &mut tool_trace,
                                )
                                .await?;
                            continue;
                        }
                    }
                    if Self::delegate_blocker_is_recoverable_workspace_boundary(&result)
                        && !Self::has_system_marker_after_latest_user(
                            messages,
                            "BENSHU_WORKSPACE_BOUNDARY_RECOVERY",
                        )
                    {
                        bridge.emit(AgentEventData::Thought {
                            content: "WORKSPACE BOUNDARY RECOVERY: delegated worker used an out-of-workspace path; retrying once with the current workspace boundary instead of finalizing.".to_string(),
                        });
                        messages.push(Message::system(Self::workspace_boundary_recovery_prompt(
                            &result,
                        )));
                        continue;
                    }
                    let final_text = Self::summarize_delegate_delivery(
                        &latest_query,
                        &result,
                        Self::query_prefers_chinese(&latest_query),
                    );
                    bridge.emit(AgentEventData::Thought {
                        content: "ORCHESTRATION FINALIZE: delegated specialist reported an external blocker; returning the blocker directly instead of looping through recovery or efficiency hints.".to_string(),
                    });
                    return bridge
                        .finalize_outcome(messages, final_text, usage, thoughts, tool_trace, steps)
                        .await;
                }
            }

            self.inject_efficiency_warning_if_needed(messages, &tool_trace, tool_calls.len());
        }
    }

    /// Prepare messages by pruning history with smart distillation
    async fn prepare_messages(&self, messages: Vec<Message>) -> Result<Vec<Message>> {
        context_pruning::prepare_messages(
            &self.provider,
            &self.config,
            &self.distillation_cache,
            self.auxiliary_session_id("distill"),
            messages,
        )
        .await
    }

    /// Primary entry point for a single "Think" phase
    pub async fn think(
        &self,
        messages: Vec<Message>,
        strategy: &ReasoningStrategy,
        emit_callback: impl Fn(AgentEventData),
        cancel_token: CancellationToken,
        model_override: Option<String>,
        bridge: Option<&dyn AgentLiaison>,
    ) -> Result<ReasonerStep> {
        let messages = self.prepare_messages(messages).await?;
        let mut full_text = String::new();
        let mut thoughts = Vec::new();
        let mut tool_calls = Vec::new();
        let mut usage = None;

        let mut extra = self
            .config
            .extra_params
            .clone()
            .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
        if !extra.is_object() {
            extra = serde_json::Value::Object(serde_json::Map::new());
        }
        if self.config.json_mode {
            if let serde_json::Value::Object(ref mut map) = extra {
                map.insert(
                    "response_format".to_string(),
                    serde_json::json!({ "type": "json_object" }),
                );
            }
        }
        if let serde_json::Value::Object(ref mut map) = extra {
            map.insert(
                "inference_priority".to_string(),
                serde_json::json!(self.config.inference_priority),
            );
            map.insert(
                "inference_session_authority".to_string(),
                serde_json::json!("backend-local-cache"),
            );
            map.insert(
                "inference_runtime_owner".to_string(),
                serde_json::json!("inference"),
            );
            map.insert(
                "brain_runtime_owner".to_string(),
                serde_json::json!("brain"),
            );
            if let Some(session_id) = &self.config.session_id {
                map.insert(
                    "inference_session_id".to_string(),
                    serde_json::json!(session_id),
                );
            }
        }

        let latest_user_input = messages
            .iter()
            .rev()
            .find(|message| matches!(message.role, Role::User))
            .map(|message| message.text());
        let language_contract = Self::language_contract_for_query(latest_user_input.as_deref());
        let routing_judgment_only = latest_user_input
            .as_deref()
            .is_some_and(query_requests_routing_judgment_only);
        let raw_capability_route = latest_user_input
            .as_deref()
            .and_then(classify_query_capability_route);
        let has_media_input = latest_user_message_has_media(&messages);
        let direct_capability_route = raw_capability_route
            .filter(|_| !routing_judgment_only)
            .filter(|route| route_requires_real_tool_call_for_turn(*route, has_media_input));

        let matched_skill_manual = matched_skill_manual_name(&extra)
            .or_else(|| matched_skill_manual_name_from_messages(&messages));
        let matched_skill_asset_path =
            resolve_skill_asset_path_from_messages(&messages, matched_skill_manual.as_deref());
        let pending_forge_followup_tools = if approved_forge_request_from_messages(&messages) {
            forged_session_tool_names_from_messages(&messages)
                .into_iter()
                .filter(|tool_name| !forged_session_tool_already_executed(&messages, tool_name))
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let media_followup_strategies = media_followup_strategies_from_messages(&messages);
        let media_followup_contract =
            media_followup_capability_contract(&media_followup_strategies);
        apply_media_followup_capability_route(&mut extra, media_followup_contract);
        let knowledge_base_readback_query = latest_user_input
            .as_deref()
            .is_some_and(query_prefers_knowledge_base_retrieval);
        let requires_skill_manual_first = !knowledge_base_readback_query
            && matched_skill_manual.as_deref().is_some_and(|skill_name| {
                self.tools.contains("read_skill_manual")
                    && !skill_manual_already_loaded(&messages, skill_name)
            });
        let requires_skill_asset_first = !requires_skill_manual_first
            && !knowledge_base_readback_query
            && matched_skill_manual
                .as_deref()
                .zip(matched_skill_asset_path.as_deref())
                .is_some_and(|(skill_name, asset_path)| {
                    self.tools.contains("read_skill_asset")
                        && skill_manual_already_loaded(&messages, skill_name)
                        && !skill_asset_already_loaded(&messages, asset_path)
                });

        let coordinator_task_mode = select_coordinator_task_mode(
            raw_capability_route,
            !media_followup_strategies.is_empty(),
        );
        let base_system_prompt = match coordinator_task_mode {
            CoordinatorTaskMode::ChatLite => Self::condensed_frontstage_preamble(self),
            _ => self.config.preamble.clone(),
        };
        let base_system_prompt_chars = base_system_prompt.chars().count();
        let mut system_prompt = base_system_prompt.clone();
        if coordinator_task_mode_should_include_reasoning_prompt(coordinator_task_mode, strategy) {
            match strategy {
                ReasoningStrategy::TreeOfThoughts => system_prompt
                    .push_str("\n\n### TREE OF THOUGHTS MODE\nExplore multiple reasoning paths."),
                ReasoningStrategy::Reflexion => system_prompt
                    .push_str("\n\n### REFLEXION MODE\nCritique your own logic for omissions."),
                ReasoningStrategy::Planning => system_prompt.push_str(
                    "\n\n### PLANNING MODE\nGenerate a detailed step-by-step plan first.",
                ),
                _ => {}
            }
        }
        if coordinator_task_mode_should_include_media_followup_prompt(
            coordinator_task_mode,
            !media_followup_strategies.is_empty(),
        ) {
            if let Some(media_followup_prompt) =
                render_media_followup_strategy_prompt(&media_followup_strategies)
            {
                system_prompt.push_str("\n\n");
                system_prompt.push_str(&media_followup_prompt);
            }
        }
        if let Some(extra_map) = extra.as_object_mut() {
            extra_map.insert(
                "task_mode".to_string(),
                serde_json::json!(coordinator_task_mode_label(coordinator_task_mode)),
            );
            extra_map.insert(
                "routing_route_source".to_string(),
                serde_json::json!(if raw_capability_route.is_some() {
                    "shared_capability_router"
                } else {
                    "none"
                }),
            );
            extra_map.insert(
                "routing_execution_source".to_string(),
                serde_json::json!(if direct_capability_route.is_some() {
                    "capability_route_tool_surface"
                } else {
                    "coordinator_mode_default"
                }),
            );
            if let Some(route) = raw_capability_route {
                extra_map.insert(
                    "routing_capability_route".to_string(),
                    serde_json::json!(format!("{route:?}")),
                );
            }
            if routing_judgment_only {
                extra_map.insert("routing_judgment_only".to_string(), serde_json::json!(true));
            }
            extra_map.insert(
                "language_contract_response".to_string(),
                serde_json::json!(language_contract.response_language.clone()),
            );
            extra_map.insert(
                "language_contract_artifact".to_string(),
                serde_json::json!(language_contract.artifact_language.clone()),
            );
        }
        system_prompt.push_str("\n\n");
        system_prompt.push_str(coordinator_task_mode_system_message(coordinator_task_mode));
        system_prompt.push_str("\n\n");
        system_prompt.push_str(&language_contract.system_prompt());
        if routing_judgment_only {
            system_prompt.push_str("\n\n");
            system_prompt.push_str(coordinator_routing_judgment_only_message());
        }
        if !matches!(coordinator_task_mode, CoordinatorTaskMode::ChatLite) {
            if let Some(specialist_selection_message) = coordinator_specialist_selection_message(
                coordinator_task_mode,
                direct_capability_route,
                latest_user_input.as_deref(),
                !media_followup_strategies.is_empty(),
            ) {
                system_prompt.push_str("\n\n");
                system_prompt.push_str(&specialist_selection_message);
            }
        }
        if let Some(route_system_prompt) = latest_user_input
            .as_deref()
            .zip(direct_capability_route)
            .and_then(|(user_request, route)| {
                coordinator_task_mode_should_include_route_prompt(coordinator_task_mode, route)
                    .then_some(route)
                    .and_then(|route| {
                        capability_route_system_message(
                            user_request,
                            route,
                            None,
                            matched_skill_manual.as_deref(),
                        )
                    })
            })
        {
            system_prompt.push_str("\n\n");
            system_prompt.push_str(&route_system_prompt);
        }
        if latest_user_input
            .as_deref()
            .is_some_and(query_prefers_session_continuity_answer)
        {
            system_prompt.push_str("\n\n");
            system_prompt.push_str(
                "### SESSION_CONTINUITY_FIRST\n\
                 This turn is an immediate same-session follow-up about something said just now.\n\
                 Frontstage rules:\n\
                 - Answer from the recent conversation already present in this session before reaching for durable memory tools.\n\
                 - Only use memory tools if the needed detail is genuinely missing from the active session context.\n\
                 - Do not dump raw tool output when a concise direct answer is possible.\n\
                 - If the user asked to reply with only the recalled sentence, do exactly that.",
            );
        }
        let truth_verification_policy_active = latest_user_input.as_deref().is_some_and(|query| {
            TruthVerificationPolicyEngine::default().should_include_guidance_for_query(query)
        });
        let truth_verification_guidance_active =
            !matches!(coordinator_task_mode, CoordinatorTaskMode::ChatLite)
                && coordinator_task_mode_should_include_truth_guidance(
                    coordinator_task_mode,
                    truth_verification_policy_active,
                    media_followup_contract.is_some(),
                );
        if truth_verification_guidance_active {
            system_prompt.push_str("\n\n");
            system_prompt.push_str(TruthVerificationPolicyEngine::default().guidance_prompt());
        }
        let pending_content_generation_turn =
            Self::turn_requires_generated_artifact_content(&messages);
        if pending_content_generation_turn {
            system_prompt.push_str("\n\n");
            system_prompt.push_str(&Self::pending_content_generation_system_message(
                latest_user_input.as_deref(),
            ));
        }

        let force_direct_multimodal_answer = should_force_direct_multimodal_answer(
            raw_capability_route,
            has_media_input,
            media_followup_contract.is_some(),
        );

        let (mut tools, mut deferred_tool_count, total_tool_count, mut tool_surface_mode) =
            if !pending_forge_followup_tools.is_empty() {
                let mut allowed: HashSet<String> =
                    pending_forge_followup_tools.iter().cloned().collect();
                if let Some(ref enabled) = self.enabled_tools {
                    let enabled_set = enabled.read().clone();
                    allowed.retain(|name| enabled_set.contains(name));
                }
                let total = self.tools.definitions_filtered(Some(&allowed)).await.len();
                let visible = self.tools.definitions_filtered(Some(&allowed)).await;
                (visible, 0, total, "minimal")
            } else if requires_skill_manual_first {
                let mut allowed = HashSet::from(["read_skill_manual".to_string()]);
                if let Some(ref enabled) = self.enabled_tools {
                    let enabled_set = enabled.read().clone();
                    allowed.retain(|name| enabled_set.contains(name));
                }
                let total = self.tools.definitions_filtered(Some(&allowed)).await.len();
                let visible = self.tools.definitions_filtered(Some(&allowed)).await;
                (visible, 0, total, "minimal")
            } else if requires_skill_asset_first {
                let mut allowed = HashSet::from(["read_skill_asset".to_string()]);
                if let Some(ref enabled) = self.enabled_tools {
                    let enabled_set = enabled.read().clone();
                    allowed.retain(|name| enabled_set.contains(name));
                }
                let total = self.tools.definitions_filtered(Some(&allowed)).await.len();
                let visible = self.tools.definitions_filtered(Some(&allowed)).await;
                (visible, 0, total, "minimal")
            } else if media_followup_contract
                .map(|contract| contract.prefer_document_understanding_tools)
                .unwrap_or(false)
            {
                let mut allowed = Self::coordinator_default_tool_allowlist_for_mode(
                    coordinator_task_mode,
                    latest_user_input.as_deref(),
                );
                if let Some(ref enabled) = self.enabled_tools {
                    let enabled_set = enabled.read().clone();
                    allowed.retain(|name| enabled_set.contains(name));
                }
                let total = self.tools.definitions_filtered(Some(&allowed)).await.len();
                let (visible, deferred) = self
                    .tools
                    .definitions_prompt_visible_filtered(Some(&allowed))
                    .await;
                (visible, deferred, total, "minimal")
            } else if routing_judgment_only {
                (Vec::new(), 0, 0, "routing_only")
            } else if force_direct_multimodal_answer {
                (Vec::new(), 0, 0, "multimodal_direct")
            } else if let Some(route) = direct_capability_route {
                let mut allowed: HashSet<String> = match route {
                    CapabilityRouteHint::RealtimeLookup(_)
                        if self.tools.contains("delegate")
                            && latest_user_input
                                .as_deref()
                                .is_some_and(query_requests_followup_execution_after_lookup) =>
                    {
                        Self::coordinator_default_tool_allowlist_for_mode(
                            coordinator_task_mode,
                            latest_user_input.as_deref(),
                        )
                    }
                    CapabilityRouteHint::RealtimeLookup(_) => {
                        capability_route_tool_allowlist_for_query(
                            route,
                            latest_user_input.as_deref(),
                        )
                    }
                    _ => capability_route_tool_allowlist_for_query(
                        route,
                        latest_user_input.as_deref(),
                    ),
                };
                if matches!(route, CapabilityRouteHint::RealtimeLookup(_)) {
                    let has_direct_runtime_tool = allowed.iter().any(|name| {
                        name != "tool_search" && name != "delegate" && self.tools.contains(name)
                    });
                    if !has_direct_runtime_tool && self.tools.contains("delegate") {
                        allowed.insert("delegate".to_string());
                        if self.tools.contains("tool_search") {
                            allowed.insert("tool_search".to_string());
                        }
                    }
                }
                if let Some(ref enabled) = self.enabled_tools {
                    let enabled_set = enabled.read().clone();
                    allowed.retain(|name| enabled_set.contains(name));
                }
                if allowed.is_empty() {
                    if let Some(ref enabled) = self.enabled_tools {
                        let allowed = enabled.read().clone();
                        let total = self.tools.definitions_filtered(Some(&allowed)).await.len();
                        let (visible, deferred) = self
                            .tools
                            .definitions_prompt_visible_filtered(Some(&allowed))
                            .await;
                        (visible, deferred, total, "full")
                    } else {
                        let total = self.tools.definitions().await.len();
                        let (visible, deferred) =
                            self.tools.definitions_prompt_visible_filtered(None).await;
                        (visible, deferred, total, "full")
                    }
                } else {
                    let total = self.tools.definitions_filtered(Some(&allowed)).await.len();
                    let visible = self.tools.definitions_filtered(Some(&allowed)).await;
                    (visible, 0, total, "minimal")
                }
            } else if let Some(ref enabled) = self.enabled_tools {
                let mut allowed = enabled.read().clone();
                let coordinator_allowed = Self::coordinator_default_tool_allowlist_for_mode(
                    coordinator_task_mode,
                    latest_user_input.as_deref(),
                );
                allowed.retain(|name| coordinator_allowed.contains(name));
                let total = self.tools.definitions_filtered(Some(&allowed)).await.len();
                let (visible, deferred) = self
                    .tools
                    .definitions_prompt_visible_filtered(Some(&allowed))
                    .await;
                (visible, deferred, total, "minimal")
            } else {
                let allowed = Self::coordinator_default_tool_allowlist_for_mode(
                    coordinator_task_mode,
                    latest_user_input.as_deref(),
                );
                let total = self.tools.definitions_filtered(Some(&allowed)).await.len();
                let (visible, deferred) = self
                    .tools
                    .definitions_prompt_visible_filtered(Some(&allowed))
                    .await;
                (visible, deferred, total, "minimal")
            };

        if pending_content_generation_turn && !tools.is_empty() {
            deferred_tool_count = deferred_tool_count.saturating_add(tools.len());
            tools.clear();
            tool_surface_mode = "content_generation";
        }

        deferred_tool_count = deferred_tool_count.saturating_add(
            Self::apply_task_specific_tool_surface_filter(&mut tools, latest_user_input.as_deref()),
        );

        if matches!(coordinator_task_mode, CoordinatorTaskMode::ChatLite) {
            tools = tools
                .into_iter()
                .map(Self::compact_frontstage_core_tool_definition)
                .collect();
        }

        let mut prompt_surface =
            PromptSurfaceReport::new(coordinator_task_mode_label(coordinator_task_mode));
        prompt_surface.add_segment(
            PromptSegmentKind::Static,
            "frontstage_preamble",
            &base_system_prompt,
        );
        prompt_surface.add_segment_chars(
            PromptSegmentKind::Governance,
            "task_mode_and_policy_guidance",
            system_prompt
                .chars()
                .count()
                .saturating_sub(base_system_prompt_chars),
        );
        prompt_surface.add_segment_chars(
            PromptSegmentKind::Dynamic,
            "request_messages",
            messages
                .iter()
                .map(|message| message.text().chars().count())
                .sum(),
        );
        prompt_surface.set_tool_surface(
            tools.len(),
            deferred_tool_count,
            total_tool_count,
            tool_surface_mode,
        );

        if latest_user_input.as_deref().is_some_and(|query| {
            !Self::query_is_creation_planning_dialogue(query)
                && query_requests_image_generation(query)
        }) && !has_media_input
            && !Self::tool_surface_has_generate_image(&tools)
            && !tools.iter().any(|tool| tool.name == "delegate")
        {
            let fallback = Self::image_generation_unavailable_fallback_text(
                latest_user_input.as_deref().unwrap_or_default(),
            );
            emit_callback(AgentEventData::Thought {
                content: "IMAGE GENERATION FALLBACK: No generate_image tool is available in the current frontstage runtime; returning an explicit unavailable response.".to_string(),
            });
            return Ok(ReasonerStep {
                text: fallback,
                thoughts,
                tool_calls: Vec::new(),
                usage: None,
            });
        }

        let output_contract = self.output_contract_for_turn(
            !tools.is_empty(),
            direct_capability_route,
            coordinator_task_mode,
            &messages,
        );
        if let Some(extra_map) = extra.as_object_mut() {
            extra_map.insert(
                "output_contract_kind".to_string(),
                serde_json::json!(output_contract.kind.label()),
            );
            extra_map.insert(
                "output_contract_surface".to_string(),
                serde_json::json!(output_contract.surface.label()),
            );
            extra_map.insert(
                "output_contract_max_tokens".to_string(),
                serde_json::json!(output_contract.max_tokens),
            );
            extra_map.insert(
                "output_contract_requires_background".to_string(),
                serde_json::json!(output_contract.requires_background),
            );
            extra_map.insert(
                "output_contract_requires_artifact".to_string(),
                serde_json::json!(output_contract.requires_artifact),
            );
        }
        if let Some(contract_guidance) =
            Self::output_contract_system_message(output_contract, latest_user_input.as_deref())
        {
            system_prompt.push_str("\n\n");
            system_prompt.push_str(&contract_guidance);
            prompt_surface.add_segment(
                PromptSegmentKind::Governance,
                "output_contract_guidance",
                &contract_guidance,
            );
        }
        prompt_surface.write_to_extra_params(&mut extra);
        let continuation_hint = self.continuation_hint_for_request(
            &system_prompt,
            &messages,
            &tools,
            Some(output_contract.max_tokens),
        );
        if let Some(hint) = continuation_hint.as_ref() {
            Self::write_continuation_hint_to_extra(&mut extra, hint);
        }
        let mut request = crate::agent::provider::ChatRequest {
            model: model_override.unwrap_or_else(|| self.config.model.clone()),
            system_prompt: Some(system_prompt),
            messages: messages.clone(),
            tools,
            temperature: self.config.temperature,
            max_tokens: Some(output_contract.max_tokens),
            extra_params: Some(extra),
            enable_cache_control: self.config.enable_cache_control,
            session_id: self.config.session_id.clone(),
            continuation_hint: continuation_hint.clone(),
            ..Default::default()
        };

        let request_model = request.model.clone();
        let mut before_llm_hook = HookEvent::new(HookTiming::BeforeLlm);
        if let Some(input) = latest_user_input.clone() {
            before_llm_hook = before_llm_hook.with_user_input(input);
        }
        before_llm_hook.metadata.insert(
            "provider_name".to_string(),
            self.provider.name().to_string(),
        );
        let visible_owner = request
            .extra_params
            .as_ref()
            .and_then(|extra| extra.get("visible_owner"))
            .and_then(|value| value.as_str())
            .unwrap_or("unknown");
        let memory_owner = request
            .extra_params
            .as_ref()
            .and_then(|extra| extra.get("memory_owner"))
            .and_then(|value| value.as_str())
            .unwrap_or(visible_owner);
        let approval_owner = request
            .extra_params
            .as_ref()
            .and_then(|extra| extra.get("approval_owner"))
            .and_then(|value| value.as_str())
            .unwrap_or(visible_owner);
        before_llm_hook
            .metadata
            .insert("visible_owner".to_string(), visible_owner.to_string());
        before_llm_hook
            .metadata
            .insert("memory_owner".to_string(), memory_owner.to_string());
        before_llm_hook
            .metadata
            .insert("approval_owner".to_string(), approval_owner.to_string());
        before_llm_hook
            .metadata
            .insert("provider_model".to_string(), request_model.clone());
        before_llm_hook
            .metadata
            .insert("chat_route".to_string(), "coordinator".to_string());
        before_llm_hook.metadata.insert(
            "task_mode".to_string(),
            coordinator_task_mode_label(coordinator_task_mode).to_string(),
        );
        before_llm_hook.metadata.insert(
            "tool_surface_mode".to_string(),
            tool_surface_mode.to_string(),
        );
        if routing_judgment_only {
            before_llm_hook
                .metadata
                .insert("routing_judgment_only".to_string(), "true".to_string());
        }
        if let Some((session_title, title_source)) =
            runtime_session_title(request.extra_params.as_ref())
        {
            before_llm_hook
                .metadata
                .insert("session_title".to_string(), session_title);
            before_llm_hook
                .metadata
                .insert("session_title_source".to_string(), title_source.to_string());
            before_llm_hook
                .metadata
                .insert("session_title_present".to_string(), "true".to_string());
        } else {
            before_llm_hook
                .metadata
                .insert("session_title_source".to_string(), "missing".to_string());
            before_llm_hook
                .metadata
                .insert("session_title_present".to_string(), "false".to_string());
        }
        before_llm_hook.metadata.insert(
            "requested_tool_count".to_string(),
            request.tools.len().to_string(),
        );
        before_llm_hook
            .metadata
            .insert("total_tool_count".to_string(), total_tool_count.to_string());
        if deferred_tool_count > 0 {
            before_llm_hook.metadata.insert(
                "deferred_tool_count".to_string(),
                deferred_tool_count.to_string(),
            );
        }
        if let Some(hint) = continuation_hint.as_ref() {
            Self::write_continuation_hint_to_metadata(&mut before_llm_hook.metadata, hint);
        }
        if let Some(capability_route) = request
            .extra_params
            .as_ref()
            .and_then(|extra| extra.get("capability_route"))
            .and_then(|value| value.as_str())
        {
            before_llm_hook
                .metadata
                .insert("capability_route".to_string(), capability_route.to_string());
        }
        if let Some(skill_name) = matched_skill_manual.as_deref() {
            before_llm_hook
                .metadata
                .insert("matched_skill_manual".to_string(), skill_name.to_string());
        }
        if let Some(asset_path) = matched_skill_asset_path.as_deref() {
            before_llm_hook.metadata.insert(
                "matched_skill_asset_path".to_string(),
                asset_path.to_string(),
            );
        }
        if !pending_forge_followup_tools.is_empty() {
            before_llm_hook.metadata.insert(
                "forge_followup_tool_names".to_string(),
                pending_forge_followup_tools.join(","),
            );
            before_llm_hook
                .metadata
                .insert("forge_followup_gate_active".to_string(), "true".to_string());
        }
        if !media_followup_strategies.is_empty() {
            before_llm_hook.metadata.insert(
                "media_followup_strategies".to_string(),
                media_followup_strategies.join(","),
            );
            before_llm_hook.metadata.insert(
                "media_followup_guidance_active".to_string(),
                "true".to_string(),
            );
            if let Some(contract) = media_followup_contract {
                before_llm_hook.metadata.insert(
                    "media_followup_capability_route".to_string(),
                    contract.capability_route.to_string(),
                );
                before_llm_hook.metadata.insert(
                    "media_followup_execution_surface".to_string(),
                    contract.execution_surface.to_string(),
                );
            }
        }
        if truth_verification_guidance_active {
            before_llm_hook.metadata.insert(
                "truth_verification_guidance_active".to_string(),
                "true".to_string(),
            );
        }
        if requires_skill_manual_first {
            before_llm_hook
                .metadata
                .insert("skill_manual_gate_active".to_string(), "true".to_string());
        }
        if requires_skill_asset_first {
            before_llm_hook
                .metadata
                .insert("skill_asset_gate_active".to_string(), "true".to_string());
        }

        if let Some(bridge) = bridge {
            match bridge.run_runtime_hook(before_llm_hook).await? {
                HookResult::Continue | HookResult::Skip => {}
                HookResult::Modify(injected_system_prompt) => {
                    if !injected_system_prompt.trim().is_empty() {
                        request.system_prompt = Some(match request.system_prompt.take() {
                            Some(existing) if !existing.trim().is_empty() => {
                                format!("{existing}\n\n{injected_system_prompt}")
                            }
                            _ => injected_system_prompt,
                        });
                    }
                }
                HookResult::Abort(reason) => {
                    return Err(ReasoningError::ProviderError(format!(
                        "Before-LLM middleware aborted runtime: {reason}"
                    ))
                    .into());
                }
            }
        }

        crate::agent::runtime_context_budget::clamp_local_chat_request_to_context(
            self.provider.as_ref(),
            self.config.max_tokens,
            crate::agent::protocol::constants::DEFAULT_RESPONSE_RESERVE,
            &mut request,
        );

        let request_messages = request.messages.clone();
        let request_timeout = self.effective_llm_timeout_for_request(&request);
        emit_callback(AgentEventData::Thought {
            content: format!(
                "LLM STEP: waiting for model output with a bounded per-turn budget of {}s; requested_tools={} max_tokens={}.",
                request_timeout.as_secs(),
                request.tools.len(),
                request
                    .max_tokens
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "provider_default".to_string())
            ),
        });
        let stream = match tokio::time::timeout(
            request_timeout,
            self.provider.stream_completion(request),
        )
        .await
        {
            Ok(stream) => match stream {
                Ok(stream) => stream,
                Err(error) => {
                    if let Some(context_error) =
                        ContextLimitError::from_provider_error_message(&error.to_string())
                    {
                        emit_callback(AgentEventData::Thought {
                            content: format!(
                                "LLM STEP CONTEXT LIMIT: prompt_tokens={} configured_context_tokens={} requested_output_tokens={} overflow_tokens={}.",
                                context_error.prompt_tokens,
                                context_error.configured_context_tokens,
                                context_error.requested_output_tokens,
                                context_error.overflow_tokens
                            ),
                        });
                        return Ok(ReasonerStep {
                            text: Self::context_limit_blocker_text(
                                &context_error,
                                latest_user_input.as_deref(),
                            ),
                            thoughts,
                            tool_calls,
                            usage,
                        });
                    }
                    return Err(error.into());
                }
            },
            Err(_) => {
                emit_callback(AgentEventData::Thought {
                        content: format!(
                            "LLM STEP TIMEOUT: model produced no usable output within {}s; returning a structured blocker for this turn instead of leaving the task in a silent wait.",
                            request_timeout.as_secs()
                        ),
                    });
                let query = latest_user_input.clone().unwrap_or_default();
                let prefers_chinese = Self::query_prefers_chinese(&query);
                let blocker = if prefers_chinese {
                    format!(
                            "本轮模型调用在 {} 秒内没有返回可执行工具调用或可交付内容。系统已停止这一轮等待，避免后台空转。",
                            request_timeout.as_secs()
                        )
                } else {
                    format!(
                            "The model call returned no executable tool call or deliverable content within {} seconds. The runtime stopped this turn to avoid a silent background wait.",
                            request_timeout.as_secs()
                        )
                };
                let next_step_hint = if prefers_chinese {
                    "检查当前模型/层卸载/上下文配置是否导致首 token 过慢；调整后重新发送同一任务，或让运行时选择更小的单步输出预算继续。"
                } else {
                    "Check whether the current model, offload, or context settings are causing very slow first-token latency; then retry the same task or use a smaller per-turn output budget."
                };
                return Ok(ReasonerStep {
                        text: format!(
                            "status: blocked\nerror_kind: llm_turn_timeout\nblockers: {blocker}\nnext_step_hint: {next_step_hint}"
                        ),
                        thoughts,
                        tool_calls,
                        usage,
                    });
            }
        };

        let mut stream_inner = stream.into_inner();
        let start_time = std::time::Instant::now();
        let stream_deadline = tokio::time::sleep(request_timeout);
        tokio::pin!(stream_deadline);
        let mut ttft_recorded = false;
        let mut finish_reason: Option<FinishReason> = None;
        let mut provider_telemetry: Option<ProviderTelemetry> = None;

        loop {
            tokio::select! {
                _ = cancel_token.cancelled() => return Err(ReasoningError::Cancelled.into()),
                _ = &mut stream_deadline => {
                    emit_callback(AgentEventData::Thought {
                        content: format!(
                            "LLM STREAM TIMEOUT: model stream did not finish within {}s; returning a structured blocker for this turn instead of leaving the task in a silent wait.",
                            request_timeout.as_secs()
                        ),
                    });
                    let query = latest_user_input.clone().unwrap_or_default();
                    let prefers_chinese = Self::query_prefers_chinese(&query);
                    let blocker = if prefers_chinese {
                        format!(
                            "本轮模型流式输出在 {} 秒内没有完成。系统已停止这一轮等待，避免后台空转。",
                            request_timeout.as_secs()
                        )
                    } else {
                        format!(
                            "The model stream did not finish within {} seconds. The runtime stopped this turn to avoid a silent background wait.",
                            request_timeout.as_secs()
                        )
                    };
                    let next_step_hint = if prefers_chinese {
                        "检查当前模型吞吐、思考模式、层卸载、上下文和单步输出预算；运行时可以用更小的分段任务继续。"
                    } else {
                        "Check model throughput, thinking mode, offload, context, and per-step output budget; the runtime can continue with smaller staged work."
                    };
                    return Ok(ReasonerStep {
                        text: format!(
                            "status: blocked\nerror_kind: llm_stream_timeout\nblockers: {blocker}\nnext_step_hint: {next_step_hint}"
                        ),
                        thoughts,
                        tool_calls,
                        usage,
                    });
                }
                chunk = stream_inner.next() => {
                    match chunk {
                        None => break,
                        Some(chunk) => {
                            if !ttft_recorded {
                                emit_callback(AgentEventData::LatencyTTFT { duration_ms: start_time.elapsed().as_millis() as u64 });
                                ttft_recorded = true;
                            }
                            let chunk = match chunk {
                                Ok(chunk) => chunk,
                                Err(error) => {
                                    if let Some(context_error) =
                                        ContextLimitError::from_provider_error_message(
                                            &error.to_string(),
                                        )
                                    {
                                        emit_callback(AgentEventData::Thought {
                                            content: format!(
                                                "LLM STREAM CONTEXT LIMIT: prompt_tokens={} configured_context_tokens={} requested_output_tokens={} overflow_tokens={}.",
                                                context_error.prompt_tokens,
                                                context_error.configured_context_tokens,
                                                context_error.requested_output_tokens,
                                                context_error.overflow_tokens
                                            ),
                                        });
                                        return Ok(ReasonerStep {
                                            text: Self::context_limit_blocker_text(
                                                &context_error,
                                                latest_user_input.as_deref(),
                                            ),
                                            thoughts,
                                            tool_calls,
                                            usage,
                                        });
                                    }
                                    return Err(ReasoningError::ProviderError(error.to_string()).into());
                                }
                            };
                            match chunk {
                                StreamingChoice::Message(text) => {
                                    emit_callback(AgentEventData::PartialResponse { content: text.clone() });
                                    full_text.push_str(&text);
                                }
                                StreamingChoice::Thought(thought) => {
                                    thoughts.push(thought.clone());
                                    emit_callback(AgentEventData::Thought { content: thought });
                                }
                                StreamingChoice::ToolCall { id, name, arguments } => {
                                    tool_calls.push((id, name, arguments));
                                }
                                StreamingChoice::ParallelToolCalls(map) => {
                                    let mut sorted: Vec<_> = map.into_iter().collect();
                                    sorted.sort_by_key(|(k, _)| *k);
                                    for (_, tc) in sorted {
                                        tool_calls.push((tc.id, tc.name, tc.arguments));
                                    }
                                }
                                StreamingChoice::Usage(u) => {
                                    let usage_mapped = TokenUsage {
                                        prompt_tokens: u.prompt_tokens,
                                        completion_tokens: u.completion_tokens,
                                        total_tokens: u.total_tokens,
                                    };

                                    // Hard safety check: Prevent runaway token consumption in a single step
                                    let max_step_tokens = self.max_step_tokens();
                                    if usage_mapped.total_tokens as usize > max_step_tokens {
                                        warn!(
                                            "Reasoner: Step token limit exceeded ({} > {})",
                                            usage_mapped.total_tokens,
                                            max_step_tokens
                                        );
                                        return Err(ReasoningError::TokenLimitExceeded.into());
                                    }

                                    usage = Some(usage_mapped.clone());
                                    emit_callback(AgentEventData::TokenUsage { usage: usage_mapped });
                                }
                                StreamingChoice::Finish(reason) => {
                                    finish_reason = Some(reason);
                                }
                                StreamingChoice::Telemetry(telemetry) => {
                                    provider_telemetry = Some(telemetry);
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }

        let defer_lookup_tool_delivery_to_loop = Self::latest_delegate_role(&request_messages)
            .as_deref()
            .is_some_and(|role| role == "researcher")
            && Self::latest_lookup_result_for_followup_execution(&request_messages).is_some();

        if full_text.is_empty() && tool_calls.is_empty() {
            if routing_judgment_only {
                full_text = Self::routing_judgment_fallback_text(raw_capability_route);
            } else if defer_lookup_tool_delivery_to_loop {
                info!(
                    "Reasoner: deferring researcher lookup delivery to execute_loop so bounded follow-up orchestration can decide whether to summarize or import."
                );
            } else if let Some(synthesized) =
                Self::synthesize_successful_tool_delivery(&request_messages)
            {
                full_text = synthesized;
            } else if let Some((failed_tool_name, error)) =
                Self::latest_tool_error_result(&request_messages)
            {
                let query = Self::latest_user_query(&request_messages).unwrap_or_default();
                full_text = Self::tool_failure_delivery_text(&query, &failed_tool_name, &error);
            } else if let Some(query) = Self::latest_user_query(&request_messages) {
                let simple_media_understanding = latest_user_message_has_media(&request_messages);
                if simple_media_understanding {
                    if let Some(retry_text) = self
                        .retry_simple_media_answer(&request_messages, &request_model)
                        .await
                    {
                        if !retry_text.trim().is_empty() {
                            full_text = retry_text;
                        }
                    }
                    if full_text.trim().is_empty() {
                        if let Some(summary) = self
                            .document_understand_fallback_summary(&request_messages)
                            .await
                        {
                            info!(
                                "Reasoner: multimodal direct empty; using document_understand fallback summary."
                            );
                            full_text = summary;
                        }
                    }
                }
                if full_text.trim().is_empty() {
                    if let Some(kind) = decide_finalization_fallback(FinalizationFallbackInput {
                        failure_classification: FailureClass::Quality,
                        has_media_input: latest_user_message_has_media(&request_messages),
                        simple_media_understanding,
                    }) {
                        full_text = Self::classified_finalization_fallback_text(kind, &query);
                    } else {
                        return Err(ReasoningError::NoResponse.into());
                    }
                }
            } else {
                return Err(ReasoningError::NoResponse.into());
            }
        }

        if tool_calls.is_empty() && latest_user_message_has_media(&request_messages) {
            if let Some(query) = Self::latest_user_query(&request_messages) {
                if Self::query_requests_structured_media_output(&query) {
                    if let Some(normalized) = Self::normalized_structured_media_output(&full_text) {
                        full_text = normalized;
                    }
                }
                let low_value = Self::is_low_value_media_answer(&query, &full_text);
                info!(
                    "Reasoner: multimodal direct low-value check: low_value={} response_preview=\"{}\"",
                    low_value,
                    full_text.chars().take(120).collect::<String>()
                );
                if low_value {
                    let mut recovered = false;
                    if let Some(retry_text) = self
                        .retry_simple_media_answer(&request_messages, &request_model)
                        .await
                    {
                        let normalized_retry =
                            if Self::query_requests_structured_media_output(&query) {
                                Self::normalized_structured_media_output(&retry_text)
                                    .unwrap_or(retry_text)
                            } else {
                                retry_text
                            };
                        if !Self::is_low_value_media_answer(&query, &normalized_retry) {
                            info!(
                                "Reasoner: multimodal direct low-value answer recovered by retry_simple_media_answer."
                            );
                            full_text = normalized_retry;
                            recovered = true;
                        }
                        if !recovered {
                            warn!(
                                "Reasoner: multimodal retry still returned low-value output; falling back to document_understand."
                            );
                        }
                    }

                    if !recovered {
                        if let Some(summary) = self
                            .document_understand_fallback_summary(&request_messages)
                            .await
                        {
                            info!(
                                "Reasoner: multimodal direct low-value answer; using document_understand fallback summary."
                            );
                            full_text = summary;
                        } else {
                            warn!(
                                "Reasoner: multimodal direct low-value answer; document_understand fallback returned empty."
                            );
                            full_text = Self::media_understanding_failure_text(&query);
                        }
                    }
                } else if Self::media_answer_needs_text_enrichment(&full_text) {
                    if let Some(summary) = self
                        .document_understand_fallback_summary(&request_messages)
                        .await
                    {
                        let normalized_summary =
                            if Self::query_requests_structured_media_output(&query) {
                                Self::normalized_structured_media_output(&summary)
                                    .unwrap_or_else(|| summary.clone())
                            } else {
                                summary.clone()
                            };
                        if !Self::is_low_value_media_answer(&query, &normalized_summary)
                            && normalized_summary.trim() != full_text.trim()
                        {
                            info!(
                                "Reasoner: multimodal direct generic text mention upgraded via document_understand fallback summary."
                            );
                            full_text = normalized_summary;
                        }
                    }
                }
            }
        }

        let mut after_llm_hook =
            HookEvent::new(HookTiming::AfterLlm).with_llm_response(full_text.clone());
        if let Some(input) = latest_user_input {
            after_llm_hook = after_llm_hook.with_user_input(input);
        }
        after_llm_hook.metadata.insert(
            "provider_name".to_string(),
            self.provider.name().to_string(),
        );
        after_llm_hook
            .metadata
            .insert("provider_model".to_string(), request_model.clone());
        after_llm_hook
            .metadata
            .insert("tool_call_count".to_string(), tool_calls.len().to_string());
        after_llm_hook.metadata.insert(
            "response_chars".to_string(),
            full_text.chars().count().to_string(),
        );
        if let Some(usage) = usage.as_ref() {
            after_llm_hook.metadata.insert(
                "provider_usage_prompt_tokens".to_string(),
                usage.prompt_tokens.to_string(),
            );
            after_llm_hook.metadata.insert(
                "provider_usage_completion_tokens".to_string(),
                usage.completion_tokens.to_string(),
            );
            after_llm_hook.metadata.insert(
                "provider_usage_total_tokens".to_string(),
                usage.total_tokens.to_string(),
            );
        }
        if let Some(reason) = finish_reason.as_ref() {
            after_llm_hook
                .metadata
                .insert("finish_reason".to_string(), reason.as_str().to_string());
        }
        if let Some(telemetry) = provider_telemetry.as_ref() {
            if let Some(provider_name) = telemetry.provider_name.as_ref() {
                after_llm_hook
                    .metadata
                    .insert("provider_name".to_string(), provider_name.clone());
            }
            if let Some(model) = telemetry.model.as_ref() {
                after_llm_hook
                    .metadata
                    .insert("provider_model".to_string(), model.clone());
            }
            if let Some(latency_ms) = telemetry.latency_ms {
                after_llm_hook
                    .metadata
                    .insert("provider_latency_ms".to_string(), latency_ms.to_string());
            }
            if let Some(continuation) = telemetry.continuation.as_ref() {
                after_llm_hook.metadata.insert(
                    "provider_continuation_mode".to_string(),
                    continuation.mode.clone(),
                );
                after_llm_hook.metadata.insert(
                    "provider_continuation_cache_source".to_string(),
                    continuation.cache_source.clone(),
                );
                if let Some(prompt_tokens) = continuation.prompt_tokens {
                    after_llm_hook.metadata.insert(
                        "provider_continuation_prompt_tokens".to_string(),
                        prompt_tokens.to_string(),
                    );
                }
                if let Some(prefill_ms) = continuation.prefill_ms {
                    after_llm_hook.metadata.insert(
                        "provider_continuation_prefill_ms".to_string(),
                        prefill_ms.to_string(),
                    );
                }
                if let Some(decode_ms) = continuation.decode_ms {
                    after_llm_hook.metadata.insert(
                        "provider_continuation_decode_ms".to_string(),
                        decode_ms.to_string(),
                    );
                }
                if let Some(miss_reason) = continuation.miss_reason.as_ref() {
                    after_llm_hook.metadata.insert(
                        "provider_continuation_miss_reason".to_string(),
                        miss_reason.clone(),
                    );
                }
                after_llm_hook.metadata.insert(
                    "provider_continuation_tool_exact_replay_used".to_string(),
                    continuation.tool_exact_replay_used.to_string(),
                );
                after_llm_hook.metadata.insert(
                    "provider_continuation_protocol_live_used".to_string(),
                    continuation.protocol_live_continuation_used.to_string(),
                );
            }
            for (key, value) in &telemetry.extra {
                after_llm_hook
                    .metadata
                    .insert(format!("provider_telemetry_{key}"), value.clone());
            }
        }
        if let Some(hint) = continuation_hint.as_ref() {
            Self::write_continuation_hint_to_metadata(&mut after_llm_hook.metadata, hint);
        }

        if let Some(bridge) = bridge {
            match bridge.run_runtime_hook(after_llm_hook).await? {
                HookResult::Continue | HookResult::Skip => {}
                HookResult::Modify(modified_response) => {
                    info!(
                        "Reasoner: after_llm hook modified response. before_preview=\"{}\" after_preview=\"{}\"",
                        full_text.chars().take(160).collect::<String>(),
                        modified_response.chars().take(160).collect::<String>()
                    );
                    full_text = modified_response;
                }
                HookResult::Abort(reason) => {
                    return Err(ReasoningError::ProviderError(format!(
                        "After-LLM middleware aborted runtime: {reason}"
                    ))
                    .into());
                }
            }
        }

        if force_direct_multimodal_answer
            && tool_calls.is_empty()
            && (Self::is_pseudo_tool_call_leak(&full_text)
                || Self::is_multimodal_procedural_placeholder(&full_text))
        {
            info!(
                "Reasoner: multimodal_direct detected placeholder/pseudo tool output; replacing with parsed summary fallback."
            );
            if let Some(summary) = Self::extract_latest_parsed_attachment_summary(&messages) {
                full_text = summary;
            } else {
                let query = Self::latest_user_query(&request_messages).unwrap_or_default();
                full_text = Self::classified_finalization_fallback_text(
                    FinalizationFallbackKind::MediaUnderstandingRetryHint,
                    &query,
                );
            }
        }

        // Production Hardening: Post-reasoning tool validation
        let mut validated_tool_calls = Vec::new();
        for (id, name, args) in tool_calls {
            let (name, args) = self.normalize_local_pseudo_tool_call(name, args);
            if let Some(_tool) = self.tools.get(&name) {
                // If we have an enabled list, check it
                let is_allowed = if let Some(ref enabled) = self.enabled_tools {
                    enabled.read().contains(&name)
                } else {
                    true
                };

                if is_allowed {
                    validated_tool_calls.push((id, name, args));
                } else {
                    warn!(
                        "Reasoner: Tool '{}' was called by LLM but it is currently DISABLED.",
                        name
                    );
                    emit_callback(AgentEventData::Error {
                        message: format!("Tool '{}' is disabled", name),
                    });
                }
            } else {
                warn!("Reasoner: Unknown tool '{}' called by LLM.", name);
                emit_callback(AgentEventData::Error {
                    message: format!("Tool '{}' not found", name),
                });
            }
        }

        if force_direct_multimodal_answer && !validated_tool_calls.is_empty() {
            warn!(
                "Reasoner: Suppressing {} tool call(s) during multimodal_direct turn to preserve direct media answering. tools={}",
                validated_tool_calls.len(),
                validated_tool_calls
                    .iter()
                    .map(|(_, name, _)| name.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            );
            emit_callback(AgentEventData::Thought {
                content: "MULTIMODAL DIRECT GUARD: suppressing tool execution and forcing direct media answer."
                    .to_string(),
            });

            validated_tool_calls.clear();
            if let Some(summary) = Self::extract_latest_parsed_attachment_summary(&messages) {
                full_text = summary;
            } else if full_text.trim().is_empty()
                || Self::is_pseudo_tool_call_leak(&full_text)
                || Self::is_multimodal_procedural_placeholder(&full_text)
            {
                let query = Self::latest_user_query(&request_messages).unwrap_or_default();
                full_text = Self::classified_finalization_fallback_text(
                    FinalizationFallbackKind::MediaUnderstandingRetryHint,
                    &query,
                );
            }
        } else if force_direct_multimodal_answer {
            info!(
                "Reasoner: multimodal_direct proceeding without tool execution. response_chars={}",
                full_text.chars().count()
            );
            let preview: String = full_text.chars().take(240).collect();
            info!(
                "Reasoner: multimodal_direct raw response preview={:?}",
                preview
            );
        }

        Ok(ReasonerStep {
            text: full_text,
            thoughts,
            tool_calls: validated_tool_calls,
            usage,
        })
    }
}

#[cfg(test)]
mod tests;
