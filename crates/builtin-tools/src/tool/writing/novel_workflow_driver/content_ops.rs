use super::*;

#[derive(Clone)]
pub struct NovelContentOperationConfig {
    pub workspace: PathBuf,
    pub worker_label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NovelContentOperationKind {
    Read,
    Add,
    Delete,
    Modify,
    RepairProjectState,
    Export,
}

impl NovelContentOperationKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Add => "add",
            Self::Delete => "delete",
            Self::Modify => "modify",
            Self::RepairProjectState => "repair_project_state",
            Self::Export => "export",
        }
    }
}

#[derive(Debug, Clone)]
struct WritingContentOperationRequest {
    project_path: String,
    operation: NovelContentOperationKind,
    target_chapter: Option<usize>,
    metadata_only: bool,
    surface_cleanup: bool,
    project_status: bool,
    user_request: String,
}

impl WritingContentOperationRequest {
    fn from_task(task: &str) -> anyhow::Result<Self> {
        if let Some(command) = crate::tool::writing::session_route::writing_command_from_task(task)
        {
            let operation = match command.operation {
                Some(crate::tool::writing::session_route::WritingOperationKind::Read) => {
                    NovelContentOperationKind::Read
                }
                Some(crate::tool::writing::session_route::WritingOperationKind::Add) => {
                    NovelContentOperationKind::Add
                }
                Some(crate::tool::writing::session_route::WritingOperationKind::Delete) => {
                    NovelContentOperationKind::Delete
                }
                Some(crate::tool::writing::session_route::WritingOperationKind::Modify) => {
                    NovelContentOperationKind::Modify
                }
                Some(
                    crate::tool::writing::session_route::WritingOperationKind::RepairProjectState,
                ) => NovelContentOperationKind::RepairProjectState,
                Some(crate::tool::writing::session_route::WritingOperationKind::Export) => {
                    NovelContentOperationKind::Export
                }
                None => NovelContentOperationKind::Modify,
            };
            return Ok(Self {
                project_path: command.project_path,
                operation,
                target_chapter: command.target_chapter,
                metadata_only: command.metadata_only,
                surface_cleanup: command.surface_cleanup,
                project_status: command.project_status,
                user_request: command.user_request,
            });
        }
        let project_path = extract_marked_line(task, "project_path:")
            .ok_or_else(|| anyhow::anyhow!("novel content operation missing project_path"))?;
        let user_request =
            extract_marked_line(task, "用户原话：").unwrap_or_else(|| task.to_string());
        let operation = match novel_content_operation_kind(task) {
            "read" => NovelContentOperationKind::Read,
            "add" => NovelContentOperationKind::Add,
            "delete" => NovelContentOperationKind::Delete,
            _ => NovelContentOperationKind::Modify,
        };
        let target_chapter = extract_target_chapter_number(task);
        let metadata_only =
            crate::tool::writing::creation_contract::message_requests_metadata_only_content_operation(
                &user_request,
            );
        let surface_cleanup = content_operation_requests_surface_cleanup(&user_request);
        let project_status =
            crate::tool::writing::session_route::intent_requests_project_status(&user_request);
        Ok(Self {
            project_path,
            operation,
            target_chapter,
            metadata_only,
            surface_cleanup,
            project_status,
            user_request,
        })
    }
}

pub async fn run_novel_content_operation_for_delegate(
    agent: Arc<dyn MultiAgent>,
    task: &str,
    config: NovelContentOperationConfig,
) -> anyhow::Result<String> {
    let tool = NovelStudioTool::new(config.workspace, config.worker_label.clone());
    let request = WritingContentOperationRequest::from_task(task)?;
    if request.operation == NovelContentOperationKind::Export {
        let export = export_novel_project_txt(&tool, &request.project_path).await?;
        return Ok(format_project_export_result(&request.project_path, &export));
    }
    if request.operation == NovelContentOperationKind::RepairProjectState {
        let repaired = call_novel_studio_json(
            &tool,
            json!({
                "action": "repair_project_state",
                "project_path": request.project_path,
                "feedback": request.user_request
            }),
        )
        .await?;
        return Ok(format_novel_project_state_repair_result(&repaired));
    }
    if request.operation == NovelContentOperationKind::Modify && request.metadata_only {
        let chapter_number = request.target_chapter;
        let mut args = json!({
            "action": "repair_latest_chapter_metadata",
            "project_path": request.project_path
        });
        if let Some(number) = chapter_number {
            args["chapter_number"] = json!(number);
        }
        let repaired = call_novel_studio_json(&tool, args).await?;
        let mut reports = vec![format_novel_metadata_repair_result(
            &request.project_path,
            chapter_number,
            &repaired,
        )];
        if content_operation_requests_chapter_approval(&request.user_request) {
            let mut audit_args = json!({
                "action": "audit_chapter",
                "project_path": request.project_path
            });
            if let Some(number) = chapter_number {
                audit_args["chapter_number"] = json!(number);
            }
            let _audit = call_novel_studio_json(&tool, audit_args).await?;
            let mut approve_args = json!({
                "action": "approve_chapter",
                "project_path": request.project_path
            });
            if let Some(number) = chapter_number {
                approve_args["chapter_number"] = json!(number);
            }
            let approved = call_novel_studio_json_raw(&tool, approve_args).await?;
            reports.push(format_novel_chapter_approval_result(
                &request.project_path,
                chapter_number,
                &approved,
            ));
        }
        if request.surface_cleanup {
            let target_chapters = if let Some(number) = chapter_number {
                vec![number]
            } else {
                detect_surface_contaminated_chapters(&tool, &request.project_path).await?
            };
            for chapter_number in target_chapters {
                reports.push(
                    run_single_novel_content_operation(
                        agent.clone(),
                        &tool,
                        &request.project_path,
                        chapter_number,
                        request.operation.as_str(),
                        &request.user_request,
                        task,
                    )
                    .await?,
                );
            }
        }
        let export = export_novel_project_txt(&tool, &request.project_path).await?;
        reports.push(format_project_wide_cleanup_export_result(
            &request.project_path,
            &export,
        ));
        return Ok(reports.join("\n\n---\n\n"));
    }
    if request.operation == NovelContentOperationKind::Read
        && request.target_chapter.is_none()
        && request.project_status
    {
        let status = call_novel_studio_json(
            &tool,
            json!({
                "action": "status",
                "project_path": request.project_path
            }),
        )
        .await?;
        return Ok(format_novel_project_status_read_result(
            &request.project_path,
            &status,
        ));
    }
    let target_chapters = if let Some(chapter_number) = request.target_chapter {
        vec![chapter_number]
    } else if request.operation == NovelContentOperationKind::Modify && request.surface_cleanup {
        detect_surface_contaminated_chapters(&tool, &request.project_path).await?
    } else {
        anyhow::bail!(
            "novel content operation needs an explicit chapter number unless it is a project-wide surface cleanup"
        );
    };
    let operation = request.operation.as_str();
    if target_chapters.is_empty() {
        return Ok(format!(
            "status: completed\nworker: writer\nexecuted_tool: novel_studio\noperation: {operation}_chapter\nproject_path: {}\nruntime_effect: artifact.verified\nsummary: 未发现需要清理的章节正文污染。",
            request.project_path
        ));
    }

    let project_wide_surface_cleanup =
        request.operation == NovelContentOperationKind::Modify && request.surface_cleanup;
    let mut reports = Vec::new();
    for chapter_number in target_chapters {
        reports.push(
            run_single_novel_content_operation(
                agent.clone(),
                &tool,
                &request.project_path,
                chapter_number,
                operation,
                &request.user_request,
                task,
            )
            .await?,
        );
    }
    if project_wide_surface_cleanup {
        let export = export_novel_project_txt(&tool, &request.project_path).await?;
        reports.push(format_project_wide_cleanup_export_result(
            &request.project_path,
            &export,
        ));
    }
    Ok(reports.join("\n\n---\n\n"))
}

async fn export_novel_project_txt(
    tool: &NovelStudioTool,
    project_path: &str,
) -> anyhow::Result<Value> {
    call_novel_studio_json(
        tool,
        json!({
            "action": "export",
            "project_path": project_path,
            "format": "txt",
            "approved_only": true
        }),
    )
    .await
}

async fn run_single_novel_content_operation(
    agent: Arc<dyn MultiAgent>,
    tool: &NovelStudioTool,
    project_path: &str,
    chapter_number: usize,
    operation: &str,
    user_request: &str,
    task: &str,
) -> anyhow::Result<String> {
    let read = call_novel_studio_json(
        tool,
        json!({
            "action": "read_chapter",
            "project_path": project_path,
            "chapter_number": chapter_number
        }),
    )
    .await?;
    if operation == "read" {
        return Ok(format_novel_content_read_result(&read, chapter_number));
    }

    let content = required_string(&read, "content")?.to_string();
    let chapter = read.get("chapter").cloned().unwrap_or_else(|| json!({}));
    let title = chapter.get("title").and_then(Value::as_str).unwrap_or("");
    let language = extract_marked_line(task, "语言：").unwrap_or_else(|| "zh-CN".to_string());
    let revised = if operation == "modify"
        && content_operation_requests_surface_cleanup(&user_request)
    {
        sanitize_chapter_body(&content, title, &language)
    } else {
        let prompt = format!(
            "你是同一个 writer worker 的正文修订阶段。请只输出修订后的完整章节正文，不要解释，不要包裹 Markdown 代码块。\n\
语言：{language}\n\
项目路径：{project_path}\n\
目标章节：第{chapter_number}章\n\
章节标题：{title}\n\
用户修改要求：{user_request}\n\
操作类型：{operation}\n\
硬性要求：保留同一项目、同一章节、主角和既有人物名；不要新建项目；不要续写下一章；不要只输出局部片段；必须输出完整修订后章节正文；不要输出字数、路径、修改摘要、审查状态或任何工具/面板回执。\n\n\
原章节正文：\n{content}"
        );
        agent
            .generate_text_only_with_max_tokens(&prompt, Some(8192))
            .await?
    };
    let revised = strip_wrapping_code_fence(revised.trim()).trim().to_string();
    if revised.chars().count() < content.chars().count().saturating_div(3) {
        anyhow::bail!("revised chapter is too short to safely replace the existing chapter");
    }
    let revision = call_novel_studio_json(
        tool,
        json!({
            "action": "revise_chapter",
            "project_path": project_path,
            "chapter_number": chapter_number,
            "content": revised,
            "revision_notes": user_request
        }),
    )
    .await?;
    let audit = call_novel_studio_json(
        tool,
        json!({
            "action": "audit_chapter",
            "project_path": project_path,
            "chapter_number": chapter_number
        }),
    )
    .await
    .unwrap_or_else(|error| {
        json!({
            "success": false,
            "error": error.to_string()
        })
    });
    Ok(format_novel_content_mutation_result(
        operation,
        chapter_number,
        &revision,
        &audit,
    ))
}

async fn detect_surface_contaminated_chapters(
    tool: &NovelStudioTool,
    project_path: &str,
) -> anyhow::Result<Vec<usize>> {
    let numbers = project_chapter_numbers(project_path)?;
    let mut out = Vec::new();
    for chapter_number in numbers {
        let read = call_novel_studio_json(
            tool,
            json!({
                "action": "read_chapter",
                "project_path": project_path,
                "chapter_number": chapter_number
            }),
        )
        .await?;
        let content = read.get("content").and_then(Value::as_str).unwrap_or("");
        if content_contains_surface_cleanup_target(content) {
            out.push(chapter_number);
        }
    }
    Ok(out)
}

fn novel_content_operation_kind(task: &str) -> &'static str {
    if task.contains("操作类型：查询") {
        "read"
    } else if task.contains("操作类型：增加") {
        "add"
    } else if task.contains("操作类型：删除") {
        "delete"
    } else {
        "modify"
    }
}

fn content_operation_requests_surface_cleanup(request: &str) -> bool {
    let lowered = request.to_ascii_lowercase();
    let cleanup_terms = [
        "元文本",
        "模型说明",
        "输出限制",
        "字数限制",
        "篇幅限制",
        "格式",
        "格式污染",
        "污染",
        "残留",
        "乱码",
        "表面清理",
        "正文清理",
        "重复标题",
        "文件路径",
        "修改摘要",
        "审查状态",
        "非小说正文",
        "非正文",
        "note",
        "character limit",
        "production environment",
        "artifact_path",
        "runtime_effect",
        "quality_gate",
        "latex",
        "markdown",
        "转义残片",
        "数学残片",
        "\\rightarrow",
        "ightarrow",
    ];
    cleanup_terms
        .iter()
        .any(|term| request.contains(term) || lowered.contains(&term.to_ascii_lowercase()))
}

fn content_operation_requests_chapter_approval(request: &str) -> bool {
    let lowered = request.to_ascii_lowercase();
    [
        "批准",
        "通过",
        "保存",
        "正式保存",
        "批准保存",
        "纳入正式章节",
        "approve",
        "approved",
        "accept",
        "finalize",
        "mark approved",
    ]
    .iter()
    .any(|term| request.contains(term) || lowered.contains(&term.to_ascii_lowercase()))
}

fn format_novel_metadata_repair_result(
    project_path: &str,
    requested_chapter: Option<usize>,
    repaired: &Value,
) -> String {
    let repaired_chapters = repaired
        .get("repaired_chapters")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let changed = !repaired_chapters.is_empty();
    let chapter_summary = if !changed {
        requested_chapter
            .map(|number| format!("第 {number} 章标题元数据已检查，无需修改。"))
            .unwrap_or_else(|| "最新章节标题元数据已检查，无需修改。".to_string())
    } else {
        repaired_chapters
            .iter()
            .filter_map(|chapter| {
                let number = chapter.get("chapter_number").and_then(Value::as_u64)?;
                let previous = chapter
                    .get("previous_title")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let title = chapter.get("title").and_then(Value::as_str).unwrap_or("");
                Some(format!("第 {number} 章：{previous} -> {title}"))
            })
            .collect::<Vec<_>>()
            .join("；")
    };
    let status = if changed { "completed" } else { "blocked" };
    let runtime_effect = if changed {
        "artifact.written, artifact.verified"
    } else {
        "artifact.verified"
    };
    let blocker = if changed {
        String::new()
    } else {
        "\nblockers: requested metadata mutation produced no persisted changes".to_string()
    };
    format!(
        "status: {status}\nworker: writer\nexecuted_tool: novel_studio\noperation: repair_chapter_metadata\nproject_path: {project_path}\nruntime_effect: {runtime_effect}{blocker}\nsummary: {chapter_summary}"
    )
}

pub(super) fn format_novel_project_state_repair_result(repaired: &Value) -> String {
    let success = repaired
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let repaired_count = repaired
        .get("repaired_chapters")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    let authority_update_count = repaired
        .get("authority_updates")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    let integrity_blocker_count = repaired
        .get("integrity_blockers")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    let status = if success { "completed" } else { "blocked" };
    let runtime_effect = if success {
        "artifact.repaired, artifact.verified"
    } else {
        "artifact.repair_blocked"
    };
    let blockers = if integrity_blocker_count > 0 {
        format!(
            "\nblockers: {integrity_blocker_count} 个已批准章节与修复后的项目权威仍有冲突，必须修订并重新审稿"
        )
    } else {
        String::new()
    };
    format!(
        "status: {status}\nworker: writer\nexecuted_tool: novel_studio\noperation: repair_project_state\nruntime_effect: {runtime_effect}\nrepaired_chapter_records: {repaired_count}\nauthority_updates: {authority_update_count}\nintegrity_blockers: {integrity_blocker_count}{blockers}\nsummary: 小说项目事实、连续性、故事圣经和角色身份权威已重新校验。"
    )
}

fn format_novel_chapter_approval_result(
    project_path: &str,
    requested_chapter: Option<usize>,
    approved: &Value,
) -> String {
    let success = approved
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let chapter_number = approved
        .get("chapter_number")
        .and_then(Value::as_u64)
        .or_else(|| requested_chapter.map(|number| number as u64))
        .unwrap_or(0);
    if success {
        let state = approved.get("state").cloned().unwrap_or_else(|| json!({}));
        let approved_units = state_usize(&state, "approved_units").unwrap_or(0);
        return format!(
            "status: completed\nworker: writer\nexecuted_tool: novel_studio\noperation: approve_chapter\nproject_path: {project_path}\nchapter_number: {chapter_number}\nruntime_effect: artifact.approved\nunit_count: {approved_units}\nsummary: 第 {chapter_number} 章已批准保存。"
        );
    }
    let error_kind = approved
        .get("error_kind")
        .and_then(Value::as_str)
        .unwrap_or("approval_not_ready");
    let error = approved
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or("chapter is not ready for approval");
    format!(
        "status: blocked\nworker: writer\nexecuted_tool: novel_studio\noperation: approve_chapter\nproject_path: {project_path}\nchapter_number: {chapter_number}\nruntime_effect: artifact.approval_blocked\nblockers: {error_kind}: {error}"
    )
}

pub(crate) fn task_requests_novel_surface_cleanup(task: &str) -> bool {
    if !request_has_surface_cleanup_action(task)
        || !content_operation_requests_surface_cleanup(task)
    {
        return false;
    }
    if request_asks_to_continue_writing(task) && !request_explicitly_scopes_surface_cleanup(task) {
        return false;
    }
    true
}

fn request_has_surface_cleanup_action(request: &str) -> bool {
    let lowered = request.to_ascii_lowercase();
    [
        "清理", "清除", "去掉", "删除", "移除", "拿掉", "剔除", "修复", "修正", "消除", "cleanup",
        "clean up", "sanitize", "remove", "strip",
    ]
    .iter()
    .any(|term| request.contains(term) || lowered.contains(&term.to_ascii_lowercase()))
}

fn request_asks_to_continue_writing(request: &str) -> bool {
    let lowered = request.to_ascii_lowercase();
    [
        "继续",
        "继续写",
        "继续生成",
        "继续创作",
        "接着写",
        "接着生成",
        "下一章",
        "下章",
        "后续章节",
        "剩余章节",
        "写完",
        "完成全文",
        "continue writing",
        "continue drafting",
        "next chapter",
        "remaining chapters",
        "keep writing",
    ]
    .iter()
    .any(|term| request.contains(term) || lowered.contains(&term.to_ascii_lowercase()))
}

fn request_explicitly_scopes_surface_cleanup(request: &str) -> bool {
    let lowered = request.to_ascii_lowercase();
    [
        "项目级表面清理",
        "项目级正文表面清理",
        "清理项目",
        "清理整个项目",
        "清理全书",
        "清理全文",
        "全书清理",
        "全文清理",
        "所有章节",
        "全部章节",
        "project cleanup",
        "project-wide cleanup",
        "cleanup the project",
        "clean up the whole project",
        "all chapters",
    ]
    .iter()
    .any(|term| request.contains(term) || lowered.contains(&term.to_ascii_lowercase()))
}

pub(crate) fn content_contains_surface_cleanup_target(content: &str) -> bool {
    content.lines().any(|line| {
        let trimmed = line.trim();
        line_contains_placeholder_or_omission_marker(trimmed)
            || line_looks_like_generation_meta_note(trimmed)
            || crate::tool::writing::surface_sanitizer::line_looks_like_story_planning_meta(trimmed)
            || line_looks_like_json_artifact_residue(trimmed)
            || line_contains_provider_protocol_marker(trimmed)
            || line_is_standalone_markup_residue(trimmed)
            || line_contains_inline_markdown_emphasis_residue(trimmed)
            || line_contains_markup_math_residue(trimmed)
            || line_starts_with_short_escape_residue_before_cjk(trimmed)
            || line_contains_short_escape_residue_near_cjk(trimmed)
    })
}

fn format_project_wide_cleanup_export_result(project_path: &str, export: &Value) -> String {
    format_project_export_result_with_operation(
        project_path,
        export,
        "project_surface_cleanup_export",
        "已完成项目级正文表面清理并重新导出 TXT。",
    )
}

fn format_project_export_result(project_path: &str, export: &Value) -> String {
    format_project_export_result_with_operation(
        project_path,
        export,
        "export_project",
        "已导出当前小说中所有通过质量门的章节。",
    )
}

fn format_project_export_result_with_operation(
    project_path: &str,
    export: &Value,
    operation: &str,
    summary: &str,
) -> String {
    let export_path = export
        .pointer("/export/artifact_path")
        .or_else(|| export.pointer("/export/output_path"))
        .or_else(|| export.get("artifact_path"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let state = export.get("state").cloned().unwrap_or_else(|| json!({}));
    let approved_units = state_usize(&state, "approved_units").unwrap_or(0);
    format!(
        "status: completed\nworker: writer\nexecuted_tool: novel_studio\noperation: {operation}\nproject_path: {project_path}\nexport_path: {export_path}\noutput_path: {export_path}\nformat: txt\nmedia_type: text/plain\nruntime_effects: artifact.exported, artifact.txt, artifact.verified\nunit_count: {approved_units}\nsummary: {summary}"
    )
}

fn line_contains_markup_math_residue(line: &str) -> bool {
    if !line.chars().any(is_cjk_char) {
        return false;
    }
    let lowered = line.to_ascii_lowercase();
    lowered.contains("\\rightarrow")
        || lowered.contains("rightarrow$")
        || lowered.contains("ightarrow$")
        || lowered.starts_with("$\\rightarrow")
        || lowered.starts_with("$\\\\rightarrow")
        || line.contains("$ $")
        || line_contains_short_escape_residue_near_cjk(line)
}

fn line_starts_with_short_escape_residue_before_cjk(line: &str) -> bool {
    let chars = line.chars().collect::<Vec<_>>();
    let mut index = 0usize;
    while chars.get(index).is_some_and(|ch| ch.is_whitespace()) {
        index += 1;
    }
    if chars.get(index) != Some(&'\\') {
        return false;
    }
    while chars
        .get(index)
        .is_some_and(|ch| *ch == '\\' || ch.is_whitespace())
    {
        index += 1;
    }
    let letters_start = index;
    while chars.get(index).is_some_and(|ch| ch.is_ascii_alphabetic()) {
        index += 1;
    }
    let letter_count = index.saturating_sub(letters_start);
    while chars.get(index).is_some_and(|ch| ch.is_whitespace()) {
        index += 1;
    }
    (letter_count == 0 || (1..=3).contains(&letter_count))
        && chars
            .get(index)
            .is_some_and(|ch| is_cjk_char(*ch) || is_chinese_noise_boundary(*ch))
}

fn line_contains_short_escape_residue_near_cjk(line: &str) -> bool {
    strip_short_escape_residue_near_cjk_line(line) != line
}

fn strip_wrapping_code_fence(text: &str) -> &str {
    let trimmed = text.trim();
    let Some(stripped) = trimmed.strip_prefix("```") else {
        return trimmed;
    };
    let Some(end) = stripped.rfind("```") else {
        return trimmed;
    };
    let body = &stripped[..end];
    body.strip_prefix("markdown")
        .or_else(|| body.strip_prefix("md"))
        .or_else(|| body.strip_prefix("text"))
        .unwrap_or(body)
        .trim_start_matches(['\r', '\n'])
        .trim()
}

fn format_novel_content_read_result(read: &Value, chapter_number: usize) -> String {
    let project_path = read
        .get("project_path")
        .and_then(Value::as_str)
        .unwrap_or("");
    let artifact_path = read
        .get("artifact_path")
        .and_then(Value::as_str)
        .unwrap_or("");
    let chapter = read.get("chapter").cloned().unwrap_or_else(|| json!({}));
    let title = chapter.get("title").and_then(Value::as_str).unwrap_or("");
    let unit_count = chapter
        .get("unit_count")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let summary = chapter
        .get("summary")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            read.get("content")
                .and_then(Value::as_str)
                .map(|content| preview_text(content, 500))
                .unwrap_or_else(|| "未找到章节摘要。".to_string())
        });
    let key_facts = chapter
        .get("key_facts")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .take(8)
                .collect::<Vec<_>>()
                .join("；")
        })
        .unwrap_or_default();
    format!(
        "status: completed\nworker: writer\nexecuted_tool: novel_studio\noperation: read_chapter\nproject_path: {project_path}\nchapter_number: {chapter_number}\nchapter_title: {title}\nunit_count: {unit_count}\nartifact_path: {artifact_path}\nruntime_effect: artifact.verified\nsummary: {summary}\nkey_facts: {key_facts}"
    )
}

fn format_novel_project_status_read_result(project_path: &str, status: &Value) -> String {
    let state = status.get("state").cloned().unwrap_or_else(|| json!({}));
    let title = state.get("title").and_then(Value::as_str).unwrap_or("");
    let chapters = state.get("chapters").and_then(Value::as_u64).unwrap_or(0);
    let approved = state
        .get("approved_chapters")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let first_unapproved = state
        .get("first_unapproved_chapter")
        .and_then(Value::as_u64)
        .map(|value| value.to_string())
        .unwrap_or_else(|| "none".to_string());
    let blockers = status
        .get("identity_integrity_blockers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let blocker_summary = if blockers.is_empty() {
        "未发现已批准章节存在主角身份替换类漂移；普通章节不需要所有配角出场。".to_string()
    } else {
        blockers
            .iter()
            .take(12)
            .map(|item| {
                let number = item
                    .get("chapter_number")
                    .and_then(Value::as_u64)
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "?".to_string());
                let title = item
                    .get("chapter_title")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let issues = item
                    .get("issues")
                    .and_then(Value::as_array)
                    .map(|values| {
                        values
                            .iter()
                            .filter_map(Value::as_str)
                            .take(3)
                            .collect::<Vec<_>>()
                            .join("；")
                    })
                    .unwrap_or_default();
                format!("第{number}章《{title}》：{issues}")
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "status: completed\nworker: writer\nexecuted_tool: novel_studio\noperation: status\nproject_path: {project_path}\nruntime_effect: artifact.verified\nproject_title: {title}\nchapters: {chapters}\napproved_chapters: {approved}\nfirst_unapproved_chapter: {first_unapproved}\nidentity_integrity_blockers:\n{blocker_summary}"
    )
}

pub(crate) fn format_novel_content_mutation_result(
    operation: &str,
    chapter_number: usize,
    revision: &Value,
    audit: &Value,
) -> String {
    let project_path = revision
        .get("project_path")
        .and_then(Value::as_str)
        .unwrap_or("");
    let artifact_path = revision
        .get("artifact_path")
        .and_then(Value::as_str)
        .unwrap_or("");
    let txt_path = revision
        .get("txt_artifact_path")
        .and_then(Value::as_str)
        .or_else(|| {
            revision
                .get("preferred_artifact_path")
                .and_then(Value::as_str)
        })
        .unwrap_or("");
    let chapter = revision
        .get("chapter")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let unit_count = chapter
        .get("unit_count")
        .and_then(Value::as_u64)
        .or_else(|| revision.get("unit_count").and_then(Value::as_u64))
        .unwrap_or(0);
    let quality_passed = quality_gate_body_passed(revision);
    let audit_success = audit
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let audit_passed = audit_passed(audit);
    let audit_status = if audit_success {
        "completed"
    } else {
        "not_completed"
    };
    let status = if quality_passed && audit_passed {
        "completed"
    } else {
        "blocked"
    };
    let runtime_effect = if status == "completed" {
        "artifact.written, artifact.txt, artifact.reviewed"
    } else {
        "artifact.needs_revision"
    };
    let blockers = if status == "completed" {
        String::new()
    } else {
        format!(
            "\nblockers: chapter quality gate did not pass; audit_passed={audit_passed}; issues={}",
            revision_issue_summary(revision, audit)
        )
    };
    format!(
        "status: {status}\nworker: writer\nexecuted_tool: novel_studio\noperation: {operation}_chapter\nproject_path: {project_path}\nchapter_number: {chapter_number}\nunit_count: {unit_count}\nartifact_path: {artifact_path}\ntxt_artifact_path: {txt_path}\nruntime_effect: {runtime_effect}\nquality_gate_passed: {quality_passed}\naudit_passed: {audit_passed}\naudit_status: {audit_status}\nchapter_approval: pending_state_settlement{blockers}\nsummary: 已按用户要求修订同一项目的第{chapter_number}章，并同步了可读 TXT 导出；正式批准仍须完成最终正文状态结算。"
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn chapter_metadata_repair_recognizes_natural_title_fix_request() {
        assert!(crate::tool::writing::creation_contract::message_requests_metadata_only_content_operation(
            "第一章标题明显不对，请只修第一章标题，不要改正文，然后告诉我文件路径"
        ));
    }

    #[test]
    fn chapter_metadata_repair_recognizes_summary_fix_request() {
        assert!(crate::tool::writing::creation_contract::message_requests_metadata_only_content_operation(
            "请只修第6章摘要和关键事实，不要改正文"
        ));
    }

    #[test]
    fn chapter_metadata_repair_recognizes_single_verb_title_fix_request() {
        assert!(crate::tool::writing::creation_contract::message_requests_metadata_only_content_operation(
            "修第3章标题，不改正文。"
        ));
    }

    #[test]
    fn metadata_only_request_can_also_request_approval() {
        assert!(super::content_operation_requests_chapter_approval(
            "批准保存第3章；如果只差元数据就只修元数据，不重写正文。"
        ));
        assert!(!super::content_operation_requests_chapter_approval(
            "只修第3章标题和摘要，不重写正文。"
        ));
    }

    #[test]
    fn metadata_repair_reports_write_only_when_studio_persisted_changes() {
        let changed = super::format_novel_metadata_repair_result(
            "/tmp/project",
            Some(3),
            &json!({
                "repaired_chapters": [{
                    "chapter_number": 3,
                    "previous_title": "抉择",
                    "title": "雨港断讯"
                }]
            }),
        );
        let unchanged = super::format_novel_metadata_repair_result(
            "/tmp/project",
            Some(3),
            &json!({"repaired_chapters": []}),
        );

        assert!(changed.contains("status: completed"), "{changed}");
        assert!(changed.contains("artifact.written"), "{changed}");
        assert!(unchanged.contains("status: blocked"), "{unchanged}");
        assert!(!unchanged.contains("artifact.written"), "{unchanged}");
        assert!(unchanged.contains("artifact.verified"), "{unchanged}");
    }

    #[test]
    fn project_state_repair_does_not_report_verified_when_integrity_is_blocked() {
        let output = super::format_novel_project_state_repair_result(&json!({
            "success": false,
            "repaired_chapters": [],
            "authority_updates": ["沈青萝"],
            "integrity_blockers": ["第1章仍把沈青萝写成旧角色定位"]
        }));

        assert!(output.contains("status: blocked"), "{output}");
        assert!(output.contains("artifact.repair_blocked"), "{output}");
        assert!(output.contains("integrity_blockers: 1"), "{output}");
        assert!(!output.contains("artifact.verified"), "{output}");
    }
}
