//! Session-facing writing project helpers.
//!
//! Gateway owns HTTP/session/task plumbing. This module owns the writing-domain
//! interpretation of saved novel projects, chapter references, and read-only
//! project summaries.
//!
//! Boundary rule: this module renders writing state for chat/panel surfaces.
//! It must not start writing, repair contracts, approve drafts, or mutate
//! project/chapter state.

use benshu_compression::preview_text;
use benshu_state::{ArtifactRecord, TaskState, TaskStatus};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

const MAX_ARTIFACT_PREVIEW_BYTES: u64 = 2 * 1024 * 1024;

pub const CREATION_CONTRACT_QUALITY_BLOCKED_METADATA_KEY: &str =
    crate::tool::writing::creation_contract::CREATION_CONTRACT_QUALITY_BLOCKED_METADATA_KEY;

pub fn longform_progress_report_contract() -> Value {
    serde_json::json!({
        "chat_should_include": [
            "status",
            "chapter_number",
            "chapter_title",
            "saved_path_when_available",
            "unit_count_when_available",
            "brief_summary",
            "quality_gate_status"
        ],
        "chat_should_not_include": [
            "complete long-form body",
            "raw source dumps",
            "internal prompt text"
        ]
    })
}

/// Converts an internal writing-tool receipt into a concise user-facing
/// status. HTTP/session plumbing should not interpret writing operations.
pub fn naturalize_writing_response(response: &str) -> Option<String> {
    naturalize_novel_studio_response(response)
}

/// Selects and naturalizes the first writing receipt from a complete exchange.
/// Keeping this selection here prevents the chat gateway from understanding
/// writing-tool receipt formats or operation combinations.
pub fn naturalize_writing_exchange<'a>(
    response: &str,
    tool_results: impl IntoIterator<Item = &'a str>,
) -> Option<String> {
    tool_results
        .into_iter()
        .find_map(naturalize_novel_studio_response)
        .or_else(|| naturalize_novel_studio_response(response))
}

fn naturalize_novel_studio_response(response: &str) -> Option<String> {
    let lowered = response.to_ascii_lowercase();
    if !lowered.contains("executed_tool: novel_studio") {
        return None;
    }
    if lowered.contains("status: blocked") {
        return Some(naturalize_blocked_novel_studio_response(response));
    }
    if !lowered.contains("status: completed") {
        return None;
    }

    let output_path = receipt_line_value(response, "output_path")
        .or_else(|| receipt_line_value(response, "export_path"));
    let operation = receipt_line_value(response, "operation");
    let unit_count = receipt_line_value(response, "unit_count");
    let total_units = receipt_line_value(response, "total_units");
    let chapters_completed = receipt_line_value(response, "chapters_completed");
    let chapters_planned = receipt_line_value(response, "chapters_planned");
    let project_complete = receipt_line_value(response, "project_complete")
        .is_some_and(|value| value.eq_ignore_ascii_case("true"));
    let is_export = operation
        .as_deref()
        .is_some_and(|value| value.contains("export"));
    let is_metadata_repair = operation
        .as_deref()
        .is_some_and(|value| value.contains("repair_chapter_metadata"));
    let is_chapter_approval = lowered.contains("operation: approve_chapter");
    let is_read = operation
        .as_deref()
        .is_some_and(|value| value.contains("read_chapter"));
    let is_status = operation.as_deref() == Some("status");
    let is_project_state_repair = operation.as_deref() == Some("repair_project_state");
    let is_content_edit = operation.as_deref().is_some_and(|value| {
        [
            "add_chapter",
            "delete_chapter",
            "modify_chapter",
            "revise_chapter",
        ]
        .iter()
        .any(|kind| value.contains(kind))
    });
    let chapter_number = receipt_line_value(response, "chapter_number");

    let mut lines = vec![if is_metadata_repair && is_chapter_approval {
        chapter_number
            .as_deref()
            .map(|chapter| {
                format!("章节元数据已修复，第 {chapter} 章已批准保存，并已重新检查导出。")
            })
            .unwrap_or_else(|| "章节元数据已修复，章节已批准保存，并已重新检查导出。".to_string())
    } else if is_chapter_approval {
        chapter_number
            .as_deref()
            .map(|chapter| format!("第 {chapter} 章已批准保存，并已重新检查导出。"))
            .unwrap_or_else(|| "章节已批准保存，并已重新检查导出。".to_string())
    } else if is_metadata_repair {
        "章节元数据已修复，并已重新检查导出。".to_string()
    } else if is_export {
        "小说导出已完成。".to_string()
    } else if is_read {
        "章节读取完成。".to_string()
    } else if is_status {
        "小说项目状态已读取。".to_string()
    } else if is_project_state_repair {
        "小说项目状态修复已完成，事实、连续性、故事圣经和角色权威已重新校验。".to_string()
    } else if is_content_edit {
        "章节修改已完成，并已重新检查保存状态。".to_string()
    } else {
        "本轮写作已完成，并已通过保存前检查。".to_string()
    }];

    if let Some(completed) = chapters_completed.as_deref() {
        let planned = chapters_planned
            .as_deref()
            .filter(|value| !value.is_empty())
            .unwrap_or(completed);
        lines.push(format!("- 完成章节：{completed}/{planned}"));
    }
    if let Some(units) = unit_count.as_deref() {
        let label = if is_metadata_repair || is_export || is_chapter_approval {
            "已批准字数"
        } else {
            "本轮字数"
        };
        lines.push(format!("- {label}：{units}"));
    }
    if let Some(units) = total_units.as_deref() {
        lines.push(format!("- 累计字数：{units}"));
    }
    if output_path
        .as_deref()
        .is_some_and(|value| !value.is_empty())
    {
        lines.push("- TXT：已生成，可从本条消息的文件入口打开".to_string());
    }
    if !project_complete {
        lines.push("这本书还没有完结；可以继续说“写下一章”或提出修改要求。".to_string());
    }
    Some(lines.join("\n"))
}

fn naturalize_blocked_novel_studio_response(response: &str) -> String {
    let chapter = receipt_line_value(response, "chapter_number");
    let blocker_kind = receipt_line_value(response, "blocker_kind");
    let blocker = if blocker_kind.as_deref() == Some("state_repair_required") {
        Some(
            "章节正文已保留，但人物、世界观、关系或伏笔状态还没有从最终正文完成可靠结算。旧状态未被改动。"
                .to_string(),
        )
    } else {
        receipt_line_value(response, "blockers")
            .or_else(|| receipt_line_value(response, "revision_issues"))
            .map(|value| naturalize_novel_studio_blocker_reason(&value))
    };
    let mut lines = vec![chapter
        .as_deref()
        .map(|chapter| format!("第 {chapter} 章草稿已保留，但还没有通过最终批准保存。"))
        .unwrap_or_else(|| "当前写作草稿已保留，但还没有通过最终批准保存。".to_string())];
    lines.push(
        blocker
            .filter(|value| !value.trim().is_empty())
            .map(|value| format!("原因：{value}"))
            .unwrap_or_else(|| {
                "原因：系统认为还需要一次受控修复，才能把草稿纳入正式章节。".to_string()
            }),
    );
    lines.push(
        "你可以说“继续处理当前章节”让系统沿用已保留草稿继续修复；不会从头新开项目。".to_string(),
    );
    lines.join("\n")
}

fn naturalize_novel_studio_blocker_reason(reason: &str) -> String {
    let lowered = reason.to_ascii_lowercase();
    if lowered.contains("revision did not converge") {
        return "章节正文已保留，但自动修复没有在限定轮次内收敛。".to_string();
    }
    if lowered.contains("metadata") {
        return "章节标题、摘要或连续性元数据还需要修复。".to_string();
    }
    if lowered.contains("quality") {
        return "章节质量门还有未解决问题。".to_string();
    }
    if lowered.contains("state change")
        || lowered.contains("state observer")
        || lowered.contains("settlement")
        || lowered.contains("evidence excerpt")
        || lowered.contains("final-body")
    {
        return "人物、世界观、关系或伏笔状态还没有从最终正文完成可靠结算；旧状态未被改动。"
            .to_string();
    }
    preview_text(reason.trim(), 120).to_string()
}

fn receipt_line_value(text: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}:");
    text.lines().find_map(|line| {
        line.trim()
            .strip_prefix(&prefix)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct CreationContractPanelDto {
    pub status: String,
    pub text: String,
    pub visible_to_panel: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub provisional: bool,
}

pub fn creation_contract_panel_payload(
    status: impl Into<String>,
    text: impl Into<String>,
    provisional: bool,
) -> Value {
    serde_json::to_value(CreationContractPanelDto {
        status: status.into(),
        text: text.into(),
        visible_to_panel: true,
        provisional,
    })
    .unwrap_or_else(|_| {
        serde_json::json!({
            "status": "unknown",
            "text": "",
            "visible_to_panel": true,
            "provisional": provisional,
        })
    })
}

pub fn creation_contract_panel_status_for_draft(
    draft: &crate::tool::writing::creation_contract::SessionCreationDraftState,
) -> String {
    use crate::tool::writing::creation_contract::CreationDraftLifecycleStatus;

    let surface =
        crate::tool::writing::creation_contract::CreationContractSurfaceState::from_draft(draft);
    let lifecycle = surface.lifecycle;
    if surface.confirmable {
        return lifecycle.as_str().to_string();
    }

    match lifecycle {
        CreationDraftLifecycleStatus::Blocked => "blocked",
        CreationDraftLifecycleStatus::Cleared => "cancelled",
        CreationDraftLifecycleStatus::DraftingContract
        | CreationDraftLifecycleStatus::ContractReady
        | CreationDraftLifecycleStatus::Approved
        | CreationDraftLifecycleStatus::Writing => "drafting_contract",
    }
    .to_string()
}

pub fn creation_contract_lifecycle_status_for_task_status(status: &TaskStatus) -> String {
    match status {
        TaskStatus::Completed => "ready",
        TaskStatus::Blocked { .. } => "blocked",
        TaskStatus::Failed(_) => "failed",
        TaskStatus::Cancelled => "cancelled",
        TaskStatus::Paused(_) => "paused",
        TaskStatus::Running => "running",
        TaskStatus::Pending => "pending",
        TaskStatus::Queued => "queued",
        TaskStatus::AwaitingApproval { .. } => "awaiting_approval",
        TaskStatus::Deferred { .. } => "deferred",
    }
    .to_string()
}

pub fn creation_contract_quality_blocker_from_panel_response(
    response: &str,
    quality_blocked: bool,
) -> Option<String> {
    if !quality_blocked {
        return None;
    }

    let response = response.trim();
    let reason = response
        .split_once("需要继续修复的问题：")
        .map(|(_, rest)| rest.lines().next().unwrap_or(rest).trim())
        .filter(|value| !value.is_empty())
        .unwrap_or(response);
    Some(preview_text(reason, 360).to_string())
}

pub fn stabilize_creation_contract_panel_response(
    draft: &crate::tool::writing::creation_contract::SessionCreationDraftState,
    response: &str,
) -> String {
    crate::tool::writing::creation_contract::stabilize_creation_contract_user_response(
        draft, response,
    )
}

pub fn creation_contract_status_for_draft(
    draft: Option<&crate::tool::writing::creation_contract::SessionCreationDraftState>,
    load_error: Option<&str>,
) -> Option<TaskStatus> {
    if let Some(error) = load_error {
        return Some(TaskStatus::Blocked {
            reason: format!("合同任务无法读取会话草案状态：{error}"),
        });
    }
    let Some(draft) = draft else {
        return Some(TaskStatus::Blocked {
            reason: "合同任务没有找到可确认的会话草案状态".to_string(),
        });
    };

    let surface =
        crate::tool::writing::creation_contract::CreationContractSurfaceState::from_draft(draft);
    if !surface.confirmable
        && !matches!(
            surface.lifecycle,
            crate::tool::writing::creation_contract::CreationDraftLifecycleStatus::Approved
                | crate::tool::writing::creation_contract::CreationDraftLifecycleStatus::Writing
        )
    {
        return Some(TaskStatus::Blocked {
            reason: format!(
                "合同草案尚未通过可写检查：{}",
                crate::tool::writing::creation_contract::creation_contract_issue_summary(
                    &surface.issues,
                )
            ),
        });
    }

    match surface.lifecycle {
        crate::tool::writing::creation_contract::CreationDraftLifecycleStatus::ContractReady
        | crate::tool::writing::creation_contract::CreationDraftLifecycleStatus::Approved
        | crate::tool::writing::creation_contract::CreationDraftLifecycleStatus::Writing => {
            Some(TaskStatus::Completed)
        }
        crate::tool::writing::creation_contract::CreationDraftLifecycleStatus::DraftingContract
        | crate::tool::writing::creation_contract::CreationDraftLifecycleStatus::Blocked => {
            let reason = if surface.issues.is_empty() {
                format!(
                    "合同草案仍处于 {}，尚未进入可确认状态",
                    surface.lifecycle.as_str()
                )
            } else {
                format!(
                    "合同草案尚未通过可写检查：{}",
                    crate::tool::writing::creation_contract::creation_contract_issue_summary(
                        &surface.issues,
                    )
                )
            };
            Some(TaskStatus::Blocked { reason })
        }
        crate::tool::writing::creation_contract::CreationDraftLifecycleStatus::Cleared => {
            Some(TaskStatus::Cancelled)
        }
    }
}

pub fn creation_contract_draft_is_confirmable(
    draft: &crate::tool::writing::creation_contract::SessionCreationDraftState,
) -> bool {
    crate::tool::writing::creation_contract::creation_contract_draft_is_confirmable(draft)
}

pub fn creation_contract_quality_blocked_response(issues: &[String]) -> String {
    crate::tool::writing::creation_contract::creation_contract_quality_blocked_response(issues)
}

pub fn referenced_artifact_segment_numbers(intent: &str) -> Vec<usize> {
    crate::tool::writing::creation_contract::referenced_artifact_segment_numbers(intent)
}

pub fn latest_project_path_from_tasks(tasks: &[TaskState]) -> Option<String> {
    let mut tasks = tasks.iter().collect::<Vec<_>>();
    tasks.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    for task in tasks {
        for reference in task_work_refs(task) {
            if let Some(path) = reference.strip_prefix("project_path:") {
                if !path.trim().is_empty() {
                    return Some(path.trim().to_string());
                }
            }
        }
    }
    None
}

pub fn task_work_refs(task: &TaskState) -> Vec<String> {
    let mut refs = Vec::new();
    for artifact in &task.artifacts {
        let uri = artifact.uri.trim();
        if path_looks_like_writing_workspace(uri) {
            refs.push(format!("artifact_path:{uri}"));
            if let Some(project_path) = infer_writing_project_path(uri) {
                refs.push(format!("project_path:{project_path}"));
            }
        }
    }
    for checkpoint in task.checkpoints.iter().rev().take(4) {
        if let Some(summary) = checkpoint.summary.as_deref() {
            collect_work_refs_from_text(summary, &mut refs);
        }
    }
    dedupe(&mut refs);
    refs.truncate(8);
    refs
}

pub fn collect_work_refs_from_text(text: &str, refs: &mut Vec<String>) {
    for path in writing_workspace_paths_from_text(text) {
        refs.push(format!("artifact_path:{path}"));
        if let Some(project_path) = infer_writing_project_path(&path) {
            refs.push(format!("project_path:{project_path}"));
        }
    }
}

pub fn writing_workspace_paths_from_text(text: &str) -> Vec<String> {
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
        let path = trim_path_candidate(candidate);
        if path_looks_like_writing_workspace(path) && !paths.iter().any(|item| item == path) {
            paths.push(path.to_string());
        }
    }
    paths
}

pub fn infer_writing_project_path(path: &str) -> Option<String> {
    let normalized = path.replace('\\', "/");
    if let Some(project_path) = infer_generated_novel_project_path(&normalized) {
        return Some(project_path);
    }
    let file_name = Path::new(&normalized)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    if file_name == "project.json" {
        return Path::new(&normalized)
            .parent()
            .map(|parent| parent.to_string_lossy().to_string());
    }
    for marker in [
        "chapters", "plans", "reviews", "runtime", "truth", "exports",
    ] {
        if let Some(index) = normalized.find(&format!("/{marker}/")) {
            return Some(normalized[..index].to_string());
        }
    }
    None
}

pub async fn existing_project_path_for_candidate(
    base_dir: &Path,
    project_path: &str,
) -> anyhow::Result<Option<String>> {
    let candidate = Path::new(project_path);
    let mut paths = Vec::new();
    if candidate.is_absolute() {
        paths.push(candidate.to_path_buf());
    } else {
        if project_path.replace('\\', "/").starts_with("data/") {
            if let Some(parent) = base_dir.parent() {
                paths.push(parent.join(project_path));
            }
        }
        paths.push(base_dir.join(project_path));
    }

    for path in paths {
        let Some(path) = authorized_existing_path(base_dir, &path) else {
            continue;
        };
        let project_json = path.join("project.json");
        if tokio::fs::metadata(&project_json).await.is_ok() {
            return Ok(Some(path.to_string_lossy().to_string()));
        }
    }
    Ok(None)
}

pub async fn render_project_segments_answer(
    project_path: &str,
    segment_numbers: &[usize],
) -> anyhow::Result<Option<String>> {
    let mut rendered = Vec::new();
    for segment_number in segment_numbers.iter().copied() {
        if let Some(answer) = render_project_segment_answer(project_path, segment_number).await? {
            rendered.push(answer);
        }
    }
    if rendered.is_empty() {
        return Ok(None);
    }
    Ok(Some(rendered.join("\n\n---\n\n")))
}

pub async fn render_project_status_answer(project_path: &str) -> anyhow::Result<Option<String>> {
    let Some(project) = read_project_json(project_path).await else {
        return Ok(None);
    };
    let title = project_title(&project);
    let target_units = project
        .get("target_units")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let chapters = project
        .get("chapters")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let approved = chapters
        .iter()
        .filter(|chapter| chapter.get("status").and_then(Value::as_str) == Some("approved"))
        .collect::<Vec<_>>();
    let approved_units = approved
        .iter()
        .filter_map(|chapter| chapter.get("unit_count").and_then(Value::as_u64))
        .sum::<u64>();
    let last_approved = approved
        .iter()
        .filter_map(|chapter| {
            let number = chapter.get("number").and_then(Value::as_u64)?;
            Some((number, *chapter))
        })
        .max_by_key(|(number, _)| *number);
    let last_title = last_approved
        .and_then(|(_, chapter)| chapter.get("title").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("未找到已批准章节");
    let current_txt = Path::new(project_path).join("exports/current.txt");
    let collection_txt = Path::new(project_path).join("exports/章节合集.txt");
    let export_path = if tokio::fs::metadata(&collection_txt).await.is_ok() {
        collection_txt
    } else {
        current_txt
    };
    let export_display = export_path.to_string_lossy();
    let complete_text = if target_units > 0 && approved_units >= target_units {
        "最低字数目标已达到；是否叙事完全收束以写作工具完成门/审稿状态为准。"
    } else if target_units > 0 {
        "未达到最低字数目标。"
    } else {
        "未设置总字数目标。"
    };
    Ok(Some(format!(
        "《{title}》当前项目状态：\n\n是否完成：{complete_text}\n总字数：{approved_units}\n目标字数：{}\n章节数：{}（已批准 {} 章）\n最后一章标题：{last_title}\nTXT 导出路径：{export_display}",
        if target_units > 0 {
            target_units.to_string()
        } else {
            "未设置".to_string()
        },
        chapters.len(),
        approved.len()
    )))
}

pub async fn append_recent_text_artifact_previews(
    text: &mut String,
    base_dir: &Path,
    tasks: &[TaskState],
    artifacts: &[ArtifactRecord],
    user_request: &str,
) {
    let mut paths = Vec::new();
    let mut project_paths = Vec::new();
    for task in tasks {
        for artifact in &task.artifacts {
            let uri = artifact.uri.trim();
            collect_previewable_project_path(&mut project_paths, uri);
            push_previewable_artifact_path(&mut paths, uri);
        }
    }
    for artifact in artifacts {
        let uri = artifact.uri.trim();
        collect_previewable_project_path(&mut project_paths, uri);
        push_previewable_artifact_path(&mut paths, uri);
    }
    dedupe(&mut project_paths);

    let mut prioritized_paths = Vec::new();
    let segment_numbers = referenced_artifact_segment_numbers(user_request);
    for project_path in &project_paths {
        for number in segment_numbers.iter().take(3) {
            push_previewable_artifact_path(
                &mut prioritized_paths,
                &format!("{project_path}/chapters/{number:04}.md"),
            );
        }
        for relative in [
            "truth/chapter-summaries.md",
            "truth/current-state.md",
            "continuity.md",
        ] {
            push_previewable_artifact_path(
                &mut prioritized_paths,
                &format!("{project_path}/{relative}"),
            );
        }
    }
    prioritized_paths.extend(paths);
    paths = prioritized_paths;
    dedupe(&mut paths);
    paths.truncate(6);
    if paths.is_empty() {
        return;
    }

    text.push_str("\nRecent artifact text previews for read-only follow-up:\n");
    text.push_str(
        "Answer with exact artifact paths from this context. Do not invent, translate, or \
         normalize local filesystem paths unless an explicit converted path is provided here.\n",
    );
    for (index, path) in paths.iter().enumerate() {
        let Some(path) = authorized_artifact_path(base_dir, path) else {
            continue;
        };
        let Ok(metadata) = tokio::fs::metadata(&path).await else {
            continue;
        };
        if !metadata.is_file() || metadata.len() > MAX_ARTIFACT_PREVIEW_BYTES {
            continue;
        }
        let Ok(content) = tokio::fs::read_to_string(&path).await else {
            continue;
        };
        let preview = preview_text(strip_yaml_frontmatter(content.trim()), 1800);
        if preview.trim().is_empty() {
            continue;
        }
        text.push_str(&format!(
            "{}. path={}\n   preview={}\n",
            index + 1,
            preview_text(&path.to_string_lossy(), 220),
            preview.replace('\n', "\n   ")
        ));
    }
}

fn authorized_artifact_path(base_dir: &Path, candidate: &str) -> Option<PathBuf> {
    let raw = Path::new(candidate);
    if raw.is_absolute() {
        return authorized_existing_path(base_dir, raw);
    }
    let normalized = candidate.replace('\\', "/");
    let joined = if normalized.starts_with("data/") {
        base_dir.parent()?.join(candidate)
    } else {
        base_dir.join(candidate)
    };
    authorized_existing_path(base_dir, &joined)
}

fn authorized_existing_path(base_dir: &Path, candidate: &Path) -> Option<PathBuf> {
    if candidate
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return None;
    }
    let candidate = candidate.canonicalize().ok()?;
    let base_dir = base_dir
        .canonicalize()
        .unwrap_or_else(|_| base_dir.to_path_buf());
    if candidate.starts_with(&base_dir) {
        return Some(candidate);
    }
    if let Ok(trusted) = benshu_brain::skills::CURRENT_WORKSPACES.try_with(|roots| roots.clone()) {
        for root in trusted {
            let root = root.canonicalize().unwrap_or(root);
            if candidate.starts_with(root) {
                return Some(candidate);
            }
        }
    }
    None
}

fn render_project_segment_answer(
    project_path: &str,
    segment_number: usize,
) -> impl std::future::Future<Output = anyhow::Result<Option<String>>> + '_ {
    async move {
        let chapter_path = format!("{project_path}/chapters/{segment_number:04}.md");
        let Ok(chapter_content) = tokio::fs::read_to_string(&chapter_path).await else {
            return Ok(None);
        };
        let project = read_project_json(project_path).await;
        let title = project.as_ref().map(project_title).unwrap_or("当前作品");
        let chapter_title = frontmatter_string(&chapter_content, "title")
            .or_else(|| markdown_heading_title(&chapter_content))
            .unwrap_or_else(|| format!("第{segment_number}章"));
        let summary = frontmatter_string(&chapter_content, "summary")
            .or_else(|| chapter_summary_from_project(project_path, segment_number));
        let protagonist = project
            .as_ref()
            .and_then(protagonist_from_project_json)
            .unwrap_or_else(|| "未在项目合同中明确标注".to_string());

        let mut response =
            format!("根据当前会话最近的写作项目《{title}》：\n\n{chapter_title}\n\n");
        if let Some(summary) = summary.map(|value| value.trim().to_string()) {
            if !summary.is_empty() {
                response.push_str(&format!("第{segment_number}章大概内容：{summary}\n\n"));
            }
        } else {
            response.push_str(&format!(
                "第{segment_number}章大概内容：{}\n\n",
                preview_text(strip_yaml_frontmatter(&chapter_content), 900)
            ));
        }
        response.push_str(&format!("目前主角：{protagonist}\n\n"));
        if let Some(project) = project.as_ref() {
            if let Some(chapter_target) = project.get("chapter_unit_target").and_then(Value::as_u64)
            {
                response.push_str(&format!(
                    "当前每章字数要求：不少于约 {chapter_target} 字\n\n"
                ));
            }
        }
        response.push_str(&format!("章节文件：{chapter_path}"));
        Ok(Some(response))
    }
}

async fn read_project_json(project_path: &str) -> Option<Value> {
    let path = format!("{project_path}/project.json");
    let content = tokio::fs::read_to_string(path).await.ok()?;
    serde_json::from_str(&content).ok()
}

fn project_title(project: &Value) -> &str {
    project
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("当前作品")
}

fn frontmatter_string(content: &str, key: &str) -> Option<String> {
    let rest = content.strip_prefix("---\n")?;
    let end = rest.find("\n---\n")?;
    let frontmatter = &rest[..end];
    let prefix = format!("{key}:");
    for line in frontmatter.lines() {
        let line = line.trim();
        let Some(value) = line.strip_prefix(&prefix) else {
            continue;
        };
        let value = value.trim().trim_matches('"').trim_matches('\'').trim();
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

fn markdown_heading_title(content: &str) -> Option<String> {
    strip_yaml_frontmatter(content)
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("# ").map(str::trim))
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn chapter_summary_from_project(project_path: &str, segment_number: usize) -> Option<String> {
    let path = format!("{project_path}/truth/chapter-summaries.md");
    let content = std::fs::read_to_string(path).ok()?;
    let prefix = format!("第{segment_number}章：");
    content
        .lines()
        .find_map(|line| line.trim().strip_prefix(&prefix).map(str::trim))
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn protagonist_from_project_json(project: &Value) -> Option<String> {
    if let Some(characters) = project.get("character_ledger").and_then(Value::as_array) {
        let protagonist = characters.iter().find(|character| {
            character
                .get("role")
                .and_then(Value::as_str)
                .is_some_and(|role| {
                    let lowered = role.to_ascii_lowercase();
                    role.contains("主角")
                        || lowered.contains("protagonist")
                        || lowered == "lead"
                        || lowered == "main"
                })
        });
        if let Some(name) = protagonist
            .and_then(|character| character.get("canonical_name"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
        {
            return Some(name.to_string());
        }
    }

    // Migration fallback for projects created before character_ledger became authoritative.
    let characters = project.pointer("/contract/characters")?.as_array()?;
    for character in characters {
        let Some(text) = character.as_str() else {
            continue;
        };
        let lowered = text.to_ascii_lowercase();
        if text.contains("主角") || lowered.contains("protagonist") || lowered.contains("main") {
            if let Some(name) = contract_character_name(text) {
                return Some(name);
            }
            return Some(text.trim().to_string());
        }
    }
    None
}

fn contract_character_name(text: &str) -> Option<String> {
    for marker in ["name:", "name：", "姓名:", "姓名："] {
        let Some(after_marker) = text.split_once(marker).map(|(_, after)| after.trim()) else {
            continue;
        };
        let name = after_marker
            .split([';', '；', ',', '，'])
            .next()
            .unwrap_or(after_marker)
            .trim();
        if !name.is_empty() {
            return Some(name.to_string());
        }
    }
    None
}

fn path_looks_like_writing_workspace(path: &str) -> bool {
    if path.trim().is_empty() {
        return false;
    }
    let lowered = path.to_ascii_lowercase().replace('\\', "/");
    (path.starts_with('/') || path.contains(":\\") || lowered.starts_with("data/generated/"))
        && (lowered.contains("/generated/")
            || lowered.starts_with("data/generated/")
            || lowered.contains("/novels/")
            || lowered.ends_with(".txt")
            || lowered.ends_with(".md")
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
        _ => None,
    }
}

fn infer_generated_novel_project_path(normalized: &str) -> Option<String> {
    let markers = ["/generated/novels/", "data/generated/novels/"];
    for marker in markers {
        let Some(index) = normalized.find(marker) else {
            continue;
        };
        let prefix_end = index + marker.len();
        let name = normalized[prefix_end..]
            .split('/')
            .next()
            .unwrap_or("")
            .trim();
        if name.is_empty() {
            continue;
        }
        return Some(format!("{}{}", &normalized[..prefix_end], name));
    }
    None
}

fn collect_previewable_project_path(project_paths: &mut Vec<String>, uri: &str) {
    if let Some(project_path) = infer_writing_project_path(uri) {
        project_paths.push(project_path);
    }
}

fn push_previewable_artifact_path(paths: &mut Vec<String>, uri: &str) {
    if uri.is_empty()
        || !path_looks_like_writing_workspace(uri)
        || !matches!(
            media_type_for_artifact_uri(uri),
            Some("text/plain") | Some("text/markdown")
        )
        || paths.iter().any(|known| known == uri)
    {
        return;
    }
    paths.push(uri.to_string());
}

fn strip_yaml_frontmatter(content: &str) -> &str {
    let Some(rest) = content.strip_prefix("---\n") else {
        return content;
    };
    let Some(end) = rest.find("\n---\n") else {
        return content;
    };
    &rest[end + "\n---\n".len()..]
}

fn trim_path_candidate(candidate: &str) -> &str {
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

fn dedupe(values: &mut Vec<String>) {
    let mut seen = HashSet::new();
    values.retain(|value| seen.insert(value.clone()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn novel_studio_receipts_are_rendered_without_internal_paths() {
        let raw = "status: completed\nworker: writer\nexecuted_tool: novel_studio\nproject_path: /tmp/novel\noutput_path: /tmp/novel/exports/current.txt\nproject_complete: false\nunit_count: 3035\ntotal_units: 3035\nchapters_completed: 1\nchapters_planned: 1";

        let rendered = naturalize_writing_response(raw).expect("writing receipt");

        assert!(rendered.contains("本轮写作已完成"), "{rendered}");
        assert!(rendered.contains("本轮字数：3035"), "{rendered}");
        assert!(!rendered.contains("/tmp/"), "{rendered}");
        assert!(!rendered.contains("project_path"), "{rendered}");
    }

    #[test]
    fn project_state_repair_receipt_is_not_presented_as_new_writing() {
        let raw = "status: completed\nworker: writer\nexecuted_tool: novel_studio\noperation: repair_project_state\nproject_path: /tmp/novel\nruntime_effect: artifact.repaired, artifact.verified\nauthority_updates: 1";

        let rendered = naturalize_writing_response(raw).expect("writing receipt");

        assert!(rendered.contains("小说项目状态修复已完成"), "{rendered}");
        assert!(!rendered.contains("本轮写作已完成"), "{rendered}");
        assert!(!rendered.contains("/tmp/novel"), "{rendered}");
    }

    #[test]
    fn blocked_novel_studio_receipt_is_naturalized_in_writing_surface() {
        let raw = "status: blocked\nworker: writer\nexecuted_tool: novel_studio\nworkflow_driver: writing.longform_fiction\nproject_path: /tmp/novel\noperation: revise_draft\nchapter_number: 3\nblockers: chapter draft is preserved, but revision did not converge within bounded attempts";

        let rendered = naturalize_writing_response(raw).expect("writing receipt");

        assert!(rendered.contains("第 3 章草稿已保留"), "{rendered}");
        assert!(
            rendered.contains("自动修复没有在限定轮次内收敛"),
            "{rendered}"
        );
        assert!(!rendered.contains("/tmp/novel"), "{rendered}");
        assert!(!rendered.contains("executed_tool"), "{rendered}");
    }

    #[test]
    fn state_repair_blocker_hides_observer_parser_details() {
        let raw = "status: blocked\nworker: observer\nexecuted_tool: novel_studio\noperation: settle_chapter_state\nproject_path: /tmp/novel\nchapter_number: 1\nchapter_status: state_repair_required\nblocker_kind: state_repair_required\nblockers: invalid final chapter observation: EOF while parsing a string at line 1 column 4128";

        let rendered = naturalize_writing_response(raw).expect("writing receipt");

        assert!(rendered.contains("状态还没有从最终正文完成可靠结算"));
        assert!(rendered.contains("旧状态未被改动"));
        assert!(!rendered.contains("EOF"), "{rendered}");
        assert!(!rendered.contains("column"), "{rendered}");
        assert!(!rendered.contains("/tmp/novel"), "{rendered}");
    }

    #[test]
    fn combined_novel_studio_receipts_are_naturalized_in_writing_surface() {
        let raw = "status: completed\nworker: writer\nexecuted_tool: novel_studio\noperation: repair_chapter_metadata\nproject_path: /tmp/novel\nsummary: 第 3 章元数据已修复。\n\n---\n\nstatus: completed\nworker: writer\nexecuted_tool: novel_studio\noperation: approve_chapter\nproject_path: /tmp/novel\nchapter_number: 3\nunit_count: 10063\n\n---\n\nstatus: completed\nworker: writer\nexecuted_tool: novel_studio\noperation: export_project\noutput_path: /tmp/novel/exports/current.txt";

        let rendered = naturalize_writing_exchange("ignored", [raw]).expect("writing receipt");

        assert!(
            rendered.contains("章节元数据已修复，第 3 章已批准保存"),
            "{rendered}"
        );
        assert!(rendered.contains("已批准字数：10063"), "{rendered}");
        assert!(rendered.contains("TXT：已生成"), "{rendered}");
        assert!(!rendered.contains("/tmp/novel"), "{rendered}");
    }

    #[tokio::test]
    async fn project_candidates_outside_authorized_data_root_are_rejected() {
        let root = tempfile::tempdir().expect("root");
        let data = root.path().join("data");
        let outside = root.path().join("outside");
        tokio::fs::create_dir_all(&data).await.expect("data dir");
        tokio::fs::create_dir_all(&outside)
            .await
            .expect("outside dir");
        tokio::fs::write(outside.join("project.json"), "{}")
            .await
            .expect("project json");

        let resolved =
            existing_project_path_for_candidate(&data, outside.to_string_lossy().as_ref())
                .await
                .expect("resolution");

        assert!(resolved.is_none());
    }

    #[test]
    fn project_status_prefers_authoritative_character_ledger() {
        let project = serde_json::json!({
            "character_ledger": [
                {
                    "canonical_name": "沈听澜",
                    "role": "主角"
                }
            ],
            "contract": {
                "characters": ["主角; name: 旧名字"]
            }
        });

        assert_eq!(
            protagonist_from_project_json(&project).as_deref(),
            Some("沈听澜")
        );
    }

    #[test]
    fn normalized_pending_contract_uses_recomputed_issues_instead_of_stale_diagnostics() {
        let mut draft = crate::tool::writing::creation_contract::build_initial_creation_draft(
            "pending-issues-authority",
            "fiction",
            "写海岛家族悬疑小说，每章2500字，一共5万字",
        )
        .expect("creation draft");
        draft.pending_contract_candidate = Some(serde_json::json!({
            "normalized": {
                "genre": "海岛家族悬疑"
            },
            "issues": [
                "ContractBlocker: 小说合同缺少世界规则",
                "ContractBlocker: 角色底线锚点缺少明确边界"
            ]
        }));

        let issues =
            crate::tool::writing::creation_contract::latest_contract_status_issues(&draft, &[]);

        assert!(
            issues.is_empty(),
            "stored diagnostics must not outlive the normalized contract state: {issues:?}"
        );
    }

    #[test]
    fn existing_novel_project_path_is_a_project_ref() {
        let paths = writing_workspace_paths_from_text(
            "继续当前项目，项目路径 data/generated/novels/长歌记。从第13章继续写。",
        );

        assert_eq!(paths, vec!["data/generated/novels/长歌记"]);
        assert_eq!(
            infer_writing_project_path(&paths[0]).as_deref(),
            Some("data/generated/novels/长歌记")
        );
        assert_eq!(
            infer_writing_project_path(
                "/home/user/benshu/data/generated/novels/长歌记/chapters/0013.md"
            )
            .as_deref(),
            Some("/home/user/benshu/data/generated/novels/长歌记")
        );
    }
}
