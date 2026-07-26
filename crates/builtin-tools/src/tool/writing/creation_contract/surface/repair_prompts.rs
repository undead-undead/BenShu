use super::*;

pub fn final_prompt_from_creation_framework_request(
    draft: &SessionCreationDraftState,
    user_message: &str,
) -> String {
    let language_boundary = creation_planning_language_boundary(&draft.language);
    if draft.artifact_kind == "fiction" {
        if draft.current_contract.is_none() {
            final_prompt_from_initial_contract_batch(draft, user_message)
        } else {
            let issues = creation_draft_contract_blocking_findings_for_scope(
                draft,
                ContractReadinessScope::LockedAuthorityContract,
            );
            final_prompt_from_staged_contract_completion(draft, user_message, &issues)
        }
    } else {
        format!(
            "{CREATION_PLANNING_DIALOGUE_MARKER}\n\
用户正在定写作文档框架，当前处于框架确认阶段。请直接给出可确认的文档框架，不要启动正式写作，不要把它当成已完成正文产物。\n\
当前草案：\n\
- 类型：{}\n\
- 标题：{}\n\
- 语言：{}\n\
- 主题/论点：{}\n\
- 受众：{}\n\
- 用途：{}\n\
- 目标字数：{}\n\
- 导出格式：{}\n\
\n用户最新要求：{}\n\
\n请输出：结构框架、关键论点/段落安排、证据需求、还需要用户确认的问题；如果已经足够，请说明用户可以回复“开始写”。\n\
\n语言边界：{}",
            creation_kind_label(&draft.artifact_kind),
            empty_display(&draft.title, "由 BenShu 生成候选"),
            empty_display(&draft.language, "跟随用户语言"),
            empty_display(&draft.thesis_or_premise, &draft.brief),
            empty_display(&draft.audience, "未指定"),
            empty_display(&draft.purpose, "未指定"),
            draft
                .target_units
                .map(|value| value.to_string())
                .unwrap_or_else(|| "未指定".to_string()),
            draft.export_format,
            user_message.trim(),
            language_boundary,
        )
    }
}

pub fn final_prompt_from_contract_quality_repair(
    draft: &SessionCreationDraftState,
    user_message: &str,
    issues: &[String],
) -> String {
    if draft.artifact_kind == "fiction" {
        let mut typed_issues = creation_draft_contract_blocking_findings_for_scope(
            draft,
            ContractReadinessScope::LockedAuthorityContract,
        );
        if typed_issues.is_empty() {
            typed_issues = persisted_contract_quality_findings(issues);
        }
        return final_prompt_from_staged_contract_completion(draft, user_message, &typed_issues);
    }

    let base = final_prompt_from_creation_framework_request(draft, user_message);
    let clean_anchor = repair_prompt_clean_contract_anchor(draft);
    format!(
        "{base}\n\n\
当前已验证的干净草案锚点如下；这些是系统从用户需求和已通过字段整理出的约束，不是失败候选原文：\n\
{clean_anchor}\n\n\
上一版创作蓝图未通过输出质量门，不能进入正式写作。问题：{}\n\
请重新输出故事蓝图字段包来修复这些问题。优先 JSON；如果 JSON 不稳定，也可以输出清楚的中文字段行。不要输出 Markdown 代码块、解释文字、正文或文件生成声明。\n\
修复重点：围绕干净草案锚点补齐缺字段；不要复制失败候选原文；去掉乱码/外文残片/破损字段；角色表必须恰好 1 个主角，并且至少再给 1 个关系对象/盟友/导师、1 个关键对手/反派/压力源；近期章节包保留 3 到 8 章即可，章节 goal 写本章事件和不可逆变化。\n\
	书名修复必须重新生成 title.candidates：给 3 到 5 个候选，每个候选都带 title、hook_type、rationale；候选必须由当前合同的关键物件、地点、制度、事件、人物关系或结局变化支撑；canonical_title 只能从候选中选择；title.rationale 必须解释 canonical_title 里的关键字如何来自终局、主线、世界规则或关键事件。命名只检查故事证据和文字完整性，不检查营销吸引力。\n\
	书名、卷名、章节名从终局、主线、世界规则和实际章节内容反推，但不要为了说明规则而拉长输出。",
        issues.join("；")
    )
}

fn persisted_contract_quality_findings(
    issues: &[String],
) -> super::super::issue::ContractIssueList {
    use super::super::issue::{
        ContractIssue, ContractIssueEvidence, ContractIssueKind, ContractIssueList,
    };

    let mut findings = ContractIssueList::new(
        "contract.runtime_feedback",
        ContractIssueKind::Other,
        "runtime_feedback",
    );
    for issue in issues {
        let (code, kind, field) = if issue.contains("semantic.outline_character_authority") {
            let candidate_field = persisted_semantic_candidate_field(issue).unwrap_or(issue);
            (
                "semantic.outline_character_authority",
                super::super::issue::user_story_semantic_issue_kind(candidate_field),
                candidate_field,
            )
        } else if issue.contains("semantic.user_story_authority") {
            let candidate_field = persisted_semantic_candidate_field(issue).unwrap_or(issue);
            (
                "semantic.user_story_authority",
                super::super::issue::user_story_semantic_issue_kind(candidate_field),
                candidate_field,
            )
        } else {
            (
                "contract.runtime_feedback",
                ContractIssueKind::Other,
                "runtime_feedback",
            )
        };
        findings.push_issue(ContractIssue::new(
            code,
            kind,
            ContractIssueEvidence::new(field, issue.clone()),
            issue.clone(),
        ));
    }
    findings.sort_dedup();
    findings
}

fn persisted_semantic_candidate_field(issue: &str) -> Option<&str> {
    issue
        .split_once("候选证据 ")
        .map(|(_, rest)| rest)
        .and_then(|rest| rest.split_once("=`").map(|(field, _)| field.trim()))
        .filter(|field| !field.is_empty())
}

pub fn final_prompt_from_title_metadata_repair(
    draft: &SessionCreationDraftState,
    issues: &[String],
) -> Option<String> {
    let clean_anchor = repair_prompt_clean_contract_anchor(draft);
    if !clean_anchor.contains("上一版候选中可复用的结构化字段") {
        return None;
    }
    let title_anchor = title_repair_anchor_with_character_authority(&clean_anchor);
    let title_issues = title_metadata_issues_for_prompt(issues);
    Some(format!(
        "你正在修复小说故事蓝图的书名 metadata。不要重写故事蓝图、角色、世界观、大纲或章节包。\n\n\
    可复用故事锚点：\n{title_anchor}\n\n\
    当前只需要修复这些书名问题：{}\n\n\
		请只输出一个 JSON 对象，格式必须是：\n\
			{{\"title\":{{\"canonical_title\":\"中文作品名\",\"candidates\":[{{\"title\":\"候选1\",\"hook_type\":\"关键物件/地点事件/制度/关键事件/结局变化/人物关系\",\"rationale\":\"候选1如何来自故事证据\"}},{{\"title\":\"候选2\",\"hook_type\":\"不同证据类型\",\"rationale\":\"候选2如何来自故事证据\"}},{{\"title\":\"候选3\",\"hook_type\":\"不同证据类型\",\"rationale\":\"候选3如何来自故事证据\"}}],\"rationale\":\"用一句具体中文说明最终书名如何来自终局、主线、世界规则或关键事件\"}}}}\n\n\
如果 JSON 输出不稳定，也可以只输出这三行字段包：\n\
书名：中文作品名\n\
书名候选：候选1；候选2；候选3\n\
	书名理由：用一句具体中文说明书名如何来自终局、主线、世界规则或关键事件\n\n\
				要求：candidates/书名候选必须给 3 到 5 个彼此不同的候选，并由当前故事里的地点、物件、制度、事件、人物关系或结局变化支撑；JSON 候选应带 hook_type 和各自 rationale；title.rationale/书名理由必须解释 canonical_title/书名里的关键字如何来自合同证据；已锁定角色名是唯一可使用的人名，不得新造角色名充当书名锚点；文字必须完整、自然、无乱码和残句；不要输出 Markdown、解释文字、正文、英文、拼音、韩文/日文或任何额外说明。",
        title_issues.join("；")
    ))
}

fn title_repair_anchor_with_character_authority(clean_anchor: &str) -> String {
    clean_anchor.to_string()
}

fn title_metadata_issues_for_prompt(issues: &[String]) -> Vec<String> {
    let filtered = issues
        .iter()
        .filter(|issue| {
            let lowered = issue.to_ascii_lowercase();
            issue.contains("书名")
                || issue.contains("标题")
                || issue.contains("作品名")
                || issue.contains("读者钩子")
                || lowered.contains("title")
        })
        .cloned()
        .collect::<Vec<_>>();
    if filtered.is_empty() {
        vec!["书名 metadata 未通过质量门".to_string()]
    } else {
        filtered
    }
}

pub fn final_prompt_from_contract_metadata_repair(
    draft: &SessionCreationDraftState,
    issues: &[String],
) -> Option<String> {
    let clean_anchor = repair_prompt_clean_contract_anchor(draft);
    if !clean_anchor.contains("上一版候选中可复用的结构化字段") {
        return None;
    }
    if contract_metadata_repair_only_needs_world_rules(issues) {
        return Some(format!(
            "你正在修复小说故事蓝图的局部 metadata。不要重写故事蓝图、书名、角色、终局、总字数、分卷或章节规划。\n\n\
可复用故事锚点：\n{clean_anchor}\n\n\
当前只需要修复这些 metadata 问题：{}\n\n\
请只输出 world_rules 字段。优先 JSON：\n\
{{\"world_rules\":[\"世界如何运行的可执行规则1\",\"世界如何运行的可执行规则2\",\"世界如何运行的可执行规则3\"]}}\n\n\
如果本地模型不能稳定输出 JSON，也可以只输出这一行中文字段包：\n\
世界规则：规则1；规则2；规则3\n\n\
要求：world_rules 至少 3 条；每条必须描述虚构世界如何运行，包含能力、资源、制度、关系压力或交易机制的代价、限制、失败后果或稀缺条件；不能复述世界观意象；不能写成“不要怎样写”的写作禁令；不要输出 Markdown、解释文字、正文、英文、拼音、韩文/日文或额外说明。",
            issues.join("；")
        ));
    }
    Some(format!(
        "你正在修复小说故事蓝图的局部 metadata。不要重写故事蓝图、角色、世界观、终局、总字数或完整大纲。\n\n\
可复用故事锚点：\n{clean_anchor}\n\n\
当前只需要修复这些 metadata 问题：{}\n\n\
请优先输出一个 JSON 对象，格式是：\n\
	{{\"title\":{{\"canonical_title\":\"中文作品名\",\"candidates\":[\"候选1\",\"候选2\",\"候选3\"],\"rationale\":\"用一句具体中文说明书名如何来自终局、主线、世界规则或关键事件\"}},\"world_rules\":[\"世界如何运行的可执行规则1\",\"世界如何运行的可执行规则2\",\"世界如何运行的可执行规则3\"],\"outline\":{{\"volumes\":[{{\"title\":\"第一卷名\",\"objective\":\"本卷必须达成的阶段目标\",\"ending_change\":\"卷尾不可逆变化\"}}],\"near_chapters\":[{{\"number\":1,\"goal\":\"本章具体事件目标\",\"expected_turn\":\"本章结束时发生的不可逆变化\"}},{{\"number\":2,\"goal\":\"本章具体事件目标\",\"expected_turn\":\"本章结束时发生的不可逆变化\"}},{{\"number\":3,\"goal\":\"本章具体事件目标\",\"expected_turn\":\"本章结束时发生的不可逆变化\"}}]}}}}\n\n\
如果本地模型不能稳定输出 JSON，也可以输出清楚的中文字段包，只包含这些字段：\n\
书名：中文作品名\n\
书名候选：候选1；候选2；候选3\n\
	书名理由：用一句具体中文说明书名如何来自终局、主线、世界规则或关键事件\n\
世界规则：规则1；规则2；规则3\n\
分卷规划：第一卷《卷名》：阶段目标；卷尾变化：不可逆变化\n\
近期章节包：第1章 本章目标：具体事件目标；预期转折：不可逆变化\n\n\
	要求：只修 title、world_rules、outline.volumes 和 outline.near_chapters；world_rules 至少 3 条，必须是能力/资源/制度/关系压力的代价、限制、失败后果或稀缺条件，不能复述世界观意象；volumes 保留 1 到 5 卷，每卷必须有 objective 和 ending_change；near_chapters 保留 3 到 8 章；expected_turn 必须是事件变化，不能是数字、章节号或空泛词；书名必须文字完整，并由当前故事的地点、物件、制度、事件、选择或结局变化支撑。不要输出 Markdown、解释文字、正文、英文、拼音、韩文/日文或额外说明。",
        issues.join("；")
    ))
}

fn contract_metadata_repair_only_needs_world_rules(issues: &[String]) -> bool {
    !issues.is_empty()
        && issues.iter().all(|issue| {
            let lowered = issue.to_ascii_lowercase();
            (issue.contains("世界规则") || lowered.contains("world_rules"))
                && !issue.contains("书名")
                && !issue.contains("标题")
                && !issue.contains("分卷")
                && !issue.contains("章节")
                && !lowered.contains("title")
                && !lowered.contains("outline")
                && !lowered.contains("chapter")
                && !lowered.contains("volume")
        })
}

pub(crate) fn repair_prompt_clean_contract_anchor(draft: &SessionCreationDraftState) -> String {
    let mut effective = draft.clone();
    if let Some(mut pending_contract) = pending_normalized_contract_for_repair_anchor(draft) {
        apply_strong_novel_contract_to_creation_draft(&mut effective, &mut pending_contract);
        normalize_fiction_creation_draft_after_contract_change(&mut effective);
        sanitize_creation_draft_control_noise(&mut effective);
    }
    let characters = governed_contract_characters_for_view(&effective);
    let mut lines = Vec::new();
    if draft.pending_contract_candidate.is_some() {
        lines.push("上一版候选中可复用的结构化字段：".to_string());
    }
    lines.push(format!(
        "用户原始简述：{}",
        empty_display(&effective.brief, "由当前用户消息补齐")
    ));
    lines.push(format!(
        "语言：{}",
        empty_display(&effective.language, "跟随用户语言")
    ));
    lines.push(format!(
        "题材：{}",
        empty_display(&effective.genre, "由用户需求归纳")
    ));
    lines.push(format!(
        "总目标字数：{}",
        effective
            .target_units
            .map(|value| value.to_string())
            .unwrap_or_else(|| "由用户需求补齐".to_string())
    ));
    lines.push(format!(
        "每章目标档位：{}",
        effective
            .chapter_unit_target
            .map(|value| value.to_string())
            .unwrap_or_else(|| longform_policy::novel_chapter_unit_band_label())
    ));
    lines.push(format!(
        "故事前提：{}",
        empty_display(&effective.fiction_premise, "根据用户需求生成")
    ));
    lines.push(format!(
        "终局方向：{}",
        empty_display(&effective.fiction_ending_direction, "根据用户需求生成")
    ));
    lines.push(format!(
        "主角弧线：{}",
        empty_display(&effective.fiction_protagonist_arc, "根据用户需求生成")
    ));
    lines.push(format!(
        "世界观意象：{}",
        empty_display(&effective.fiction_world_imagery, "根据用户需求生成")
    ));
    lines.push(format!(
        "总主线因果链：{}",
        empty_display(&effective.fiction_main_causal_spine, "根据用户需求生成")
    ));
    lines.push(format!(
        "已锁定角色：{}",
        empty_display(&characters.join("；"), "本轮重新生成具体姓名和角色锚点")
    ));
    lines.push(format!(
        "已记录大纲：{}",
        empty_display(
            &creation_outline_payload(&effective),
            "本轮重新生成分卷和近期章节"
        )
    ));
    preview_text(&lines.join("\n"), 3000).to_string()
}

pub(crate) fn pending_normalized_contract_for_repair_anchor(
    draft: &SessionCreationDraftState,
) -> Option<NovelCreationContract> {
    let normalized = draft
        .pending_contract_candidate
        .as_ref()
        .and_then(|value| value.get("normalized"))?;
    let text = serde_json::to_string(normalized).ok()?;
    NovelCreationContract::parse_json_boundary(&text)
}

pub(crate) fn creation_draft_pending_quality_repair_issues(
    draft: &SessionCreationDraftState,
) -> Vec<String> {
    draft
        .diagnostics
        .iter()
        .chain(draft.planning_notes.iter())
        .rev()
        .find_map(|note| {
            note.strip_prefix(CONTRACT_QUALITY_BLOCKER_DIAGNOSTIC_PREFIX)
                .or_else(|| {
                    note.split_once("上一版合同草案未通过质量门")
                        .map(|(_, rest)| rest)
                })
                .or_else(|| {
                    note.split_once("合同草案未通过质量门")
                        .map(|(_, rest)| rest)
                })
        })
        .map(|rest| {
            rest.trim_start_matches(|ch| matches!(ch, '：' | ':' | ' ' | '\t'))
                .split('；')
                .map(str::trim)
                .filter(|issue| !issue.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persisted_outline_semantic_blocker_keeps_plot_stage_ownership() {
        let issue = "ContractBlocker[semantic.outline_character_authority]: 大纲事件违反角色底线"
            .to_string();
        let findings = persisted_contract_quality_findings(std::slice::from_ref(&issue));

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].code, "semantic.outline_character_authority");
        assert_eq!(
            findings[0].kind,
            super::super::super::issue::ContractIssueKind::Plot
        );
        assert_eq!(findings[0].text, issue);
    }

    #[test]
    fn draft_quality_repair_reads_the_latest_persisted_blocker() {
        let mut draft = build_initial_creation_draft(
            "persisted-quality-blocker",
            "fiction",
            "写赛博朋克小说，每章2500字，一共10万字。",
        )
        .expect("draft");
        record_contract_quality_blocker_diagnostic(
            &mut draft,
            &[
                "ContractBlocker[semantic.outline_character_authority]: 大纲事件违反角色底线"
                    .to_string(),
            ],
        );

        assert_eq!(
            creation_draft_pending_quality_repair_issues(&draft),
            vec![
                "ContractBlocker[semantic.outline_character_authority]: 大纲事件违反角色底线"
                    .to_string()
            ]
        );
    }

    #[test]
    fn persisted_user_story_blocker_uses_candidate_evidence_owner() {
        let issue = "ContractBlocker[semantic.user_story_authority]: 用户修订未落实；权威证据 后续明确修订=`删除自指关系`；候选证据 合同大纲-第2卷=`仍保留自指关系`".to_string();
        let findings = persisted_contract_quality_findings(std::slice::from_ref(&issue));

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].code, "semantic.user_story_authority");
        assert_eq!(
            findings[0].kind,
            super::super::super::issue::ContractIssueKind::Plot
        );
        assert_eq!(findings[0].evidence.field, "合同大纲-第2卷");
    }

    #[test]
    fn persisted_internal_semantic_blocker_uses_candidate_evidence_owner() {
        let issue = "ContractBlocker[semantic.outline_character_authority]: 核心道具名称漂移；权威证据 书名=`噬骨罗盘`；候选证据 故事前提=`蚀骨罗盘`".to_string();
        let findings = persisted_contract_quality_findings(std::slice::from_ref(&issue));

        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].kind,
            super::super::super::issue::ContractIssueKind::Skeleton
        );
        assert_eq!(findings[0].evidence.field, "故事前提");
    }
}
