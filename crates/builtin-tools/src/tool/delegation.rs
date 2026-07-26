use async_trait::async_trait;
use benshu_brain::agent::message::Message;
use benshu_brain::agent::multi_agent::{AgentRole, Coordinator, MultiAgent};
use benshu_brain::runtime::continuous_task::{
    continuous_completion_gate_decision, ContinuousActionHandler, ContinuousActionRunner,
    ContinuousArtifactTarget, ContinuousCompletionGateDecision, ContinuousStepAction,
    ContinuousStepRequest, ContinuousStepResult, ContinuousTaskAnchor, ContinuousTaskContract,
    ContinuousTaskExecutor, ContinuousTaskPlan, ContinuousTaskPolicy, ContinuousTaskRun,
    ContinuousTaskStatus, ContinuousTaskStep, FileAppendCheckpointSink,
    PersistentTaskCheckpointSink,
};
use benshu_compression::ellipsize;
use benshu_compression::json::summarize_github_search_items;
use benshu_compression::{knowledge_snippet_text, preview_text};
use benshu_engram::HybridSearchEngine;
use benshu_infra::{Tool, ToolDefinition};
use benshu_memory_api::Memory;
use chrono::{Datelike, Utc};
use regex::Regex;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};
use uuid::Uuid;

#[cfg(feature = "browser")]
use super::browser::{BrowserSearchResult, BrowserTool};
use super::chart::ChartTool;
use super::cipher::CipherTool;
use super::command_exec::CommandExecTool;
use super::data_transform::DataTransformTool;
use super::filesystem::WriteFileTool;
use super::git_ops::GitOpsTool;
use super::knowledge_import::KnowledgeImportUrlTool;
use super::knowledge_manage::KnowledgeManageDocumentTool;
use super::media_runtime::ProbeMediaTool;
use super::office_parse::OfficeParseTool;
use super::pdf_parse::PdfParseTool;
use super::skill_manager::SkillManagerTool;
use super::text_extract::TextExtractTool;
use super::voice::TranscribeTool;
use super::web_fetch::WebFetchTool;
use super::web_search::policy::{LookupIntent, SearchPolicy};
use super::web_search::{WebSearchConfig, WebSearchTool};
use super::writing::NovelStudioTool;
use crate::tool::browser_site_policy::policy_for_host;
use crate::tool::browser_site_policy::SiteFetchMode;
use crate::SkillLoader;

mod continuous;
mod fast_path;
pub(crate) mod policy;
mod search_evidence;

#[cfg(test)]
mod tests;

#[cfg(test)]
#[cfg(test)]
use super::writing::longform_policy::LongformContinuationSeed;
#[cfg(test)]
use continuous::DelegateContinuousActionHandler;
use policy::{artifact_policy_capabilities, PolicyPhase, RuntimePolicyResolver, TaskPolicyInput};

/// Tool that allows an agent to delegate a task to another agent role
pub struct DelegateTool {
    coordinator: Weak<Coordinator>,
    search_engine: Option<Arc<HybridSearchEngine>>,
    memory: Option<Arc<dyn Memory>>,
    skill_loader: Option<Arc<SkillLoader>>,
    data_dir: Option<PathBuf>,
    enabled_tools: Option<Arc<parking_lot::RwLock<std::collections::HashSet<String>>>>,
    task_manager: Option<Arc<benshu_state::TaskManager>>,
    runtime_event_manager: Option<Arc<benshu_state::RuntimeEventManager>>,
}

impl DelegateTool {
    const STRUCTURED_LOOKUP_MAILTO: &'static str = "mailto=research@benshu.local";

    /// Create a new DelegateTool with a weak reference to the coordinator
    pub fn new(coordinator: Weak<Coordinator>) -> Self {
        Self {
            coordinator,
            search_engine: None,
            memory: None,
            skill_loader: None,
            data_dir: None,
            enabled_tools: None,
            task_manager: None,
            runtime_event_manager: None,
        }
    }

    pub fn with_knowledge_import(
        coordinator: Weak<Coordinator>,
        search_engine: Arc<HybridSearchEngine>,
        memory: Arc<dyn Memory>,
    ) -> Self {
        Self {
            coordinator,
            search_engine: Some(search_engine),
            memory: Some(memory),
            skill_loader: None,
            data_dir: None,
            enabled_tools: None,
            task_manager: None,
            runtime_event_manager: None,
        }
    }

    pub fn with_skill_management(
        mut self,
        loader: Arc<SkillLoader>,
        data_dir: PathBuf,
        enabled_tools: Arc<parking_lot::RwLock<std::collections::HashSet<String>>>,
    ) -> Self {
        self.skill_loader = Some(loader);
        self.data_dir = Some(data_dir);
        self.enabled_tools = Some(enabled_tools);
        self
    }

    pub fn with_runtime_state(
        mut self,
        task_manager: Arc<benshu_state::TaskManager>,
        runtime_event_manager: Arc<benshu_state::RuntimeEventManager>,
    ) -> Self {
        self.task_manager = Some(task_manager);
        self.runtime_event_manager = Some(runtime_event_manager);
        self
    }

    fn parse_known_role(label: &str) -> Option<AgentRole> {
        match label.trim().to_lowercase().as_str() {
            "benshu" => Some(AgentRole::Custom("benshu".to_string())),
            "researcher" => Some(AgentRole::Researcher),
            "trader" => Some(AgentRole::Trader),
            "risk_analyst" => Some(AgentRole::RiskAnalyst),
            "strategist" => Some(AgentRole::Strategist),
            _ => None,
        }
    }

    fn normalize_role_label(label: &str) -> String {
        label
            .trim()
            .to_lowercase()
            .chars()
            .map(|ch| match ch {
                'a'..='z' | '0'..='9' => ch,
                _ => '_',
            })
            .collect::<String>()
            .split('_')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("_")
    }

    fn delegated_worker_session_id(
        parent_session_id: Option<&str>,
        task_id: Option<Uuid>,
        role_name: &str,
        phase_label: &str,
    ) -> String {
        let parent = parent_session_id
            .map(Self::normalize_role_label)
            .filter(|value| !value.is_empty())
            .or_else(|| task_id.map(|id| format!("task_{}", id.simple())))
            .unwrap_or_else(|| format!("delegate_{}", Uuid::new_v4().simple()));
        let role = Self::normalize_role_label(role_name);
        let phase = Self::normalize_role_label(phase_label);
        format!("{parent}::worker::{role}::{phase}")
    }

    fn role_alias(normalized: &str) -> Option<&'static str> {
        match normalized {
            "document_understanding" | "document_analysis" | "document_parser" => Some("document"),
            "visual_understanding" | "image_understanding" | "vision_understanding" | "vision" => {
                Some("image")
            }
            "knowledge_import"
            | "knowledge_ingestion"
            | "knowledge_import_url"
            | "knowledge_manage"
            | "knowledge_manage_document"
            | "knowledge_delete"
            | "knowledge_base"
            | "knowledge_library" => Some("knowledge"),
            "web_research" | "source_research" | "web_lookup" | "latest_lookup" => {
                Some("researcher")
            }
            "optical_character_recognition" | "text_ocr" | "ocr_reader" => Some("ocr"),
            "pdf_parse" | "pdf_parser" | "pdf_understanding" => Some("pdf"),
            "office_specialist" | "office_parser" | "word_parser" | "excel_parser"
            | "powerpoint_parser" => Some("office"),
            "data_specialist" | "data_transformer" | "csv_transformer" => Some("data"),
            "chart_specialist" | "chart_generator" | "visualization" | "visualisation" => {
                Some("chart")
            }
            "terminal_specialist" | "shell" | "runtime_surface" | "command_runner" => {
                Some("terminal")
            }
            "code_specialist" | "git_specialist" | "repository" | "repo_specialist" => Some("repo"),
            "browser_specialist" | "browser_tool" | "web_browser" => Some("browser"),
            "image_generation" | "image_generator" | "image_specialist" => Some("image"),
            "voice_specialist" | "speech" | "stt" | "tts" => Some("voice"),
            "skill_manager" | "skill_installer" | "skill_install" | "skill_management"
            | "plugin_installer" | "plugin_manager" => Some("skill_manager"),
            _ => None,
        }
    }

    fn resolve_target_role(coordinator: &Coordinator, requested_role: &str) -> AgentRole {
        Self::resolve_target_role_for_task(coordinator, requested_role, "")
    }

    fn resolve_target_role_for_task(
        coordinator: &Coordinator,
        requested_role: &str,
        task: &str,
    ) -> AgentRole {
        let normalized = Self::normalize_role_label(requested_role);
        let auto_request = normalized.is_empty()
            || matches!(
                normalized.as_str(),
                "auto" | "worker" | "specialist" | "best_worker" | "best_specialist"
            );

        if auto_request {
            return coordinator
                .best_worker_capability_match(Some("auto"), task)
                .map(|candidate| candidate.role)
                .unwrap_or_else(|| AgentRole::Custom("researcher".to_string()));
        }

        if let Some(role) = Self::registered_worker_role(coordinator, &normalized)
            .or_else(|| Self::parse_known_role(&normalized))
        {
            return role;
        }
        if let Some(alias) = Self::role_alias(&normalized) {
            let normalized_alias = Self::normalize_role_label(alias);
            if let Some(role) = Self::registered_worker_role(coordinator, &normalized_alias)
                .or_else(|| Self::parse_known_role(&normalized_alias))
            {
                return role;
            }
        }
        if let Some(candidate) =
            coordinator.best_worker_capability_match(Some(requested_role), task)
        {
            return candidate.role;
        }

        AgentRole::Custom(requested_role.trim().to_string())
    }

    fn registered_worker_role(coordinator: &Coordinator, normalized: &str) -> Option<AgentRole> {
        coordinator
            .worker_blueprints()
            .into_iter()
            .find_map(|blueprint| {
                let role = blueprint.role.name().to_string();
                let normalized_role = Self::normalize_role_label(&role);
                let normalized_display = Self::normalize_role_label(&blueprint.display_name);
                (normalized == normalized_role || normalized == normalized_display)
                    .then_some(blueprint.role)
            })
    }

    fn role_is_writing_owner(role: &AgentRole, blueprint_tools: &[String]) -> bool {
        let normalized = Self::normalize_role_label(role.name());
        matches!(
            normalized.as_str(),
            "writer" | "writing" | "author" | "novelist" | "essay_writer"
        ) || blueprint_tools
            .iter()
            .any(|tool| matches!(tool.as_str(), "writing" | "writing_studio" | "novel_studio"))
    }

    fn worker_has_external_acquisition_tools(blueprint_tools: &[String]) -> bool {
        blueprint_tools.iter().any(|tool| {
            matches!(
                tool.as_str(),
                "web_search" | "web_fetch" | "browser" | "browser_browse"
            )
        })
    }

    fn task_requires_external_acquisition_before_artifact(task: &str) -> bool {
        if Self::task_is_worker_contract_recovery(task) {
            return false;
        }
        let has_existing_artifact_ref =
            Self::extract_existing_artifact_project_path(task).is_some();
        let task = Self::user_request_slice_for_phase_boundary(task);
        if has_existing_artifact_ref && Self::task_requests_local_writing_context(task) {
            return false;
        }
        let lowered = task.to_ascii_lowercase();
        let asks_external_acquisition = [
            "search",
            "find",
            "lookup",
            "browse",
            "download",
            "fetch",
            "import",
            "material gathering",
            "source discovery",
            "retrieve",
            "crawl",
            "scrape",
            "collect",
        ]
        .iter()
        .any(|term| lowered.contains(term))
            || [
                "搜索", "查找", "检索", "浏览", "下载", "抓取", "采集", "导入", "入库", "联网",
                "收集", "获取",
            ]
            .iter()
            .any(|term| task.contains(term));
        let asks_artifact = [
            "write", "draft", "create", "article", "novel", "report", "paper", "artifact", "file",
            "txt", "markdown", "pdf",
        ]
        .iter()
        .any(|term| lowered.contains(term))
            || [
                "写", "创作", "生成", "小说", "论文", "文章", "报告", "文件", "文档", "保存",
            ]
            .iter()
            .any(|term| task.contains(term));

        asks_external_acquisition && asks_artifact
    }

    fn extract_existing_artifact_project_path(task: &str) -> Option<String> {
        let mut candidates = Vec::new();
        let project_path_re =
            Regex::new(r#"(?i)(?:project_path|path|项目路径|目标路径|路径)\s*[:：=]\s*["`']?(?P<path>[^\n\r|"`'，。]+)"#).ok()?;
        for capture in project_path_re.captures_iter(task) {
            if let Some(path) = capture.name("path") {
                candidates.push(path.as_str().trim().to_string());
            }
        }
        for token in task.split(|ch: char| {
            ch.is_whitespace()
                || matches!(
                    ch,
                    '"' | '\'' | '`' | ',' | ';' | ')' | '(' | ']' | '[' | '{' | '}' | '|'
                )
        }) {
            let path = token.trim_matches(|ch: char| matches!(ch, ':' | '.' | '，' | '。'));
            if !path.is_empty() {
                candidates.push(path.to_string());
            }
        }
        candidates
            .into_iter()
            .filter_map(|path| Self::normalize_existing_artifact_project_path(&path))
            .next()
    }

    fn extract_creation_draft_path(task: &str) -> Option<String> {
        let draft_path_re = Regex::new(
            r#"(?i)(?:draft_path|草案路径)\s*[:：=]\s*["`']?(?P<path>[^\n\r|"`'，。]+)"#,
        )
        .ok()?;
        let path = draft_path_re
            .captures_iter(task)
            .filter_map(|capture| capture.name("path"))
            .filter_map(|path| Self::normalize_creation_draft_path(path.as_str()))
            .next();
        path
    }

    fn normalize_creation_draft_path(path: &str) -> Option<String> {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            return None;
        }
        let normalized = trimmed.replace('\\', "/");
        let lowered = normalized.to_ascii_lowercase();
        if !lowered.contains("/generated/novels/drafts/") || !lowered.ends_with(".json") {
            return None;
        }
        if !(normalized.starts_with('/')
            || lowered.starts_with("data/generated/")
            || lowered.starts_with("./data/generated/"))
        {
            return None;
        }
        Some(normalized)
    }

    fn normalize_existing_artifact_project_path(path: &str) -> Option<String> {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            return None;
        }
        let lowered = trimmed.to_ascii_lowercase();
        let normalized_lowered = lowered.replace('\\', "/");
        if normalized_lowered.contains("/generated/novels/drafts/")
            && normalized_lowered.ends_with(".json")
        {
            return None;
        }
        let looks_like_safe_relative_generated_path =
            lowered.starts_with("data/generated/") || lowered.starts_with("./data/generated/");
        if !(trimmed.starts_with('/')
            || lowered.contains(":\\")
            || looks_like_safe_relative_generated_path)
            || !(lowered.contains("/generated/")
                || lowered.contains("\\generated\\")
                || lowered.contains("/novels/")
                || lowered.contains("\\novels\\")
                || lowered.ends_with("project.json")
                || lowered.ends_with(".md")
                || lowered.ends_with(".txt"))
        {
            return None;
        }
        let normalized = trimmed.replace('\\', "/");
        let path = Path::new(&normalized);
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == "project.json")
        {
            return path
                .parent()
                .map(|parent| parent.to_string_lossy().to_string());
        }
        for marker in ["chapters", "plans", "runtime", "truth", "exports"] {
            if let Some(index) = normalized.find(&format!("/{marker}/")) {
                return Some(normalized[..index].to_string());
            }
        }
        Some(normalized)
    }

    fn task_state_project_path(task: &benshu_state::TaskState) -> Option<String> {
        if let Some(intent) = task.contract.as_ref().and_then(|contract| {
            contract
                .intent
                .as_deref()
                .filter(|intent| !intent.trim().is_empty())
        }) {
            if let Some(path) = Self::extract_existing_artifact_project_path(intent) {
                return Some(path);
            }
        }
        if let Some(result) = task.result.as_ref() {
            if let Ok(text) = serde_json::to_string(result) {
                if let Some(path) = Self::extract_existing_artifact_project_path(&text) {
                    return Some(path);
                }
            }
        }
        for artifact in &task.artifacts {
            if let Some(path) = Self::extract_existing_artifact_project_path(&artifact.uri) {
                return Some(path);
            }
        }
        for checkpoint in task.checkpoints.iter().rev() {
            if let Some(summary) = checkpoint.summary.as_deref() {
                if let Some(path) = Self::extract_existing_artifact_project_path(summary) {
                    return Some(path);
                }
            }
        }
        None
    }

    async fn latest_session_project_path_for_delegate(
        &self,
        session_id: Option<&str>,
        current_task_id: Option<Uuid>,
    ) -> Option<String> {
        let session_id = session_id?;
        let task_manager = self.task_manager.as_ref()?;
        if let Some(task_id) = current_task_id {
            if let Ok(Some(task)) = task_manager.load(&task_id.to_string()).await {
                if let Some(path) = Self::task_state_project_path(&task) {
                    return Some(path);
                }
            }
        }
        let mut tasks = task_manager.list_by_session(session_id).await.ok()?;
        tasks.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        for task in tasks {
            if current_task_id.is_some_and(|task_id| task.id == task_id) {
                continue;
            }
            if let Some(path) = Self::task_state_project_path(&task) {
                return Some(path);
            }
        }
        None
    }

    fn task_requests_existing_work_continuation(task: &str) -> bool {
        let lowered = task.to_ascii_lowercase();
        [
            "继续",
            "续写",
            "接着",
            "沿用",
            "承接",
            "下一章",
            "上一章",
            "前一章",
            "刚才",
            "刚刚",
            "上次",
            "上一轮",
            "之前",
            "前面",
            "已经",
            "已生成",
            "生成的",
            "这个项目",
            "这个文档",
            "这个文件",
            "这个小说",
            "current",
            "previous",
            "last",
            "existing",
            "continue",
            "append",
            "same project",
            "same document",
            "next chapter",
        ]
        .iter()
        .any(|term| task.contains(term) || lowered.contains(term))
    }

    fn user_request_slice_for_phase_boundary(task: &str) -> &str {
        for marker in ["Full user request:", "Original user request:"] {
            let Some((_, tail)) = task.split_once(marker) else {
                continue;
            };
            let tail = tail.trim_start();
            if marker == "Original user request:" {
                if let Some((original, _)) = tail.split_once("\n\nDelegated task:") {
                    return original.trim();
                }
            }
            return tail.trim();
        }
        task
    }

    fn task_is_worker_contract_recovery(task: &str) -> bool {
        let lowered = task.to_ascii_lowercase();
        [
            "worker tool-contract recovery",
            "tool-contract recovery",
            "previous contract detail",
            "previous worker attempt reached an equipped tool",
            "missing_required_content",
            "missing required content",
            "example_shape",
        ]
        .iter()
        .any(|marker| lowered.contains(marker))
    }

    fn task_has_verified_acquisition_evidence(task: &str) -> bool {
        let lowered = task.to_ascii_lowercase();
        [
            "verified researcher evidence",
            "knowledge import receipt",
            "runtime_effect: knowledge.imported",
            "runtime_effect:knowledge.imported",
            "storage_target: durable_knowledge_store",
            "recently imported knowledge",
            "imported knowledge",
            "imported source",
            "imported material",
            "knowledge is already imported",
            "knowledge has already been imported",
            "material is already imported",
            "material has already been imported",
            "collection:",
            "source_url:",
            "fetched_result:",
            "已有素材",
            "已导入",
            "已入库",
            "素材已经入库",
            "素材已入库",
            "资料已经入库",
            "资料已入库",
            "材料已经入库",
            "材料已入库",
            "已经写入知识库",
            "已写入知识库",
            "知识库里的素材",
            "知识库里的资料",
            "知识库中的素材",
            "知识库中的资料",
            "知识库写入已经完成",
            "已验证",
        ]
        .iter()
        .any(|term| lowered.contains(&term.to_ascii_lowercase()) || task.contains(term))
    }

    fn suggested_external_acquisition_role(
        coordinator: &Coordinator,
        current_role: &AgentRole,
    ) -> Option<String> {
        coordinator
            .worker_blueprints()
            .into_iter()
            .filter(|blueprint| blueprint.role.name() != current_role.name())
            .filter(|blueprint| Self::worker_has_external_acquisition_tools(&blueprint.tools))
            .map(|blueprint| {
                let score = blueprint
                    .tools
                    .iter()
                    .map(|tool| match tool.as_str() {
                        "web_search" => 4,
                        "web_fetch" => 3,
                        "browser" | "browser_browse" => 2,
                        _ => 0,
                    })
                    .sum::<i32>();
                (score, blueprint.role.name().to_string())
            })
            .max_by(|left, right| left.0.cmp(&right.0))
            .map(|(_, role)| role)
    }

    fn artifact_owner_phase_boundary_result(
        role: &AgentRole,
        suggested_role: Option<String>,
        task: &str,
    ) -> String {
        let suggested_role = suggested_role.unwrap_or_else(|| "auto".to_string());
        format!(
            "status: blocked\nworker: {}\nerror_kind: phase_boundary\nphase: prerequisite_external_acquisition\nsuggested_role: {}\nblockers: this artifact owner worker lacks external acquisition tools, but the delegated task still requires source discovery, fetch, download, import, or knowledge-base material before drafting\nnext_step_hint: delegate the prerequisite acquisition/import stage to `suggested_role`, then return the verified source body, source URL, collection/path, or knowledge import receipt to this artifact owner worker before drafting\noriginal_task_preview: {}",
            role.name(),
            suggested_role,
            preview_text(task, 500)
        )
    }

    fn delegate_routing_receipt(
        coordinator: &Coordinator,
        requested_role: &str,
        task: &str,
        selected_role: &AgentRole,
    ) -> serde_json::Value {
        let candidates = coordinator
            .worker_capability_candidates(Some(requested_role), task)
            .into_iter()
            .take(5)
            .map(|candidate| {
                serde_json::json!({
                    "role": candidate.role.name().to_string(),
                    "score": candidate.score,
                    "source": candidate.source.as_str(),
                    "capabilities": candidate.capabilities,
                    "tools": candidate.tools,
                    "reasons": candidate.reasons,
                })
            })
            .collect::<Vec<_>>();
        let selected_source = candidates
            .iter()
            .find(|candidate| {
                candidate
                    .get("role")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|role| role == selected_role.name())
            })
            .and_then(|candidate| candidate.get("source"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("fallback");

        serde_json::json!({
            "selected_source": selected_source,
            "requested_role": requested_role,
            "selected_role": selected_role.name().to_string(),
            "candidate_count": candidates.len(),
            "candidates": candidates,
        })
    }

    fn explicit_worker_role_from_task(coordinator: &Coordinator, task: &str) -> Option<String> {
        let normalized_task = Self::normalize_role_label(task);
        for blueprint in coordinator.worker_blueprints() {
            let role = blueprint.role.name().to_string();
            let normalized_role = Self::normalize_role_label(&role);
            let normalized_display = Self::normalize_role_label(&blueprint.display_name);
            let role_worker = format!("{normalized_role}_worker");
            let worker_role = format!("worker_{normalized_role}");
            if normalized_task.contains(&role_worker)
                || normalized_task.contains(&worker_role)
                || normalized_task.contains(&format!("委派给_{normalized_role}"))
                || normalized_task.contains(&format!("delegate_to_{normalized_role}"))
                || (!normalized_display.is_empty()
                    && normalized_task.contains(&format!("{normalized_display}_worker")))
            {
                return Some(role);
            }
        }
        None
    }

    fn artifact_policy_capabilities(policy: &Option<serde_json::Value>) -> Vec<String> {
        artifact_policy_capabilities(policy, 4)
    }

    fn artifact_policy_tool_config_usize(
        policy: Option<&serde_json::Value>,
        tools: &[&str],
        key: &str,
    ) -> Option<usize> {
        let policy = policy?;
        tools.iter().find_map(|tool| {
            policy
                .pointer(&format!("/tool_config/{tool}/{key}"))
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| usize::try_from(value).ok())
                .filter(|value| *value > 0)
        })
    }

    fn build_worker_execution_contract(
        role: &AgentRole,
        blueprint_tools: &[String],
        task: &str,
        full_user_request: Option<&str>,
    ) -> String {
        Self::build_worker_execution_contract_with_policy(
            role,
            blueprint_tools,
            None,
            task,
            full_user_request,
        )
    }

    fn build_worker_execution_contract_with_policy(
        role: &AgentRole,
        blueprint_tools: &[String],
        artifact_policy: Option<&serde_json::Value>,
        task: &str,
        full_user_request: Option<&str>,
    ) -> String {
        let role_name = role.name().to_string();
        let expanded_tools = Self::expanded_worker_tool_names_for_task(&blueprint_tools, task);
        let tools = if expanded_tools.is_empty() {
            "none declared".to_string()
        } else {
            expanded_tools.join(", ")
        };
        let tool_set = blueprint_tools
            .iter()
            .map(|tool| tool.as_str())
            .collect::<std::collections::HashSet<_>>();
        let has_writing_package =
            tool_set.contains("writing") || tool_set.contains("writing_studio");

        let mut contract = format!(
            "### Delegated Specialist Contract\n\
             You are `{role_name}`, executing one internal sub-task for BenShu.\n\
             Available specialist tools: {tools}.\n\n\
             Rules:\n\
             - Complete the sub-task; do not ask the user follow-up questions.\n\
             - If one of your tools can satisfy the task, call the tool before writing the final answer.\n\
             - Call tool names exactly as listed above. Never prefix tool names with namespaces like `runtime_surface.command_exec`, `web.web_fetch`, or `knowledge.search`.\n\
             - Do not call orchestration tools such as `delegate`, `handover`, or `decompose` unless they are explicitly listed as available specialist tools.\n\
             - Use the minimum useful number of tool calls. Do not loop, re-plan, or repeat the same tool call.\n\
             - Return a compact result for BenShu to synthesize, not a conversational user-facing essay.\n\
             - Include `status`, `result`, `source_urls`, and `blockers` when applicable.\n\n"
        );

        if let Some(full_user_request) = full_user_request
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            contract.push_str(
                "Constraint preservation contract:\n\
                 - Treat the original user request below as the source of truth for explicit constraints.\n\
                 - Treat `Original delegated task` as a role-scoped execution hint, not permission to satisfy a weaker substitute.\n\
                 - Do not weaken, replace, or broaden explicit constraints such as source, availability, action, quantity, format, storage target, language, safety boundary, or artifact requirement.\n\
                 - If this delegated task conflicts with the original request, follow the original request and report the conflict in `blockers`.\n\
                 - If a fallback, summary, metadata-only, or alternate-source approach cannot satisfy a requested source body, import, artifact, or quantity constraint, return `status: blocked` with the missing evidence instead of marking the phase complete.\n\
                 - If only part of the original request is in your role scope, complete that part and name the remaining scope explicitly.\n\n\
                 Original user request:\n",
            );
            contract.push_str(full_user_request);
            contract.push_str("\n\n");
        }

        if let Some(policy) = artifact_policy {
            let policy_bundle = RuntimePolicyResolver::resolve(
                TaskPolicyInput::new(task)
                    .with_full_user_request(full_user_request)
                    .with_worker(role_name.clone(), blueprint_tools)
                    .with_phase(PolicyPhase::Delegation),
                std::slice::from_ref(policy),
            );
            if !policy_bundle.is_empty() {
                contract.push_str("Runtime policy bundle:\n");
                for line in policy_bundle.compact_summary().lines() {
                    contract.push_str("- ");
                    contract.push_str(line);
                    contract.push('\n');
                }
                contract.push('\n');
            }
        }

        if tool_set.contains("web_search") || tool_set.contains("web_fetch") {
            contract.push_str(
                "Research contract:\n\
                 - For lookup tasks, call `web_search` once with the best query.\n\
                 - If the task asks to save, import, download, quote, analyze source content, or reuse content, search-result metadata is not completion evidence; use `web_fetch` on the best concrete source URL when available, or return a clear blocker explaining why no fetchable source was verified.\n\
                 - Do not invent extra source-use constraints by default. Preserve only the constraints explicitly present in the original user request or runtime policy.\n\
                 - Use `web_fetch` on concrete source URLs, not search pages, filters, ads, login pages, or sources explicitly excluded by the original request.\n\
                 - Return the top usable source URLs with titles/snippets, fetched-source status, and any blockers. Do not delegate onward.\n\n",
            );
        }

        if tool_set.contains("browser") || tool_set.contains("browser_browse") {
            contract.push_str(
                "Browser contract:\n\
                 - For browser/page tasks, call `browser_browse` exactly as the real tool name.\n\
                 - Use `search` for open-ended web search; use `navigate` with `wait_until` for a concrete page; then inspect with `snapshot`, `extract_links`, or readonly `evaluate`.\n\
                 - Prefer `snapshot` formats `semantic`, `text`, `links`, or `markdown` according to the task instead of guessing from a search snippet.\n\
                 - Use readonly `evaluate` only for DOM extraction such as titles, links, list items, tables, and visible text. Do not mutate the page or initiate network side effects from evaluate.\n\
                 - If a page is empty, blocked, login-gated, or lacks item-level evidence, return the browser blocker and action trace instead of inventing facts.\n\
                 - Do not return pseudo tool-call tags as text; execute the real browser tool or return a blocker.\n\n",
            );
        }

        if tool_set.contains("command_exec") {
            contract.push_str(
                "Command execution contract:\n\
                 - If the delegated task contains an explicit local command, call `command_exec` exactly once.\n\
                 - Return the runtime, working directory, exit status, stdout/stderr summary, and blockers.\n\
                 - Do not answer from intuition when a command was explicitly requested.\n\n",
            );
        }

        if has_writing_package {
            contract.push_str(super::writing::policy::worker_contract_guidance());
        }

        if has_writing_package
            || tool_set.contains("knowledge")
            || tool_set.contains("tiered_search")
        {
            contract.push_str(
                "Knowledge retrieval contract:\n\
                 - For knowledge-base lookup or readback, call `tiered_search` first with the user's query.\n\
                 - If the search result includes a collection and path and exact details are needed, call `fetch_document` once.\n\
                 - For update/delete management requests, call `knowledge_manage_document`; natural-language update/delete must ask for explicit confirmation before overwriting or physical deletion.\n\
                 - Do not call `manage_facts` for imported knowledge-base documents.\n\
                 - Do not invent namespaced tools such as `knowledge.search_knowledge`; use direct tool names.\n\
                 - Return only the retrieved answer plus collection/path evidence or a clear not-found blocker.\n\n",
            );
        }

        if has_writing_package
            || tool_set.contains("knowledge")
            || tool_set.contains("knowledge_import_url")
        {
            contract.push_str(
                "Knowledge import contract:\n\
                 - If the task contains a concrete URL, call `knowledge_import_url` exactly once.\n\
                 - If there is no concrete URL or importable source, do not search; return a blocker.\n\
                 - Return collection, path, source URL, and import status.\n\n",
            );
        }

        if tool_set.contains("skill_manager") {
            contract.push_str(
                "Skill management contract:\n\
                 - You must call `skill_manager` before returning any result.\n\
                 - For local installed-skill inventory, list, or status requests, call `skill_manager` with `action: \"list\"`; do not search the network.\n\
                 - For source discovery or name-only install requests, call `skill_manager` with `action: \"resolve\"` and `skill_name`.\n\
                 - For installation, call `skill_manager` with `action: \"install\"`, `confirmed: true`, and the confirmed `source_url` when available.\n\
                 - Never invent registry/search results in natural language. If the tool fails, return the tool error as the blocker.\n\
                 - Return the candidate source, confirmation status, installed skill name, worker role, API key hint, and smoke-test result when available.\n\n",
            );
        }

        let task = Self::strip_unrequested_source_use_constraints(task, full_user_request);
        contract.push_str("Original delegated task:\n");
        contract.push_str(task.trim());
        contract
    }

    fn contains_source_use_constraint_term(text: &str) -> bool {
        let lowered = text.to_ascii_lowercase();
        [
            "copyright",
            "copyrighted",
            "license",
            "licensed",
            "licence",
            "permission",
            "authorized",
            "authorised",
            "authorization",
            "authorisation",
            "legal",
            "legally",
            "permissible",
            "permissibly",
            "free-to-use",
            "free to use",
            "public domain",
            "open-license",
            "open license",
            "rights",
            "版权",
            "授权",
            "许可",
            "合法",
            "公共领域",
            "开放许可",
            "权利",
        ]
        .iter()
        .any(|term| lowered.contains(term) || text.contains(term))
    }

    fn strip_unrequested_source_use_constraints(
        task: &str,
        full_user_request: Option<&str>,
    ) -> String {
        let Some(full_user_request) = full_user_request
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return task.to_string();
        };
        if Self::contains_source_use_constraint_term(full_user_request)
            || !Self::contains_source_use_constraint_term(task)
        {
            return task.to_string();
        }

        let mut kept = Vec::new();
        for line in task.lines() {
            let mut rebuilt = String::new();
            for sentence in line.split_inclusive(['.', ';', '。', '；']) {
                if !Self::contains_source_use_constraint_term(sentence) {
                    rebuilt.push_str(sentence);
                }
            }
            let rebuilt = rebuilt.trim();
            if !rebuilt.is_empty() {
                kept.push(rebuilt.to_string());
            }
        }

        if kept.is_empty() {
            task.to_string()
        } else {
            kept.join("\n")
        }
    }

    fn task_with_constraint_source(task: &str, full_user_request: Option<&str>) -> String {
        let task = task.trim();
        let Some(full_user_request) = full_user_request
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return task.to_string();
        };
        if task.eq_ignore_ascii_case(full_user_request) {
            return task.to_string();
        }
        let delegated_task = Self::strip_embedded_constraint_source(task, full_user_request);
        format!("Original user request:\n{full_user_request}\n\nDelegated task:\n{delegated_task}")
    }

    fn governed_writing_workflow_task<'a>(
        task: &'a str,
        full_user_request: Option<&'a str>,
    ) -> &'a str {
        let original = full_user_request
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| Self::user_request_slice_for_phase_boundary(task));
        if original != task.trim()
            && Self::task_requests_governed_fiction_project(original)
            && Self::delegated_task_is_weaker_writing_hint(task)
        {
            return original;
        }
        task.trim()
    }

    fn delegated_task_is_weaker_writing_hint(task: &str) -> bool {
        let delegated = task
            .split_once("\n\nDelegated task:")
            .map(|(_, tail)| tail.trim())
            .unwrap_or(task)
            .trim();
        if delegated.is_empty() {
            return false;
        }
        let lowered = delegated.to_ascii_lowercase();
        let planning_only = [
            "outline",
            "synopsis",
            "plan",
            "planning",
            "structure",
            "chapter plan",
            "story bible",
            "大纲",
            "提纲",
            "规划",
            "计划",
            "设定",
            "梗概",
            "简介",
        ]
        .iter()
        .any(|term| lowered.contains(term) || delegated.contains(term));
        if !planning_only {
            return false;
        }
        ![
            "正文",
            "完整正文",
            "写第一章",
            "写第",
            "续写",
            "write chapter",
            "draft chapter",
            "complete prose",
            "full prose",
        ]
        .iter()
        .any(|term| lowered.contains(term) || delegated.contains(term))
    }

    fn strip_embedded_constraint_source(task: &str, full_user_request: &str) -> String {
        if !task.contains(full_user_request) {
            return task.trim().to_string();
        }

        let mut stripped = task.replace(full_user_request, "");
        for marker in [
            "Original user request:",
            "original user request:",
            "Full user request:",
            "full_user_request:",
            "完整用户请求（必须保留查找之后的后续阶段，不能只完成查找片段）：",
            "完整用户请求：",
            "完整用户请求:",
        ] {
            stripped = stripped.replace(marker, "");
        }

        let lines = stripped
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>();
        if lines.is_empty() {
            task.trim().to_string()
        } else {
            lines.join("\n")
        }
    }

    fn current_runtime_task_refs() -> (Option<Uuid>, Option<String>) {
        benshu_brain::skills::CURRENT_RUNTIME_SECURITY_CONTEXT
            .try_with(|context| {
                (
                    context
                        .task_id
                        .as_deref()
                        .and_then(|task_id| Uuid::parse_str(task_id).ok()),
                    context.session_id.clone(),
                )
            })
            .unwrap_or((None, None))
    }

    pub(crate) fn fast_path_uses_attempt_budget(
        _supervised_fast_path: bool,
        managed_continuous_fast_path: bool,
    ) -> bool {
        !managed_continuous_fast_path
    }

    pub(crate) fn delegate_fast_path_budget_secs_for_task(task: &str) -> u64 {
        let base = SearchPolicy::delegate_fast_path_budget_secs_for_task(task);
        if !Self::task_requests_file_write(task) {
            return base;
        }

        let intent = Self::artifact_intent_surface(task);
        let requested_units = Self::requested_text_target_chars(&intent)
            .or_else(|| Self::requested_text_target_chars(task));
        let artifact_budget = requested_units
            .map(|units| 120u64.saturating_add(units.div_ceil(8) as u64))
            .unwrap_or(240)
            .clamp(180, 900);
        base.max(artifact_budget)
    }

    pub(crate) fn fast_path_blocker_should_fall_back(
        role: &AgentRole,
        task: &str,
        result: &str,
    ) -> bool {
        if !Self::looks_like_worker_blocker_status(result) {
            return false;
        }

        let role_name = role.name();
        matches!(role_name, "researcher" | "browser")
            && Self::task_requests_lookup(task)
            && (result.contains("browser search failed")
                || result.contains("anti-bot")
                || result.contains("source_material_mismatch")
                || result.contains("source_alignment_mismatch")
                || result.contains("no relevant parsable results")
                || result.contains("no usable observable results"))
    }

    pub(crate) fn guard_fast_path_completion_against_source_contract(
        role: &AgentRole,
        task: &str,
        result: String,
    ) -> String {
        let trimmed = result.trim_start();
        if role.name() != "researcher"
            || !trimmed.starts_with("status: completed\nworker: researcher")
            || !trimmed.contains("executed_tool: web_fetch")
            || !(Self::task_requires_verified_fetch_result(task)
                || Self::task_requests_narrative_source_material(task))
        {
            return result;
        }

        let Some(fetch_payload) = Self::fast_path_fetched_result_payload(trimmed) else {
            return result;
        };
        if Self::fetched_result_looks_usable_for_task(task, fetch_payload) {
            return result;
        }

        format!(
            "status: blocked\nworker: researcher\nerror_kind: source_material_mismatch\nblockers: fast path fetched source body did not preserve the user's requested source-material intent\nnext_step_hint: continue with a different source/query/provider that preserves the original material constraints, or report a clear blocker if no aligned source can be verified\n\nRejected fast-path receipt:\n{}",
            result
        )
    }

    fn fast_path_fetched_result_payload(result: &str) -> Option<&str> {
        let (_, mut payload) = result.split_once("fetched_result:")?;
        for marker in [
            "\n\nsearch_result_preview:",
            "\n\nsearch_result:",
            "\n\nresult_summary:",
            "\n\nOriginal user request:",
            "\n\n完整用户请求",
        ] {
            if let Some((head, _)) = payload.split_once(marker) {
                payload = head;
            }
        }
        let payload = payload.trim();
        (!payload.is_empty()).then_some(payload)
    }

    async fn resolve_delegate_checkpoint_task_id(
        task_manager: &benshu_state::TaskManager,
        task_id: Option<Uuid>,
        session_id: Option<&str>,
    ) -> Option<Uuid> {
        if let Some(task_id) = task_id {
            if matches!(task_manager.load(&task_id.to_string()).await, Ok(Some(_))) {
                return Some(task_id);
            }
        }

        let Some(session_id) = session_id else {
            return task_id;
        };
        let Ok(tasks) = task_manager.list_by_session(session_id).await else {
            return task_id;
        };
        tasks
            .into_iter()
            .find(|task| {
                matches!(
                    task.status,
                    benshu_state::TaskStatus::Running | benshu_state::TaskStatus::Queued
                )
            })
            .map(|task| task.id)
            .or(task_id)
    }

    async fn record_delegate_checkpoint(
        task_manager: Option<Arc<benshu_state::TaskManager>>,
        runtime_event_manager: Option<Arc<benshu_state::RuntimeEventManager>>,
        task_id: Option<Uuid>,
        session_id: Option<String>,
        role_name: &str,
        label: &str,
        summary: String,
        receipt_status: &str,
    ) {
        let resolved_task_id = if let Some(task_manager) = task_manager.as_ref() {
            Self::resolve_delegate_checkpoint_task_id(task_manager, task_id, session_id.as_deref())
                .await
        } else {
            task_id
        };
        let Some(resolved_task_id) = resolved_task_id else {
            return;
        };

        if let Some(event_manager) = runtime_event_manager {
            let mut receipt = benshu_state::RuntimeReceipt::new(receipt_status.to_string());
            receipt.actor = Some(role_name.to_string());
            receipt.action = Some(label.to_string());
            receipt.output_preview = Some(preview_text(&summary, 500));
            if receipt_status == "failed" || receipt_status == "blocked" {
                receipt.blocker = Some(preview_text(&summary, 500));
            }
            if let Err(error) = event_manager
                .append(
                    benshu_state::RuntimeEventRecord::new("delegate.worker.checkpoint")
                        .with_task(resolved_task_id)
                        .with_actor(role_name.to_string())
                        .with_receipt(receipt)
                        .with_payload(serde_json::json!({
                            "label": label,
                            "summary": summary,
                        })),
                )
                .await
            {
                tracing::warn!(
                    "Failed to append delegate worker runtime event for {}: {}",
                    role_name,
                    error
                );
            }
        }

        if let Some(task_manager) = task_manager {
            match task_manager.load(&resolved_task_id.to_string()).await {
                Ok(Some(mut task)) => {
                    let step = task.checkpoints.len().saturating_add(1) as u32;
                    task.updated_at = Utc::now();
                    task.status = benshu_state::TaskStatus::Running;
                    task.current_step = task.current_step.max(step);
                    task.checkpoints.push(benshu_state::TaskCheckpoint {
                        step,
                        label: label.to_string(),
                        recorded_at: Utc::now(),
                        summary: Some(summary),
                    });
                    if let Err(error) = task_manager.save(task).await {
                        tracing::warn!(
                            "Failed to save delegate worker checkpoint for {}: {}",
                            role_name,
                            error
                        );
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!(
                        "Failed to load task {} for delegate worker checkpoint: {}",
                        resolved_task_id,
                        error
                    );
                }
            }
        }
    }

    async fn run_worker_process_with_checkpoints(
        &self,
        agent: Arc<dyn MultiAgent>,
        role: &AgentRole,
        task: &str,
        phase: &str,
    ) -> anyhow::Result<String> {
        let (task_id, session_id) = Self::current_runtime_task_refs();
        let task_manager = self.task_manager.clone();
        let runtime_event_manager = self.runtime_event_manager.clone();
        let role_name = role.name().to_string();
        let task_preview = preview_text(task, 260);
        let phase_label = Self::normalize_role_label(phase);
        let worker_session_id = Self::delegated_worker_session_id(
            session_id.as_deref(),
            task_id,
            &role_name,
            &phase_label,
        );

        Self::record_delegate_checkpoint(
            task_manager.clone(),
            runtime_event_manager.clone(),
            task_id,
            session_id.clone(),
            &role_name,
            &format!("worker:{role_name}:{phase_label}:start"),
            format!(
                "Worker `{role_name}` accepted delegated work. continuation_worker_session_id={worker_session_id}. Task preview: {task_preview}"
            ),
            "running",
        )
        .await;

        let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel::<()>();
        let heartbeat = if (task_id.is_some() || session_id.is_some()) && task_manager.is_some() {
            let heartbeat_task_manager = task_manager.clone();
            let heartbeat_event_manager = runtime_event_manager.clone();
            let heartbeat_session_id = session_id.clone();
            let heartbeat_role = role_name.clone();
            let heartbeat_phase = phase_label.clone();
            Some(tokio::spawn(async move {
                let started = Instant::now();
                loop {
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_secs(30)) => {
                            let elapsed = started.elapsed().as_secs();
                            Self::record_delegate_checkpoint(
                                heartbeat_task_manager.clone(),
                                heartbeat_event_manager.clone(),
                                task_id,
                                heartbeat_session_id.clone(),
                                &heartbeat_role,
                                &format!("worker:{heartbeat_role}:{heartbeat_phase}:heartbeat"),
                                format!("Worker `{heartbeat_role}` is still running delegated work after {elapsed}s."),
                                "running",
                            ).await;
                        }
                        _ = &mut stop_rx => {
                            break;
                        }
                    }
                }
            }))
        } else {
            None
        };

        let mut current_task = task.to_string();
        let mut continuation_attempts = 0usize;
        let mut last_progress_fingerprint: Option<String> = None;
        let result = loop {
            let result = agent
                .chat(
                    vec![Message::user(current_task.clone())],
                    Some(worker_session_id.clone()),
                )
                .await
                .map(|outcome| outcome.response);
            match result {
                Ok(result) => {
                    if Self::task_requests_checkpointed_text_artifact(task)
                        && !Self::checkpoint_summary_has_artifact_written_receipt(&result)
                    {
                        if let Some(progress_checkpoint) =
                            Self::latest_artifact_progress_checkpoint_summary(
                                task_manager.as_ref(),
                                task_id,
                                session_id.as_deref(),
                            )
                            .await
                        {
                            let fingerprint =
                                Self::progress_checkpoint_fingerprint(&progress_checkpoint);
                            if last_progress_fingerprint.as_deref() == Some(fingerprint.as_str()) {
                                break Ok(Self::delegated_progress_no_new_work_blocker(
                                    &role_name,
                                    &progress_checkpoint,
                                ));
                            }
                            if continuation_attempts >= Self::delegate_progress_continuation_limit()
                            {
                                break Ok(Self::delegated_progress_continuation_limit_blocker(
                                    &role_name,
                                    continuation_attempts,
                                    &progress_checkpoint,
                                ));
                            }
                            continuation_attempts += 1;
                            last_progress_fingerprint = Some(fingerprint);
                            Self::record_delegate_checkpoint(
                                task_manager.clone(),
                                runtime_event_manager.clone(),
                                task_id,
                                session_id.clone(),
                                &role_name,
                                &format!(
                                    "worker:{role_name}:{phase_label}:progress_continuation"
                                ),
                                format!(
                                    "Worker `{role_name}` returned before the requested artifact was complete, but durable progress exists; continuing from the latest checkpoint. Attempt {}. Latest progress: {}",
                                    continuation_attempts,
                                    preview_text(&progress_checkpoint, 500)
                                ),
                                "running",
                            )
                            .await;
                            current_task = Self::build_delegated_progress_continuation_task(
                                task,
                                &progress_checkpoint,
                                continuation_attempts,
                            );
                            continue;
                        }
                    }
                    break Ok(result);
                }
                Err(error) => {
                    let error_text = error.to_string();
                    if Self::delegate_error_is_max_steps_exhausted(&error_text) {
                        if let Some(progress_checkpoint) =
                            Self::latest_artifact_progress_checkpoint_summary(
                                task_manager.as_ref(),
                                task_id,
                                session_id.as_deref(),
                            )
                            .await
                        {
                            let fingerprint =
                                Self::progress_checkpoint_fingerprint(&progress_checkpoint);
                            if last_progress_fingerprint.as_deref() == Some(fingerprint.as_str()) {
                                break Err(anyhow::anyhow!(
                                    "Max agent steps exceeded after artifact progress, but no new durable progress was observed on the continuation attempt"
                                ));
                            }
                            if continuation_attempts >= Self::delegate_progress_continuation_limit()
                            {
                                break Ok(Self::delegated_progress_continuation_limit_blocker(
                                    &role_name,
                                    continuation_attempts,
                                    &progress_checkpoint,
                                ));
                            }
                            continuation_attempts += 1;
                            last_progress_fingerprint = Some(fingerprint);
                            Self::record_delegate_checkpoint(
                                task_manager.clone(),
                                runtime_event_manager.clone(),
                                task_id,
                                session_id.clone(),
                                &role_name,
                                &format!(
                                    "worker:{role_name}:{phase_label}:progress_continuation"
                                ),
                                format!(
                                    "Worker `{role_name}` reached its step budget after durable artifact progress; continuing from the latest checkpoint instead of failing the whole delegated task. Attempt {}. Latest progress: {}",
                                    continuation_attempts,
                                    preview_text(&progress_checkpoint, 500)
                                ),
                                "running",
                            )
                            .await;
                            current_task = Self::build_delegated_progress_continuation_task(
                                task,
                                &progress_checkpoint,
                                continuation_attempts,
                            );
                            continue;
                        }
                    }
                    break Err(anyhow::anyhow!(error_text));
                }
            }
        };
        let _ = stop_tx.send(());
        if let Some(heartbeat) = heartbeat {
            if let Err(error) = heartbeat.await {
                tracing::warn!(
                    "Delegate worker heartbeat task for {} failed to join: {}",
                    role_name,
                    error
                );
            }
        }

        match result {
            Ok(result) => {
                if Self::delegated_worker_result_is_runtime_failure(&result) {
                    let blocked_result =
                        Self::delegated_worker_runtime_failure_blocker(&role_name, &result);
                    if blocked_result.is_some() {
                        if let Some(artifact_checkpoint) =
                            Self::latest_artifact_written_checkpoint_summary(
                                task_manager.as_ref(),
                                task_id,
                                session_id.as_deref(),
                            )
                            .await
                        {
                            let recovered_result = format!(
                                "status: completed\nworker: {role_name}\nruntime_effect: artifact.written\nwarnings: delegated worker reported a post-write runtime blocker after a durable artifact receipt; required write evidence is preserved and the blocker is surfaced as a warning\nartifact_checkpoint: {}\npost_write_blocker_preview: {}",
                                preview_text(&artifact_checkpoint, 500),
                                preview_text(&result, 500)
                            );
                            Self::record_delegate_checkpoint(
                                task_manager,
                                runtime_event_manager,
                                task_id,
                                session_id,
                                &role_name,
                                &format!("worker:{role_name}:{phase_label}:completed_with_warnings"),
                                format!(
                                    "Worker `{role_name}` produced durable artifact evidence before a post-write blocker. Completing from artifact receipt. Blocker preview: {}",
                                    preview_text(&result, 500)
                                ),
                                "completed",
                            )
                            .await;
                            return Ok(recovered_result);
                        }
                    }
                    let error_text = format!(
                        "worker `{role_name}` returned a runtime failure instead of completed delegated output: {}",
                        preview_text(&result, 500)
                    );
                    Self::record_delegate_checkpoint(
                        task_manager,
                        runtime_event_manager,
                        task_id,
                        session_id,
                        &role_name,
                        &format!("worker:{role_name}:{phase_label}:blocked"),
                        if blocked_result.is_some() {
                            format!(
                                "Worker `{role_name}` returned a recoverable runtime blocker. Preview: {}",
                                preview_text(&result, 500)
                            )
                        } else {
                            error_text.clone()
                        },
                        if blocked_result.is_some() {
                            "blocked"
                        } else {
                            "failed"
                        },
                    )
                    .await;
                    if let Some(blocked_result) = blocked_result {
                        return Ok(blocked_result);
                    }
                    anyhow::bail!(error_text);
                }
                Self::record_delegate_checkpoint(
                    task_manager.clone(),
                    runtime_event_manager,
                    task_id,
                    session_id.clone(),
                    &role_name,
                    &format!("worker:{role_name}:{phase_label}:completed"),
                    format!(
                        "Worker `{role_name}` returned delegated result. Preview: {}",
                        preview_text(&result, 500)
                    ),
                    "completed",
                )
                .await;
                if !Self::checkpoint_summary_has_artifact_written_receipt(&result) {
                    if let Some(artifact_checkpoint) =
                        Self::latest_artifact_written_checkpoint_summary(
                            task_manager.as_ref(),
                            task_id,
                            session_id.as_deref(),
                        )
                        .await
                    {
                        return Ok(Self::delegated_result_with_artifact_receipt(
                            &role_name,
                            &result,
                            &artifact_checkpoint,
                        ));
                    }
                }
                Ok(result)
            }
            Err(error) => {
                let error_text = error.to_string();
                Self::record_delegate_checkpoint(
                    task_manager,
                    runtime_event_manager,
                    task_id,
                    session_id,
                    &role_name,
                    &format!("worker:{role_name}:{phase_label}:failed"),
                    format!("Worker `{role_name}` failed delegated work: {error_text}"),
                    "failed",
                )
                .await;
                Err(anyhow::anyhow!(error_text))
            }
        }
    }

    async fn latest_artifact_written_checkpoint_summary(
        task_manager: Option<&Arc<benshu_state::TaskManager>>,
        task_id: Option<Uuid>,
        session_id: Option<&str>,
    ) -> Option<String> {
        let task_manager = task_manager?;
        let resolved_task_id =
            Self::resolve_delegate_checkpoint_task_id(task_manager, task_id, session_id).await?;
        let task = task_manager
            .load(&resolved_task_id.to_string())
            .await
            .ok()
            .flatten()?;
        task.checkpoints
            .iter()
            .rev()
            .filter_map(|checkpoint| checkpoint.summary.as_deref())
            .find(|summary| Self::checkpoint_summary_has_artifact_written_receipt(summary))
            .map(str::to_string)
    }

    pub(crate) fn checkpoint_summary_has_artifact_written_receipt(summary: &str) -> bool {
        let lowered = summary.to_ascii_lowercase();
        let compact = lowered.replace([' ', '\n', '\r', '\t'], "");
        if lowered.contains("\"read_only\": true") || lowered.contains("\"read_only\":true") {
            return false;
        }
        if lowered.contains("artifact.needs_revision")
            || lowered.contains("\"runtime_effect\":\"artifact.needs_revision\"")
            || lowered.contains("\"runtime_effect\": \"artifact.needs_revision\"")
            || lowered.contains("\"status\":\"needs_revision\"")
            || lowered.contains("\"status\": \"needs_revision\"")
            || lowered.contains("status: \"needs_revision\"")
            || lowered.contains("status: needs_revision")
            || lowered.contains("\"passed\":false")
            || lowered.contains("\"passed\": false")
        {
            return false;
        }
        if Self::checkpoint_summary_is_process_artifact_only(&lowered) {
            return false;
        }
        if Self::checkpoint_summary_is_error_or_blocker_artifact(&lowered, &compact) {
            return false;
        }
        if Self::checkpoint_summary_is_review_or_audit_artifact_only(&lowered) {
            return false;
        }
        if Self::checkpoint_summary_has_unmet_artifact_target(&lowered, &compact) {
            return false;
        }
        if (lowered.contains("artifact.checkpointed")
            || lowered.contains("\"completion_scope\":\"checkpoint\"")
            || lowered.contains("\"completion_scope\": \"checkpoint\""))
            && (lowered.contains("\"target_reached\":false")
                || lowered.contains("\"target_reached\": false")
                || lowered.contains("\"completion_scope\":\"checkpoint\"")
                || lowered.contains("\"completion_scope\": \"checkpoint\""))
        {
            return false;
        }
        let success_signal = lowered.contains("finished success=true")
            || lowered.contains("\"success\": true")
            || lowered.contains("\"success\":true")
            || lowered.contains("status: completed")
            || Self::checkpoint_summary_has_saved_artifact_path(summary);
        let write_signal = lowered.contains("runtime_effect: artifact.written")
            || lowered.contains("\"runtime_effect\":\"artifact.written\"")
            || lowered.contains("\"runtime_effect\": \"artifact.written\"")
            || lowered.contains("\"runtime_effects\"") && lowered.contains("artifact.written")
            || lowered.contains("artifact.exported")
            || lowered.contains("\"output_path\"") && lowered.contains("artifact.")
            || lowered.contains("artifact.written")
            || Self::checkpoint_summary_has_saved_artifact_path(summary);
        success_signal && write_signal
    }

    fn checkpoint_summary_has_saved_artifact_path(summary: &str) -> bool {
        let lowered = summary.to_ascii_lowercase().replace('\\', "/");
        let saved_signal = summary.contains("已保存")
            || summary.contains("文件：")
            || summary.contains("文件:")
            || lowered.contains(" saved ");
        if !saved_signal {
            return false;
        }
        if Self::checkpoint_summary_is_process_artifact_only(&lowered) {
            return false;
        }
        lowered.contains("/generated/")
            && (lowered.contains(".md") || lowered.contains(".txt") || lowered.contains(".pdf"))
    }

    fn checkpoint_summary_has_unmet_artifact_target(lowered: &str, compact: &str) -> bool {
        if compact.contains("\"target_reached\":false")
            || compact.contains("target_reached:false")
            || lowered.contains("completion_scope: checkpoint")
            || compact.contains("\"completion_scope\":\"checkpoint\"")
            || lowered.contains("far below the chapter target")
            || lowered.contains("below the chapter target")
        {
            return true;
        }
        let mentions_target = compact.contains("\"target_units\"")
            || compact.contains("target_units:")
            || lowered.contains("target scale")
            || lowered.contains("目标规模");
        let target_confirmed = compact.contains("\"target_reached\":true")
            || compact.contains("target_reached:true")
            || lowered.contains("artifact.exported");
        mentions_target && !target_confirmed
    }

    fn checkpoint_summary_is_review_or_audit_artifact_only(lowered: &str) -> bool {
        let compact = lowered.replace([' ', '\n', '\r', '\t'], "");
        let review_path = lowered.contains("/reviews/")
            || lowered.contains("\\reviews\\")
            || compact.contains("\"review_path\"")
            || compact.contains("review_path:")
            || compact.contains("chapter-review")
            || compact.contains("chapter-audit")
            || compact.contains("audit-")
            || compact.contains("-audit-");
        if !review_path {
            return false;
        }
        let final_export = lowered.contains("artifact.exported")
            || lowered.contains("artifact.txt")
            || lowered.contains("artifact.md")
            || lowered.contains("/exports/")
            || lowered.contains("\\exports\\");
        !final_export
    }

    fn checkpoint_summary_is_process_artifact_only(lowered: &str) -> bool {
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

    fn checkpoint_summary_is_error_or_blocker_artifact(lowered: &str, compact: &str) -> bool {
        if compact.contains("status:blocker")
            || compact.contains("status:blocked")
            || compact.contains("\"status\":\"blocker\"")
            || compact.contains("\"status\":\"blocked\"")
            || lowered.contains("error_kind")
            || lowered.contains("missing_required_argument")
            || lowered.contains("missing_required_content")
        {
            return true;
        }

        let has_blocker_field = lowered.contains("blockers:")
            || lowered.contains("blocked:")
            || lowered.contains("\"blockers\"")
            || lowered.contains("\"blocked\"");
        let blocker_is_failure = lowered.contains("missing")
            || lowered.contains("failed")
            || lowered.contains("failure")
            || lowered.contains("error")
            || lowered.contains("cannot")
            || lowered.contains("unable")
            || lowered.contains("缺少")
            || lowered.contains("失败")
            || lowered.contains("无法");
        has_blocker_field && blocker_is_failure
    }

    fn delegated_result_with_artifact_receipt(
        role_name: &str,
        result: &str,
        artifact_checkpoint: &str,
    ) -> String {
        format!(
            "status: completed\nworker: {role_name}\nruntime_effect: artifact.written\nartifact_checkpoint: {}\nresult:\n{}",
            preview_text(artifact_checkpoint, 800),
            result
        )
    }

    async fn latest_artifact_progress_checkpoint_summary(
        task_manager: Option<&Arc<benshu_state::TaskManager>>,
        task_id: Option<Uuid>,
        session_id: Option<&str>,
    ) -> Option<String> {
        let task_manager = task_manager?;
        let resolved_task_id =
            Self::resolve_delegate_checkpoint_task_id(task_manager, task_id, session_id).await?;
        let task = task_manager
            .load(&resolved_task_id.to_string())
            .await
            .ok()
            .flatten()?;
        task.checkpoints
            .iter()
            .rev()
            .filter_map(|checkpoint| checkpoint.summary.as_deref())
            .find(|summary| Self::checkpoint_summary_has_artifact_progress_receipt(summary))
            .map(str::to_string)
    }

    pub(crate) fn checkpoint_summary_has_artifact_progress_receipt(summary: &str) -> bool {
        let lowered = summary.to_ascii_lowercase();
        if lowered.contains("\"read_only\": true") || lowered.contains("\"read_only\":true") {
            return false;
        }
        if lowered.contains("artifact.needs_revision")
            || lowered.contains("\"runtime_effect\":\"artifact.needs_revision\"")
            || lowered.contains("\"runtime_effect\": \"artifact.needs_revision\"")
            || lowered.contains("\"passed\":false")
            || lowered.contains("\"passed\": false")
        {
            return false;
        }
        if Self::checkpoint_summary_is_process_artifact_only(&lowered) {
            return false;
        }
        let compact = lowered.replace([' ', '\n', '\r', '\t'], "");
        if Self::checkpoint_summary_is_error_or_blocker_artifact(&lowered, &compact) {
            return false;
        }
        if Self::checkpoint_summary_is_review_or_audit_artifact_only(&lowered) {
            return false;
        }
        let success_signal = lowered.contains("finished success=true")
            || lowered.contains("\"success\": true")
            || lowered.contains("\"success\":true")
            || lowered.contains("status: completed");
        let progress_signal = lowered.contains("artifact.checkpointed")
            || lowered.contains("artifact.written")
            || lowered.contains("\"artifact_path\"")
            || lowered.contains("\"output_path\"")
            || lowered.contains("\"total_units\"");
        success_signal && progress_signal
    }

    fn delegate_error_is_max_steps_exhausted(error_text: &str) -> bool {
        let lowered = error_text.to_ascii_lowercase();
        lowered.contains("max agent steps exceeded") || lowered.contains("max steps reached")
    }

    fn delegate_progress_continuation_limit() -> usize {
        std::env::var("BENSHU_DELEGATE_PROGRESS_CONTINUATION_LIMIT")
            .ok()
            .and_then(|value| value.trim().parse::<usize>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(128)
    }

    fn progress_checkpoint_fingerprint(summary: &str) -> String {
        let mut out = String::new();
        for line in summary.lines() {
            let trimmed = line.trim();
            if trimmed.contains("artifact_path")
                || trimmed.contains("output_path")
                || trimmed.contains("runtime_effect")
                || trimmed.contains("total_units")
                || trimmed.contains("unit_count")
                || trimmed.contains("chapter_number")
                || trimmed.contains("chapter")
            {
                out.push_str(trimmed);
                out.push('\n');
            }
        }
        if out.trim().is_empty() {
            preview_text(summary, 500)
        } else {
            out
        }
    }

    fn build_delegated_progress_continuation_task(
        original_task: &str,
        progress_checkpoint: &str,
        attempt: usize,
    ) -> String {
        format!(
            "### Delegated Progress Continuation\n\
             Your previous worker turn reached its step budget after producing durable artifact progress. Continue the same delegated task from the latest saved project/artifact state.\n\n\
             Continuation attempt: {attempt}\n\n\
             Rules:\n\
             - Do not restart the project, duplicate prior material, rename the artifact, or create a new unrelated artifact.\n\
             - Use the existing project/artifact path from your prior tool results or from the latest checkpoint. If uncertain, inspect/status/read the existing project first.\n\
             - Continue the next unfinished bounded unit, preserve established names/facts/continuity, and persist each new unit through the equipped writing/file tool.\n\
             - If the target scale is reached, export/save the requested final file format and report the final path and total units.\n\
             - If the target scale is not reached, keep making durable progress; the outer runtime may resume you again after another checkpoint.\n\n\
             Original delegated task:\n{original_task}\n\n\
             Latest durable progress checkpoint:\n{}\n",
            preview_text(progress_checkpoint, 1_500)
        )
    }

    fn delegated_progress_continuation_limit_blocker(
        role_name: &str,
        attempts: usize,
        progress_checkpoint: &str,
    ) -> String {
        format!(
            "status: blocked\nworker: {role_name}\nblockers: delegated worker reached the configured progress continuation limit after producing durable artifact progress; the task is resumable from the latest checkpoint\ncontinuation_attempts: {attempts}\nlatest_progress_checkpoint: {}",
            preview_text(progress_checkpoint, 800)
        )
    }

    fn delegated_progress_no_new_work_blocker(
        role_name: &str,
        progress_checkpoint: &str,
    ) -> String {
        format!(
            "status: blocked\nworker: {role_name}\nblockers: delegated worker returned before final completion and the latest durable progress did not change; the task is resumable but needs a fresh continuation strategy\nlatest_progress_checkpoint: {}",
            preview_text(progress_checkpoint, 800)
        )
    }

    fn delegated_worker_runtime_failure_blocker(role_name: &str, result: &str) -> Option<String> {
        let lowered = result.to_ascii_lowercase();
        let recoverable_boundary = lowered.contains("runtime tool error")
            || lowered.contains("error executing tool")
            || lowered.contains("tool not found")
            || lowered.contains("tool is not equipped")
            || lowered.contains("tool not equipped")
            || (lowered.contains("planned tool call")
                && lowered.contains("did not produce a matching tool result"));
        let recoverable_contract = Self::result_contains_structured_tool_contract_error(&lowered);
        if !recoverable_boundary && !recoverable_contract {
            return None;
        }
        let available_tools = Self::available_tools_from_runtime_failure(result)
            .map(|tools| format!("\navailable_tools: {tools}"))
            .unwrap_or_default();
        let blocker = if recoverable_contract && !recoverable_boundary {
            "delegated worker returned a structured tool contract error before producing a reliable result"
        } else {
            "delegated worker hit a tool boundary or runtime execution boundary before producing a reliable result"
        };
        Some(format!(
            "status: blocked\nworker: {role_name}\nblockers: {blocker}{available_tools}\nruntime_error_preview: {}",
            preview_text(result, 500)
        ))
    }

    fn result_contains_structured_tool_contract_error(lowered: &str) -> bool {
        if Self::result_contains_structured_not_found_observation(lowered) {
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

    fn result_contains_structured_not_found_observation(lowered: &str) -> bool {
        (lowered.contains("\"error_kind\":\"not_found\"")
            || lowered.contains("\"error_kind\": \"not_found\"")
            || lowered.contains("\"error_kind\":\"chapter_not_found\"")
            || lowered.contains("\"error_kind\": \"chapter_not_found\""))
            && !lowered.contains("missing_required")
            && !lowered.contains("missing required")
            && !lowered.contains(" is required")
            && !lowered.contains(" required for ")
    }

    fn available_tools_from_runtime_failure(result: &str) -> Option<String> {
        let marker = "Available tools right now:";
        let (_, tail) = result.split_once(marker)?;
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

    fn delegated_worker_result_is_runtime_failure(result: &str) -> bool {
        let trimmed = result.trim();
        if trimmed.is_empty() {
            return true;
        }
        let mut explicit_status = None;
        for line in trimmed.lines() {
            let line = line.trim();
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            if key.trim().eq_ignore_ascii_case("status") {
                explicit_status = Some(value.trim().to_ascii_lowercase());
                break;
            }
        }
        if matches!(explicit_status.as_deref(), Some("blocked")) {
            return false;
        }
        if matches!(explicit_status.as_deref(), Some("failed" | "error")) {
            return true;
        }
        let lowered = trimmed.to_ascii_lowercase();
        lowered.contains("runtime tool error")
            || lowered.contains("error executing tool")
            || lowered.contains("tool not found")
            || lowered.contains("tool is not equipped")
            || (lowered.contains("planned tool call")
                && lowered.contains("did not produce a matching tool result"))
            || Self::result_contains_structured_tool_contract_error(&lowered)
    }

    fn contains_unexecuted_pseudo_tool_call(text: &str) -> bool {
        let trimmed = text.trim();
        trimmed.contains("<|tool_call>") || trimmed.contains("<tool_call|>")
    }

    fn build_worker_pseudo_tool_recovery_contract(
        role: &AgentRole,
        blueprint_tools: &[String],
        original_task: &str,
        full_user_request: Option<&str>,
        leaked_result: &str,
    ) -> String {
        let base = Self::build_worker_execution_contract(
            role,
            blueprint_tools,
            original_task,
            full_user_request,
        );
        let leaked_preview = preview_text(leaked_result, 1_200);
        format!(
            "{base}\n\n\
             ### REQUIRED RECOVERY STEP\n\
             Your previous response returned an unexecuted pseudo tool tag as plain text.\n\
             That is not a completed result.\n\
             You must now actually call the matching real tool from your available specialist tools.\n\
             Do not repeat `<|tool_call>` tags in text.\n\
             If no matching real tool is available, return `status: blocked` with a concise `blockers` reason.\n\n\
             Previous invalid worker output preview:\n{leaked_preview}"
        )
    }

    fn expanded_worker_tool_names(blueprint_tools: &[String]) -> Vec<String> {
        let mut expanded = Vec::new();
        for tool in blueprint_tools {
            if tool == "knowledge" {
                Self::push_unique(&mut expanded, "knowledge_search");
                Self::push_unique(&mut expanded, "tiered_search");
                Self::push_unique(&mut expanded, "fetch_document");
                Self::push_unique(&mut expanded, "knowledge_import_url");
                Self::push_unique(&mut expanded, "knowledge_manage_document");
                continue;
            }
            if tool == "writing" || tool == "writing_studio" {
                Self::push_unique(&mut expanded, "read_file");
                Self::push_unique(&mut expanded, "write_file");
                Self::push_unique(&mut expanded, "list_dir");
                Self::push_unique(&mut expanded, "edit_file");
                Self::push_unique(&mut expanded, "knowledge_search");
                Self::push_unique(&mut expanded, "tiered_search");
                Self::push_unique(&mut expanded, "fetch_document");
                Self::push_unique(&mut expanded, "knowledge_import_url");
                Self::push_unique(&mut expanded, "knowledge_manage_document");
                Self::push_unique(&mut expanded, "writing_studio");
                if tool == "writing" {
                    Self::push_unique(&mut expanded, "novel_studio");
                }
                continue;
            }
            if tool == "image_gen" {
                Self::push_unique(&mut expanded, "generate_image");
                continue;
            }
            if tool == "browser" {
                Self::push_unique(&mut expanded, "browser_browse");
                continue;
            }
            Self::push_unique(&mut expanded, tool);
        }
        expanded
    }

    fn expanded_worker_tool_names_for_task(blueprint_tools: &[String], task: &str) -> Vec<String> {
        let mut expanded = Self::expanded_worker_tool_names(blueprint_tools);
        if Self::should_route_writer_fiction_to_novel_studio(blueprint_tools, task)
            && expanded.iter().any(|tool| tool == "novel_studio")
        {
            expanded.retain(|tool| {
                !matches!(
                    tool.as_str(),
                    "writing_studio" | "write_file" | "read_file" | "list_dir" | "edit_file"
                )
            });
        }
        expanded
    }

    pub(crate) fn worker_has_novel_studio_tool(blueprint_tools: &[String]) -> bool {
        super::writing::longform_policy::worker_has_novel_studio_tool(blueprint_tools)
    }

    pub(crate) fn task_requests_governed_fiction_project(task: &str) -> bool {
        super::writing::longform_policy::task_requests_governed_fiction_project(
            task,
            Self::requested_text_target_chars,
            Self::longform_step_target_chars(),
        )
    }

    pub(crate) fn should_route_writer_fiction_to_novel_studio(
        blueprint_tools: &[String],
        task: &str,
    ) -> bool {
        super::writing::longform_policy::should_route_writer_fiction_to_novel_studio(
            blueprint_tools,
            task,
            Self::requested_text_target_chars,
            Self::longform_step_target_chars(),
        )
    }

    pub(crate) fn should_use_managed_continuous_fast_path(
        role: &AgentRole,
        blueprint_tools: &[String],
        task: &str,
    ) -> bool {
        role.name() == "writer"
            && Self::task_requests_local_file_continuation(task)
            && !Self::should_route_writer_fiction_to_novel_studio(blueprint_tools, task)
    }

    fn extract_backticked_command(task: &str) -> Option<String> {
        let mut rest = task;
        while let Some(start) = rest.find('`') {
            let after_start = &rest[start + 1..];
            let Some(end) = after_start.find('`') else {
                break;
            };
            let candidate = after_start[..end].trim();
            if !candidate.is_empty() {
                return Some(candidate.to_string());
            }
            rest = &after_start[end + 1..];
        }
        None
    }

    fn extract_terminal_command(task: &str) -> Option<String> {
        if let Some(command) = Self::extract_backticked_command(task) {
            return Some(command);
        }

        let trimmed = task.trim();
        let lower = trimmed.to_ascii_lowercase();
        for marker in [
            "执行命令",
            "运行命令",
            "run command",
            "execute command",
            "command:",
            "命令：",
            "命令:",
        ] {
            if let Some((_, tail)) = lower.split_once(marker) {
                let offset = trimmed.len().saturating_sub(tail.len());
                let candidate = trimmed[offset..]
                    .trim()
                    .trim_start_matches(':')
                    .trim_start_matches('：')
                    .trim();
                if Self::looks_like_explicit_command(candidate) {
                    return Some(candidate.to_string());
                }
            }
        }

        if Self::looks_like_explicit_command(trimmed) {
            return Some(trimmed.to_string());
        }

        None
    }

    fn looks_like_explicit_command(candidate: &str) -> bool {
        let first = candidate
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .trim_matches(|ch| matches!(ch, '"' | '\''));
        matches!(
            first,
            "rg" | "grep"
                | "find"
                | "ls"
                | "cat"
                | "pwd"
                | "git"
                | "cargo"
                | "npm"
                | "pnpm"
                | "node"
                | "python"
                | "python3"
                | "bash"
                | "sh"
                | "powershell"
                | "powershell.exe"
                | "cmd"
                | "seq"
        )
    }

    fn format_command_exec_result(output: &str) -> String {
        let Ok(payload) = serde_json::from_str::<serde_json::Value>(output) else {
            return format!(
                "status: completed\nworker: terminal\nexecuted_tool: command_exec\nresult:\n{}",
                output
            );
        };

        let success = payload
            .get("success")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let outcome_kind = payload
            .get("outcome_kind")
            .and_then(|value| value.as_str())
            .unwrap_or(if success { "success" } else { "failure" });
        let summary = payload
            .get("outcome_summary")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let working_dir = payload
            .get("working_dir")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let raw_exit_status = payload
            .get("status")
            .map(|value| value.to_string())
            .unwrap_or_else(|| "null".to_string());
        let stdout = payload
            .get("stdout")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let stderr = payload
            .get("stderr")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let artifacts = payload
            .get("evidence_artifacts")
            .and_then(|value| value.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.get("uri").and_then(|uri| uri.as_str()))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();

        let mut lines = vec![
            "status: completed".to_string(),
            "worker: terminal".to_string(),
            "executed_tool: command_exec".to_string(),
            format!("outcome: {outcome_kind}"),
            format!("result: {summary}"),
            format!("working_dir: {working_dir}"),
            format!("raw_exit_status: {raw_exit_status}"),
        ];
        if !artifacts.is_empty() {
            lines.push(format!("evidence_artifacts: {artifacts}"));
        }
        if !stdout.trim().is_empty() {
            lines.push(format!(
                "stdout_preview: {}",
                Self::preview_worker_output(stdout.trim(), 1200)
            ));
        }
        if !stderr.trim().is_empty() {
            lines.push(format!(
                "stderr_preview: {}",
                Self::preview_worker_output(stderr.trim(), 1200)
            ));
        }
        if success {
            lines.push("blockers: none".to_string());
        } else {
            lines.push("blockers: command execution failed".to_string());
        }
        lines.join("\n")
    }

    fn preview_worker_output(output: &str, max_chars: usize) -> String {
        let total = output.chars().count();
        if total <= max_chars {
            return output.to_string();
        }

        let keep = max_chars.saturating_sub(96).max(64);
        let head = keep / 2;
        let tail = keep - head;
        let start = output.chars().take(head).collect::<String>();
        let end = output
            .chars()
            .rev()
            .take(tail)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<String>();
        format!("{start}\n[worker stdout/stderr preview truncated: {total} chars]\n{end}")
    }

    fn parse_delegate_args(arguments: &str) -> anyhow::Result<DelegateArgs> {
        match serde_json::from_str::<DelegateArgs>(arguments) {
            Ok(args) => return Ok(args),
            Err(original_error) => {
                let recovered = Self::recover_delegate_args_from_fragmented_object(arguments)
                    .ok_or(original_error)?;
                Ok(recovered)
            }
        }
    }

    fn recover_delegate_args_from_fragmented_object(arguments: &str) -> Option<DelegateArgs> {
        let value = serde_json::from_str::<serde_json::Value>(arguments).ok()?;
        let object = value.as_object()?;
        let mut role = None;
        let mut task_parts = Vec::new();

        for (key, value) in object {
            let normalized_key = Self::normalize_fragmented_tool_arg_text(key);
            let normalized_value = value
                .as_str()
                .map(Self::normalize_fragmented_tool_arg_text)
                .unwrap_or_default();

            if let Some(fragment) = Self::field_fragment(&normalized_key, "role") {
                let fragment = if fragment.is_empty() {
                    normalized_value.clone()
                } else {
                    fragment
                };
                if !fragment.is_empty() {
                    role = Some(fragment);
                }
                continue;
            }

            if let Some(fragment) = Self::field_fragment(&normalized_key, "task") {
                if !fragment.is_empty() {
                    task_parts.push(fragment);
                }
                if !normalized_value.is_empty() {
                    task_parts.push(normalized_value);
                }
                continue;
            }

            if !normalized_key.is_empty() {
                task_parts.push(normalized_key);
            }
            if !normalized_value.is_empty() {
                task_parts.push(normalized_value);
            }
        }

        let role = role?;
        let task = task_parts
            .into_iter()
            .map(|part| part.trim().to_string())
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" ");

        if task.is_empty() {
            return None;
        }

        Some(DelegateArgs {
            role,
            task,
            fallback_role: None,
            full_user_request: None,
        })
    }

    fn normalize_fragmented_tool_arg_text(input: &str) -> String {
        input
            .replace("<|\\\"|>", "\"")
            .replace("<|\"|>", "\"")
            .replace("<|\\'|>", "'")
            .replace("<|'|>", "'")
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .trim_matches(',')
            .trim()
            .to_string()
    }

    fn field_fragment(input: &str, field: &str) -> Option<String> {
        let trimmed = input.trim();
        let lower = trimmed.to_ascii_lowercase();
        let needle = format!("{field}:");
        let index = lower.find(&needle)?;
        let fragment = trimmed[index + needle.len()..]
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .trim()
            .to_string();
        Some(fragment)
    }

    fn task_requests_lookup(task: &str) -> bool {
        let lowered = task.to_lowercase();
        lowered.contains("search")
            || lowered.contains("lookup")
            || lowered.contains("find")
            || lowered.contains("check")
            || lowered.contains("inspect")
            || lowered.contains("visit")
            || lowered.contains("browse")
            || lowered.contains("source")
            || lowered.contains("sources")
            || lowered.contains("youtube")
            || lowered.contains("video")
            || lowered.contains("搜索")
            || lowered.contains("查询")
            || lowered.contains("查找")
            || lowered.contains("检查")
            || lowered.contains("访问")
            || lowered.contains("浏览")
            || lowered.contains("检索")
            || lowered.contains("论文")
            || lowered.contains("资料")
            || lowered.contains("来源")
            || lowered.contains("视频")
    }

    fn task_requests_knowledge_retrieval(task: &str) -> bool {
        let lowered = task.to_lowercase();
        if Self::task_requests_knowledge_management(task) {
            return false;
        }
        if Self::task_requests_knowledge_create(task) {
            return false;
        }
        let asks_retrieval = lowered.contains("knowledge")
            || lowered.contains("lookup")
            || lowered.contains("search")
            || lowered.contains("read")
            || lowered.contains("recall")
            || lowered.contains("知识库")
            || lowered.contains("查询")
            || lowered.contains("查找")
            || lowered.contains("检索")
            || lowered.contains("读出")
            || lowered.contains("读取")
            || lowered.contains("回答")
            || lowered.contains("电话");
        let asks_import = lowered.contains("import")
            || lowered.contains("ingest")
            || lowered.contains("save")
            || lowered.contains("store")
            || lowered.contains("导入")
            || lowered.contains("保存")
            || lowered.contains("入库")
            || lowered.contains("写入");
        asks_retrieval && (!asks_import || Self::first_url(task).is_none())
    }

    fn task_requests_knowledge_create(task: &str) -> bool {
        if Self::first_url(task).is_some() {
            return false;
        }
        let lowered = task.to_lowercase();
        let has_knowledge_scope = lowered.contains("knowledge base")
            || lowered.contains("知识库")
            || lowered.contains("知识文档")
            || lowered.contains("durable knowledge");
        let has_create_action = lowered.contains("save")
            || lowered.contains("store")
            || lowered.contains("write")
            || lowered.contains("persist")
            || lowered.contains("保存")
            || lowered.contains("存到")
            || lowered.contains("写入")
            || lowered.contains("记录到")
            || lowered.contains("入库");
        has_knowledge_scope && has_create_action
    }

    fn extract_knowledge_create_content(task: &str) -> String {
        const MARKERS: &[&str] = &[
            "内容是：",
            "内容是:",
            "内容：",
            "内容:",
            "content is:",
            "content:",
            "save:",
            "保存：",
            "保存:",
        ];
        for marker in MARKERS {
            if let Some((_, tail)) = task.split_once(marker) {
                let content = tail
                    .split("保存后")
                    .next()
                    .unwrap_or(tail)
                    .split("完成后")
                    .next()
                    .unwrap_or(tail)
                    .split("然后")
                    .next()
                    .unwrap_or(tail)
                    .trim()
                    .trim_matches(|ch| matches!(ch, '。' | '.' | '，' | ','));
                if !content.is_empty() {
                    return content.to_string();
                }
            }
        }
        task.trim().to_string()
    }

    fn infer_knowledge_create_title(content: &str) -> String {
        let compact = content.split_whitespace().collect::<Vec<_>>().join(" ");
        let title = compact
            .chars()
            .take(48)
            .collect::<String>()
            .trim()
            .trim_matches(|ch| matches!(ch, '。' | '.' | '，' | ','))
            .to_string();
        if title.is_empty() {
            "User provided knowledge".to_string()
        } else {
            title
        }
    }

    fn task_requests_knowledge_management(task: &str) -> bool {
        let command_surface = task
            .split("fetched_result:")
            .next()
            .unwrap_or(task)
            .split("fetched result:")
            .next()
            .unwrap_or(task)
            .split("\nresult:")
            .next()
            .unwrap_or(task)
            .split("source evidence:")
            .next()
            .unwrap_or(task);
        let lowered = command_surface.to_lowercase();
        (lowered.contains("knowledge")
            || lowered.contains("知识库")
            || lowered.contains("文档")
            || lowered.contains("资料"))
            && (lowered.contains("update")
                || lowered.contains("delete")
                || lowered.contains("remove")
                || lowered.contains("replace")
                || lowered.contains("更新")
                || lowered.contains("修改")
                || lowered.contains("删除")
                || lowered.contains("移除")
                || lowered.contains("替换")
                || lowered.contains("覆盖"))
    }

    fn extract_management_confirmation(task: &str, action: &str) -> Option<(String, String)> {
        let marker = format!("{} ", action);
        let upper = task.to_ascii_uppercase();
        let start = upper.find(&marker)? + marker.len();
        let rest = task.get(start..)?.trim_start();
        let target = rest
            .split(|ch: char| ch.is_whitespace() || ch == '，' || ch == ',' || ch == '。')
            .next()?
            .trim();
        let (collection, path) = target.split_once('/')?;
        if collection.trim().is_empty() || path.trim().is_empty() {
            return None;
        }
        Some((collection.trim().to_string(), path.trim().to_string()))
    }

    fn extract_update_content(task: &str) -> Option<String> {
        const MARKERS: &[&str] = &[
            "新内容：",
            "新内容:",
            "更新为：",
            "更新为:",
            "替换为：",
            "替换为:",
            "content:",
            "new content:",
        ];
        for marker in MARKERS {
            if let Some((_, rest)) = task.split_once(marker) {
                let content = rest.trim();
                if !content.is_empty() {
                    return Some(content.to_string());
                }
            }
        }
        None
    }

    fn summarize_knowledge_documents(docs: &[benshu_brain::knowledge::rag::Document]) -> String {
        if docs.is_empty() {
            return "No results found.".to_string();
        }

        docs.iter()
            .enumerate()
            .map(|(index, doc)| {
                let collection = doc.collection.as_deref().unwrap_or("default");
                let path = doc.path.as_deref().unwrap_or("-");
                format!(
                    "Result {}:\ntitle: {}\ncollection: {}\npath: {}\ncontent:\n{}",
                    index + 1,
                    doc.title,
                    collection,
                    path,
                    knowledge_snippet_text(&doc.content, 1_600)
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n---\n\n")
    }

    fn extract_quoted_terms(task: &str) -> Vec<String> {
        let mut terms = Vec::new();
        let mut current = String::new();
        let mut in_quote = false;
        for ch in task.chars() {
            match ch {
                '「' | '“' | '"' if !in_quote => {
                    in_quote = true;
                    current.clear();
                }
                '」' | '”' | '"' if in_quote => {
                    let value = current.trim();
                    if !value.is_empty() {
                        terms.push(value.to_string());
                    }
                    in_quote = false;
                    current.clear();
                }
                _ if in_quote => current.push(ch),
                _ => {}
            }
        }
        terms
    }

    fn knowledge_retrieval_queries(task: &str) -> Vec<String> {
        let mut queries = Vec::new();
        for term in Self::extract_quoted_terms(task) {
            Self::push_unique(&mut queries, term);
        }
        if let Some(url) = Self::first_url(task) {
            Self::push_unique(&mut queries, url);
        }
        Self::push_unique(&mut queries, task.trim().to_string());
        queries
    }

    fn summarize_hybrid_knowledge_results(
        search_engine: &HybridSearchEngine,
        results: &[benshu_engram::HybridSearchResult],
    ) -> String {
        if results.is_empty() {
            return "No results found.".to_string();
        }

        let store = search_engine.engram_store();
        results
            .iter()
            .enumerate()
            .map(|(index, result)| {
                let doc = &result.document;
                let content =
                    store.get_content(doc).ok().flatten().unwrap_or_else(|| {
                        doc.summary.clone().unwrap_or_else(|| doc.title.clone())
                    });
                let source = doc
                    .metadata
                    .get("source_url")
                    .map(|url| format!("\nsource_url: {}", url))
                    .unwrap_or_default();
                format!(
                    "Result {}:\ntitle: {}\ncollection: {}\npath: {}\nscore: {:.4}{}\ncontent:\n{}",
                    index + 1,
                    doc.title,
                    doc.collection,
                    doc.path,
                    result.rrf_score,
                    source,
                    knowledge_snippet_text(&content, 1_600)
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n---\n\n")
    }

    fn summarize_engram_documents(
        search_engine: &HybridSearchEngine,
        docs: &[benshu_engram::prelude::Document],
    ) -> String {
        if docs.is_empty() {
            return "No results found.".to_string();
        }

        let store = search_engine.engram_store();
        docs.iter()
            .enumerate()
            .map(|(index, doc)| {
                let content =
                    store.get_content(doc).ok().flatten().unwrap_or_else(|| {
                        doc.summary.clone().unwrap_or_else(|| doc.title.clone())
                    });
                let source = doc
                    .metadata
                    .get("source_url")
                    .map(|url| format!("\nsource_url: {}", url))
                    .unwrap_or_default();
                format!(
                    "Result {}:\ntitle: {}\ncollection: {}\npath: {}{}\ncontent:\n{}",
                    index + 1,
                    doc.title,
                    doc.collection,
                    doc.path,
                    source,
                    knowledge_snippet_text(&content, 1_600)
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n---\n\n")
    }

    fn exact_knowledge_documents(
        search_engine: &HybridSearchEngine,
        terms: &[String],
    ) -> anyhow::Result<Vec<benshu_engram::prelude::Document>> {
        if terms.is_empty() {
            return Ok(Vec::new());
        }

        let normalized_terms = terms
            .iter()
            .map(|term| term.trim().to_lowercase())
            .filter(|term| !term.is_empty())
            .collect::<Vec<_>>();
        if normalized_terms.is_empty() {
            return Ok(Vec::new());
        }

        let docs = search_engine.list_documents()?;
        let mut matches = Vec::new();
        for doc in docs {
            let source_url = doc.metadata.get("source_url").cloned().unwrap_or_default();
            let haystacks = [
                doc.title.to_lowercase(),
                doc.collection.to_lowercase(),
                doc.path.to_lowercase(),
                doc.docid.to_lowercase(),
                source_url.to_lowercase(),
            ];
            if normalized_terms
                .iter()
                .any(|term| haystacks.iter().any(|value| value.contains(term)))
            {
                matches.push(doc);
            }
        }
        matches.sort_by(|left, right| right.updated_at_ms.cmp(&left.updated_at_ms));
        matches.truncate(5);
        Ok(matches)
    }

    fn push_unique(target: &mut Vec<String>, value: impl Into<String>) {
        SearchPolicy::push_unique(target, value);
    }

    async fn write_local_file_for_delegate(
        &self,
        task: &str,
        worker_label: &str,
    ) -> anyhow::Result<Option<String>> {
        let Some(path) = Self::extract_write_target_path(task)
            .or_else(|| Self::default_generated_artifact_path(task))
        else {
            return Ok(None);
        };
        let current_dir = std::env::current_dir()?;
        if let Some(blocker) = Self::evidence_quality_blocker_for_file_artifact(task) {
            return Ok(Some(format!(
                "status: blocked\nworker: {worker_label}\nexecuted_tool: write_file\npath: {}\nblockers: {}",
                path, blocker
            )));
        }
        let coordinator = self
            .coordinator
            .upgrade()
            .ok_or_else(|| anyhow::anyhow!("Coordinator has been dropped"))?;
        let artifact_role = Self::resolve_target_role_for_task(&coordinator, worker_label, task);
        let quality_contract = Self::artifact_quality_contract_for_coordinator(&coordinator, task);
        let agent = coordinator
            .get_or_spawn(&artifact_role)
            .await
            .ok_or_else(|| anyhow::anyhow!("No agent registered for role: {:?}", artifact_role))?;
        let prompt = Self::build_delegated_file_artifact_prompt_with_contract(
            task,
            &path,
            &quality_contract,
        );
        let generated = agent.generate_text_only(&prompt).await?;
        let mut content = Self::sanitize_generated_file_artifact(&generated, task);
        let mut quality =
            Self::artifact_quality_report_with_contract(task, &content, &quality_contract);
        let mut revision_attempts = 0usize;
        for attempt in 1..=2 {
            if quality.passed || !quality.should_attempt_revision() {
                break;
            }
            revision_attempts = attempt;
            let revision_prompt = Self::build_delegated_file_artifact_revision_prompt(
                task, &path, &content, &quality, attempt,
            );
            let revised = agent.generate_text_only(&revision_prompt).await?;
            content = Self::sanitize_generated_file_artifact(&revised, task);
            quality =
                Self::artifact_quality_report_with_contract(task, &content, &quality_contract);
        }
        let quality_summary = format!(
            "{}\nquality_revision_attempts: {revision_attempts}",
            quality.to_tool_result_section()
        );
        if !quality.passed {
            let executed_tool = if path.to_ascii_lowercase().ends_with(".pdf") {
                "pdf_build"
            } else {
                "write_file"
            };
            return Ok(Some(format!(
                "status: blocked\nworker: {worker_label}\nexecuted_tool: {executed_tool}\npath: {}\n{}\nblockers: artifact quality contract failed",
                path, quality_summary
            )));
        }
        if path.to_ascii_lowercase().ends_with(".pdf") {
            let safe_path = if Path::new(&path).is_absolute() {
                PathBuf::from(&path)
            } else {
                current_dir.join(&path)
            };
            let bytes = Self::write_pdf_text_artifact(&safe_path, &content)?;
            return Ok(Some(format!(
                "status: completed\nworker: {worker_label}\nexecuted_tool: pdf_build\npath: {}\nruntime_effect: artifact.written\nruntime_effect: artifact.pdf\n{}\nresult:\nSuccessfully wrote PDF document with {} bytes to {}",
                path, quality_summary, bytes, path
            )));
        }
        let output = WriteFileTool::new(current_dir)
            .call(
                &json!({
                    "path": path,
                    "content": content
                })
                .to_string(),
            )
            .await?;
        Ok(Some(format!(
            "status: completed\nworker: {worker_label}\nexecuted_tool: write_file\npath: {}\nruntime_effect: artifact.written\n{}\nresult:\n{}",
            path, quality_summary, output
        )))
    }

    fn path_is_inside(base: &Path, candidate: &Path) -> bool {
        let Ok(base) = base.canonicalize() else {
            return false;
        };
        if let Ok(candidate) = candidate.canonicalize() {
            return candidate.starts_with(&base);
        }

        let mut existing_parent = candidate.parent();
        while let Some(parent) = existing_parent {
            if let Ok(parent) = parent.canonicalize() {
                return parent.starts_with(&base);
            }
            existing_parent = parent.parent();
        }

        false
    }

    fn workspace_boundary_blocker(
        worker_label: &str,
        executed_tool: &str,
        path: &Path,
        workspace_root: &Path,
    ) -> String {
        format!(
            "status: blocked\nworker: {worker_label}\nexecuted_tool: {executed_tool}\nblockers: requested path is outside the current BenShu workspace\npath: {}\nworkspace_root: {}\nrecovery_hint: retry only with a path inside workspace_root or an explicitly trusted workspace; do not infer hidden sibling directories from diagnostic text",
            path.display(),
            workspace_root.display()
        )
    }

    fn read_local_file_for_delegate(
        task: &str,
        worker_label: &str,
    ) -> anyhow::Result<Option<String>> {
        let Some(path) = Self::extract_local_path(task) else {
            return Ok(None);
        };
        let current_dir = std::env::current_dir()?;
        if !Self::path_is_inside(&current_dir, &path) {
            return Ok(Some(Self::workspace_boundary_blocker(
                worker_label,
                "read_file",
                &path,
                &current_dir,
            )));
        }

        let metadata = std::fs::metadata(&path)?;
        if metadata.is_dir() {
            return Ok(None);
        }
        if !metadata.is_file() {
            return Ok(Some(format!(
                "status: blocked\nworker: {worker_label}\nexecuted_tool: read_file\nblockers: requested path is not a file\npath: {}",
                path.display()
            )));
        }
        if metadata.len() > 20 * 1024 * 1024 {
            return Ok(Some(format!(
                "status: blocked\nworker: {worker_label}\nexecuted_tool: read_file\nblockers: file is larger than the 20MB single-file safety limit\npath: {}\nsize_bytes: {}",
                path.display(),
                metadata.len()
            )));
        }

        let content = std::fs::read_to_string(&path)?;
        let snippet = ellipsize(&content, 16_000);
        Ok(Some(format!(
            "status: completed\nworker: {worker_label}\nexecuted_tool: read_file\npath: {}\nresult:\n{}",
            path.display(),
            snippet
        )))
    }

    fn summarize_lookup_blocker(error: &anyhow::Error) -> Option<&'static str> {
        let lowered = error.to_string().to_ascii_lowercase();

        if lowered.contains("challenge")
            || lowered.contains("captcha")
            || lowered.contains("anti-bot")
            || lowered.contains("cloudflare")
            || lowered.contains("turnstile")
        {
            return Some("external search was blocked by an anti-bot or challenge page");
        }

        if lowered.contains("no parsable search results")
            || lowered.contains("no parsable results")
            || lowered.contains("no relevant parsable results")
            || lowered.contains("landing/challenge page")
            || lowered.contains("temporarily blocked due to rate limiting")
        {
            return Some("external search returned no reliable parsable results");
        }

        None
    }

    fn extract_quoted_text(task: &str) -> Option<String> {
        for quote in ['"', '\'', '“', '”'] {
            let mut parts = task.split(quote);
            let _before = parts.next();
            if let Some(text) = parts.next() {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }
        None
    }

    fn extract_hash_text(task: &str) -> Option<String> {
        if let Some(text) = Self::extract_quoted_text(task) {
            return Some(text);
        }

        let lowered = task.to_ascii_lowercase();
        for marker in ["text ", "文本", "string "] {
            if let Some(index) = lowered.find(marker) {
                let candidate = task[index + marker.len()..]
                    .split(['.', '。', ',', '，', '\n'])
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .trim_matches(':')
                    .trim();
                if !candidate.is_empty() {
                    return Some(candidate.to_string());
                }
            }
        }

        None
    }

    fn extract_github_repo_shorthand(task: &str) -> Option<String> {
        for raw in task.split(|ch: char| {
            ch.is_whitespace()
                || matches!(
                    ch,
                    '"' | '\'' | '“' | '”' | '，' | '。' | ',' | ';' | '；' | ')' | '('
                )
        }) {
            let token = raw.trim().trim_matches(['.', ':', '：']);
            let parts: Vec<&str> = token.split('/').collect();
            if parts.len() == 2
                && parts[0]
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
                && parts[1]
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.')
            {
                return Some(format!("https://github.com/{token}"));
            }
        }
        None
    }

    fn extract_skill_name_for_management(task: &str) -> Option<String> {
        if let Some(quoted) = Self::extract_quoted_text(task) {
            return Some(quoted);
        }

        for marker in ["名为", "叫", "安装", "install", "skill"] {
            if let Some(index) = task.to_lowercase().find(marker) {
                let candidate = task[index + marker.len()..]
                    .split(|ch: char| {
                        ch.is_whitespace()
                            || matches!(ch, '，' | '。' | ',' | ';' | '；' | '.' | ':' | '：')
                    })
                    .find(|part| {
                        let trimmed = part.trim();
                        !trimmed.is_empty()
                            && trimmed
                                .chars()
                                .any(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
                    })
                    .map(|part| part.trim().trim_matches(['`', '"', '\'']).to_string());
                if candidate.as_deref().is_some_and(|value| !value.is_empty()) {
                    return candidate;
                }
            }
        }

        task.split(|ch: char| {
            ch.is_whitespace() || matches!(ch, '，' | '。' | ',' | ';' | '；' | '.' | ':' | '：')
        })
        .find(|part| {
            let trimmed = part.trim();
            trimmed.chars().any(|ch| ch.is_ascii_uppercase())
                && trimmed.chars().any(|ch| ch.is_ascii_lowercase())
        })
        .map(|part| part.trim().trim_matches(['`', '"', '\'']).to_string())
    }

    fn extract_inline_numbers(task: &str) -> Vec<serde_json::Value> {
        task.split(|ch: char| !(ch.is_ascii_digit() || ch == '.' || ch == '-'))
            .filter_map(|part| {
                let trimmed = part.trim();
                if trimmed.is_empty() || trimmed == "-" || trimmed == "." {
                    return None;
                }
                trimmed
                    .parse::<f64>()
                    .ok()
                    .and_then(serde_json::Number::from_f64)
                    .map(serde_json::Value::Number)
            })
            .collect()
    }

    fn is_skill_inventory_request(task: &str) -> bool {
        let lowered = task.to_ascii_lowercase();
        (task.contains("已安装")
            || task.contains("本地")
            || task.contains("列出")
            || task.contains("清单")
            || task.contains("有哪些")
            || lowered.contains("installed")
            || lowered.contains("local")
            || lowered.contains("inventory")
            || lowered.contains("list"))
            && (task.contains("skill")
                || task.contains("技能")
                || lowered.contains("skill")
                || lowered.contains("plugin"))
    }

    fn extract_chart_arguments(task: &str) -> Option<serde_json::Value> {
        let lowered = task.to_ascii_lowercase();
        if lowered.contains("info") || task.contains("能力") || task.contains("支持") {
            return Some(json!({ "action": "info" }));
        }

        let data = Self::extract_json_value_after_marker(task, "data=")
            .or_else(|| Self::extract_json_value_after_marker(task, "数据="))?;
        let chart_type = Self::extract_assignment_value(task, "chart_type")
            .or_else(|| Self::extract_assignment_value(task, "type"))
            .unwrap_or_else(|| "bar".to_string());
        let backend = Self::extract_assignment_value(task, "backend").unwrap_or_else(|| {
            if lowered.contains("svg") {
                "svg".to_string()
            } else {
                "svg".to_string()
            }
        });
        let title = Self::extract_assignment_value(task, "title")
            .or_else(|| Self::extract_assignment_value(task, "标题"))
            .unwrap_or_else(|| "BenShu chart".to_string());

        Some(json!({
            "action": "generate",
            "backend": backend,
            "chart_type": chart_type,
            "data": data,
            "title": title
        }))
    }

    fn extract_assignment_value(task: &str, key: &str) -> Option<String> {
        let marker = format!("{key}=");
        let start = task.find(&marker)? + marker.len();
        let rest = task[start..].trim_start();
        let end = rest
            .find(|ch: char| ch == '，' || ch == ',' || ch == '。' || ch == ';' || ch == '\n')
            .unwrap_or(rest.len());
        let value = rest[..end].trim().trim_matches(['`', '"', '\'']);
        if value.is_empty() {
            None
        } else {
            Some(value.to_string())
        }
    }

    fn extract_json_value_after_marker(task: &str, marker: &str) -> Option<serde_json::Value> {
        let start = task.find(marker)? + marker.len();
        let rest = task[start..].trim_start();
        let mut chars = rest.char_indices();
        let (_, first) = chars.find(|(_, ch)| !ch.is_whitespace())?;
        let (open, close) = match first {
            '{' => ('{', '}'),
            '[' => ('[', ']'),
            _ => return None,
        };
        let first_pos = rest.find(open)?;
        let mut depth = 0usize;
        let mut in_string = false;
        let mut escape = false;
        for (idx, ch) in rest[first_pos..].char_indices() {
            if in_string {
                if escape {
                    escape = false;
                } else if ch == '\\' {
                    escape = true;
                } else if ch == '"' {
                    in_string = false;
                }
                continue;
            }

            match ch {
                '"' => in_string = true,
                c if c == open => depth += 1,
                c if c == close => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        let end = first_pos + idx + ch.len_utf8();
                        return serde_json::from_str(&rest[first_pos..end]).ok();
                    }
                }
                _ => {}
            }
        }
        None
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct DelegateArgs {
    /// The role to delegate the task to (e.g., "researcher", "trader")
    role: String,
    /// The specific task or instruction for the sub-agent
    task: String,
    /// Optional fallback role used only when `role` is `auto` and no worker policy matches.
    #[serde(default)]
    fallback_role: Option<String>,
    /// Optional original user request carried by BenShu so workers can preserve constraints.
    #[serde(default)]
    full_user_request: Option<String>,
}

#[async_trait]
impl Tool for DelegateTool {
    fn name(&self) -> String {
        "delegate".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        let (role_names, worker_count, role_hint) =
            if let Some(coordinator) = self.coordinator.upgrade() {
                let mut workers = coordinator.worker_blueprints();
                workers.sort_by(|left, right| left.role.name().cmp(right.role.name()));

                let role_names: Vec<String> = workers
                    .iter()
                    .map(|worker| worker.role.name().to_string())
                    .collect();
                let role_hint = if role_names.is_empty() {
                    "No specialist roles are currently registered.".to_string()
                } else {
                    let role_summaries = workers
                        .iter()
                        .map(|worker| {
                            let role = worker.role.name().to_string();
                            let capabilities =
                                Self::artifact_policy_capabilities(&worker.artifact_policy);
                            if capabilities.is_empty() {
                                role
                            } else {
                                format!("{role} [{}]", capabilities.join(", "))
                            }
                        })
                        .collect::<Vec<_>>();
                    format!(
                        "Known specialist roles right now: {}.",
                        role_summaries.join(", ")
                    )
                };
                (role_names, workers.len(), role_hint)
            } else {
                (
                    Vec::new(),
                    0,
                    "No specialist roles are currently registered.".to_string(),
                )
            };
        let mut role_options = role_names.clone();
        if !role_options.iter().any(|role| role == "auto") {
            role_options.push("auto".to_string());
        }
        let role_ts = if role_options.is_empty() {
            "string".to_string()
        } else {
            role_options
                .iter()
                .map(|role| format!("{role:?}"))
                .collect::<Vec<_>>()
                .join(" | ")
        };

        let mut role_property = serde_json::json!({
            "type": "string",
            "description": format!(
                "The target specialist role to delegate the task to. Use one of the registered role names exactly when available, or `auto` when the runtime should select from worker policy and equipped tools. {} Discover the best specialist with `tool_search` first when the role is not obvious.",
                role_hint
            )
        });
        if !role_options.is_empty() {
            role_property["enum"] = serde_json::Value::Array(
                role_options
                    .iter()
                    .cloned()
                    .map(serde_json::Value::String)
                    .collect(),
            );
        }

        ToolDefinition {
            name: self.name(),
            description: format!(
                "Delegate a sub-task to another specialized agent role. Prefer the narrowest matching worker instead of broad decomposition when a clear specialist exists. Registered specialist count: {}. {} Use `auto` for policy-indexed worker selection when the role is not clear, or call `tool_search` first for explicit discovery.",
                worker_count, role_hint
            ),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "role": role_property,
                    "task": {
                        "type": "string",
                        "description": "The specific instruction for the delegated agent"
                    },
                    "fallback_role": {
                        "type": "string",
                        "description": "Optional fallback registered role to use only when role is `auto` and runtime policy produces no worker match."
                    },
                    "full_user_request": {
                        "type": "string",
                        "description": "Optional original user request. Include it when the delegated task is only one step of a larger request so the worker preserves explicit constraints."
                    }
                },
                "required": ["role", "task"]
            }),
            parameters_ts: Some(format!(
                "interface DelegateArgs {{\n  role: {}; \n  task: string; // Instructions for the specialist worker\n  fallback_role?: string; // Used only when role is auto and no policy match exists\n  full_user_request?: string; // Original user request for constraint preservation\n}}",
                role_ts
            )),
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some("Use this when a task can be performed more efficiently by a specialized worker. Prefer a direct narrow specialist first; only fall back to broad decomposition when no single worker cleanly fits. If the right role is unclear, call `tool_search` before delegating. Specify the task clearly.".to_string()),
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let args = Self::parse_delegate_args(arguments)?;

        let coordinator = self
            .coordinator
            .upgrade()
            .ok_or_else(|| anyhow::anyhow!("Coordinator has been dropped"))?;

        let sanitized_task = Self::strip_unrequested_source_use_constraints(
            &args.task,
            args.full_user_request.as_deref(),
        );
        let constraint_task =
            Self::task_with_constraint_source(&sanitized_task, args.full_user_request.as_deref());
        let explicit_role = args.role.trim();
        let auto_role_requested = explicit_role.is_empty()
            || matches!(
                explicit_role.to_ascii_lowercase().as_str(),
                "auto" | "worker" | "specialist"
            );
        let mut requested_role = if auto_role_requested {
            Self::explicit_worker_role_from_task(&coordinator, &constraint_task)
                .or_else(|| {
                    if Self::task_requests_local_git(&constraint_task) {
                        Some("repo".to_string())
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| "auto".to_string())
        } else {
            args.role.clone()
        };
        if Self::should_rewrite_requested_role_for_local_continuation(
            auto_role_requested,
            &requested_role,
            &constraint_task,
        ) {
            requested_role = "writer".to_string();
        }
        let role = if requested_role == "auto" {
            coordinator
                .best_worker_capability_match(Some("auto"), &constraint_task)
                .map(|candidate| candidate.role)
                .or_else(|| {
                    args.fallback_role.as_deref().map(|fallback| {
                        Self::resolve_target_role_for_task(&coordinator, fallback, &constraint_task)
                    })
                })
                .unwrap_or_else(|| AgentRole::Custom("researcher".to_string()))
        } else {
            Self::resolve_target_role_for_task(&coordinator, &requested_role, &constraint_task)
        };
        let routing_receipt =
            Self::delegate_routing_receipt(&coordinator, &requested_role, &constraint_task, &role);
        tracing::info!(
            target: "benshu.routing",
            selected_source = routing_receipt
                .get("selected_source")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("fallback"),
            requested_role = requested_role.as_str(),
            selected_role = role.name(),
            candidate_count = routing_receipt
                .get("candidate_count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0),
            "delegate worker capability route selected"
        );
        let worker_blueprint = coordinator.worker_blueprint(&role);
        let blueprint_tools = worker_blueprint
            .as_ref()
            .map(|blueprint| blueprint.tools.clone())
            .unwrap_or_default();
        let blueprint_artifact_policy = worker_blueprint
            .as_ref()
            .and_then(|blueprint| blueprint.artifact_policy.as_ref());

        if Self::role_is_writing_owner(&role, &blueprint_tools)
            && !Self::worker_has_external_acquisition_tools(&blueprint_tools)
            && Self::task_requires_external_acquisition_before_artifact(&constraint_task)
            && !Self::task_has_verified_acquisition_evidence(&constraint_task)
        {
            let suggested_role = Self::suggested_external_acquisition_role(&coordinator, &role);
            return Ok(Self::artifact_owner_phase_boundary_result(
                &role,
                suggested_role,
                &constraint_task,
            ));
        }

        let (fast_task_id, fast_session_id) = Self::current_runtime_task_refs();
        let fast_role_name = role.name().to_string();
        Self::record_delegate_checkpoint(
            self.task_manager.clone(),
            self.runtime_event_manager.clone(),
            fast_task_id,
            fast_session_id.clone(),
            &fast_role_name,
            &format!("worker:{fast_role_name}:fast_path:start"),
            format!(
                "Worker `{fast_role_name}` is checking direct execution paths. Routing receipt: {}. Task preview: {}",
                preview_text(&routing_receipt.to_string(), 420),
                preview_text(&constraint_task, 260)
            ),
            "running",
        )
        .await;
        let (fast_stop_tx, mut fast_stop_rx) = tokio::sync::oneshot::channel::<()>();
        let fast_heartbeat = if (fast_task_id.is_some() || fast_session_id.is_some())
            && self.task_manager.is_some()
        {
            let heartbeat_task_manager = self.task_manager.clone();
            let heartbeat_event_manager = self.runtime_event_manager.clone();
            let heartbeat_session_id = fast_session_id.clone();
            let heartbeat_role = fast_role_name.clone();
            Some(tokio::spawn(async move {
                let started = Instant::now();
                loop {
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_secs(30)) => {
                            let elapsed = started.elapsed().as_secs();
                            Self::record_delegate_checkpoint(
                                heartbeat_task_manager.clone(),
                                heartbeat_event_manager.clone(),
                                fast_task_id,
                                heartbeat_session_id.clone(),
                                &heartbeat_role,
                                &format!("worker:{heartbeat_role}:fast_path:heartbeat"),
                                format!("Worker `{heartbeat_role}` is still running direct execution after {elapsed}s."),
                                "running",
                            ).await;
                        }
                        _ = &mut fast_stop_rx => {
                            break;
                        }
                    }
                }
            }))
        } else {
            None
        };
        let fast_path_budget_secs = Self::delegate_fast_path_budget_secs_for_task(&constraint_task);
        let supervised_fast_path =
            self.task_manager.is_some() && (fast_task_id.is_some() || fast_session_id.is_some());
        let managed_continuous_fast_path = Self::should_use_managed_continuous_fast_path(
            &role,
            &blueprint_tools,
            &constraint_task,
        );
        if supervised_fast_path {
            Self::record_delegate_checkpoint(
                self.task_manager.clone(),
                self.runtime_event_manager.clone(),
                fast_task_id,
                fast_session_id.clone(),
                &fast_role_name,
                &format!("worker:{fast_role_name}:fast_path:supervised"),
                format!(
                    "Worker `{fast_role_name}` is running under the background task supervisor; direct execution uses a configurable attempt budget, tool-level budgets, and heartbeat checkpoints before falling back to the normal worker loop."
                ),
                "running",
            )
            .await;
        } else if managed_continuous_fast_path {
            Self::record_delegate_checkpoint(
                self.task_manager.clone(),
                self.runtime_event_manager.clone(),
                fast_task_id,
                fast_session_id.clone(),
                &fast_role_name,
                &format!("worker:{fast_role_name}:fast_path:managed_continuous"),
                format!(
                    "Worker `{fast_role_name}` is running a supervised continuous artifact path; direct execution budget is not applied to the whole continuation."
                ),
                "running",
            )
            .await;
        }
        let routing_task = if let Some(full_user_request) = args.full_user_request.as_deref() {
            if role.name() == "writer" && !constraint_task.contains(full_user_request) {
                format!("{constraint_task}\n\nOriginal user request: {full_user_request}")
            } else {
                constraint_task.clone()
            }
        } else {
            constraint_task.clone()
        };
        let fast_path_result = if !Self::fast_path_uses_attempt_budget(
            supervised_fast_path,
            managed_continuous_fast_path,
        ) {
            Ok(self
                .try_fast_path(&role, &blueprint_tools, &routing_task)
                .await)
        } else {
            tokio::time::timeout(
                Duration::from_secs(fast_path_budget_secs),
                self.try_fast_path(&role, &blueprint_tools, &routing_task),
            )
            .await
        };
        let _ = fast_stop_tx.send(());
        if let Some(heartbeat) = fast_heartbeat {
            if let Err(error) = heartbeat.await {
                tracing::warn!(
                    "Delegate fast-path heartbeat task for {} failed to join: {}",
                    fast_role_name,
                    error
                );
            }
        }
        let mut fast_path_result = match fast_path_result {
            Ok(result) => result?,
            Err(_) => {
                Self::record_delegate_checkpoint(
                    self.task_manager.clone(),
                    self.runtime_event_manager.clone(),
                    fast_task_id,
                    fast_session_id.clone(),
                    &fast_role_name,
                    &format!("worker:{fast_role_name}:fast_path:timeout"),
                    format!(
                        "Worker `{fast_role_name}` direct execution attempt exceeded its configurable {}s budget; falling back to the normal worker loop.",
                        fast_path_budget_secs
                    ),
                    "running",
                )
                .await;
                None
            }
        };
        fast_path_result = fast_path_result.map(|result| {
            Self::guard_fast_path_completion_against_source_contract(&role, &routing_task, result)
        });
        if let Some(result) = fast_path_result.as_deref() {
            if Self::fast_path_blocker_should_fall_back(&role, &routing_task, result) {
                Self::record_delegate_checkpoint(
                    self.task_manager.clone(),
                    self.runtime_event_manager.clone(),
                    fast_task_id,
                    fast_session_id.clone(),
                    &fast_role_name,
                    &format!("worker:{fast_role_name}:fast_path:blocker_fallback"),
                    format!(
                        "Worker `{fast_role_name}` direct execution returned a recoverable blocker; falling back to the normal worker loop. Preview: {}",
                        preview_text(result, 500)
                    ),
                    "running",
                )
                .await;
                fast_path_result = None;
            }
        }
        if let Some(result) = fast_path_result {
            Self::record_delegate_checkpoint(
                self.task_manager.clone(),
                self.runtime_event_manager.clone(),
                fast_task_id,
                fast_session_id,
                &fast_role_name,
                &format!("worker:{fast_role_name}:fast_path:completed"),
                format!(
                    "Worker `{fast_role_name}` completed direct execution. Preview: {}",
                    preview_text(&result, 500)
                ),
                "completed",
            )
            .await;
            return Ok(result);
        }

        let agent = coordinator
            .get_or_spawn(&role)
            .await
            .ok_or_else(|| anyhow::anyhow!("No agent registered for role: {:?}", role))?;

        if role.name() == "writer" && constraint_task.contains("[BENSHU_NOVEL_CONTENT_OPERATION]") {
            let result =
                super::writing::novel_workflow_driver::run_novel_content_operation_for_delegate(
                    agent,
                    &constraint_task,
                    super::writing::novel_workflow_driver::NovelContentOperationConfig {
                        workspace: std::env::current_dir()?,
                        worker_label: role.name().to_string(),
                    },
                )
                .await?;
            Self::record_delegate_checkpoint(
                self.task_manager.clone(),
                self.runtime_event_manager.clone(),
                fast_task_id,
                fast_session_id,
                &fast_role_name,
                &format!("worker:{fast_role_name}:novel_content_operation:completed"),
                format!(
                    "Worker `{fast_role_name}` completed novel content operation. Preview: {}",
                    preview_text(&result, 500)
                ),
                if result.starts_with("status: completed") {
                    "completed"
                } else {
                    "blocked"
                },
            )
            .await;
            return Ok(result);
        }

        if role.name() == "writer" {
            let workflow_task = Self::governed_writing_workflow_task(
                &routing_task,
                args.full_user_request.as_deref(),
            );
            let existing_project_path =
                Self::extract_existing_artifact_project_path(&constraint_task)
                    .or_else(|| Self::extract_existing_artifact_project_path(workflow_task));
            if let Some(project_path) = existing_project_path.filter(|_| {
                super::writing::novel_workflow_driver::task_requests_novel_surface_cleanup(
                    workflow_task,
                )
            }) {
                let content_operation_task = format!(
                    "[BENSHU_NOVEL_CONTENT_OPERATION]\nproject_path: {project_path}\n操作类型：修改\n用户原话：项目级表面清理；移除非正文转义残片、Markdown/LaTeX残片和模型输出说明。\n原始请求：\n{workflow_task}"
                );
                let result =
                    super::writing::novel_workflow_driver::run_novel_content_operation_for_delegate(
                        agent,
                        &content_operation_task,
                        super::writing::novel_workflow_driver::NovelContentOperationConfig {
                            workspace: std::env::current_dir()?,
                            worker_label: role.name().to_string(),
                        },
                    )
                    .await?;
                Self::record_delegate_checkpoint(
                    self.task_manager.clone(),
                    self.runtime_event_manager.clone(),
                    fast_task_id,
                    fast_session_id,
                    &fast_role_name,
                    &format!("worker:{fast_role_name}:novel_content_operation:completed"),
                    format!(
                        "Worker `{fast_role_name}` completed existing project maintenance. Preview: {}",
                        preview_text(&result, 500)
                    ),
                    if result.starts_with("status: completed") {
                        "completed"
                    } else {
                        "blocked"
                    },
                )
                .await;
                return Ok(result);
            }
        }

        if Self::should_route_writer_fiction_to_novel_studio(&blueprint_tools, &routing_task) {
            let workflow_task = Self::governed_writing_workflow_task(
                &routing_task,
                args.full_user_request.as_deref(),
            );
            let workspace = std::env::current_dir()?;
            let policy_target_units = Self::artifact_policy_tool_config_usize(
                blueprint_artifact_policy,
                &["novel_studio", "novel", "writing"],
                "target_units",
            );
            let policy_chapter_unit_target = Self::artifact_policy_tool_config_usize(
                blueprint_artifact_policy,
                &["novel_studio", "novel", "writing"],
                "chapter_unit_target",
            );
            let requested_target_units = Self::requested_total_text_target_chars(workflow_task);
            let target_units = requested_target_units.or(policy_target_units);
            let requested_chapter_unit_target =
                Self::requested_chapter_unit_target_chars(workflow_task);
            let chapter_unit_target = requested_chapter_unit_target.or(policy_chapter_unit_target);
            let chapter_step_target =
                chapter_unit_target.unwrap_or_else(Self::longform_step_target_chars);
            let supervisor_task_id = if let Some(task_manager) = self.task_manager.as_ref() {
                Self::resolve_delegate_checkpoint_task_id(
                    task_manager,
                    fast_task_id,
                    fast_session_id.as_deref(),
                )
                .await
            } else {
                fast_task_id
            };
            let existing_project_path =
                Self::extract_existing_artifact_project_path(&constraint_task)
                    .or_else(|| Self::extract_existing_artifact_project_path(workflow_task));
            let creation_draft_path = Self::extract_creation_draft_path(&constraint_task)
                .or_else(|| Self::extract_creation_draft_path(workflow_task));
            let existing_project_path = if existing_project_path.is_some()
                || !Self::task_requests_existing_work_continuation(workflow_task)
            {
                existing_project_path
            } else {
                self.latest_session_project_path_for_delegate(
                    fast_session_id.as_deref(),
                    supervisor_task_id.or(fast_task_id),
                )
                .await
            };
            let result = super::writing::novel_workflow_driver::run_novel_workflow_for_delegate(
                agent,
                workflow_task,
                super::writing::novel_workflow_driver::NovelWorkflowConfig {
                    workspace,
                    worker_label: role.name().to_string(),
                    target_units,
                    chapter_unit_target,
                    chapter_count: Self::requested_chapter_count_with_step_target(
                        workflow_task,
                        chapter_step_target,
                    ),
                    requested_start_chapter: Self::requested_start_chapter(workflow_task),
                    existing_project_path,
                    creation_draft_path,
                    runtime: super::writing::novel_workflow_driver::NovelWorkflowRuntimeState {
                        task_id: supervisor_task_id,
                        task_manager: self.task_manager.clone(),
                        event_manager: self.runtime_event_manager.clone(),
                    },
                },
            )
            .await?;
            Self::record_delegate_checkpoint(
                self.task_manager.clone(),
                self.runtime_event_manager.clone(),
                supervisor_task_id.or(fast_task_id),
                fast_session_id,
                &fast_role_name,
                &format!("worker:{fast_role_name}:workflow_driver:completed"),
                format!(
                    "Worker `{fast_role_name}` completed governed workflow driver execution. Preview: {}",
                    preview_text(&result, 500)
                ),
                if result.starts_with("status: completed") {
                    "completed"
                } else {
                    "blocked"
                },
            )
            .await;
            return Ok(result);
        }

        let task = Self::build_worker_execution_contract_with_policy(
            &role,
            &blueprint_tools,
            blueprint_artifact_policy,
            &sanitized_task,
            args.full_user_request.as_deref(),
        );
        let result = self
            .run_worker_process_with_checkpoints(agent.clone(), &role, &task, "delegated_task")
            .await?;
        if Self::contains_unexecuted_pseudo_tool_call(&result) {
            let recovery_task = Self::build_worker_pseudo_tool_recovery_contract(
                &role,
                &blueprint_tools,
                &args.task,
                args.full_user_request.as_deref(),
                &result,
            );
            let retry_result = self
                .run_worker_process_with_checkpoints(
                    agent,
                    &role,
                    &recovery_task,
                    "pseudo_tool_recovery",
                )
                .await?;
            if Self::contains_unexecuted_pseudo_tool_call(&retry_result) {
                return Ok(format!(
                    "status: blocked\nworker: {}\nblockers: specialist returned an unexecuted pseudo tool call tag after a bounded recovery attempt; no result was accepted as completed",
                    role.name()
                ));
            }
            return Ok(retry_result);
        }

        Ok(result)
    }
}
