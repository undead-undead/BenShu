//! Writing-domain intent policy.
//!
//! This module only decides which writing action a user turn is asking for. It
//! does not judge prose quality, names, titles, or story content.

use super::creation_contract::CreationDraftLifecycleStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WritingIntent {
    StartContract,
    UpdateContract,
    ApproveContract,
    ContinueWriting,
    ReadProjectStatus,
    ReadChapter,
    ModifyChapter,
    RenameProject,
    CancelDraft,
    PauseTask,
    ResumeTask,
    Unknown,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct WritingIntentInput<'a> {
    pub(crate) message: &'a str,
    pub(crate) session_has_draft: bool,
    pub(crate) draft_status: Option<CreationDraftLifecycleStatus>,
    pub(crate) latest_project_path: Option<&'a str>,
    pub(crate) active_task_status: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub(crate) struct WritingIntentDecision {
    pub(crate) intent: WritingIntent,
    pub(crate) confidence: f32,
    pub(crate) requires_confirmation: bool,
    pub(crate) route_hint: String,
}

impl WritingIntentDecision {
    fn new(intent: WritingIntent, confidence: f32, route_hint: impl Into<String>) -> Self {
        Self {
            intent,
            confidence,
            requires_confirmation: false,
            route_hint: route_hint.into(),
        }
    }
}

pub(crate) fn decide(input: WritingIntentInput<'_>) -> WritingIntentDecision {
    let message = input.message.trim();
    if message.is_empty() {
        return WritingIntentDecision::new(WritingIntent::Unknown, 0.0, "empty");
    }
    let lowered = message.to_ascii_lowercase();

    if has_unnegated_any(message, &lowered, DISCARD_TERMS) {
        return WritingIntentDecision::new(WritingIntent::CancelDraft, 0.96, "discard_draft");
    }
    if has_unnegated_any(message, &lowered, PAUSE_TERMS) {
        return WritingIntentDecision::new(WritingIntent::PauseTask, 0.92, "pause_task");
    }
    if has_unnegated_any(message, &lowered, RESUME_TERMS) {
        return WritingIntentDecision::new(WritingIntent::ResumeTask, 0.9, "resume_task");
    }
    if has_any(message, &lowered, DEFER_START_TERMS)
        || deferred_start_with_explicit_future_signal(message, &lowered)
    {
        return WritingIntentDecision::new(WritingIntent::UpdateContract, 0.88, "defer_start");
    }
    if has_unnegated_any(message, &lowered, RENAME_TERMS) {
        return WritingIntentDecision::new(WritingIntent::RenameProject, 0.88, "rename_project");
    }
    if matches!(
        input.draft_status,
        Some(CreationDraftLifecycleStatus::ContractReady)
    ) && explicit_contract_confirmation_with_execution(message, &lowered)
    {
        return WritingIntentDecision::new(
            WritingIntent::ApproveContract,
            0.95,
            "approve_contract",
        );
    }
    if has_unnegated_any(message, &lowered, START_WRITING_TERMS) {
        return WritingIntentDecision::new(
            WritingIntent::ApproveContract,
            0.94,
            "approve_contract",
        );
    }
    if has_any(message, &lowered, READ_STATUS_TERMS)
        && (input.session_has_draft
            || input.latest_project_path.is_some()
            || input.active_task_status.is_some())
    {
        return WritingIntentDecision::new(WritingIntent::ReadProjectStatus, 0.86, "read_status");
    }
    if has_any(message, &lowered, READ_CHAPTER_TERMS) && mentions_chapter_surface(message, &lowered)
    {
        return WritingIntentDecision::new(WritingIntent::ReadChapter, 0.86, "read_chapter");
    }
    if input.session_has_draft
        && !matches!(
            input.draft_status,
            Some(CreationDraftLifecycleStatus::Approved)
        )
        && input.latest_project_path.is_none()
        && content_surface(message, &lowered)
    {
        return WritingIntentDecision::new(WritingIntent::UpdateContract, 0.82, "update_contract");
    }
    if has_any(message, &lowered, MODIFY_CHAPTER_TERMS) && content_surface(message, &lowered) {
        return WritingIntentDecision::new(WritingIntent::ModifyChapter, 0.84, "modify_chapter");
    }
    if has_any(message, &lowered, CONTINUE_TERMS) {
        return WritingIntentDecision::new(WritingIntent::ContinueWriting, 0.9, "continue_writing");
    }
    if has_any(message, &lowered, APPROVE_TERMS)
        && !has_any(message, &lowered, PLANNING_TERMS)
        && !content_surface(message, &lowered)
    {
        return WritingIntentDecision::new(WritingIntent::ApproveContract, 0.9, "approve_contract");
    }
    if has_any(message, &lowered, CONTRACT_UPDATE_TERMS)
        || (input.session_has_draft && content_surface(message, &lowered))
    {
        return WritingIntentDecision::new(WritingIntent::UpdateContract, 0.78, "update_contract");
    }
    if writing_request_surface(message, &lowered) {
        let mut decision =
            WritingIntentDecision::new(WritingIntent::StartContract, 0.76, "start_contract");
        decision.requires_confirmation = !matches!(
            input.draft_status,
            Some(CreationDraftLifecycleStatus::Approved)
        );
        return decision;
    }
    WritingIntentDecision::new(WritingIntent::Unknown, 0.1, "unknown")
}

fn explicit_contract_confirmation_with_execution(message: &str, lowered: &str) -> bool {
    let confirms_current_contract = has_unnegated_any(
        message,
        lowered,
        &[
            "我确认这份合同",
            "确认这份合同",
            "确认这个合同",
            "确认当前合同",
            "合同确认无误",
            "批准这份合同",
            "i approve this contract",
        ],
    );
    let requests_execution = has_unnegated_any(
        message,
        lowered,
        &[
            "写出",
            "创作第",
            "生成第",
            "完成第",
            "保存第",
            "write chapter",
            "draft chapter",
        ],
    );
    confirms_current_contract && requests_execution
}

fn has_any(message: &str, lowered: &str, terms: &[&str]) -> bool {
    terms
        .iter()
        .any(|term| message.contains(term) || lowered.contains(&term.to_ascii_lowercase()))
}

fn has_unnegated_any(message: &str, lowered: &str, terms: &[&str]) -> bool {
    terms.iter().any(|term| {
        let normalized_term = term.to_ascii_lowercase();
        contains_unnegated_term(message, term) || contains_unnegated_term(lowered, &normalized_term)
    })
}

fn contains_unnegated_term(message: &str, term: &str) -> bool {
    message.match_indices(term).any(|(index, _)| {
        let prefix = message[..index]
            .chars()
            .rev()
            .take(12)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>();
        let compact = prefix
            .trim_end_matches(|ch: char| ch.is_whitespace() || ch.is_ascii_punctuation())
            .to_ascii_lowercase();
        ![
            "不",
            "别",
            "不要",
            "不必",
            "无需",
            "暂不",
            "先不",
            "别再",
            "不是要",
            "do not",
            "don't",
            "dont",
            "not",
        ]
        .iter()
        .any(|negation| compact.ends_with(negation))
    })
}

fn deferred_start_with_explicit_future_signal(message: &str, lowered: &str) -> bool {
    let future_signal = has_unnegated_any(
        message,
        lowered,
        &[
            "等我",
            "等下一条",
            "等我下一条",
            "下一条",
            "下条",
            "之后再",
            "稍后再",
            "later",
            "next message",
        ],
    );
    let confirmation_first_signal = has_unnegated_any(
        message,
        lowered,
        &[
            "确认后",
            "我确认后",
            "批准后",
            "同意后",
            "确认再",
            "确认之后",
            "确认以后",
            "after confirmation",
            "after i confirm",
        ],
    );
    (future_signal || confirmation_first_signal)
        && (has_any(message, lowered, START_WRITING_TERMS)
            || has_any(message, lowered, CONTINUE_TERMS)
            || writing_request_surface(message, lowered))
}

fn mentions_chapter_surface(message: &str, lowered: &str) -> bool {
    has_any(
        message,
        lowered,
        &["章", "章节", "本章", "上一章", "下一章", "chapter"],
    )
}

fn content_surface(message: &str, lowered: &str) -> bool {
    has_any(
        message,
        lowered,
        &[
            "主角",
            "角色",
            "人物",
            "章节",
            "剧情",
            "故事",
            "正文",
            "内容",
            "大纲",
            "世界观",
            "情感线",
            "结局",
            "protagonist",
            "character",
            "chapter",
            "plot",
            "ending",
        ],
    )
}

fn writing_request_surface(message: &str, lowered: &str) -> bool {
    has_any(
        message,
        lowered,
        &[
            "写小说",
            "写一部小说",
            "创作小说",
            "写故事",
            "写文章",
            "写论文",
            "写报告",
            "novel",
            "fiction",
            "story",
            "paper",
            "report",
        ],
    )
}

const START_WRITING_TERMS: &[&str] = &[
    "开始写",
    "开始正式写作",
    "开始创作",
    "开始生成正文",
    "按这个开始",
    "按这个合同开始",
    "按这份合同开始",
    "按当前合同开始",
    "按已确认合同开始",
    "按这个创作合同开始",
    "按这个小说合同开始",
    "按合同开始",
    "按草案开始",
    "按这个写",
    "按这个合同写",
    "按这个草案写",
    "启动正式写作",
    "启动正文",
    "直接写正文",
    "正式写",
    "先写第一章",
    "先写第1章",
    "写第一章",
    "写第1章",
    "先写一章",
    "写一章",
    "start writing",
    "begin writing",
    "begin drafting",
];

const APPROVE_TERMS: &[&str] = &[
    "确认",
    "同意",
    "批准",
    "可以",
    "就这样",
    "你来定",
    "你决定",
    "approve",
    "go ahead",
    "you decide",
];

const PLANNING_TERMS: &[&str] = &[
    "先",
    "多轮",
    "定制",
    "不要立刻",
    "大纲",
    "框架",
    "合同",
    "草案",
    "before writing",
    "clarify first",
    "outline first",
    "plan first",
];

const DEFER_START_TERMS: &[&str] = &[
    "先不要写正文",
    "不要写正文",
    "不要直接写正文",
    "不要直接写",
    "先定合同",
    "先定大纲",
    "先给合同",
    "先给完整合同",
    "先给创作合同",
    "先给完整创作合同",
    "先给小说合同",
    "先输出合同",
    "先输出完整合同",
    "先生成合同",
    "先生成完整合同",
    "先别写正文",
    "先不写正文",
    "不要开始写",
    "别开始写",
    "don't write yet",
    "do not write yet",
    "do not start writing",
];

const CONTRACT_UPDATE_TERMS: &[&str] = &[
    "合同草案",
    "创作合同",
    "小说合同",
    "写作合同",
    "草案",
    "大纲",
    "框架",
    "设定",
    "更新",
    "修改",
    "调整",
    "完善",
    "补充",
    "定下",
    "确定",
    "改成",
    "改为",
    "story contract",
    "writing contract",
    "draft",
    "outline",
    "framework",
    "update",
    "revise",
    "adjust",
    "refine",
];

const CONTINUE_TERMS: &[&str] = &[
    "继续",
    "续写",
    "接着",
    "下一章",
    "后续章节",
    "剩余章节",
    "写完",
    "continue",
    "append",
    "next chapter",
];

const READ_STATUS_TERMS: &[&str] = &[
    "状态",
    "进度",
    "完成了吗",
    "完成了没",
    "完成没",
    "好了吗",
    "好了没",
    "生成好了吗",
    "生成好了没",
    "合同好了吗",
    "合同好了没",
    "合同生成好了吗",
    "总字数",
    "章节数",
    "导出路径",
    "status",
    "progress",
    "done",
    "complete",
    "ready",
    "path",
];

const READ_CHAPTER_TERMS: &[&str] = &[
    "讲了什么",
    "大概内容",
    "总结",
    "概括",
    "查看",
    "读取",
    "看一下",
    "tell me",
    "summary",
    "read",
    "inspect",
];

const MODIFY_CHAPTER_TERMS: &[&str] = &[
    "改", "修改", "换成", "补充", "添加", "删除", "删掉", "细腻", "加强", "rewrite", "revise",
    "add", "delete", "remove",
];

const RENAME_TERMS: &[&str] = &[
    "重新取名",
    "改书名",
    "改标题",
    "换书名",
    "换标题",
    "rename",
    "change title",
];

const DISCARD_TERMS: &[&str] = &[
    "取消草案",
    "放弃草案",
    "删掉草案",
    "不要这个草案",
    "discard draft",
    "cancel draft",
];

const PAUSE_TERMS: &[&str] = &["暂停", "先停", "等一下", "pause", "hold on"];
const RESUME_TERMS: &[&str] = &["恢复", "继续执行", "接着执行", "resume"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_novel_request_starts_contract() {
        let decision = decide(WritingIntentInput {
            message: "帮我写小说",
            session_has_draft: false,
            draft_status: None,
            latest_project_path: None,
            active_task_status: None,
        });
        assert_eq!(decision.intent, WritingIntent::StartContract);
    }

    #[test]
    fn approval_is_not_planning_update() {
        let decision = decide(WritingIntentInput {
            message: "按这个开始，写第一章",
            session_has_draft: true,
            draft_status: Some(CreationDraftLifecycleStatus::ContractReady),
            latest_project_path: None,
            active_task_status: None,
        });
        assert_eq!(decision.intent, WritingIntent::ApproveContract);
    }

    #[test]
    fn approval_with_surface_safety_constraints_still_starts() {
        let decision = decide(WritingIntentInput {
            message: "按这个开始，先写第一章。请不要展示 JSON、内部路径或工具参数；正文写完后告诉我保存和审稿状态。",
            session_has_draft: true,
            draft_status: Some(CreationDraftLifecycleStatus::ContractReady),
            latest_project_path: None,
            active_task_status: None,
        });
        assert_eq!(decision.intent, WritingIntent::ApproveContract);
    }

    #[test]
    fn confirmed_current_contract_with_full_book_scope_starts() {
        let decision = decide(WritingIntentInput {
            message: "合同方向确认，按这份合同开始并持续写完整本。",
            session_has_draft: true,
            draft_status: Some(CreationDraftLifecycleStatus::ContractReady),
            latest_project_path: None,
            active_task_status: None,
        });

        assert_eq!(decision.intent, WritingIntent::ApproveContract);
    }

    #[test]
    fn approval_that_explicitly_waives_chapter_confirmation_starts() {
        let decision = decide(WritingIntentInput {
            message: "按这个合同开始写，每次一章，自动连续写完整本并保存；中途不要等我逐章确认。",
            session_has_draft: true,
            draft_status: Some(CreationDraftLifecycleStatus::ContractReady),
            latest_project_path: None,
            active_task_status: None,
        });

        assert_eq!(decision.intent, WritingIntent::ApproveContract);
    }

    #[test]
    fn explicit_contract_confirmation_with_first_chapter_still_starts() {
        let decision = decide(WritingIntentInput {
            message: "确认这个合同，先写第一章。正文写完后告诉我保存和审稿状态；不要展示JSON、内部路径或工具参数。",
            session_has_draft: true,
            draft_status: Some(CreationDraftLifecycleStatus::ContractReady),
            latest_project_path: None,
            active_task_status: None,
        });

        assert_eq!(decision.intent, WritingIntent::ApproveContract);
    }

    #[test]
    fn natural_confirmation_with_review_and_save_still_starts() {
        let decision = decide(WritingIntentInput {
            message: "我确认这份合同。请严格按合同完整写出、审稿并保存第1章；第1章通过后停下来，只告诉我实际结果。",
            session_has_draft: true,
            draft_status: Some(CreationDraftLifecycleStatus::ContractReady),
            latest_project_path: None,
            active_task_status: None,
        });

        assert_eq!(decision.intent, WritingIntent::ApproveContract);
    }

    #[test]
    fn weak_approval_with_content_is_contract_update() {
        let decision = decide(WritingIntentInput {
            message: "可以，主角改成女性，情感线慢热一点",
            session_has_draft: true,
            draft_status: Some(CreationDraftLifecycleStatus::ContractReady),
            latest_project_path: None,
            active_task_status: None,
        });
        assert_eq!(decision.intent, WritingIntent::UpdateContract);
    }

    #[test]
    fn future_start_signal_defers_approval() {
        let decision = decide(WritingIntentInput {
            message: "等我下一条明确说开始写后再进入正文。",
            session_has_draft: true,
            draft_status: Some(CreationDraftLifecycleStatus::ContractReady),
            latest_project_path: None,
            active_task_status: None,
        });
        assert_eq!(decision.intent, WritingIntent::UpdateContract);
        assert_eq!(decision.route_hint, "defer_start");
    }

    #[test]
    fn confirmation_first_contract_request_defers_start() {
        let decision = decide(WritingIntentInput {
            message:
                "写异界修仙小说，每章2500字，至少5万字起。先给我完整创作合同，我确认后再开始写。",
            session_has_draft: false,
            draft_status: None,
            latest_project_path: None,
            active_task_status: None,
        });
        assert_eq!(decision.intent, WritingIntent::UpdateContract);
        assert_eq!(decision.route_hint, "defer_start");
    }

    #[test]
    fn confirmation_first_full_book_request_stays_in_contract_planning() {
        let decision = decide(WritingIntentInput {
            message: "请写一本工业悬疑长篇。先建立完整创作合同，合同确认后再写完整本书。",
            session_has_draft: false,
            draft_status: None,
            latest_project_path: None,
            active_task_status: None,
        });
        assert_eq!(decision.intent, WritingIntent::UpdateContract);
        assert_eq!(decision.route_hint, "defer_start");
    }

    #[test]
    fn chapter_question_reads_chapter() {
        let decision = decide(WritingIntentInput {
            message: "第三章讲了什么？",
            session_has_draft: true,
            draft_status: Some(CreationDraftLifecycleStatus::Approved),
            latest_project_path: Some("data/generated/novels/demo"),
            active_task_status: None,
        });
        assert_eq!(decision.intent, WritingIntent::ReadChapter);
    }

    #[test]
    fn contract_ready_question_reads_status_instead_of_updating_contract() {
        let decision = decide(WritingIntentInput {
            message: "合同生成好了吗？如果好了，请展示可确认合同；如果还没好，请说明还缺什么。",
            session_has_draft: true,
            draft_status: Some(CreationDraftLifecycleStatus::Blocked),
            latest_project_path: None,
            active_task_status: None,
        });
        assert_eq!(decision.intent, WritingIntent::ReadProjectStatus);
    }

    #[test]
    fn negated_destructive_action_does_not_cancel_draft() {
        let decision = decide(WritingIntentInput {
            message: "不要取消草案，先保留当前合同。",
            session_has_draft: true,
            draft_status: Some(CreationDraftLifecycleStatus::ContractReady),
            latest_project_path: None,
            active_task_status: None,
        });
        assert_ne!(decision.intent, WritingIntent::CancelDraft);
    }

    #[test]
    fn negated_pause_allows_explicit_continue() {
        let decision = decide(WritingIntentInput {
            message: "先不要暂停，继续写下一章。",
            session_has_draft: true,
            draft_status: Some(CreationDraftLifecycleStatus::Approved),
            latest_project_path: Some("data/generated/novels/demo"),
            active_task_status: Some("running"),
        });
        assert_eq!(decision.intent, WritingIntent::ContinueWriting);
    }
}
