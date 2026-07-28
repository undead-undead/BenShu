#[cfg(test)]
use super::issue::{ContractIssue, ContractIssueEvidence};
use super::issue::{ContractIssueKind, ContractIssueList, ContractIssueSet};
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContractCompletionStage {
    Skeleton,
    Characters,
    Plot,
    Governance,
}

pub fn contract_completion_stage_output_budget(stage: ContractCompletionStage) -> u64 {
    match stage {
        ContractCompletionStage::Skeleton => 4096,
        ContractCompletionStage::Characters => 4096,
        ContractCompletionStage::Plot => 6144,
        ContractCompletionStage::Governance => 6144,
    }
}

pub fn contract_completion_stage_output_budget_for_issues(
    stage: ContractCompletionStage,
    issues: &ContractIssueList,
) -> u64 {
    let stage_issues = super::patch_prompt::stage_relevant_contract_issues(stage, issues);
    if matches!(stage, ContractCompletionStage::Governance)
        && (super::patch_prompt::governance_issue_focus_is_only_visible_fields(&stage_issues)
            || super::patch_prompt::governance_issue_focus_is_only_relationship_ledger(
                &stage_issues,
            ))
    {
        return 2048;
    }
    if matches!(stage, ContractCompletionStage::Plot)
        && super::patch_prompt::plot_issue_focus_is_only_near_chapters(&stage_issues)
    {
        return 2048;
    }
    contract_completion_stage_output_budget(stage)
}

pub fn select_contract_completion_stage(
    draft: &SessionCreationDraftState,
    issues: &ContractIssueList,
) -> ContractCompletionStage {
    select_contract_completion_stage_excluding(draft, issues, &[])
        .unwrap_or(ContractCompletionStage::Governance)
}

pub fn select_contract_completion_stage_excluding(
    draft: &SessionCreationDraftState,
    issues: &ContractIssueList,
    excluded: &[ContractCompletionStage],
) -> Option<ContractCompletionStage> {
    contract_completion_stage_candidates(draft, issues)
        .into_iter()
        .find(|stage| !excluded.contains(stage))
}

fn contract_completion_stage_candidates(
    draft: &SessionCreationDraftState,
    issues: &ContractIssueList,
) -> Vec<ContractCompletionStage> {
    let effective_draft = staged_effective_draft(draft);
    let draft = &effective_draft;
    let characters_ready = fiction_characters_ready(draft);
    let issue_set = ContractIssueSet::new(issues);
    let actionable_issues = issue_set.actionable().collect::<Vec<_>>();
    let has_title_metadata_issue = actionable_issues
        .iter()
        .any(|issue| issue.code.starts_with("contract.title"));
    let has_only_title_metadata_issues = !actionable_issues.is_empty()
        && actionable_issues
            .iter()
            .all(|issue| issue.code.starts_with("contract.title"));
    let has_user_story_skeleton_issue = actionable_issues.iter().any(|issue| {
        issue.code == "semantic.user_story_authority" && issue.kind == ContractIssueKind::Skeleton
    });
    let has_user_story_plot_issue = actionable_issues.iter().any(|issue| {
        issue.code == "semantic.user_story_authority" && issue.kind == ContractIssueKind::Plot
    });
    let has_user_story_character_issue = actionable_issues.iter().any(|issue| {
        issue.code == "semantic.user_story_authority" && issue.kind == ContractIssueKind::Characters
    });
    let skeleton_fields_ready = !value_missing(&draft.fiction_premise)
        && !value_missing(&draft.fiction_ending_direction)
        && !value_missing(&draft.fiction_world_imagery)
        && !value_missing(&draft.fiction_main_causal_spine);
    let has_non_title_skeleton_issue = actionable_issues.iter().any(|issue| {
        issue.kind == ContractIssueKind::Skeleton && !issue.code.starts_with("contract.title")
    });
    let mut stages = Vec::new();
    if !skeleton_fields_ready || has_user_story_skeleton_issue || has_only_title_metadata_issues {
        push_stage_once(&mut stages, ContractCompletionStage::Skeleton);
    }
    if !characters_ready || has_user_story_character_issue {
        push_stage_once(&mut stages, ContractCompletionStage::Characters);
    }
    if actionable_issues
        .iter()
        .any(|issue| issue.code == "contract.character_anchor")
    {
        push_stage_once(&mut stages, ContractCompletionStage::Characters);
    }
    if skeleton_fields_ready && has_non_title_skeleton_issue && !has_user_story_skeleton_issue {
        push_stage_once(&mut stages, ContractCompletionStage::Skeleton);
    }
    let plot_surface_ready = !draft.fiction_outline.trim().is_empty();
    let typed_plot_plan_ready = strong_novel_contract_from_creation_draft(draft)
        .outline
        .has_stage_or_near_chapter_plan();
    let governance_ready = fiction_list_ready(&draft.fiction_themes)
        && fiction_list_ready(&draft.fiction_world_rules)
        && fiction_list_ready(&draft.fiction_style_rules)
        && fiction_list_ready(&draft.fiction_must_avoid);
    let plot_issue_requires_patch = actionable_issues.iter().any(|issue| {
        issue.kind == ContractIssueKind::Plot
            && (issue.code != "contract.outline.plan" || !typed_plot_plan_ready)
    });
    if !plot_surface_ready || plot_issue_requires_patch || has_user_story_plot_issue {
        push_stage_once(&mut stages, ContractCompletionStage::Plot);
    }
    if has_title_metadata_issue {
        push_stage_once(&mut stages, ContractCompletionStage::Skeleton);
    }
    if issue_set.has_actionable(ContractIssueKind::Characters) {
        push_stage_once(&mut stages, ContractCompletionStage::Characters);
    }
    if !governance_ready || issue_set.has_actionable(ContractIssueKind::Governance) {
        push_stage_once(&mut stages, ContractCompletionStage::Governance);
    }
    if stages.is_empty() {
        push_stage_once(&mut stages, ContractCompletionStage::Governance);
    }
    stages
}

fn push_stage_once(stages: &mut Vec<ContractCompletionStage>, stage: ContractCompletionStage) {
    if !stages.contains(&stage) {
        stages.push(stage);
    }
}

fn fiction_list_ready(values: &[String]) -> bool {
    values.iter().any(|value| !value_missing(value))
}

fn fiction_characters_ready(draft: &SessionCreationDraftState) -> bool {
    if draft.fiction_characters.len() < 3 {
        return false;
    }
    let characters = draft
        .fiction_characters
        .iter()
        .map(|line| super::draft_character_line_to_contract(line))
        .collect::<Vec<_>>();
    let has_primary = characters
        .iter()
        .any(|character| character.role_looks_primary());
    let has_supporting = characters
        .iter()
        .any(|character| !character.role_looks_primary());
    has_primary
        && has_supporting
        && characters.iter().all(|character| {
            !value_missing(&character.canonical_name)
                && !value_missing(&character.role)
                && !value_missing(&character.desire)
                && !value_missing(&character.fear)
                && !value_missing(&character.bottom_line)
        })
}

pub fn final_prompt_from_staged_contract_completion(
    draft: &SessionCreationDraftState,
    user_message: &str,
    issues: &ContractIssueList,
) -> String {
    let stage = select_contract_completion_stage(draft, issues);
    final_prompt_from_staged_contract_completion_stage(draft, user_message, issues, stage)
}

pub fn final_prompt_from_initial_contract_batch(
    draft: &SessionCreationDraftState,
    user_message: &str,
) -> String {
    let effective_draft = staged_effective_draft(draft);
    let effective = &effective_draft;
    let language_boundary = creation_planning_language_boundary(&effective.language);
    let exact_total_units = effective
        .target_units
        .map(|value| value.to_string())
        .unwrap_or_else(|| "未指定".to_string());
    let exact_chapter_unit = effective
        .chapter_unit_target
        .map(|value| value.to_string())
        .unwrap_or_else(|| {
            format!(
                "未指定，可选 {}",
                longform_policy::novel_chapter_unit_band_label()
            )
        });
    let expected_chapters = effective
        .target_units
        .zip(effective.chapter_unit_target)
        .and_then(|(total, per_chapter)| {
            longform_policy::expected_chapter_count(total, per_chapter)
        })
        .unwrap_or_default();
    super::patch_prompt::initial_contract_batch_prompt(
        effective,
        user_message,
        &exact_total_units,
        &exact_chapter_unit,
        expected_chapters,
        &language_boundary,
    )
}

pub fn final_prompt_from_staged_contract_completion_stage(
    draft: &SessionCreationDraftState,
    user_message: &str,
    issues: &ContractIssueList,
    stage: ContractCompletionStage,
) -> String {
    let effective_draft = staged_effective_draft(draft);
    let effective = &effective_draft;
    let language_boundary = creation_planning_language_boundary(&effective.language);
    let exact_total_units = effective
        .target_units
        .map(|value| value.to_string())
        .unwrap_or_else(|| "未指定".to_string());
    let exact_chapter_unit = effective
        .chapter_unit_target
        .map(|value| value.to_string())
        .unwrap_or_else(|| {
            format!(
                "未指定，可选 {}",
                longform_policy::novel_chapter_unit_band_label()
            )
        });
    let expected_chapters = effective
        .target_units
        .zip(effective.chapter_unit_target)
        .and_then(|(total, per_chapter)| {
            longform_policy::expected_chapter_count(total, per_chapter)
        })
        .unwrap_or_default();
    let stable_anchor = staged_contract_anchor(effective, issues);
    super::patch_prompt::final_prompt_from_patch_completion(
        effective,
        user_message,
        stage,
        issues,
        &stable_anchor,
        &exact_total_units,
        &exact_chapter_unit,
        expected_chapters,
        &language_boundary,
    )
    .unwrap_or_else(|| "合同补齐阶段暂不可用，请稍后重试。".to_string())
}

fn staged_effective_draft(draft: &SessionCreationDraftState) -> SessionCreationDraftState {
    creation_draft_with_pending_contract_applied(draft)
}

fn staged_contract_anchor(draft: &SessionCreationDraftState, issues: &ContractIssueList) -> String {
    let mut lines = Vec::new();
    for authority in draft
        .planning_notes
        .iter()
        .filter_map(|note| note.strip_prefix("用户故事核心权威："))
    {
        lines.push(format!(
            "用户故事核心权威（高于模型生成的故事字段，不得改变其核心行为主体、对手目的、作案/冲突机制和终局结果）：{}",
            authority.trim()
        ));
    }
    let forbidden = super::forbidden_naming_authority(draft);
    if !forbidden.titles.is_empty() {
        lines.push(format!(
            "用户明确禁用书名（不得再次作为作品书名，但不禁止这些词作为普通故事词汇）：{}",
            forbidden.titles.join("、")
        ));
    }
    if !forbidden.character_names.is_empty() {
        lines.push(format!(
            "用户明确禁用角色名（不得作为当前角色姓名或故事中的人物称呼；历史姓名记录除外）：{}",
            forbidden.character_names.join("、")
        ));
    }
    lines.push(format!("题材：{}", empty_display(&draft.genre, "未指定")));
    lines.push(format!("简述：{}", empty_display(&draft.brief, "未指定")));
    if issues
        .iter()
        .any(|issue| issue.code.starts_with("contract.title"))
    {
        lines.push("书名：待重新生成（当前书名未通过质量门，不得复用）".to_string());
    } else {
        lines.push(format!("书名：{}", empty_display(&draft.title, "尚未锁定")));
    }
    lines.push(format!(
        "故事前提：{}",
        empty_display(&draft.fiction_premise, "待补")
    ));
    lines.push(format!(
        "终局方向：{}",
        empty_display(&draft.fiction_ending_direction, "待补")
    ));
    lines.push(format!(
        "主角弧线：{}",
        empty_display(&draft.fiction_protagonist_arc, "待补")
    ));
    lines.push(format!(
        "世界观意象：{}",
        empty_display(&draft.fiction_world_imagery, "待补")
    ));
    lines.push(format!(
        "总主线因果链：{}",
        empty_display(&draft.fiction_main_causal_spine, "待补")
    ));
    if !draft.fiction_characters.is_empty() {
        lines.push(format!(
            "角色权威表：{}",
            draft.fiction_characters.join("；")
        ));
    }
    if !draft.fiction_outline.trim().is_empty() {
        lines.push(format!("大纲：{}", draft.fiction_outline.trim()));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_issue(code: &str, kind: ContractIssueKind, text: &str) -> super::ContractIssue {
        super::ContractIssue::new(
            code,
            kind,
            super::ContractIssueEvidence::new("test", text),
            text,
        )
    }

    #[test]
    fn focused_governance_budget_ignores_unrelated_character_issues() {
        let mut issues = ContractIssueList::from_messages(
            "contract.governance",
            ContractIssueKind::Governance,
            "governance",
            vec![
                "ContractBlocker: 小说合同缺少世界规则".to_string(),
                "ContractBlocker: 小说合同缺少必须避免".to_string(),
                "ContractBlocker: 小说合同缺少核心主题".to_string(),
            ],
        );
        issues.push_issue(test_issue(
            "contract.character_authority",
            ContractIssueKind::Characters,
            "ContractBlocker: 小说合同角色权威表缺少明确主角",
        ));

        assert_eq!(
            contract_completion_stage_output_budget_for_issues(
                ContractCompletionStage::Governance,
                &issues,
            ),
            2048
        );
    }

    #[test]
    fn invalid_title_is_not_rendered_as_a_stable_anchor() {
        let mut draft = super::build_initial_creation_draft(
            "session-title-repair-anchor",
            "fiction",
            "写历史商战小说，每章2500字，一共5万字",
        )
        .expect("draft");
        draft.title = "盐铁账".to_string();

        let issues = ContractIssueList::single(
            "contract.title",
            ContractIssueKind::Skeleton,
            "title",
            "ContractBlocker: 盐铁账: 书名像裸制度或账册名词",
        );
        let anchor = staged_contract_anchor(&draft, &issues);

        assert!(anchor.contains("当前书名未通过质量门，不得复用"));
        assert!(!anchor.contains("书名：盐铁账"));
    }

    #[test]
    fn staged_anchor_keeps_the_complete_bounded_outline_for_repairs() {
        let mut draft = super::build_initial_creation_draft(
            "session-complete-outline-anchor",
            "fiction",
            "写都市小说，每章2500字，一共10万字",
        )
        .expect("draft");
        draft.fiction_outline = [
            "第1卷《起势》：建立第一条线索；卷尾变化：对手开始施压。",
            &"中段推进。".repeat(180),
            "第5卷《终局》：完成核心冲突；卷尾变化：末卷终局兑现。",
            "第8章 本章目标：保留后续主线债务；预期转折：发现更大的利益链。",
        ]
        .join("\n");

        let anchor = staged_contract_anchor(&draft, &ContractIssueList::default());

        assert!(anchor.contains("第1卷《起势》"));
        assert!(anchor.contains("第5卷《终局》"));
        assert!(anchor.contains("第8章 本章目标"));
    }

    #[test]
    fn plot_stage_is_not_starved_by_repairable_character_anchor_issue() {
        let mut draft = super::build_initial_creation_draft(
            "session-stage-plot-after-characters",
            "fiction",
            "写都市言情小说，每章2500字，至少5万字。",
        )
        .expect("draft");
        draft.fiction_premise = "职场误会把两个人推到同一个公开危机里。".to_string();
        draft.fiction_ending_direction = "主角公开真相后仍保留事业和情感选择。".to_string();
        draft.fiction_world_imagery = "玻璃幕墙、深夜茶水间、被隐藏的合同条款".to_string();
        draft.fiction_main_causal_spine =
            "合同黑箱引发误会，证据和信任逐步推动终局公开。".to_string();
        draft.fiction_characters = vec![
            "姓名：钟望宁；角色定位：主角；欲望：守住职业尊严；恐惧：失去判断；底线：不伪造证据；弧线起点：习惯退让；弧线终点：主动选择".to_string(),
            "姓名：白望棠；角色定位：关键关系对象；欲望：帮助钟栖晚看清真相；恐惧：再次失去信任；底线：不利用钟望宁的脆弱；弧线起点：保持距离；弧线终点：共同承担".to_string(),
            "姓名：许砚安；角色定位：对手；欲望：压住合同漏洞；恐惧：失去资源；底线：不公开核心证据；弧线起点：操控局面；弧线终点：被迫让步".to_string(),
        ];

        let mut issues = ContractIssueList::single(
            "contract.character_reference",
            ContractIssueKind::Characters,
            "characters",
            "ContractBlocker: 角色 `白望棠` 的欲望锚点引用了权威表外角色 `钟栖晚`",
        );
        issues.extend_findings([
            test_issue(
                "contract.outline",
                ContractIssueKind::Plot,
                "ContractBlocker: 小说合同缺少分卷/阶段安排或近期章节包",
            ),
            test_issue(
                "contract.outline",
                ContractIssueKind::Plot,
                "小说合同尚未形成逐章规划或分卷/阶段大纲",
            ),
        ]);
        let stage = select_contract_completion_stage(&draft, &issues);

        assert_eq!(stage, ContractCompletionStage::Plot);
    }

    #[test]
    fn typed_outline_blocker_routes_to_plot_even_when_raw_outline_summary_exists() {
        let mut draft = super::build_initial_creation_draft(
            "session-stage-typed-outline-after-summary",
            "fiction",
            "写星际科幻小说，每章2500字，一共10万字。",
        )
        .expect("draft");
        draft.fiction_premise = "导航员发现航船收到来自自身未来的求救信号。".to_string();
        draft.fiction_ending_direction =
            "导航员公开被改写的船史，并让乘员共同决定真实航线。".to_string();
        draft.fiction_world_imagery = "世代航船、折叠航图与记忆档案库。".to_string();
        draft.fiction_main_causal_spine =
            "未来信号暴露船史矛盾，导航员沿航线证据追到记忆篡改源头。".to_string();
        draft.fiction_characters = vec![
            "姓名：季观澜；角色定位：主角；欲望：找回真实航线；恐惧：航船失去目标；底线：不以乘员记忆换取抵达；弧线起点：服从航图；弧线终点：公开真相".to_string(),
            "姓名：顾砚舟；角色定位：关键同伴；欲望：保存船史；恐惧：档案被彻底覆盖；底线：不伪造证据；弧线起点：独自查证；弧线终点：共同公开".to_string(),
            "姓名：谢临川；角色定位：关键对手；欲望：维持既定航线；恐惧：篡改曝光；底线：不交出主脑权限；弧线起点：控制档案；弧线终点：失去垄断".to_string(),
        ];
        draft.fiction_outline =
            "导航员从未来信号入局，逐步查清航线与船史矛盾，最终公开记忆篡改。".to_string();
        draft.fiction_themes = vec!["记忆权与共同选择".to_string()];
        draft.fiction_world_rules = vec!["每次改写公共记忆都会留下航图校验差异。".to_string()];
        draft.fiction_style_rules = vec!["以航行行动和证据推进悬疑。".to_string()];
        draft.fiction_must_avoid = vec!["避免无代价改写全船记忆。".to_string()];

        let mut issues = ContractIssueList::single(
            "contract.outline.plan",
            ContractIssueKind::Plot,
            "outline",
            "ContractBlocker: 小说合同缺少分卷/阶段安排或近期章节包",
        );
        issues.push_issue(test_issue(
            "contract.structured.authored",
            ContractIssueKind::Governance,
            "ContractBlocker: 小说合同缺少可执行的结构化治理内容，不能锁定为章节写作权威",
        ));

        assert_eq!(
            select_contract_completion_stage(&draft, &issues),
            ContractCompletionStage::Plot
        );
        assert_eq!(
            select_contract_completion_stage_excluding(
                &draft,
                &issues,
                &[ContractCompletionStage::Plot],
            ),
            Some(ContractCompletionStage::Governance)
        );
    }

    #[test]
    fn populated_outline_with_terminal_blocker_returns_to_plot_owner() {
        let mut draft = super::build_initial_creation_draft(
            "session-stage-terminal-outline-repair",
            "fiction",
            "写赛博朋克小说，每章2500字，一共10万字。",
        )
        .expect("draft");
        draft.fiction_premise = "记忆税迫使底层技师修复非法旧记忆。".to_string();
        draft.fiction_ending_direction = "主角公开记忆税原始账本并切断垄断系统。".to_string();
        draft.fiction_world_imagery = "海上巨城、记忆仓与税务节点。".to_string();
        draft.fiction_main_causal_spine =
            "非法旧记忆引出税务黑箱，证据推进到终局公开。".to_string();
        draft.fiction_characters = vec![
            "姓名：沈砚川；角色定位：主角；欲望：修复被征税的真实记忆；恐惧：失去自我；底线：不伪造证据；弧线起点：只求自保；弧线终点：公开真相".to_string(),
            "姓名：顾听澜；角色定位：关键同伴；欲望：找回家人的记忆；恐惧：证据被销毁；底线：不牺牲无辜者；弧线起点：拒绝合作；弧线终点：共同承担".to_string(),
            "姓名：陆承枢；角色定位：对手；欲望：维持记忆税垄断；恐惧：旧账公开；底线：不交出核心权限；弧线起点：控制全城；弧线终点：失去制度权力".to_string(),
        ];
        draft.fiction_outline =
            "第1卷：主角取得旧记忆；第2卷：主角调查税务节点；第3卷：尾声。".to_string();
        draft.fiction_themes = vec!["记忆权与人格尊严".to_string()];
        draft.fiction_world_rules = vec!["读取记忆必须支付记忆税".to_string()];
        draft.fiction_style_rules = vec!["以行动和证据推进".to_string()];
        draft.fiction_must_avoid = vec!["避免无代价改写记忆".to_string()];
        let issues = ContractIssueList::single(
            "contract.outline",
            ContractIssueKind::Plot,
            "outline",
            "ContractBlocker[outline.terminal_coverage]: 小说合同末卷没有执行权威终局的核心解决事件",
        );

        assert_eq!(
            select_contract_completion_stage(&draft, &issues),
            ContractCompletionStage::Plot
        );
    }

    #[test]
    fn missing_character_authority_precedes_stale_name_cleanup_in_filled_skeleton() {
        let mut draft = super::build_initial_creation_draft(
            "session-stage-characters-before-name-cleanup",
            "fiction",
            "写修仙小说，每章2500字，一共10万字。",
        )
        .expect("draft");
        draft.fiction_premise = "林尘身负绝灵体，被残星碎片选中。".to_string();
        draft.fiction_ending_direction = "主角重开灵脉并终结百年干旱。".to_string();
        draft.fiction_world_imagery = "枯竭灵脉、残星与骨碑。".to_string();
        draft.fiction_main_causal_spine = "残星现世引发争夺，主角追查到终局重铸天枢。".to_string();
        draft.fiction_characters.clear();

        let mut issues = ContractIssueList::single(
            "contract.story_external_character_reference",
            ContractIssueKind::Skeleton,
            "premise",
            "ContractBlocker: 小说合同故事前提引用了角色权威表外角色 `林尘身负`",
        );
        issues.push_issue(test_issue(
            "contract.character_authority",
            ContractIssueKind::Characters,
            "ContractBlocker: 小说合同角色权威表缺少明确主角",
        ));

        assert_eq!(
            select_contract_completion_stage(&draft, &issues),
            ContractCompletionStage::Characters
        );
    }

    #[test]
    fn user_story_semantic_conflict_routes_to_its_typed_owner() {
        let mut draft = super::build_initial_creation_draft(
            "session-stage-user-story-semantic",
            "fiction",
            "写末日废土小说，每章5000字，一共100万字。",
        )
        .expect("draft");
        draft.fiction_premise = "测绘员发现酸雨预报被人为延迟。".to_string();
        draft.fiction_ending_direction = "主角广播真实预报并终结信息垄断。".to_string();
        draft.fiction_world_imagery = "高架城邦、酸雨、旧气象塔。".to_string();
        draft.fiction_main_causal_spine =
            "测绘员取得原始数据，验证延迟阴谋，最终广播真相。".to_string();
        draft.fiction_outline = "分卷规划已经存在，但含有未授权能力。".to_string();
        draft.fiction_themes = vec!["信息权力与生存尊严".to_string()];
        draft.fiction_world_rules = vec!["气象塔只能观测并广播数据".to_string()];
        draft.fiction_style_rules = vec!["以行动和证据推动悬疑".to_string()];
        draft.fiction_must_avoid = vec!["避免凭空获得天气控制能力".to_string()];
        draft.fiction_characters = vec![
            "姓名：商清岚；角色定位：主角；欲望：公开真相；恐惧：证据被毁；底线：不牺牲居民；弧线起点：谨慎测绘员；弧线终点：公开证据的人".to_string(),
            "姓名：南怀遥；角色定位：同伴；欲望：保护街区；恐惧：再次失去家园；底线：不出卖同伴；弧线起点：独行者；弧线终点：自治网络联络人".to_string(),
            "姓名：沈景禾；角色定位：对手；欲望：维持信息垄断；恐惧：原始数据公开；底线：不交出塔芯；弧线起点：追捕者；弧线终点：被证据击败".to_string(),
        ];
        let issues = ContractIssueList::single(
            "semantic.user_story_authority",
            ContractIssueKind::Skeleton,
            "user_authority",
            "ContractBlocker[semantic.user_story_authority]: 故事前提、总主线因果、终局或大纲偏离用户故事核心权威；必须按用户原始核心重写这些字段并同步大纲",
        );

        assert_eq!(
            select_contract_completion_stage_excluding(&draft, &issues, &[]),
            Some(ContractCompletionStage::Skeleton)
        );
        assert_eq!(
            select_contract_completion_stage_excluding(
                &draft,
                &issues,
                &[ContractCompletionStage::Skeleton],
            ),
            None
        );

        let character_issues = ContractIssueList::single(
            "semantic.user_story_authority",
            ContractIssueKind::Characters,
            "角色权威表",
            "ContractBlocker[semantic.user_story_authority]: 男主被标成对手",
        );
        assert_eq!(
            select_contract_completion_stage_excluding(&draft, &character_issues, &[]),
            Some(ContractCompletionStage::Characters)
        );
        assert_eq!(
            select_contract_completion_stage_excluding(
                &draft,
                &character_issues,
                &[ContractCompletionStage::Characters],
            ),
            None
        );

        let plot_issues = ContractIssueList::single(
            "semantic.user_story_authority",
            ContractIssueKind::Plot,
            "outline",
            "ContractBlocker[semantic.user_story_authority]: 第2卷违反用户明确修订",
        );
        assert_eq!(
            select_contract_completion_stage_excluding(&draft, &plot_issues, &[]),
            Some(ContractCompletionStage::Plot)
        );
        assert_eq!(
            select_contract_completion_stage_excluding(
                &draft,
                &plot_issues,
                &[ContractCompletionStage::Plot],
            ),
            None
        );
    }

    #[test]
    fn incomplete_character_anchors_return_to_character_stage_before_plot() {
        let mut draft = super::build_initial_creation_draft(
            "session-stage-character-anchor-before-plot",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字。",
        )
        .expect("draft");
        draft.fiction_premise = "灵气复苏后的现代都市里，底层青年追查资源垄断。".to_string();
        draft.fiction_ending_direction = "主角公开城市灵气账本，改写资源秩序。".to_string();
        draft.fiction_world_imagery = "旧城区、天枢大阵、地下道观".to_string();
        draft.fiction_main_causal_spine =
            "古鼎契约引出城市资源垄断，证据推进到终局公开。".to_string();
        draft.fiction_characters = vec![
            "姓名：司庭棠；角色定位：主角；欲望：救回妹妹；恐惧：古鼎反噬；底线：不牺牲无辜；弧线起点：底层青年；弧线终点：秩序改写者".to_string(),
            "姓名：韩砚晚；角色定位：导师；欲望：复兴旧道观；恐惧：传承断绝；底线：契约公平".to_string(),
            "姓名：晏晴舟；角色定位：对手；欲望：维持大阵垄断；恐惧：世家根基动摇；底线：".to_string(),
        ];

        let mut issues = ContractIssueList::single(
            "contract.outline",
            ContractIssueKind::Plot,
            "outline",
            "ContractBlocker: 小说合同缺少分卷/阶段安排或近期章节包",
        );
        issues.push_issue(test_issue(
            "contract.character_anchor",
            ContractIssueKind::Characters,
            "ContractBlocker: 角色 晏晴舟（对手）缺少底线锚点",
        ));
        let stage = select_contract_completion_stage(&draft, &issues);

        assert_eq!(stage, ContractCompletionStage::Characters);
    }

    #[test]
    fn mixed_character_and_governance_issues_rotate_after_character_no_progress() {
        let mut draft = super::build_initial_creation_draft(
            "session-stage-character-governance-rotation",
            "fiction",
            "写灾难救援小说，每章2500字，一共5万字。",
        )
        .expect("draft");
        draft.fiction_premise =
            "超级台风切断海岛交通，救援队必须在潮墙抵达前转移居民。".to_string();
        draft.fiction_ending_direction =
            "救援队公开错误预警的真相，并在潮墙前完成最后一次撤离。".to_string();
        draft.fiction_world_imagery = "被海水淹没的高架、闪烁的救援信标、失联的气象塔".to_string();
        draft.fiction_main_causal_spine =
            "错误预警延误撤离，救援队追查失联气象塔，最终修正数据并完成撤离。".to_string();
        draft.fiction_outline =
            "第一阶段确认错误预警；第二阶段修复气象塔；终局完成全岛撤离。".to_string();
        draft.fiction_characters = vec![
            "姓名：陶照声；角色定位：主角；欲望：完成全岛撤离；恐惧：错误判断再次伤及居民；底线：绝不隐瞒会危及居民的数据；弧线起点：只相信模型；弧线终点：愿意承担判断责任".to_string(),
            "姓名：阮予宁；角色定位：导师；欲望：修复气象塔；恐惧：救援窗口彻底关闭；底线：绝不放弃仍有生命信号的区域；弧线起点：谨慎保守；弧线终点：公开承担决策".to_string(),
            "姓名：许砚安；角色定位：关键对手；欲望：掩盖错误预警；恐惧：决策记录被公开；底线：必须保住指挥权；弧线起点：控制信息；弧线终点：被证据逼到台前".to_string(),
        ];

        let mut issues = ContractIssueList::single(
            "contract.world_rules",
            ContractIssueKind::Governance,
            "world_rules",
            "ContractBlocker: 小说合同缺少世界规则",
        );
        issues.push_issue(test_issue(
            "contract.character_authority",
            ContractIssueKind::Characters,
            "ContractBlocker: 角色 `阮予宁`（导师）的底线锚点缺少明确边界、禁令或必须守住的行动",
        ));
        let stage = select_contract_completion_stage_excluding(
            &draft,
            &issues,
            &[ContractCompletionStage::Characters],
        );

        assert_eq!(stage, Some(ContractCompletionStage::Governance));
    }

    #[test]
    fn bad_nonempty_character_anchor_returns_to_character_stage_before_plot() {
        let mut draft = super::build_initial_creation_draft(
            "session-stage-bad-character-anchor-before-plot",
            "fiction",
            "写异界修仙小说，每章2500字，至少5万字。",
        )
        .expect("draft");
        draft.fiction_premise = "被逐出山门的修士发现灵契账册。".to_string();
        draft.fiction_ending_direction = "主角公开灵契账册并改写飞升门槛。".to_string();
        draft.fiction_world_imagery = "九幽灵契、旧山门、命格债务。".to_string();
        draft.fiction_main_causal_spine =
            "灵契账册暴露旧宗门垄断，主角用证据和修行破局。".to_string();
        draft.fiction_characters = vec![
            "姓名：祝珩川；角色定位：主角；欲望：夺回命格；恐惧：妹妹被灵契反噬；底线：即使妥协；弧线起点：被逐出山门；弧线终点：改写飞升门槛的人".to_string(),
            "姓名：韩照晚；角色定位：盟友；欲望：找回师门真相；恐惧：旧道观被抹去；底线：不私藏证据；弧线起点：市井阵修；弧线终点：公开阵眼证据的同行者".to_string(),
            "姓名：晏阙舟；角色定位：关键对手；欲望：维持宗门垄断；恐惧：灵契账册公开；底线：不承认低阶修士能制定新规；弧线起点：执掌戒律的长老；弧线终点：被新规逼到台前的人".to_string(),
        ];

        let mut issues = ContractIssueList::single(
            "contract.outline",
            ContractIssueKind::Plot,
            "outline",
            "ContractBlocker: 小说合同缺少分卷/阶段安排或近期章节包",
        );
        issues.push_issue(test_issue(
            "contract.character_anchor",
            ContractIssueKind::Characters,
            "ContractBlocker: 角色 `祝珩川`（主角）的底线锚点像全书主线、截断残句或流程说明，必须改成短的角色级锚点",
        ));
        let stage = select_contract_completion_stage(&draft, &issues);

        assert_eq!(stage, ContractCompletionStage::Characters);
    }

    #[test]
    fn title_metadata_issues_can_return_to_skeleton_stage() {
        let mut draft = super::build_initial_creation_draft(
            "session-stage-title-metadata",
            "fiction",
            "写星际科幻小说，每章2500字，至少5万字。",
        )
        .expect("draft");
        draft.fiction_premise = "拾荒者在戴森球废墟里发现旧引擎。".to_string();
        draft.fiction_ending_direction = "主角公开永动契约，让殖民地脱离能源垄断。".to_string();
        draft.fiction_world_imagery = "戴森球、旧引擎、液态光河、背景辐射。".to_string();
        draft.fiction_main_causal_spine =
            "旧引擎线索引出能源垄断，主角用契约证据反转终局。".to_string();
        draft.fiction_characters = vec![
            "姓名：孟澈弦；角色定位：主角；欲望：救回失踪同伴；恐惧：旧引擎吞掉殖民地；底线：不把平民当燃料；弧线起点：只想自保的拾荒者；弧线终点：公开能源契约的人".to_string(),
            "姓名：许砚安；角色定位：关键对手；欲望：维持能源垄断；恐惧：契约公开；底线：不亲手毁掉戴森球核心；弧线起点：垄断者；弧线终点：被迫让权".to_string(),
        ];
        draft.fiction_outline = "第一卷追查旧引擎；第二卷公开能源契约。".to_string();
        draft.fiction_world_rules = vec!["旧引擎每次启动都会消耗殖民地配额。".to_string()];
        draft.fiction_themes = vec!["能源垄断与普通人的选择".to_string()];
        draft.fiction_style_rules = vec!["硬科幻细节服务人物选择".to_string()];
        draft.fiction_must_avoid = vec!["不要把能源危机写成万能升级外挂".to_string()];

        let issues = ContractIssueList::single(
            "contract.title",
            ContractIssueKind::Skeleton,
            "title",
            "ContractBlocker: 小说合同书名未通过文字完整性和故事依据质量门",
        );
        let stage = select_contract_completion_stage(&draft, &issues);

        assert_eq!(stage, ContractCompletionStage::Skeleton);
    }

    #[test]
    fn mixed_title_metadata_issues_return_to_skeleton_after_characters_ready() {
        let mut draft = super::build_initial_creation_draft(
            "session-stage-mixed-title-metadata",
            "fiction",
            "写异界修仙小说，每章2500字，至少5万字。",
        )
        .expect("draft");
        draft.fiction_premise = "底层修士在灵契反噬中夺回命格。".to_string();
        draft.fiction_ending_direction = "主角公开灵契漏洞并改写飞升门槛。".to_string();
        draft.fiction_world_imagery = "九幽灵契、残碑、山门试炼。".to_string();
        draft.fiction_main_causal_spine =
            "灵契反噬暴露旧宗门垄断，主角借证据和修行破局。".to_string();
        draft.fiction_characters = vec![
            "姓名：司砚棠；角色定位：主角；欲望：夺回被旧宗门抽走的命格；恐惧：妹妹被灵契反噬吞掉；底线：不牺牲无辜换飞升；弧线起点：被逐出山门的低阶修士；弧线终点：改写飞升门槛的人".to_string(),
            "姓名：韩照晚；角色定位：盟友；欲望：找回师门失踪真相；恐惧：旧道观被彻底抹去；底线：不让证据被宗门私藏；弧线起点：藏身市井的阵修；弧线终点：公开阵眼证据的同行者".to_string(),
            "姓名：晏阙舟；角色定位：关键对手；欲望：维持宗门飞升垄断；恐惧：灵契账册公开；底线：不承认低阶修士能制定新规；弧线起点：执掌戒律的长老；弧线终点：被新规逼到台前的人".to_string(),
        ];
        draft.fiction_outline = "第一卷夺回命格；第二卷公开灵契漏洞。".to_string();
        draft.fiction_world_rules = vec!["灵契每次借力都会留下可追溯的命格债务。".to_string()];
        draft.fiction_themes = vec!["低阶修士改写旧秩序".to_string()];
        draft.fiction_style_rules = vec!["修行进阶必须伴随代价和选择".to_string()];
        draft.fiction_must_avoid = vec!["不要秒杀式突破".to_string()];

        let mut issues = ContractIssueList::single(
            "contract.title",
            ContractIssueKind::Skeleton,
            "title",
            "ContractBlocker: 小说合同缺少可锁定书名",
        );
        issues.push_issue(test_issue(
            "contract.world_rules",
            ContractIssueKind::Governance,
            "ContractBlocker: 小说合同缺少世界规则",
        ));
        let stage = select_contract_completion_stage(&draft, &issues);

        assert_eq!(stage, ContractCompletionStage::Skeleton);
    }
}
