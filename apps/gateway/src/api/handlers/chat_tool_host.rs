use super::{explicit_existing_project_path_from_request, ChatResponse};
use crate::api::state::{AppError, AppState};
use anyhow::Context;
use async_trait::async_trait;
use axum::Json;
use benshu_brain::agent::message::Message as AgentMessage;
use benshu_brain::agent::protocol::{AgentRole, ChatOutcome};
use benshu_brain::skills::tool::Tool;
#[cfg(test)]
use benshu_builtin_tools::tool::writing::creation_contract::{
    build_initial_creation_draft, creation_intake_response,
};
use benshu_builtin_tools::tool::writing::creation_contract::{
    creation_draft_metadata_key, creation_draft_tool_args, handle_creation_draft_chat,
    infer_project_artifact_kind,
    maybe_repair_creation_planning_outcome as writing_maybe_repair_creation_planning_outcome,
    sync_creation_draft_from_approval, CreationContractRepairRuntime, CreationDraftLifecycleStatus,
    CreationDraftRuntime, CreationDraftTurnOutcome as WritingCreationDraftTurnOutcome,
    CreationDraftUserResponse, SessionCreationDraftState,
};
use benshu_builtin_tools::tool::writing::session_surface as writing_session_surface;
use benshu_builtin_tools::tool::{NovelStudioTool, WritingStudioTool};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::warn;
use uuid::Uuid;

const CREATION_CONTRACT_REPAIR_CALL_TIMEOUT_SECS: u64 = 240;

pub(super) enum CreationDraftChatOutcome {
    Respond(Json<ChatResponse>),
    ContinueWithMessage(String),
}

#[cfg(test)]
pub(super) fn try_handle_creation_intake_chat(message: &str) -> Option<Json<ChatResponse>> {
    creation_intake_response(message).map(|response| Json(chat_response_from_creation(response)))
}

pub(super) async fn try_handle_creation_draft_chat(
    state: &AppState,
    session_id: &str,
    message: &str,
) -> Result<Option<CreationDraftChatOutcome>, AppError> {
    let mut runtime = GatewayCreationDraftRuntime { state };
    let Some(outcome) = handle_creation_draft_chat(&mut runtime, session_id, message).await? else {
        return Ok(None);
    };
    Ok(Some(match outcome {
        WritingCreationDraftTurnOutcome::Respond(response) => {
            CreationDraftChatOutcome::Respond(Json(chat_response_from_creation(response)))
        }
        WritingCreationDraftTurnOutcome::ContinueWithMessage(message) => {
            CreationDraftChatOutcome::ContinueWithMessage(message)
        }
    }))
}

fn chat_response_from_creation(response: CreationDraftUserResponse) -> ChatResponse {
    ChatResponse {
        response: response.response,
        reasoning: None,
        tool_calls: None,
        artifacts: Vec::new(),
        chat_route: Some(response.chat_route),
        tool_surface_mode: Some(response.tool_surface_mode),
        runtime_persistence_status: Some(response.runtime_persistence_status),
        task_id: None,
        run_id: None,
        trace_id: None,
    }
}

pub(super) async fn load_session_creation_draft(
    state: &AppState,
    session_id: &str,
) -> anyhow::Result<Option<SessionCreationDraftState>> {
    let mut runtime = GatewayCreationDraftRuntime { state };
    runtime.load_draft(session_id).await
}

pub(super) async fn maybe_repair_creation_planning_outcome(
    state: &AppState,
    session_id: &str,
    _messages: &[AgentMessage],
    creation_planning_dialogue: bool,
    outcome: ChatOutcome,
    supervisor_task_id: Uuid,
) -> anyhow::Result<ChatOutcome> {
    let mut runtime = GatewayCreationDraftRuntime { state };
    writing_maybe_repair_creation_planning_outcome(
        &mut runtime,
        session_id,
        creation_planning_dialogue,
        outcome,
        supervisor_task_id,
    )
    .await
}

pub(super) async fn execute_creation_planning_dialogue_transient(
    state: &AppState,
    session_id: &str,
    planning_prompt: &str,
    supervisor_task_id: Uuid,
) -> anyhow::Result<ChatOutcome> {
    let primary_role = state.kernel.coordinator().primary_role();
    let response = if let Some(agent) = state.kernel.coordinator().get(&primary_role) {
        match agent
            .generate_text_only_with_max_tokens(planning_prompt, Some(4096))
            .await
        {
            Ok(text) => text,
            Err(error) if super::is_recoverable_provider_disconnect(&error.to_string()) => {
                format!(
                    "status: paused\n\
                     error_kind: provider_service_unavailable\n\
                     blockers: {error}\n\n\
                     合同生成遇到模型服务问题，已暂停在合同阶段，尚未开始写正文。"
                )
            }
            Err(error) => {
                format!(
                    "合同生成遇到模型服务问题，已暂停在合同阶段，尚未开始写正文。\n\n错误：{error}"
                )
            }
        }
    } else {
        format!(
            "合同生成无法找到主 agent：{}，尚未开始写正文。",
            primary_role.name()
        )
    };

    let outcome = ChatOutcome {
        response,
        thoughts: vec![
            "gateway ran creation-contract planning through transient text generation; internal prompt was not submitted to chat_session".to_string(),
        ],
        tool_calls: Vec::new(),
        metabolic_stats: None,
        ownership: benshu_protocol_core::TaskOwnership::direct(
            AgentRole::Custom("benshu".to_string()),
            Some(session_id.to_string()),
        ),
        delegation: None,
        handover: None,
        runtime_task: None,
        run_trace: None,
    };

    maybe_repair_creation_planning_outcome(
        state,
        session_id,
        &[],
        true,
        outcome,
        supervisor_task_id,
    )
    .await
}

pub(super) async fn creation_planning_background_response(
    state: &AppState,
    session_id: &str,
    supervisor_task_id: Uuid,
) -> Result<ChatResponse, AppError> {
    let (draft_text, next_action, ready) =
        match load_session_creation_draft(state, session_id).await {
        Ok(Some(draft)) => {
            let ready = writing_session_surface::creation_contract_draft_is_confirmable(&draft);
            let next_action = if ready {
                "如果看到的草案已经可以，请回复“开始写”或“按这个开始”。".to_string()
            } else {
                "这份草案还在自动补齐中；补齐并通过质量门前，不会开始写正文。你可以继续补充或修改要求。".to_string()
            };
            (
                writing_session_surface::stabilize_creation_contract_panel_response(&draft, ""),
                next_action,
                ready,
            )
        }
        Ok(None) => (
            "我已经进入写作合同草案阶段，但当前草案状态还没有落盘完成。完整草案生成后会写入本会话；你也可以继续补充要求。".to_string(),
            "当前没有可确认合同；完整合同补齐前不会开始写正文。".to_string(),
            false,
        ),
        Err(error) => {
            warn!(
                error = %error,
                session_id,
                "failed to load creation draft for background planning response"
            );
            (
                "我已经进入写作合同草案阶段，但读取当前草案状态时遇到临时错误。完整草案生成后会写入本会话；你也可以稍后说“显示当前合同草案”。".to_string(),
                "当前没有可确认合同；完整合同补齐前不会开始写正文。".to_string(),
                false,
            )
        }
    };

    let status_line = if ready {
        "创作合同已经通过质量门，尚未开始正文。"
    } else {
        "写作合同正在生成，我先展示当前需求摘要；这还不是可确认合同，也不会开始写正文。"
    };
    let response = format!("{status_line}\n\n{draft_text}\n\n{next_action}");
    if let Err(error) =
        save_creation_planning_provisional_result(state, supervisor_task_id, &response).await
    {
        warn!(
            error = %error,
            task_id = %supervisor_task_id,
            "failed to save provisional creation-contract result"
        );
    }

    Ok(ChatResponse {
        response,
        reasoning: None,
        tool_calls: None,
        artifacts: Vec::new(),
        chat_route: Some("coordinator::background_supervised.creation_planning".to_string()),
        tool_surface_mode: Some("creation_contract".to_string()),
        runtime_persistence_status: Some("background_running".to_string()),
        task_id: Some(supervisor_task_id),
        run_id: None,
        trace_id: None,
    })
}

async fn save_creation_planning_provisional_result(
    state: &AppState,
    supervisor_task_id: Uuid,
    response: &str,
) -> anyhow::Result<()> {
    let Some(mut task) = state
        .kernel
        .state_task()
        .load(&supervisor_task_id.to_string())
        .await?
    else {
        return Ok(());
    };
    if super::is_terminal_task_status(&task.status) || task.result.is_some() {
        return Ok(());
    }

    task.result = Some(serde_json::json!({
        "response_text": response,
        "thought_count": 0,
        "tool_call_count": 0,
        "handover": null,
        "delegation": null,
        "provider_disconnect_reason": null,
        "creation_contract": writing_session_surface::creation_contract_panel_payload(
            "running",
            response,
            true
        )
    }));
    task.updated_at = chrono::Utc::now();
    state.kernel.state_task().save(task).await?;
    Ok(())
}

pub(super) async fn creation_contract_task_status_from_session_draft(
    state: &AppState,
    session_id: &str,
) -> Option<benshu_state::TaskStatus> {
    match load_session_creation_draft(state, session_id).await {
        Ok(draft) => {
            writing_session_surface::creation_contract_status_for_draft(draft.as_ref(), None)
        }
        Err(error) => writing_session_surface::creation_contract_status_for_draft(
            None,
            Some(&error.to_string()),
        ),
    }
}

pub(super) async fn creation_contract_lifecycle_status_from_session_draft(
    state: &AppState,
    session_id: &str,
) -> Option<String> {
    load_session_creation_draft(state, session_id)
        .await
        .ok()
        .flatten()
        .map(|draft| writing_session_surface::creation_contract_panel_status_for_draft(&draft))
}

struct GatewayCreationDraftRuntime<'a> {
    state: &'a AppState,
}

#[async_trait]
impl CreationDraftRuntime for GatewayCreationDraftRuntime<'_> {
    async fn load_draft(
        &mut self,
        session_id: &str,
    ) -> anyhow::Result<Option<SessionCreationDraftState>> {
        let path = creation_draft_session_state_path(self.state, session_id);
        if let Ok(raw) = tokio::fs::read_to_string(&path).await {
            if let Some(mut draft) = parse_loadable_session_creation_draft(session_id, &raw) {
                let mut raw = raw;
                if self.refresh_draft_from_tool_status(&mut draft).await? {
                    raw = serde_json::to_string(&draft)?;
                    write_creation_draft_session_state(self.state, session_id, &raw).await?;
                }
                if let Some(memory) = self.state.kernel.coordinator().memory.get() {
                    memory
                        .set_metadata(&creation_draft_metadata_key(session_id), &raw)
                        .await?;
                }
                return Ok(Some(draft));
            }
        }

        if let Some(memory) = self.state.kernel.coordinator().memory.get() {
            if let Some(raw) = memory
                .get_metadata(&creation_draft_metadata_key(session_id))
                .await?
            {
                if let Some(mut draft) = parse_loadable_session_creation_draft(session_id, &raw) {
                    if self.refresh_draft_from_tool_status(&mut draft).await? {
                        let refreshed_raw = serde_json::to_string(&draft)?;
                        write_creation_draft_session_state(self.state, session_id, &refreshed_raw)
                            .await?;
                        memory
                            .set_metadata(&creation_draft_metadata_key(session_id), &refreshed_raw)
                            .await?;
                    }
                    return Ok(Some(draft));
                }
            }
        }

        Ok(None)
    }

    async fn save_draft(&mut self, draft: &SessionCreationDraftState) -> anyhow::Result<()> {
        let raw = serde_json::to_string(draft)?;
        write_creation_draft_session_state(self.state, &draft.session_id, &raw).await?;
        if let Some(memory) = self.state.kernel.coordinator().memory.get() {
            memory
                .set_metadata(&creation_draft_metadata_key(&draft.session_id), &raw)
                .await?;
        }
        self.update_draft(draft).await?;
        Ok(())
    }

    async fn clear_draft(&mut self, session_id: &str) -> anyhow::Result<()> {
        let raw = serde_json::json!({
            "schema_version": "benshu.writing.creation_draft.v1",
            "session_id": session_id,
            "status": "cleared",
            "updated_at": chrono::Utc::now().to_rfc3339(),
        })
        .to_string();
        write_creation_draft_session_state(self.state, session_id, &raw).await?;
        if let Some(memory) = self.state.kernel.coordinator().memory.get() {
            memory
                .set_metadata(&creation_draft_metadata_key(session_id), &raw)
                .await?;
        }
        Ok(())
    }

    async fn create_draft(&mut self, draft: &mut SessionCreationDraftState) -> anyhow::Result<()> {
        let args = creation_draft_tool_args("draft", draft);
        let result = self.call_creation_draft_tool(draft, args).await?;
        if let Some(path) = result.get("draft_path").and_then(|value| value.as_str()) {
            draft.draft_path = path.to_string();
        }
        Ok(())
    }

    async fn update_draft(&mut self, draft: &SessionCreationDraftState) -> anyhow::Result<()> {
        if draft.draft_path.trim().is_empty() || draft.is_approved() {
            return Ok(());
        }
        if tokio::fs::metadata(&draft.draft_path).await.is_err() {
            return Ok(());
        }
        let args = creation_draft_tool_args("update", draft);
        let _ = self.call_creation_draft_tool(draft, args).await?;
        Ok(())
    }

    async fn approve_draft(&mut self, draft: &SessionCreationDraftState) -> anyhow::Result<Value> {
        let args = creation_draft_tool_args("approve", draft);
        self.call_creation_draft_tool(draft, args).await
    }

    async fn approved_draft_for_existing_project(
        &mut self,
        session_id: &str,
        draft: &mut SessionCreationDraftState,
    ) -> anyhow::Result<Value> {
        if let Some(project_path) = self.existing_project_path(session_id, draft).await? {
            draft.project_path = project_path.clone();
            return self
                .call_creation_draft_tool(
                    draft,
                    serde_json::json!({
                        "action": "status",
                        "project_path": project_path,
                        "include_draft": true
                    }),
                )
                .await;
        }

        self.approve_draft(draft).await
    }

    async fn discard_draft(&mut self, draft: &SessionCreationDraftState) -> anyhow::Result<()> {
        if draft.draft_path.trim().is_empty() {
            return Ok(());
        }
        let args = creation_draft_tool_args("discard", draft);
        let _ = self.call_creation_draft_tool(draft, args).await?;
        Ok(())
    }

    async fn existing_project_path(
        &mut self,
        session_id: &str,
        draft: &SessionCreationDraftState,
    ) -> anyhow::Result<Option<String>> {
        let project_path = draft.project_path.trim();
        if !project_path.is_empty() && writing_project_path_exists(project_path).await {
            return Ok(Some(project_path.to_string()));
        }

        let tasks = match self
            .state
            .kernel
            .state_task()
            .list_by_session(session_id)
            .await
        {
            Ok(tasks) => tasks,
            Err(error) => {
                warn!("Creation draft task lookup failed: {}", error);
                return Ok(None);
            }
        };
        let Some(project_path) = writing_session_surface::latest_project_path_from_tasks(&tasks)
        else {
            return Ok(None);
        };
        if writing_project_path_exists(&project_path).await {
            Ok(Some(project_path))
        } else {
            Ok(None)
        }
    }

    async fn existing_project_path_for_continuation_message(
        &mut self,
        session_id: &str,
        message: &str,
    ) -> anyhow::Result<Option<String>> {
        if let Some(project_path) = explicit_existing_project_path_from_request(self.state, message)
            .await
            .map_err(|error| anyhow::anyhow!("{}", error.0))?
        {
            return Ok(Some(project_path));
        }

        let tasks = match self
            .state
            .kernel
            .state_task()
            .list_by_session(session_id)
            .await
        {
            Ok(tasks) => tasks,
            Err(error) => {
                warn!("Creation continuation task lookup failed: {}", error);
                return Ok(None);
            }
        };
        let Some(project_path) = writing_session_surface::latest_project_path_from_tasks(&tasks)
        else {
            return Ok(None);
        };
        if writing_project_path_exists(&project_path).await {
            Ok(Some(project_path))
        } else {
            Ok(None)
        }
    }

    async fn existing_project_artifact_kind(
        &mut self,
        project_path: &str,
    ) -> anyhow::Result<String> {
        infer_project_artifact_kind(project_path).await
    }
}

#[async_trait]
impl CreationContractRepairRuntime for GatewayCreationDraftRuntime<'_> {
    async fn generate_creation_contract_repair_text(
        &mut self,
        supervisor_task_id: Uuid,
        failure_label: &str,
        repair_prompt: &str,
        max_tokens: Option<u64>,
    ) -> anyhow::Result<Option<String>> {
        let primary_role = self.state.kernel.coordinator().primary_role();
        let Some(agent) = self.state.kernel.coordinator().get(&primary_role) else {
            self.record_creation_contract_checkpoint(
                supervisor_task_id,
                failure_label,
                Some(format!(
                    "合同内部修复无法找到主 agent：{}",
                    primary_role.name()
                )),
            )
            .await?;
            return Ok(None);
        };
        let generation = tokio::time::timeout(
            Duration::from_secs(CREATION_CONTRACT_REPAIR_CALL_TIMEOUT_SECS),
            agent.generate_text_only_with_max_tokens(repair_prompt, max_tokens),
        )
        .await;
        match generation {
            Err(_) => {
                self.record_creation_contract_checkpoint(
                    supervisor_task_id,
                    failure_label,
                    Some(format!(
                        "合同内部无工具修复调用超过 {} 秒仍未返回，停止本轮等待",
                        CREATION_CONTRACT_REPAIR_CALL_TIMEOUT_SECS
                    )),
                )
                .await?;
                Ok(None)
            }
            Ok(Ok(text)) => Ok(Some(text)),
            Ok(Err(error)) => {
                self.record_creation_contract_checkpoint(
                    supervisor_task_id,
                    failure_label,
                    Some(format!("合同内部无工具修复调用失败：{error}")),
                )
                .await?;
                if super::is_recoverable_provider_disconnect(&error.to_string()) {
                    return Err(error.into());
                }
                Ok(None)
            }
        }
    }

    async fn record_creation_contract_checkpoint(
        &mut self,
        supervisor_task_id: Uuid,
        label: &str,
        detail: Option<String>,
    ) -> anyhow::Result<()> {
        super::record_supervisor_checkpoint(self.state, supervisor_task_id, label, detail).await
    }
}

fn parse_loadable_session_creation_draft(
    session_id: &str,
    raw: &str,
) -> Option<SessionCreationDraftState> {
    let Ok(draft) = serde_json::from_str::<SessionCreationDraftState>(raw) else {
        return None;
    };
    if draft.session_id != session_id {
        warn!(
            "Ignoring creation draft state for mismatched session: requested={} draft={}",
            session_id, draft.session_id
        );
        return None;
    }
    draft.lifecycle_status().is_loadable().then_some(draft)
}

async fn write_creation_draft_session_state(
    state: &AppState,
    session_id: &str,
    raw: &str,
) -> anyhow::Result<()> {
    let path = creation_draft_session_state_path(state, session_id);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(path, raw).await?;
    Ok(())
}

fn creation_draft_session_state_path(state: &AppState, session_id: &str) -> PathBuf {
    chat_tool_data_dir(state)
        .join("generated")
        .join("writing_session_drafts")
        .join(format!(
            "{}.json",
            sanitize_creation_draft_session_file_stem(session_id)
        ))
}

fn chat_tool_data_dir(state: &AppState) -> &Path {
    state.config_path.parent().unwrap_or_else(|| Path::new("."))
}

fn sanitize_creation_draft_session_file_stem(session_id: &str) -> String {
    let mut stem = session_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    while stem.contains("..") {
        stem = stem.replace("..", ".");
    }
    let stem = stem.trim_matches(['.', '_', '-']);
    if stem.is_empty() {
        Uuid::new_v4().to_string()
    } else {
        stem.chars().take(160).collect()
    }
}

async fn writing_project_path_exists(project_path: &str) -> bool {
    let path = Path::new(project_path);
    let Ok(metadata) = tokio::fs::metadata(path).await else {
        return false;
    };
    if !metadata.is_dir() {
        return false;
    }
    tokio::fs::metadata(path.join("project.json")).await.is_ok()
}

impl GatewayCreationDraftRuntime<'_> {
    async fn refresh_draft_from_tool_status(
        &self,
        draft: &mut SessionCreationDraftState,
    ) -> anyhow::Result<bool> {
        if active_session_contract_revision(draft) {
            return Ok(false);
        }
        if draft.draft_path.trim().is_empty() {
            return Ok(false);
        }
        if tokio::fs::metadata(&draft.draft_path).await.is_err() {
            return Ok(false);
        }
        let status = self
            .call_creation_draft_tool(
                draft,
                serde_json::json!({
                    "action": "show_draft",
                    "draft_path": draft.draft_path,
                    "include_draft": true
                }),
            )
            .await?;
        let local_was_confirmable =
            writing_session_surface::creation_contract_draft_is_confirmable(draft);
        let mut refreshed = draft.clone();
        let changed = sync_creation_draft_from_approval(&mut refreshed, &status);
        if local_was_confirmable
            && !writing_session_surface::creation_contract_draft_is_confirmable(&refreshed)
        {
            return Ok(false);
        }
        *draft = refreshed;
        Ok(changed)
    }

    async fn call_creation_draft_tool(
        &self,
        draft: &SessionCreationDraftState,
        args: Value,
    ) -> anyhow::Result<Value> {
        let raw = if draft.tool_name == "novel_studio" {
            let workspace = self.creation_tool_workspace();
            let tool = NovelStudioTool::new(workspace.clone(), "writer");
            tool.call(&args.to_string()).await.with_context(|| {
                format!(
                    "novel_studio creation draft action failed; workspace={}; args={}",
                    workspace.display(),
                    args
                )
            })?
        } else {
            let workspace = self.creation_tool_workspace();
            let tool = WritingStudioTool::new(workspace.clone(), "writer");
            tool.call(&args.to_string()).await.with_context(|| {
                format!(
                    "writing_studio creation draft action failed; workspace={}; args={}",
                    workspace.display(),
                    args
                )
            })?
        };
        Ok(serde_json::from_str(&raw).unwrap_or_else(|_| serde_json::json!({ "raw": raw })))
    }

    fn creation_tool_workspace(&self) -> std::path::PathBuf {
        self.state
            .config_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf()
    }
}

fn active_session_contract_revision(draft: &SessionCreationDraftState) -> bool {
    matches!(
        draft.lifecycle_status(),
        CreationDraftLifecycleStatus::DraftingContract | CreationDraftLifecycleStatus::Blocked
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn thin_creation_request_gets_intake_gate_without_artifact_requirement() {
        let gate = super::try_handle_creation_intake_chat("帮我写小说").expect("intake response");
        assert_eq!(
            gate.0.chat_route.as_deref(),
            Some("coordinator::creation_intake")
        );
        assert_eq!(gate.0.tool_surface_mode.as_deref(), Some("fiction"));
        assert!(gate.0.response.contains("你来定"));

        let contract =
            super::super::build_chat_task_contract(&[benshu_brain::agent::message::Message::user(
                benshu_brain::agent::message::Content::text("帮我写小说"),
            )]);
        assert!(contract.required_events.is_empty());
    }

    #[test]
    fn specified_creation_request_skips_intake_gate() {
        assert!(super::try_handle_creation_intake_chat("帮我写一个草根逆袭的玄幻小说").is_none());
        assert!(super::try_handle_creation_intake_chat("帮我写小说，你来定").is_none());
    }

    #[test]
    fn active_session_contract_revision_owns_draft_over_tool_mirror() {
        let mut draft = super::build_initial_creation_draft("session", "fiction", "写一部历史小说")
            .expect("draft");
        draft.title = "会话中的新合同".to_string();
        draft.set_lifecycle_status(super::CreationDraftLifecycleStatus::DraftingContract);

        assert!(super::active_session_contract_revision(&draft));
        assert_eq!(draft.title, "会话中的新合同");
        assert!(draft.can_accept_contract_candidate());
    }
}
