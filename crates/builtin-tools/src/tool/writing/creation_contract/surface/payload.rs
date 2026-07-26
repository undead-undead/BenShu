use super::*;

pub fn creation_planning_outline_payload(draft: &SessionCreationDraftState) -> String {
    let mut parts = Vec::new();
    if !draft.brief.trim().is_empty() {
        parts.push(format!(
            "简述：{}",
            compact_creation_text(draft.brief.trim(), 320)
        ));
    }
    let planning_notes = stable_creation_planning_notes(draft);
    if !planning_notes.is_empty() {
        parts.push(format!(
            "关键设定：{}",
            planning_notes
                .iter()
                .take(8)
                .map(|note| compact_creation_text(note, 120))
                .collect::<Vec<_>>()
                .join("；")
        ));
    }
    let outline = fiction_outline_frame(draft);
    if !outline.is_empty() {
        parts.push(format!("大纲阶段：{}", outline.join("；")));
    }
    parts.join("\n")
}

pub fn compact_creation_text(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let mut compact = trimmed.chars().take(max_chars).collect::<String>();
    compact.push_str("...");
    compact
}

pub fn fiction_outline_frame(draft: &SessionCreationDraftState) -> Vec<String> {
    let planning_notes = stable_creation_planning_notes(draft);
    let haystack = format!(
        "{}；{}；{}",
        draft.genre,
        draft.brief,
        planning_notes.join("；")
    );
    let mut frame = Vec::new();
    if !haystack.trim_matches('；').trim().is_empty() {
        frame.push(
            "完整合同需要由 LLM 补齐：全书大纲、主要情节链、终局承诺、角色弧线、分卷/章节规划，以及情绪、关系、资源/成长、社会/地理/时间、对手压力、兑现矩阵和叙事口径。"
                .to_string(),
        );
    }
    frame
}

pub fn creation_outline_payload(draft: &SessionCreationDraftState) -> String {
    let mut parts = Vec::new();
    if !draft.brief.trim().is_empty() {
        parts.push(format!(
            "简述：{}",
            compact_creation_text(draft.brief.trim(), 240)
        ));
    }
    let planning_notes = stable_creation_planning_notes(draft);
    if !planning_notes.is_empty() {
        parts.push(format!("规划笔记：{}", planning_notes.join("；")));
    }
    let outline = fiction_outline_frame(draft);
    if !outline.is_empty() {
        parts.push(format!("大纲框架：{}", outline.join("；")));
    }
    parts.join("\n")
}

pub fn stable_creation_planning_notes(draft: &SessionCreationDraftState) -> Vec<String> {
    draft
        .planning_notes
        .iter()
        .filter(|note| {
            !note.starts_with("用户故事核心权威：")
                && !note.starts_with("待应用合同字段修订：")
                && !note.starts_with(CREATION_EXECUTION_SCOPE_NOTE_PREFIX)
                && !creation_planning_note_is_runtime_continuation_noise(note)
                && !creation_planning_note_is_quality_feedback(note)
        })
        .take(8)
        .cloned()
        .collect()
}

pub(crate) fn creation_planning_note_is_runtime_continuation_noise(note: &str) -> bool {
    let lowered = note.to_ascii_lowercase();
    let continuation = creation_draft_message_requests_continuation_generation(note, &lowered);
    let runtime_surface = [
        "已确认合同摘要",
        "用户已确认开始",
        "合同仍缺少系统必需字段",
        "系统将自动补齐后再进入正文",
        "当前标准小说合同草案",
        "待确认的小说创作合同草案",
        "可修改说明",
        "由于您尚未提供",
        "请先提供",
        "如果合同通过",
        "如果已经可以",
        "回复“开始写",
        "回复\"开始写",
        "不要重写",
        "不要新建",
        "从已批准",
        "从未通过",
        "当前项目",
        "项目路径",
        "完成后只返回",
        "章节号",
        "审查状态",
        "continue",
        "project_path",
    ];
    continuation
        || runtime_surface
            .iter()
            .any(|term| note.contains(term) || lowered.contains(&term.to_ascii_lowercase()))
}

pub(crate) fn creation_planning_note_is_quality_feedback(note: &str) -> bool {
    let explicit_feedback = [
        "上一版合同草案未通过质量门",
        "合同草案未通过质量门",
        "合同草案通过文本质量门但未通过可写检查",
        "失败合同禁用命名",
        "失败合同禁用书名",
        "失败合同禁用角色名",
        "ContractBlocker",
        "可写检查",
        "质量门",
        "大纲含有JSON",
        "未写入可确认草案",
        "上一版合同未通过输出质量门",
        "现有自动流程",
        "从这些内容反推书名",
        "模型默认高频名",
        "不能进入确认/写作",
    ]
    .iter()
    .any(|term| note.contains(term));
    if explicit_feedback {
        return true;
    }
    let naming_rejection = ["名字", "姓名", "角色名", "人物名", "书名", "旧名", "命名"]
        .iter()
        .any(|term| note.contains(term))
        && [
            "不要",
            "别",
            "不再",
            "禁用",
            "拒绝",
            "复用",
            "更换",
            "替换",
            "没有生效",
            "仍然是",
            "仍复用",
        ]
        .iter()
        .any(|term| note.contains(term));
    if naming_rejection {
        return true;
    }
    let inspection_action = [
        "检查",
        "自检",
        "审查",
        "校验",
        "排查",
        "复核",
        "核对",
        "验证",
        "可验证",
        "评估",
        "必须明确",
        "修复",
        "修正",
        "补齐",
        "补全",
    ]
    .iter()
    .any(|term| note.contains(term));
    let contract_quality_object = [
        "合同质量",
        "草案质量",
        "字段完整",
        "字段完整性",
        "结构完整",
        "内部一致性",
        "完整性",
        "一致性",
        "非空",
        "角色锚点",
        "人物锚点",
        "角色权威",
        "人物权威",
        "分卷目标",
        "因果句",
        "因果链",
        "主线因果",
        "世界规则",
        "具名机制",
        "既定作用",
        "既定效果",
        "终局方向",
        "兑现矩阵",
        "伏笔承诺",
        "兑现目标",
        "生命周期状态",
        "截断",
        "残句",
        "缺少谓语",
        "文本污染",
        "结构污染",
    ]
    .iter()
    .any(|term| note.contains(term));
    inspection_action && contract_quality_object
}
