use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NovelContentOperation {
    Read,
    Add,
    Delete,
    Modify,
}

pub fn creation_draft_content_operation(
    message: &str,
    artifact_kind: &str,
) -> Option<NovelContentOperation> {
    if artifact_kind != "fiction" {
        return None;
    }
    let lowered = message.to_ascii_lowercase();
    let asks_continuation_generation =
        creation_draft_message_requests_continuation_generation(message, &lowered);
    if asks_continuation_generation
        && !message_has_explicit_content_operation_target(message, &lowered)
    {
        return None;
    }
    if asks_continuation_generation && message_requests_chapter_workflow_recovery(message, &lowered)
    {
        return None;
    }
    if asks_continuation_generation
        && !message_contains_positive_content_edit_intent(message, &lowered)
    {
        return None;
    }
    let chapter_or_content_surface = [
        "章",
        "章节",
        "正文",
        "内容",
        "段",
        "段落",
        "情节",
        "剧情",
        "线索",
        "结尾",
        "开头",
        "chapter",
        "scene",
        "paragraph",
        "plot",
        "content",
    ];
    if !chapter_or_content_surface
        .iter()
        .any(|term| message.contains(term) || lowered.contains(term))
    {
        return None;
    }

    let add_terms = [
        "增加", "新增", "加入", "加上", "补充", "扩写", "插入", "添加", "append", "add", "insert",
        "expand",
    ];
    if add_terms
        .iter()
        .any(|term| message.contains(term) || lowered.contains(&term.to_ascii_lowercase()))
    {
        return Some(NovelContentOperation::Add);
    }
    let modify_terms = [
        "修",
        "只修",
        "修一下",
        "修好",
        "修改",
        "改成",
        "改为",
        "改得",
        "改到",
        "改一下",
        "改下",
        "重写",
        "重新写",
        "重新生成",
        "修复",
        "修正",
        "修订",
        "校正",
        "清理",
        "调整",
        "调得",
        "替换",
        "更名",
        "改名",
        "重命名",
        "命名",
        "润色",
        "变得",
        "写得更",
        "rewrite",
        "revise",
        "modify",
        "change",
        "replace",
    ];
    if modify_terms
        .iter()
        .any(|term| message_contains_positive_operation_term(message, &lowered, term))
    {
        return Some(NovelContentOperation::Modify);
    }
    let delete_terms = [
        "删除", "删掉", "去掉", "移除", "拿掉", "删去", "delete", "remove",
    ];
    if delete_terms
        .iter()
        .any(|term| message.contains(term) || lowered.contains(&term.to_ascii_lowercase()))
    {
        return Some(NovelContentOperation::Delete);
    }
    if message_contains_content_read_intent(message, &lowered) {
        return Some(NovelContentOperation::Read);
    }
    None
}

pub fn message_has_explicit_content_operation_target(message: &str, lowered: &str) -> bool {
    if !referenced_artifact_segment_numbers(message).is_empty() {
        return true;
    }
    [
        "本章",
        "这一章",
        "这章",
        "当前章",
        "当前章节",
        "本段",
        "这一段",
        "这段",
        "当前段落",
        "current chapter",
        "this chapter",
        "current section",
        "this paragraph",
    ]
    .iter()
    .any(|term| message.contains(term) || lowered.contains(&term.to_ascii_lowercase()))
}

pub fn message_requests_metadata_only_content_operation(message: &str) -> bool {
    let lowered = message.to_ascii_lowercase();
    let mentions_metadata = [
        "章节标题",
        "章节名",
        "章名",
        "标题",
        "摘要",
        "简介",
        "梗概",
        "关键事实",
        "连续性",
        "书名",
        "卷名",
        "元数据",
        "summary",
        "synopsis",
        "key facts",
        "continuity",
        "title",
        "chapter title",
        "heading",
        "metadata",
    ]
    .iter()
    .any(|term| message.contains(term) || lowered.contains(&term.to_ascii_lowercase()));
    mentions_metadata
        && [
            "只修",
            "修一下",
            "修好",
            "修",
            "修正",
            "修复",
            "修订",
            "修改",
            "调整",
            "更名",
            "改名",
            "重命名",
            "重新命名",
            "更符合",
            "不符合",
            "元数据",
            "导出",
            "repair",
            "rename",
            "fix",
            "metadata",
        ]
        .iter()
        .any(|term| message.contains(term) || lowered.contains(&term.to_ascii_lowercase()))
}

fn message_requests_chapter_workflow_recovery(message: &str, lowered: &str) -> bool {
    let recovery_terms = [
        "未通过",
        "没通过",
        "没有通过",
        "未通过草稿",
        "未通过的草稿",
        "未通过章节",
        "未通过的章节",
        "needs_revision",
        "need revision",
        "needs revision",
        "合格章节",
        "补足为合格",
        "补全为合格",
        "质量门",
        "审查通过",
        "通过审查",
        "审核通过",
        "通过审核",
        "批准",
        "批准为",
        "approve",
        "approval",
        "quality gate",
    ];
    recovery_terms
        .iter()
        .any(|term| message.contains(term) || lowered.contains(&term.to_ascii_lowercase()))
}

pub fn message_contains_positive_content_edit_intent(message: &str, lowered: &str) -> bool {
    [
        "增加",
        "新增",
        "加入",
        "加上",
        "补充",
        "扩写",
        "插入",
        "添加",
        "删除",
        "删掉",
        "去掉",
        "移除",
        "拿掉",
        "删去",
        "修改",
        "改成",
        "改为",
        "改得",
        "改到",
        "改一下",
        "改下",
        "重写",
        "重新写",
        "重新生成",
        "修复",
        "修正",
        "校正",
        "清理",
        "调整",
        "替换",
        "润色",
        "append",
        "add",
        "insert",
        "expand",
        "delete",
        "remove",
        "rewrite",
        "revise",
        "modify",
        "change",
        "replace",
    ]
    .iter()
    .any(|term| message_contains_positive_operation_term(message, lowered, term))
}

pub fn message_contains_writing_content_surface(message: &str, lowered: &str) -> bool {
    let terms = [
        "主角", "角色", "人物", "章节", "本章", "剧情", "小说", "故事", "场景", "段落", "正文",
        "文章", "论文", "报告", "画面", "镜头", "主题", "内容", "要求", "改成", "修改", "换成",
        "写成", "补充", "添加",
    ];
    terms
        .iter()
        .any(|term| message.contains(term) || lowered.contains(term))
}

pub fn message_contains_writing_content_continuation_surface(message: &str, lowered: &str) -> bool {
    let terms = [
        "主题",
        "内容",
        "要求",
        "改成",
        "修改",
        "换成",
        "写成",
        "补充",
        "添加",
        "角色",
        "人物",
        "章节",
        "本章",
        "下一章",
        "上一章",
        "正文",
        "剧情",
        "故事",
        "保存文件",
        "导出",
    ];
    terms
        .iter()
        .any(|term| message.contains(term) || lowered.contains(term))
}

pub fn message_contains_creation_contract_content_update_surface(
    message: &str,
    lowered: &str,
) -> bool {
    let content_terms = [
        "题材",
        "都市",
        "玄幻",
        "科幻",
        "言情",
        "异世界",
        "重生",
        "草根",
        "学校",
        "学院",
        "考试",
        "晋级",
        "主角是",
        "女主是",
        "反派是",
        "角色",
        "人物",
        "身份",
        "关系",
        "世界观",
        "大纲",
        "规划",
        "伏笔",
        "终局",
        "结局",
        "因果",
        "章",
        "每章",
        "总字数",
        "字",
        "结尾要",
        "genre",
        "chapter",
        "protagonist",
    ];
    if !content_terms
        .iter()
        .any(|term| message.contains(term) || lowered.contains(&term.to_ascii_lowercase()))
    {
        return false;
    }

    message
        .split(|ch| matches!(ch, '。' | '；' | ';' | '\n'))
        .map(str::trim)
        .filter(|clause| !clause.is_empty())
        .any(|clause| {
            let clause_lowered = clause.to_ascii_lowercase();
            let has_content_surface = content_terms.iter().any(|term| {
                clause.contains(term) || clause_lowered.contains(&term.to_ascii_lowercase())
            });
            if !has_content_surface {
                return false;
            }

            let explicit_change = [
                "改成",
                "改为",
                "换成",
                "换为",
                "变成",
                "设置为",
                "调整为",
                "统一为",
                "更正为",
                "更正成",
                "重写成",
                "替换为",
                "纠正",
                "更正",
                "增加",
                "新增",
                "删除",
                "移除",
                "去掉",
                "change",
                "replace",
                "set to",
                "switch to",
                "remove",
            ]
            .iter()
            .any(|term| message_contains_positive_operation_term(clause, &clause_lowered, term));
            if explicit_change {
                return true;
            }
            let contextual_change = ["统一", "同步", "明确", "写清楚"].iter().any(|term| {
                message_contains_positive_operation_term(clause, &clause_lowered, term)
            });
            if contextual_change
                && !creation_planning_note_is_quality_feedback(clause)
                && !creation_planning_note_is_quality_feedback(message)
            {
                return true;
            }

            let preservation_only = [
                "保持",
                "不变",
                "仍为",
                "沿用",
                "锁定",
                "不得更改",
                "不要更改",
                "不要修改",
                "不修改",
                "不得修改",
                "不能修改",
                "禁止修改",
                "禁止更改",
                "禁止替换",
                "禁止换",
                "不得换",
                "不能换",
                "preserve",
                "keep",
                "unchanged",
                "do not change",
                "must not change",
            ]
            .iter()
            .any(|term| clause.contains(term) || clause_lowered.contains(term));
            if preservation_only {
                return false;
            }

            [
                "主角是",
                "女主是",
                "男主是",
                "反派是",
                "题材是",
                "类型是",
                "角色名为",
                "人物名为",
                "书名为",
                "标题为",
                "世界观为",
                "世界规则为",
                "大纲为",
                "终局为",
                "终局是",
                "结局为",
                "结局是",
                "每章",
                "总字数",
                "一共",
                "总共",
                "genre:",
                "protagonist:",
                "chapter target",
            ]
            .iter()
            .any(|term| clause.contains(term) || clause_lowered.contains(term))
        })
}

pub fn message_contains_content_read_intent(message: &str, lowered: &str) -> bool {
    intent_requests_read_only_existing_artifact_answer(message)
        || [
            "查询",
            "查看",
            "读取",
            "检查",
            "核查",
            "校验",
            "状态",
            "连续性",
            "漂移",
            "总结",
            "概括",
            "讲了什么",
            "看一下",
            "inspect",
            "read",
        ]
        .iter()
        .any(|term| message.contains(term) || lowered.contains(&term.to_ascii_lowercase()))
}

pub fn message_contains_positive_operation_term(message: &str, lowered: &str, term: &str) -> bool {
    if term.is_ascii() {
        let term_lowered = term.to_ascii_lowercase();
        return term_occurrences(lowered, &term_lowered)
            .any(|index| !operation_term_is_non_executable(lowered, index));
    }
    term_occurrences(message, term).any(|index| !operation_term_is_non_executable(message, index))
}

pub fn term_occurrences<'a>(
    haystack: &'a str,
    needle: &'a str,
) -> impl Iterator<Item = usize> + 'a {
    let mut cursor = 0usize;
    std::iter::from_fn(move || {
        let found = haystack[cursor..].find(needle)?;
        let index = cursor + found;
        cursor = index + needle.len();
        Some(index)
    })
}

pub fn operation_term_is_negated(text: &str, index: usize) -> bool {
    let prefix = &text[..index];
    let tail = prefix
        .rsplit(|ch| {
            matches!(
                ch,
                '，' | ',' | '。' | '.' | '！' | '!' | '？' | '?' | '；' | ';' | '\n' | '\r'
            )
        })
        .next()
        .unwrap_or(prefix)
        .trim();
    [
        "不要",
        "别",
        "不用",
        "无需",
        "禁止",
        "不能",
        "不可",
        "别再",
        "不要再",
        "do not",
        "don't",
        "dont",
        "not ",
        "never ",
    ]
    .iter()
    .any(|marker| tail.contains(marker))
}

fn operation_term_is_non_executable(text: &str, index: usize) -> bool {
    operation_term_is_negated(text, index) || operation_term_is_hypothetical(text, index)
}

fn operation_term_is_hypothetical(text: &str, index: usize) -> bool {
    let clause_start = text[..index]
        .char_indices()
        .rev()
        .find(|(_, ch)| matches!(ch, '。' | '！' | '？' | ';' | '；' | '\n'))
        .map(|(position, ch)| position + ch.len_utf8())
        .unwrap_or(0);
    let prefix = text[clause_start..index]
        .chars()
        .rev()
        .take(16)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    [
        "能否",
        "可否",
        "是否可以",
        "能不能",
        "可不可以",
        "whether",
        "can it",
    ]
    .iter()
    .any(|marker| prefix.contains(marker))
}

pub fn creation_draft_message_requests_continuation_generation(
    message: &str,
    lowered: &str,
) -> bool {
    if message_requests_chapter_workflow_recovery(message, lowered)
        && message_mentions_generation_chapter_surface(message, lowered)
        && message_contains_positive_content_edit_intent(message, lowered)
    {
        return true;
    }
    let continuation_terms = [
        "继续",
        "继续写",
        "继续生成",
        "继续创作",
        "继续处理",
        "继续当前",
        "接着写",
        "接着生成",
        "接着创作",
        "从第",
        "后续章节",
        "剩余章节",
        "下一章",
        "写下一章",
        "目标仍是",
        "写完",
        "写到完整结尾",
        "完成全文",
        "continue writing",
        "continue drafting",
        "remaining chapters",
        "next chapter",
    ];
    let has_positive_continuation = continuation_terms
        .iter()
        .any(|term| message_contains_positive_operation_term(message, lowered, term));
    if has_positive_continuation {
        return true;
    }
    if intent_requests_existing_work_read_only_status(message, lowered) {
        return false;
    }
    false
}

fn message_mentions_generation_chapter_surface(message: &str, lowered: &str) -> bool {
    [
        "章",
        "章节",
        "本章",
        "下一章",
        "上一章",
        "chapter",
        "chapters",
    ]
    .iter()
    .any(|term| message.contains(term) || lowered.contains(&term.to_ascii_lowercase()))
}

pub fn requested_novel_content_chapter_scope(message: &str) -> String {
    let numbers = referenced_artifact_segment_numbers(message);
    if numbers.is_empty() {
        return "未明确章节号；如果用户请求项目状态、连续性、漂移、所有章节检查，则必须先查询项目状态；如果只是读取正文片段且仍不确定章节，先查询项目状态再决定。"
            .to_string();
    }
    numbers
        .iter()
        .map(|number| format!("第{number}章"))
        .collect::<Vec<_>>()
        .join("、")
}
