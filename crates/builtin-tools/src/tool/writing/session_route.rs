//! Session writing route helpers.
//!
//! Gateway owns HTTP/session plumbing. This module owns writing-domain route
//! interpretation for continuing governed writing work.

use benshu_compression::preview_text;
use benshu_state::TaskState;
use serde::{Deserialize, Serialize};

const WRITING_COMMAND_PREFIX: &str = "writing_command_json:";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WritingOperationKind {
    Read,
    Add,
    Delete,
    Modify,
    RepairProjectState,
    Export,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WritingCommand {
    pub project_path: String,
    pub operation: Option<WritingOperationKind>,
    pub target_chapter: Option<usize>,
    pub metadata_only: bool,
    pub surface_cleanup: bool,
    pub project_status: bool,
    pub user_request: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectWriterRoute {
    pub task: String,
    pub project_path: Option<String>,
    pub draft_path: Option<String>,
    pub chapter_count: usize,
    pub requested_start_chapter: Option<usize>,
    pub is_content_operation: bool,
}

pub fn session_work_context_guidance() -> &'static str {
    "For governed writing projects, prefer exact `project_path` work_refs with the writing tool's status/continue actions. Do not turn saved chapter paths into generic file-read tasks unless the user explicitly asks to inspect the raw file.\n"
}

pub fn task_looks_like_writing_task(task: &TaskState) -> bool {
    let contract_intent = task
        .contract
        .as_ref()
        .and_then(|contract| contract.intent.as_deref())
        .unwrap_or("");
    if contract_intent.contains(super::creation_contract::DIRECT_WRITER_CONTINUATION_MARKER)
        || contract_intent.contains("executed_tool: novel_studio")
        || contract_intent.contains("project_path:")
            && contract_intent.contains("data/generated/novels/")
    {
        return true;
    }
    task.checkpoints.iter().any(|checkpoint| {
        checkpoint.label.contains("novel-chapter")
            || checkpoint.label.contains("direct-writer-delegate")
            || checkpoint.summary.as_deref().is_some_and(|summary| {
                summary.contains("executed_tool: novel_studio")
                    || summary.contains("data/generated/novels/")
                    || summary.contains("章节生成")
            })
    })
}

pub fn task_is_creation_contract_planning(task: &TaskState) -> bool {
    if task
        .tags
        .iter()
        .any(|tag| tag == creation_contract_planning_tag())
    {
        return true;
    }
    task.contract
        .as_ref()
        .and_then(|contract| contract.intent.as_deref())
        .is_some_and(intent_is_creation_contract_planning)
}

pub fn creation_contract_planning_tag() -> &'static str {
    "creation_contract_planning"
}

pub fn intent_is_creation_contract_planning(intent: &str) -> bool {
    if intent_is_direct_writer_continuation(intent)
        || direct_writer_task_from_session_work_target(intent).is_some()
    {
        return false;
    }
    intent.contains(super::creation_contract::CREATION_PLANNING_DIALOGUE_MARKER)
        || intent.trim_start().starts_with("生成写作合同草案")
        || super::creation_contract::creation_draft_planning_dialogue_requested(intent)
}

pub fn intent_is_direct_writer_continuation(intent: &str) -> bool {
    intent.contains(super::creation_contract::DIRECT_WRITER_CONTINUATION_MARKER)
}

pub fn direct_writer_route_from_text(text: &str) -> Option<DirectWriterRoute> {
    if !intent_is_direct_writer_continuation(text) {
        return direct_writer_task_from_session_work_target(text).map(route_from_task_text);
    }
    let task = text
        .strip_prefix(super::creation_contract::DIRECT_WRITER_CONTINUATION_MARKER)
        .unwrap_or(text)
        .trim()
        .to_string();
    (!task.is_empty()).then(|| route_from_task_text(task))
}

pub fn mark_direct_writer_continuation_task(task: &str) -> String {
    format!(
        "{}\n{}",
        super::creation_contract::DIRECT_WRITER_CONTINUATION_MARKER,
        task.trim()
    )
}

fn route_from_task_text(task: String) -> DirectWriterRoute {
    let command = writing_command_from_task(&task);
    DirectWriterRoute {
        project_path: command
            .as_ref()
            .map(|command| command.project_path.clone())
            .or_else(|| direct_writer_labeled_value(&task, &["project_path"])),
        draft_path: direct_writer_labeled_value(&task, &["draft_path"]),
        chapter_count: direct_writer_requested_chapter_count(&task).unwrap_or(1),
        requested_start_chapter: direct_writer_requested_start_chapter(&task),
        is_content_operation: command
            .as_ref()
            .is_some_and(|command| command.operation.is_some())
            || task_is_novel_content_operation(&task),
        task,
    }
}

pub fn writing_command_from_task(task: &str) -> Option<WritingCommand> {
    task.lines().find_map(|line| {
        let payload = line.trim().strip_prefix(WRITING_COMMAND_PREFIX)?.trim();
        serde_json::from_str(payload).ok()
    })
}

pub fn writing_command_line(command: &WritingCommand) -> String {
    let payload = serde_json::to_string(command).expect("WritingCommand serialization cannot fail");
    format!("{WRITING_COMMAND_PREFIX} {payload}")
}

pub fn task_is_novel_content_operation(task: &str) -> bool {
    task.contains(super::creation_contract::NOVEL_CONTENT_OPERATION_MARKER)
}

pub fn mark_novel_content_operation_task(task: &str) -> String {
    format!(
        "{}\n{}",
        super::creation_contract::NOVEL_CONTENT_OPERATION_MARKER,
        task.trim()
    )
}

pub fn intent_requests_metadata_only_content_operation(intent: &str) -> bool {
    if writing_command_from_task(intent).is_some_and(|command| command.metadata_only) {
        return true;
    }
    message_requests_metadata_only_content_operation(intent)
        && (intent.contains(super::creation_contract::NOVEL_CONTENT_OPERATION_MARKER)
            || super::creation_contract::creation_draft_content_operation(intent, "fiction")
                .is_some())
}

fn message_requests_metadata_only_content_operation(intent: &str) -> bool {
    super::creation_contract::message_requests_metadata_only_content_operation(intent)
}

pub fn task_allows_file_artifact_target_verification(task: &str) -> bool {
    !task_is_novel_content_operation(task)
}

fn direct_writer_task_from_session_work_target(text: &str) -> Option<String> {
    let project_path = session_work_target_project_path(text)?;
    let user_request = session_work_target_user_request(text).unwrap_or(text);
    let content_operation =
        super::creation_contract::creation_draft_content_operation(user_request, "fiction");
    let export_requested = message_requests_project_export(user_request);
    let project_state_repair_requested = intent_requests_project_state_repair(user_request);
    let continue_after_project_repair = project_state_repair_requested
        && super::creation_contract::intent_requests_existing_work_generation(user_request);
    let metadata_only = message_requests_metadata_only_content_operation(user_request);
    if content_operation.is_none()
        && !export_requested
        && !project_state_repair_requested
        && !metadata_only
        && !super::creation_contract::intent_requests_existing_work_continuation(user_request)
    {
        return None;
    }
    let project_scale = user_request_requests_project_scale_continuation(user_request);
    let scope = if project_scale {
        "本轮范围：按当前合同推进到用户请求的目标规模和结局完成门；每章通过质量门后继续，直到目标达成、叙事闭合或出现明确 blocker。"
    } else if direct_writer_requested_chapter_count(user_request).is_some() {
        ""
    } else {
        "本轮默认只推进下一章；完成后返回进度，不要越界连续生成。"
    };
    let target_chapter = extract_requested_chapter_number_from_text(user_request);
    let operation = if export_requested {
        Some(WritingOperationKind::Export)
    } else if project_state_repair_requested && !continue_after_project_repair {
        Some(WritingOperationKind::RepairProjectState)
    } else {
        session_work_target_operation_kind(content_operation)
    };
    let command = WritingCommand {
        project_path: project_path.clone(),
        operation,
        target_chapter,
        metadata_only,
        surface_cleanup: false,
        project_status: intent_requests_project_status(user_request),
        user_request: preview_text(user_request.trim(), 500),
    };
    let command_line = writing_command_line(&command);
    let task = format!(
        "用户要求继续当前写作项目。不要重新规划合同，不要新开项目。\n\
        {command_line}\n\
        USER REQUEST\n{}\n\
        {}\n\
        {scope}\n\
        输出边界：正文保存到 artifact/TXT，不要把长正文塞进聊天框，只返回进度、章节、字数、文件路径、简短摘要和审查状态。",
        preview_text(user_request.trim(), 500),
        if continue_after_project_repair {
            "操作前置要求：先运行已有的项目状态修复并按修复后的权威重新校验；只有修复无 blocker 时才继续生成。"
        } else {
            session_work_target_operation_line(operation)
        }
    );
    if operation.is_some() || metadata_only {
        Some(mark_novel_content_operation_task(&task))
    } else {
        Some(task)
    }
}

fn user_request_requests_project_scale_continuation(user_request: &str) -> bool {
    super::creation_contract::creation_draft_requests_all_remaining(user_request, "fiction")
        || super::creation_contract::requested_total_unit_target(user_request).is_some()
}

fn session_work_target_operation_kind(
    operation: Option<super::creation_contract::NovelContentOperation>,
) -> Option<WritingOperationKind> {
    match operation {
        Some(super::creation_contract::NovelContentOperation::Read) => {
            Some(WritingOperationKind::Read)
        }
        Some(super::creation_contract::NovelContentOperation::Add) => {
            Some(WritingOperationKind::Add)
        }
        Some(super::creation_contract::NovelContentOperation::Delete) => {
            Some(WritingOperationKind::Delete)
        }
        Some(super::creation_contract::NovelContentOperation::Modify) => {
            Some(WritingOperationKind::Modify)
        }
        None => None,
    }
}

pub fn message_requests_project_export(message: &str) -> bool {
    let lowered = message.to_ascii_lowercase();
    let export_action = ["导出", "生成文件", "输出文件", "export"]
        .iter()
        .any(|term| message.contains(term) || lowered.contains(term));
    if !export_action {
        return false;
    }
    let project_surface = [
        "小说", "全书", "整本", "全文", "项目", "当前", "txt", "markdown", "md",
    ]
    .iter()
    .any(|term| message.contains(term) || lowered.contains(term));
    project_surface || message.trim().chars().count() <= 24
}

pub(crate) fn intent_requests_project_state_repair(message: &str) -> bool {
    let lowered = message.to_ascii_lowercase();
    let repair_action = [
        "修复",
        "修正",
        "纠正",
        "校准",
        "同步",
        "重建",
        "清理",
        "repair",
        "correct",
        "rebuild",
        "synchronize",
        "sync",
    ]
    .iter()
    .any(|term| message.contains(term) || lowered.contains(&term.to_ascii_lowercase()));
    let project_scope = [
        "项目状态",
        "项目级",
        "项目范围",
        "全局状态",
        "合同状态",
        "角色权威",
        "身份权威",
        "人物权威",
        "连续性状态",
        "事实状态",
        "故事状态",
        "project state",
        "project-wide",
        "character authority",
        "identity authority",
        "continuity state",
    ]
    .iter()
    .any(|term| message.contains(term) || lowered.contains(&term.to_ascii_lowercase()));
    repair_action && project_scope
}

pub fn intent_requests_project_status(user_request: &str) -> bool {
    let lowered = user_request.to_ascii_lowercase();
    let status_terms = [
        "项目状态",
        "进度",
        "写到哪",
        "状态",
        "完成了吗",
        "完成了没",
        "总字数",
        "章节数",
        "导出路径",
        "角色连续性",
        "人物连续性",
        "人物身份",
        "主角身份",
        "主角是谁",
        "主角叫什么",
        "人物名字",
        "角色名字",
        "角色漂移",
        "人物漂移",
        "哪些章节",
        "所有章节",
        "全部章节",
        "status",
        "progress",
        "project status",
        "export path",
        "continuity",
        "character drift",
        "identity drift",
        "all chapters",
    ];
    status_terms
        .iter()
        .any(|term| user_request.contains(term) || lowered.contains(&term.to_ascii_lowercase()))
}

fn session_work_target_operation_line(operation: Option<WritingOperationKind>) -> &'static str {
    match operation {
        Some(WritingOperationKind::Read) => {
            "操作类型：查询章节内容。必须先读取目标章节；只返回摘要、角色/情节要点和文件路径，不改写正文。"
        }
        Some(WritingOperationKind::Add) => {
            "操作类型：增加章节内容。必须先读取目标章节，再按用户要求把新增内容自然融入该章。"
        }
        Some(WritingOperationKind::Delete) => {
            "操作类型：删除章节内容。必须先读取目标章节，再删除用户指定内容并修补衔接。"
        }
        Some(WritingOperationKind::Modify) => {
            "操作类型：修改章节内容。必须先读取目标章节，再按用户要求改写相关内容。"
        }
        Some(WritingOperationKind::RepairProjectState) => {
            "操作类型：修复小说项目状态。运行写作工具已有的项目状态修复，重建事实、连续性、故事圣经和角色身份权威；不生成新章节。"
        }
        Some(WritingOperationKind::Export) => {
            "操作类型：导出当前小说项目。只导出已经通过质量门的章节，不生成新章节、不修改合同或正文。"
        }
        None => "",
    }
}

pub(crate) fn extract_requested_chapter_number_from_text(text: &str) -> Option<usize> {
    let normalized = text.replace('两', "二");
    let chars = normalized.chars().collect::<Vec<_>>();
    for index in 0..chars.len() {
        if chars[index] != '第' {
            continue;
        }
        let tail = chars[index + 1..].iter().collect::<String>();
        let mut digits = String::new();
        for ch in tail.chars() {
            if ch.is_ascii_digit() {
                digits.push(ch);
            } else {
                break;
            }
        }
        if !digits.is_empty() {
            if tail[digits.len()..].starts_with('章') {
                return digits.parse().ok().filter(|value| *value > 0);
            }
            continue;
        }
        let numerals = tail
            .chars()
            .take_while(|ch| {
                matches!(
                    ch,
                    '零' | '〇'
                        | '一'
                        | '二'
                        | '两'
                        | '三'
                        | '四'
                        | '五'
                        | '六'
                        | '七'
                        | '八'
                        | '九'
                        | '十'
                        | '百'
                        | '千'
                        | '万'
                )
            })
            .collect::<String>();
        if !numerals.is_empty() && tail[numerals.len()..].starts_with('章') {
            if let Some(value) =
                super::longform_guard::LongformArtifactGuard::parse_step_ordinal(&numerals)
            {
                return Some(value);
            }
        }
    }
    None
}

fn session_work_target_project_path(text: &str) -> Option<String> {
    if !text.contains("SESSION WORK TARGET") {
        return None;
    }
    direct_writer_labeled_value(text, &["project_path"])
}

fn session_work_target_user_request(text: &str) -> Option<&str> {
    text.split_once("USER REQUEST")
        .map(|(_, request)| request.trim())
        .filter(|request| !request.is_empty())
}

pub fn direct_writer_labeled_value(task: &str, labels: &[&str]) -> Option<String> {
    task.lines().find_map(|line| {
        let trimmed = line.trim();
        labels.iter().find_map(|label| {
            let tail = trimmed
                .strip_prefix(&format!("{label}:"))
                .or_else(|| trimmed.strip_prefix(&format!("{label}：")))?;
            let value = tail.trim();
            (!value.is_empty()).then(|| value.to_string())
        })
    })
}

pub fn direct_writer_requested_chapter_count(task: &str) -> Option<usize> {
    super::creation_contract::creation_draft_requested_turn_units(task, "fiction")
}

pub fn direct_writer_requested_start_chapter(task: &str) -> Option<usize> {
    writing_command_from_task(task)
        .and_then(|command| command.target_chapter)
        .or_else(|| {
            direct_writer_labeled_value(task, &["target_chapter"])
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty() && *value != "none")
                .and_then(|value| value.parse::<usize>().ok())
                .filter(|value| *value > 0)
        })
        .or_else(|| extract_requested_chapter_number_from_text(task))
}

pub fn message_contains_content_edit_surface(original: &str, lowered: &str) -> bool {
    super::creation_contract::message_contains_writing_content_surface(original, lowered)
}

pub fn message_contains_content_continuation_surface(original: &str, lowered: &str) -> bool {
    super::creation_contract::message_contains_writing_content_continuation_surface(
        original, lowered,
    )
}

pub fn message_should_bypass_foreground_control(message: &str) -> bool {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lowered = trimmed.to_lowercase();
    message_contains_content_edit_surface(trimmed, &lowered)
        || message_contains_content_continuation_surface(trimmed, &lowered)
}

pub fn intent_requests_read_only_existing_artifact_answer(intent: &str) -> bool {
    super::creation_contract::intent_requests_read_only_existing_artifact_answer(intent)
}

#[cfg(test)]
mod tests {
    #[test]
    fn finish_existing_artifact_request_is_not_read_only() {
        let message = "继续这本《碎灵余烬》，如果还没有真正完整结尾，就从当前进度接着写到完整结尾。不要新开书，不要贴正文全文，聊天里只告诉我进度、章节号、字数、文件路径、简短摘要和审查状态。";

        assert!(!super::intent_requests_read_only_existing_artifact_answer(
            message
        ));
    }

    #[test]
    fn chapter_done_question_is_read_only_artifact_answer() {
        let message = "第一章写好了吗？";

        assert!(super::intent_requests_read_only_existing_artifact_answer(
            message
        ));
        assert!(
            !super::super::creation_contract::intent_requests_existing_work_continuation(message)
        );
    }

    #[test]
    fn session_work_target_continuation_becomes_direct_writer_task() {
        let message = "SESSION WORK TARGET\n\
project_path: /home/user/benshu/data/generated/novels/demo\n\
This is runtime context for continuing or revising the current session artifact.\n\n\
USER REQUEST\n\
继续";

        let route = super::direct_writer_route_from_text(message).expect("direct writer route");

        let command = super::writing_command_from_task(&route.task).expect("writing command");
        assert_eq!(
            command.project_path,
            "/home/user/benshu/data/generated/novels/demo"
        );
        assert!(route.task.contains("不要重新规划合同"));
        assert!(route.task.contains("本轮默认只推进下一章"));
        assert_eq!(
            route.project_path.as_deref(),
            Some("/home/user/benshu/data/generated/novels/demo")
        );
    }

    #[test]
    fn session_work_target_continuation_preserves_explicit_chapter_target() {
        let message = "SESSION WORK TARGET\n\
project_path: /home/user/benshu/data/generated/novels/demo\n\n\
USER REQUEST\n\
继续写第二章。";

        let route = super::direct_writer_route_from_text(message).expect("direct writer route");

        assert_eq!(route.requested_start_chapter, Some(2));
        let command = super::writing_command_from_task(&route.task).expect("writing command");
        assert_eq!(command.target_chapter, Some(2));
    }

    #[test]
    fn existing_project_continuation_takes_precedence_over_contract_words() {
        let message = "SESSION WORK TARGET\n\
project_path: /home/user/benshu/data/generated/novels/demo\n\n\
USER REQUEST\n\
请继续写后续3章，从第8章到第10章。保持现有合同、角色和世界观不变。";

        assert!(super::direct_writer_route_from_text(message).is_some());
        assert!(!super::intent_is_creation_contract_planning(message));
    }

    #[test]
    fn session_work_target_export_becomes_typed_project_action() {
        let message = "SESSION WORK TARGET\n\
project_path: /home/user/benshu/data/generated/novels/demo\n\n\
USER REQUEST\n\
导出当前小说为 TXT";

        let route = super::direct_writer_route_from_text(message).expect("direct writer route");
        let command = super::writing_command_from_task(&route.task).expect("writing command");

        assert_eq!(command.operation, Some(super::WritingOperationKind::Export));
        assert!(route.task.contains("只导出已经通过质量门的章节"));
    }

    #[test]
    fn session_work_target_project_state_repair_uses_existing_project_action() {
        let message = "SESSION WORK TARGET\n\
project_path: /home/user/benshu/data/generated/novels/demo\n\n\
USER REQUEST\n\
执行项目范围角色权威清理并同步事实状态，不修改章节正文。";

        let route = super::direct_writer_route_from_text(message).expect("direct writer route");
        let command = super::writing_command_from_task(&route.task).expect("writing command");

        assert_eq!(
            command.operation,
            Some(super::WritingOperationKind::RepairProjectState)
        );
        assert!(route.is_content_operation);
        assert!(route.task.contains("运行写作工具已有的项目状态修复"));
    }

    #[test]
    fn session_work_target_repairs_then_continues_with_preflight() {
        let message = "SESSION WORK TARGET\n\
project_path: /home/user/benshu/data/generated/novels/demo\n\n\
USER REQUEST\n\
先修复项目范围角色权威并同步事实状态，然后继续写下一章。";

        let route = super::direct_writer_route_from_text(message).expect("direct writer route");
        let command = super::writing_command_from_task(&route.task).expect("writing command");

        assert_eq!(command.operation, None);
        assert!(!route.is_content_operation);
        assert!(route.task.contains("先运行已有的项目状态修复"));
        assert!(route.task.contains("只有修复无 blocker 时才继续生成"));
    }

    #[test]
    fn session_work_target_finish_current_novel_becomes_direct_writer_task() {
        let message = "SESSION WORK TARGET\n\
project_path: /home/user/benshu/data/generated/novels/demo\n\
This is runtime context for continuing or revising the current session artifact.\n\n\
USER REQUEST\n\
请继续把当前这部小说写完整。继续写之前先检查当前小说项目状态，如果最近章节有格式污染、角色称谓不一致、标题明显不合理或未通过审查的问题，请先按写作工具自己的流程修好，再继续一章一章写到真正结尾。";

        let route = super::direct_writer_route_from_text(message).expect("direct writer route");

        assert!(route.task.contains("不要重新规划合同"));
        assert!(!route.task.contains("本轮默认只推进下一章"));
        assert!(route
            .task
            .contains("按当前合同推进到用户请求的目标规模和结局完成门"));
        let command = super::writing_command_from_task(&route.task).expect("writing command");
        assert_eq!(
            command.project_path,
            "/home/user/benshu/data/generated/novels/demo"
        );
        assert_eq!(
            route.project_path.as_deref(),
            Some("/home/user/benshu/data/generated/novels/demo")
        );
    }

    #[test]
    fn session_work_target_total_target_request_is_project_scale() {
        let message = "SESSION WORK TARGET\n\
project_path: /home/user/benshu/data/generated/novels/demo\n\
This is runtime context for continuing or revising the current session artifact.\n\n\
USER REQUEST\n\
继续完成当前这本小说，按照当前合同和已批准章节继续写到至少5万字，最后要有完整结局。";

        let route = super::direct_writer_route_from_text(message).expect("direct writer route");

        let command = super::writing_command_from_task(&route.task).expect("writing command");
        assert!(command.user_request.starts_with("继续完成当前这本小说"));
        assert!(!route.task.contains("本轮默认只推进下一章"));
        assert!(route
            .task
            .contains("按当前合同推进到用户请求的目标规模和结局完成门"));
    }

    #[test]
    fn session_work_target_read_only_request_becomes_content_read() {
        let message = "SESSION WORK TARGET\n\
project_path: /home/user/benshu/data/generated/novels/demo\n\n\
USER REQUEST\n\
总结一下第一章";

        let route = super::direct_writer_route_from_text(message).expect("direct writer route");

        assert!(route.is_content_operation);
        assert!(route.task.contains("操作类型：查询章节内容"));
        let command = super::writing_command_from_task(&route.task).expect("writing command");
        assert_eq!(command.operation, Some(super::WritingOperationKind::Read));
        assert_eq!(command.target_chapter, Some(1));
    }

    #[test]
    fn title_repair_request_is_not_read_only_artifact_answer() {
        let message = "第一章标题明显不对，请只修第一章标题，不要改正文，然后告诉我文件路径";

        assert!(!super::intent_requests_read_only_existing_artifact_answer(
            message
        ));
        assert!(
            super::super::creation_contract::intent_requests_existing_work_continuation(message)
        );
    }

    #[test]
    fn chapter_metadata_revision_requests_artifact_verification() {
        let message = "请修订第二章，补全摘要、关键事实和连续性更新。";

        assert_eq!(
            super::super::creation_contract::creation_draft_content_operation(message, "fiction"),
            Some(super::super::creation_contract::NovelContentOperation::Modify)
        );
        assert!(super::intent_requests_metadata_only_content_operation(
            message
        ));
    }

    #[test]
    fn numeric_chapter_title_repair_requests_existing_work() {
        let message = "修第3章标题，不改正文。";

        assert!(
            super::super::creation_contract::intent_requests_existing_work_continuation(message)
        );
    }

    #[test]
    fn session_work_target_title_repair_becomes_direct_writer_task() {
        let message = "SESSION WORK TARGET\n\
project_path: /home/user/benshu/data/generated/novels/demo\n\n\
USER REQUEST\n\
第一章标题明显不对，请只修第一章标题，不要改正文，然后告诉我文件路径";

        let route = super::direct_writer_route_from_text(message).expect("direct writer route");

        assert!(route.is_content_operation);
        let command = super::writing_command_from_task(&route.task).expect("writing command");
        assert!(command.user_request.starts_with("第一章标题明显不对"));
        assert_eq!(command.target_chapter, Some(1));
        assert!(command.metadata_only);
        assert_eq!(
            route.project_path.as_deref(),
            Some("/home/user/benshu/data/generated/novels/demo")
        );
    }

    #[test]
    fn session_work_target_numeric_title_repair_preserves_target_chapter() {
        let message = "SESSION WORK TARGET\n\
project_path: /home/user/benshu/data/generated/novels/demo\n\n\
USER REQUEST\n\
修第3章标题，不改正文。";

        let route = super::direct_writer_route_from_text(message).expect("direct writer route");

        assert!(route.is_content_operation);
        let command = super::writing_command_from_task(&route.task).expect("writing command");
        assert!(command.user_request.starts_with("修第3章标题"));
        assert_eq!(command.target_chapter, Some(3));
        assert!(command.metadata_only);
    }

    #[test]
    fn session_work_target_summary_repair_preserves_target_chapter() {
        let message = "SESSION WORK TARGET\n\
project_path: /home/user/benshu/data/generated/novels/demo\n\n\
USER REQUEST\n\
请只修第6章摘要和关键事实，不要改正文";

        let route = super::direct_writer_route_from_text(message).expect("direct writer route");

        assert!(route.is_content_operation);
        let command = super::writing_command_from_task(&route.task).expect("writing command");
        assert!(command
            .user_request
            .starts_with("请只修第6章摘要和关键事实"));
        assert_eq!(command.target_chapter, Some(6));
        assert!(command.metadata_only);
    }

    #[test]
    fn metadata_only_content_operation_is_detected() {
        let message = "[BENSHU_NOVEL_CONTENT_OPERATION]\n\
用户最新要求：第一章标题明显不对，请只修第一章标题，不要改正文，然后告诉我文件路径";

        assert!(super::intent_requests_metadata_only_content_operation(
            message
        ));
    }

    #[test]
    fn explicit_long_chinese_chapter_number_uses_shared_ordinal_parser() {
        assert_eq!(
            super::extract_requested_chapter_number_from_text("请只修第一百二十三章摘要"),
            Some(123)
        );
        assert_eq!(
            super::extract_requested_chapter_number_from_text("继续写第一千零二章"),
            Some(1002)
        );
    }
}
