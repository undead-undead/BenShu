use super::*;
use crate::tool::writing::chapter_quality;
use crate::tool::writing::creation_contract::issue::ContractIssueList;
use crate::tool::writing::longform_policy;

const TERMINAL_RESOLUTION_MARKERS: &[&str] = &[
    "建立", "确立", "接受", "进入", "完成", "实现", "达成", "终结", "瓦解", "崩塌", "击败", "消灭",
    "摧毁", "激活", "牺牲", "舍身", "献祭", "放弃", "公开", "切断", "关闭", "转为", "成为", "化身",
    "化作", "重塑", "重建", "恢复", "复苏",
];
const TERMINAL_RESOLUTION_MARKER_GROUPS: &[&[&str]] = &[
    &["转为", "成为", "化身", "化作"],
    &["建立", "确立", "重塑", "重建"],
    &["恢复", "复苏"],
    &["牺牲", "舍身", "献祭", "放弃"],
    &["终结", "瓦解", "崩塌", "击败", "消灭", "摧毁"],
];
const DEFERRED_TERMINAL_MARKERS: &[&str] = &[
    "准备",
    "筹备",
    "计划",
    "试图",
    "尝试",
    "铺路",
    "作准备",
    "做准备",
    "蓄势",
    "尚未",
    "将要",
    "即将",
    "终局前",
    "决战前",
    "决赛前",
    "最终战前",
];
const PROCESS_ONLY_TERMINAL_MARKERS: &[&str] = &[
    "寻找", "调查", "筹集", "争取", "等待", "逼近", "通往", "引向",
];

pub(super) fn validate_outline_surface(
    contract: &NovelCreationContract,
    issues: &mut ContractIssueList,
    scope: ContractReadinessScope,
) {
    issues.set_scope(
        "contract.outline",
        crate::tool::writing::creation_contract::issue::ContractIssueKind::Plot,
        "outline",
    );
    if outline_text_is_polluted(&contract.outline.raw_outline) {
        issues.push("ContractBlocker: 小说合同大纲含有结构污染或控制面文本".to_string());
    }
    if outline_text_has_duplicate_plan_clause(&contract.outline.raw_outline) {
        issues.push("ContractBlocker: 小说合同大纲含有重复规划子句".to_string());
    }
    validate_longform_plan_position(contract, issues);
    validate_outline_ending_authority(contract, issues);
    validate_outline_title_authority(contract, issues);
    validate_outline_primary_role_authority(contract, issues);
    validate_outline_role_authority(contract, issues);
    if scope == ContractReadinessScope::LockedAuthorityContract
        && contract.outline.volumes.is_empty()
    {
        issues.set_scope(
            "contract.outline.volumes",
            crate::tool::writing::creation_contract::issue::ContractIssueKind::Plot,
            "outline.volumes",
        );
        issues.push("ContractBlocker: 小说合同缺少分卷/阶段安排，不能锁定长篇阶段边界".to_string());
    }
    if scope == ContractReadinessScope::LockedAuthorityContract
        && contract.outline.near_chapters.is_empty()
    {
        issues.set_scope(
            "contract.outline.near_chapters",
            crate::tool::writing::creation_contract::issue::ContractIssueKind::Plot,
            "outline.near_chapters",
        );
        issues.push(
            "ContractBlocker: 小说合同缺少从第1章开始的近期章节包，不能锁定开篇写作窗口"
                .to_string(),
        );
    }
    if scope == ContractReadinessScope::LockedAuthorityContract {
        issues.set_scope(
            "contract.outline",
            crate::tool::writing::creation_contract::issue::ContractIssueKind::Plot,
            "outline",
        );
    }
    if scope == ContractReadinessScope::LockedAuthorityContract
        && !contract.outline.near_chapters.is_empty()
        && !contract
            .outline
            .near_chapters
            .iter()
            .any(|chapter| chapter.number == Some(1))
    {
        issues
            .push("ContractBlocker: 小说合同近期章节包缺少第1章目标，不能进入写作确认".to_string());
    }
    if scope == ContractReadinessScope::LockedAuthorityContract
        && !near_chapter_numbers_are_contiguous_from_one(&contract.outline.near_chapters)
    {
        issues.push(
            "ContractBlocker: 小说合同近期章节编号必须从第1章开始连续递增，不能跳号、重号或乱序"
                .to_string(),
        );
    }
    for volume in &contract.outline.volumes {
        if outline_text_is_polluted(&volume.title)
            || outline_text_is_polluted(&volume.objective)
            || outline_text_is_polluted(&volume.ending_change)
            || outline_text_has_duplicate_plan_clause(&volume.objective)
            || outline_text_has_duplicate_plan_clause(&volume.ending_change)
            || outline_plan_text_is_placeholder(&volume.objective)
            || outline_plan_text_is_placeholder(&volume.ending_change)
            || volume_objective_and_ending_are_equivalent(&volume.objective, &volume.ending_change)
            || volume_title_is_not_contract_title(&volume.title)
        {
            issues.push("ContractBlocker: 小说合同分卷规划含有结构污染或无效卷名".to_string());
        }
    }
    for chapter in &contract.outline.near_chapters {
        if outline_text_is_polluted(&chapter.goal)
            || outline_text_is_polluted(&chapter.expected_turn)
            || outline_plan_text_is_placeholder(&chapter.goal)
            || outline_plan_text_is_placeholder(&chapter.expected_turn)
        {
            issues.push("ContractBlocker: 小说合同近期章节包含有结构污染或占位目标".to_string());
        }
        if chapter_seed_goal_and_turn_are_equivalent(&chapter.goal, &chapter.expected_turn) {
            issues.push(
                "ContractBlocker: 小说合同近期章节目标与预期转折重复，必须写出不同的事件变化"
                    .to_string(),
            );
        }
    }
}

fn near_chapter_numbers_are_contiguous_from_one(
    chapters: &[super::super::creation_contract_model::ChapterSeedContract],
) -> bool {
    chapters
        .iter()
        .enumerate()
        .all(|(index, chapter)| chapter.number == Some(index + 1))
}

fn chapter_seed_goal_and_turn_are_equivalent(goal: &str, expected_turn: &str) -> bool {
    let goal = compact_authority_clause(goal);
    let expected_turn = compact_authority_clause(expected_turn);
    goal.chars().count() >= 8 && goal == expected_turn
}

fn validate_outline_role_authority(
    contract: &NovelCreationContract,
    issues: &mut ContractIssueList,
) {
    let mut fields = Vec::<(&'static str, &str)>::new();
    fields.push(("大纲", contract.outline.raw_outline.as_str()));
    for volume in &contract.outline.volumes {
        fields.extend([
            ("分卷目标", volume.objective.as_str()),
            ("分卷变化", volume.ending_change.as_str()),
        ]);
    }
    for chapter in &contract.outline.near_chapters {
        fields.extend([
            ("近期章节目标", chapter.goal.as_str()),
            ("近期章节转折", chapter.expected_turn.as_str()),
        ]);
    }

    for (label, text) in fields {
        for role_label in [
            "主角", "男主", "女主", "反派", "对手", "导师", "盟友", "同伴",
        ] {
            for character in &contract.characters {
                let reference = character.canonical_name.trim();
                if value_missing(reference)
                    || !outline_contains_labeled_character_reference(text, role_label, reference)
                    || outline_role_label_matches_character(role_label, character)
                {
                    continue;
                }
                issues.push(format!(
                    "ContractBlocker: 小说合同{label}把角色 `{reference}` 标成 `{role_label}`，但角色权威表定位是 `{}`",
                    character.role.trim()
                ));
            }
        }
        validate_outline_self_relationships(label, text, contract, issues);
    }
}

fn outline_contains_labeled_character_reference(text: &str, label: &str, name: &str) -> bool {
    let compact = text.replace(char::is_whitespace, "");
    [
        format!("{label}{name}"),
        format!("{label}：{name}"),
        format!("{label}:{name}"),
        format!("{label}是{name}"),
        format!("{label}的{name}"),
    ]
    .iter()
    .any(|probe| compact.contains(probe))
}

fn outline_role_label_matches_character(
    label: &str,
    character: &super::super::creation_contract_model::CharacterContract,
) -> bool {
    let role = character.role.trim();
    let lowered = role.to_ascii_lowercase();
    match label {
        "主角" | "男主" | "女主" => character.role_looks_primary(),
        "反派" | "对手" => {
            role.contains("反派")
                || role.contains("对手")
                || role.contains("敌")
                || role.contains("压力源")
                || role.contains("竞争")
                || lowered.contains("antagonist")
                || lowered.contains("opponent")
                || lowered.contains("rival")
        }
        "导师" => role.contains("导师") || role.contains("师父") || role.contains("师长"),
        "盟友" | "同伴" => {
            ["盟友", "同伴", "伙伴", "搭档", "队友", "关系对象", "朋友"]
                .iter()
                .any(|marker| role.contains(marker))
                || lowered.contains("ally")
                || lowered.contains("companion")
                || lowered.contains("partner")
        }
        _ => true,
    }
}

fn validate_outline_self_relationships(
    label: &str,
    text: &str,
    contract: &NovelCreationContract,
    issues: &mut ContractIssueList,
) {
    for clause in text.split(['。', '；', ';', '\n', '\r']) {
        let compact = clause.replace(char::is_whitespace, "");
        for character in &contract.characters {
            let name = character.canonical_name.trim();
            if value_missing(name) {
                continue;
            }
            if ["与", "和", "同"]
                .iter()
                .any(|connector| compact.contains(&format!("{name}{connector}{name}")))
            {
                issues.push(format!(
                    "ContractBlocker: 小说合同{label}形成角色 `{name}` 与自身的关系变化"
                ));
            }
        }
    }
}

fn validate_outline_ending_authority(
    contract: &NovelCreationContract,
    issues: &mut ContractIssueList,
) {
    if crate::tool::writing::contract_semantic_review::ending_equivalence_review_request(contract)
        .is_none()
    {
        return;
    }
    issues.push(
        "ContractBlocker[semantic.ending_equivalence]: 小说合同大纲的显式结局需要与权威终局方向做语义一致性复核"
            .to_string(),
    );
}

fn compact_authority_clause(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_whitespace() && !matches!(ch, '，' | ',' | '。' | '.' | '；' | ';'))
        .collect()
}

fn outline_text_has_duplicate_plan_clause(value: &str) -> bool {
    let mut seen = std::collections::HashSet::new();
    value
        .split(['；', ';', '。', '\n', '\r'])
        .map(compact_authority_clause)
        .filter(|clause| clause.chars().count() >= 8)
        .any(|clause| !seen.insert(clause))
}

fn validate_outline_title_authority(
    contract: &NovelCreationContract,
    issues: &mut ContractIssueList,
) {
    let canonical = contract.title.canonical_title.trim();
    if value_missing(canonical) || contract.outline.raw_outline.trim().is_empty() {
        return;
    }
    let mut allowed = contract
        .outline
        .volumes
        .iter()
        .map(|volume| volume.title.trim().to_string())
        .filter(|title| !value_missing(title))
        .collect::<Vec<_>>();
    allowed.extend(super::structured_gate::non_character_contract_terms(
        contract,
    ));
    for chapter in &contract.outline.near_chapters {
        allowed.extend(quoted_book_title_like_segments(&format!(
            "{}\n{}",
            chapter.goal, chapter.expected_turn
        )));
    }
    for quoted in quoted_book_title_like_segments(&contract.outline.raw_outline) {
        if quoted == canonical || allowed.iter().any(|allowed| allowed == &quoted) {
            continue;
        }
        if quoted_segment_is_explicit_chapter_title(&contract.outline.raw_outline, &quoted) {
            continue;
        }
        issues.push(format!(
            "ContractBlocker: 小说合同大纲出现与权威书名不一致的标题《{quoted}》"
        ));
    }
}

fn validate_outline_primary_role_authority(
    contract: &NovelCreationContract,
    issues: &mut ContractIssueList,
) {
    let primary_names = contract
        .characters
        .iter()
        .filter(|character| character.role_looks_primary())
        .map(|character| character.canonical_name.trim())
        .filter(|name| !value_missing(name))
        .collect::<Vec<_>>();
    if primary_names.is_empty() {
        return;
    }

    let mut fields = Vec::<(&'static str, &str)>::new();
    fields.push(("大纲", contract.outline.raw_outline.as_str()));
    for volume in &contract.outline.volumes {
        fields.extend([
            ("分卷目标", volume.objective.as_str()),
            ("分卷变化", volume.ending_change.as_str()),
        ]);
    }
    for chapter in &contract.outline.near_chapters {
        fields.extend([
            ("近期章节目标", chapter.goal.as_str()),
            ("近期章节转折", chapter.expected_turn.as_str()),
        ]);
    }

    for (label, text) in fields {
        if value_missing(text) {
            continue;
        }
        for reference in character_gate::primary_role_person_references(text) {
            if primary_names
                .iter()
                .any(|primary| character_gate::authority_name_prefix_matches(&reference, primary))
                || character_gate::reference_matches_authority_name_in_text(
                    &reference,
                    text,
                    &primary_names,
                )
            {
                continue;
            }
            issues.push(format!(
                "ContractBlocker: 小说合同{label}把 `{reference}` 标成主角，但角色权威表主角是 `{}`",
                primary_names.join(" / ")
            ));
        }
    }
}

pub(super) fn outline_text_is_polluted(value: &str) -> bool {
    let text = value.trim();
    if text.is_empty() {
        return false;
    }
    let compact = text.replace(char::is_whitespace, "");
    let lowered = compact.to_ascii_lowercase();
    if super::super::creation_contract::contract_text_contains_section_heading_residue(text)
        || outline_contains_internal_annotation(&compact)
        || outline_contains_self_identity_rewrite(&compact)
    {
        return true;
    }
    if compact.matches('《').count() != compact.matches('》').count() {
        return true;
    }
    if outline_text_has_dangling_conjunction_particle(&compact) {
        return true;
    }
    if text_ends_with_dangling_connector(&compact) {
        return true;
    }
    let unscoped_volume_changes = ["卷尾变化：", "卷尾变化:", "不可逆变化：", "不可逆变化:"]
        .iter()
        .map(|marker| text.matches(marker).count())
        .sum::<usize>();
    if unscoped_volume_changes
        > super::super::creation_contract::derive_plot_contract_from_outline_text(text)
            .volumes
            .len()
    {
        return true;
    }
    if compact.contains("第4卷《第4卷")
        || compact.contains("第5卷《第")
        || compact.contains("卷《第")
        || compact.contains("章《第")
        || compact.contains("本章目标》")
        || compact.contains("章节号")
        || compact.contains("章节审稿")
        || compact.contains("章节修订")
        || compact.contains("审稿/修订")
        || compact.contains("命名理由")
        || compact.contains("回复“开始写")
        || compact.contains("回复\"开始写")
        || compact.contains("ContractBlocker")
        || compact.contains("creation_planning")
        || compact.contains("patch_type")
        || compact.contains("json")
        || lowered.contains("contractblocker")
        || lowered.contains("patchtype")
    {
        return true;
    }
    false
}

fn outline_contains_internal_annotation(value: &str) -> bool {
    ["（注：", "(注：", "（注:", "(注:"]
        .iter()
        .any(|marker| value.contains(marker))
}

fn outline_contains_self_identity_rewrite(value: &str) -> bool {
    for (opening, closing) in [("（原", "）"), ("(原", ")")] {
        let mut remaining = value;
        while let Some(index) = remaining.find(opening) {
            let before = &remaining[..index];
            let after_opening = &remaining[index + opening.len()..];
            let Some(close_index) = after_opening.find(closing) else {
                break;
            };
            let previous = after_opening[..close_index].trim();
            if !previous.is_empty() && before.ends_with(previous) {
                return true;
            }
            remaining = &after_opening[close_index + closing.len()..];
        }
    }

    for old_marker in ["放弃旧名", "舍弃旧名", "原名为"] {
        let Some(after_old_marker) = value.split_once(old_marker).map(|(_, tail)| tail) else {
            continue;
        };
        for new_marker in ["正式更名为", "更名为", "改名为", "现名为"] {
            let Some((old_name, after_new_marker)) = after_old_marker.split_once(new_marker) else {
                continue;
            };
            let old_name = trim_identity_name_surface(old_name);
            let new_name = take_identity_name_surface(after_new_marker);
            if !old_name.is_empty() && old_name == new_name {
                return true;
            }
        }
    }
    false
}

fn trim_identity_name_surface(value: &str) -> String {
    value
        .trim_matches(|ch: char| {
            ch.is_whitespace()
                || matches!(
                    ch,
                    '‘' | '’' | '“' | '”' | '\'' | '"' | '，' | ',' | '；' | ';' | '：' | ':'
                )
        })
        .to_string()
}

fn take_identity_name_surface(value: &str) -> String {
    value
        .trim_start_matches(|ch: char| {
            ch.is_whitespace() || matches!(ch, '‘' | '“' | '\'' | '"' | '：' | ':')
        })
        .chars()
        .take_while(|ch| surface_gate::is_cjk_unified(*ch))
        .collect()
}

fn validate_longform_plan_position(
    contract: &NovelCreationContract,
    issues: &mut ContractIssueList,
) {
    if let Some(expected_chapters) = longform_plan_position_issue_expected_chapters(contract) {
        issues.push(format!(
            "ContractBlocker[outline.longform_position]: 小说合同开篇窗口或非末卷提前完成权威终局/主角弧线，但全书预计约{expected_chapters}章；必须保留后续主线债务并把终局放回末卷"
        ));
    }
    if final_volume_misses_authoritative_terminal_resolution(contract) {
        issues.push(
            "ContractBlocker[outline.terminal_coverage]: 小说合同末卷没有执行权威终局的核心解决事件；不能从尚未解决的主冲突直接跳到尾声或稳定生活，必须把终局行动、结果和不可逆变化写入实际末卷"
                .to_string(),
        );
    }
}

pub(super) fn longform_plan_position_issue_expected_chapters(
    contract: &NovelCreationContract,
) -> Option<usize> {
    let expected_chapters = contract
        .target_units
        .zip(contract.chapter_unit_target)
        .and_then(|(total, per_chapter)| {
            longform_policy::expected_chapter_count(total, per_chapter)
        })?;
    let last_near_chapter = contract
        .outline
        .near_chapters
        .iter()
        .filter_map(|chapter| chapter.number)
        .max()
        .unwrap_or_default();
    if expected_chapters <= last_near_chapter {
        return None;
    }

    let terminal_clauses = [
        contract.ending.desired_resolution.as_str(),
        contract.ending.final_state.as_str(),
        contract.protagonist_arc.as_str(),
    ]
    .into_iter()
    .filter(|value| !value_missing(value))
    .collect::<Vec<_>>();
    let authority_names = contract
        .characters
        .iter()
        .map(|character| character.canonical_name.trim())
        .filter(|name| !value_missing(name))
        .collect::<Vec<_>>();
    let near_window_completes_terminal = contract.outline.near_chapters.iter().any(|chapter| {
        terminal_clauses.iter().any(|terminal| {
            clauses_share_distinctive_event(&chapter.goal, terminal, &authority_names)
                || clauses_share_distinctive_event(
                    &chapter.expected_turn,
                    terminal,
                    &authority_names,
                )
        })
    });
    let final_volume = contract.outline.volumes.last();
    let nonfinal_volume_completes_terminal = contract
        .outline
        .volumes
        .iter()
        .take(contract.outline.volumes.len().saturating_sub(1))
        .flat_map(|volume| [volume.objective.as_str(), volume.ending_change.as_str()])
        .any(|early_clause| {
            let prematurely_resolved = terminal_clauses
                .iter()
                .copied()
                .filter(|terminal| {
                    clauses_share_distinctive_event(early_clause, terminal, &authority_names)
                })
                .collect::<Vec<_>>();
            !prematurely_resolved.is_empty()
                && !final_volume_retains_distinct_terminal_debt(
                    final_volume,
                    early_clause,
                    &prematurely_resolved,
                    &authority_names,
                )
        });
    if near_window_completes_terminal || nonfinal_volume_completes_terminal {
        Some(expected_chapters)
    } else {
        None
    }
}

fn final_volume_retains_distinct_terminal_debt(
    final_volume: Option<&super::super::creation_contract_model::VolumeContract>,
    early_clause: &str,
    terminal_clauses: &[&str],
    authority_names: &[&str],
) -> bool {
    let Some(final_volume) = final_volume else {
        return false;
    };
    if final_volume_explicitly_declares_post_terminal_only(final_volume) {
        return false;
    }
    [
        final_volume.objective.as_str(),
        final_volume.ending_change.as_str(),
    ]
    .into_iter()
    .any(|final_clause| {
        terminal_clauses
            .iter()
            .any(|terminal| clause_resolves_terminal_debt(final_clause, terminal, authority_names))
            && !clauses_share_distinctive_event(early_clause, final_clause, authority_names)
            && !clauses_share_distinctive_event(final_clause, early_clause, authority_names)
    })
}

fn final_volume_misses_authoritative_terminal_resolution(contract: &NovelCreationContract) -> bool {
    let terminal = contract.ending.desired_resolution.trim();
    let Some(final_volume) = contract.outline.volumes.last() else {
        return false;
    };
    if value_missing(terminal) {
        return false;
    }
    let authority_names = contract
        .characters
        .iter()
        .map(|character| character.canonical_name.trim())
        .filter(|name| !value_missing(name))
        .collect::<Vec<_>>();
    if contract.outline.volumes.len() == 1
        && !final_volume_explicitly_declares_post_terminal_only(final_volume)
    {
        return false;
    }
    ![
        final_volume.objective.as_str(),
        final_volume.ending_change.as_str(),
    ]
    .into_iter()
    .any(|clause| clause_resolves_terminal_debt(clause, terminal, &authority_names))
}

fn final_volume_explicitly_declares_post_terminal_only(
    final_volume: &super::super::creation_contract_model::VolumeContract,
) -> bool {
    let text = format!(
        "{}；{}；{}",
        final_volume.title, final_volume.objective, final_volume.ending_change
    )
    .replace(char::is_whitespace, "");
    [
        "终局后",
        "终局之后",
        "结局后",
        "结局之后",
        "权威终局后",
        "尾声",
        "后日谈",
        "清理余波",
        "处理余波",
        "展示新时间线",
        "展示稳定生活",
        "稳定生活",
        "生活状态",
        "无重大剧情推进",
        "无主线推进",
        "不再推进主线",
    ]
    .iter()
    .any(|marker| text.contains(marker))
}

fn clause_resolves_terminal_debt(
    final_clause: &str,
    terminal: &str,
    authority_names: &[&str],
) -> bool {
    if clauses_share_distinctive_event(final_clause, terminal, authority_names) {
        return true;
    }
    let final_text = compact_clause_without_character_authority(final_clause, authority_names);
    let terminal_text = compact_clause_without_character_authority(terminal, authority_names);
    if clause_is_explicitly_limited_stage_event(&final_text, &terminal_text)
        || clause_is_explicitly_deferred_or_process_event(&final_text, &terminal_text)
    {
        return false;
    }
    let final_chars = final_text.chars().collect::<Vec<_>>();
    let terminal_chars = terminal_text.chars().collect::<Vec<_>>();
    if final_chars.len() < 7 || terminal_chars.len() < 7 {
        return false;
    }
    let mut previous = vec![0usize; terminal_chars.len() + 1];
    let mut longest = 0usize;
    for final_char in &final_chars {
        let mut current = vec![0usize; terminal_chars.len() + 1];
        for (index, terminal_char) in terminal_chars.iter().enumerate() {
            if *final_char == *terminal_char {
                current[index + 1] = previous[index] + 1;
                longest = longest.max(current[index + 1]);
            }
        }
        previous = current;
    }
    let shared_bigrams =
        chapter_quality::shared_distinctive_bigram_count(&final_text, &terminal_text);
    longest >= 7
        || shared_bigrams >= 5
        || (shared_bigrams >= 3
            && clause_has_terminal_resolution_signal(&final_text)
            && clauses_share_terminal_resolution_marker(&final_text, &terminal_text))
}

fn clauses_share_distinctive_event(left: &str, right: &str, authority_names: &[&str]) -> bool {
    let left_text = compact_clause_without_character_authority(left, authority_names);
    let right_text = compact_clause_without_character_authority(right, authority_names);
    if clause_is_explicitly_limited_stage_event(&left_text, &right_text) {
        return false;
    }
    if clause_is_explicitly_deferred_or_process_event(&left_text, &right_text) {
        return false;
    }
    let left_has_terminal_resolution_signal = clause_has_terminal_resolution_signal(&left_text);
    let left = left_text.chars().collect::<Vec<_>>();
    let right = right_text.chars().collect::<Vec<_>>();
    if left.len() < 8 || right.len() < 8 {
        return false;
    }
    let mut previous = vec![0usize; right.len() + 1];
    let mut longest = 0usize;
    for left_char in &left {
        let mut current = vec![0usize; right.len() + 1];
        for (index, right_char) in right.iter().enumerate() {
            if *left_char == *right_char {
                current[index + 1] = previous[index] + 1;
                longest = longest.max(current[index + 1]);
            }
        }
        previous = current;
    }
    let shared_bigrams = chapter_quality::shared_distinctive_bigram_count(&left_text, &right_text);
    if left_has_terminal_resolution_signal {
        return clauses_share_terminal_resolution_marker(&left_text, &right_text)
            && (longest >= 7 || shared_bigrams >= 5);
    }
    longest >= 9
}

fn clause_is_explicitly_limited_stage_event(left: &str, terminal: &str) -> bool {
    [
        "部分",
        "局部",
        "小范围",
        "单个",
        "单一",
        "外围",
        "试点",
        "阶段性",
        "初步",
        "初次",
        "首次",
        "第一次",
        "第一步",
        "第一阶段",
        "半激活",
        "未完全",
        "未完成",
        "不稳定",
    ]
    .iter()
    .any(|marker| left.contains(marker) && !terminal.contains(marker))
}

fn clause_is_explicitly_deferred_or_process_event(left: &str, terminal: &str) -> bool {
    DEFERRED_TERMINAL_MARKERS
        .iter()
        .chain(PROCESS_ONLY_TERMINAL_MARKERS)
        .any(|marker| left.contains(marker) && !terminal.contains(marker))
}

fn compact_clause_without_character_authority(value: &str, authority_names: &[&str]) -> String {
    let mut text = compact_authority_clause(value);
    let mut names = authority_names
        .iter()
        .map(|name| name.trim())
        .filter(|name| !value_missing(name))
        .collect::<Vec<_>>();
    names.sort_unstable_by_key(|name| std::cmp::Reverse(name.chars().count()));
    names.dedup();
    for name in names {
        text = text.replace(name, "");
    }
    text
}

fn clause_has_terminal_resolution_signal(value: &str) -> bool {
    let explicitly_deferred = DEFERRED_TERMINAL_MARKERS
        .iter()
        .any(|marker| value.contains(marker));
    if explicitly_deferred {
        return false;
    }
    let direct_terminal_conflict = ["最终决战", "终局决战", "最终战", "终局之战", "大结局"]
        .iter()
        .any(|marker| value.contains(marker));
    let completes_character_arc = ["完成", "实现", "达成"]
        .iter()
        .any(|marker| value.contains(marker))
        && [
            "身份转变",
            "身份转换",
            "角色转变",
            "成长为",
            "蜕变为",
            "弧线终点",
        ]
        .iter()
        .any(|marker| value.contains(marker));
    let resolution = TERMINAL_RESOLUTION_MARKERS
        .iter()
        .any(|marker| value.contains(marker));
    let process_only_terminal_reference = direct_terminal_conflict
        && !resolution
        && PROCESS_ONLY_TERMINAL_MARKERS
            .iter()
            .any(|marker| value.contains(marker));
    !process_only_terminal_reference
        && (direct_terminal_conflict || completes_character_arc || resolution)
}

fn clauses_share_terminal_resolution_marker(left: &str, right: &str) -> bool {
    let exact_marker_match = TERMINAL_RESOLUTION_MARKERS.iter().any(|marker| {
        let Some((_, left_effect)) = left.split_once(marker) else {
            return false;
        };
        let Some((_, right_effect)) = right.split_once(marker) else {
            return false;
        };
        chapter_quality::shared_distinctive_bigram_count(left_effect, right_effect) >= 1
    });
    exact_marker_match
        || TERMINAL_RESOLUTION_MARKER_GROUPS.iter().any(|group| {
            group.iter().any(|left_marker| {
                let Some((_, left_effect)) = left.split_once(left_marker) else {
                    return false;
                };
                group.iter().any(|right_marker| {
                    let Some((_, right_effect)) = right.split_once(right_marker) else {
                        return false;
                    };
                    chapter_quality::shared_distinctive_bigram_count(left_effect, right_effect) >= 1
                })
            })
        })
}

fn outline_text_has_dangling_conjunction_particle(value: &str) -> bool {
    let mut rest = value;
    while let Some(index) = rest.find("与的") {
        let before = &rest[..index];
        let lexical_prefix = before
            .chars()
            .last()
            .is_some_and(|ch| matches!(ch, '参' | '授' | '赠' | '给'));
        if !lexical_prefix {
            return true;
        }
        rest = &rest[index + "与".len()..];
    }
    false
}

pub(crate) fn volume_title_is_not_contract_title(title: &str) -> bool {
    let compact = title.trim().replace(char::is_whitespace, "");
    if compact.is_empty() {
        return false;
    }
    let character_count = compact.chars().count();
    compact == "卷"
        || compact == "卷名"
        || compact == "标题"
        || compact == "未命名"
        || compact == "本章目标"
        || compact == "卷尾变化"
        || compact == "卷尾转折"
        || compact == "不可逆变化"
        || compact == "预期转折"
        || compact.starts_with("第") && compact.ends_with("卷")
        || compact.starts_with(['，', '、', '：'])
        || text_ends_with_dangling_connector(&compact)
        || compact
            .chars()
            .any(|ch| matches!(ch, '。' | '！' | '？' | ';' | '；' | '，' | ','))
        || character_count > 32
        || character_count > 20 && compact.chars().any(|ch| matches!(ch, '：' | ':'))
}

fn text_ends_with_dangling_connector(value: &str) -> bool {
    let compact = value.trim().trim_end_matches(['，', ',', '、', '：', ':']);
    ["从", "向", "与", "和", "及", "而", "但", "并", "或", "为"]
        .iter()
        .any(|connector| {
            let Some(prefix) = compact.strip_suffix(connector) else {
                return false;
            };
            prefix.is_empty()
                || prefix
                    .chars()
                    .last()
                    .is_some_and(|ch| matches!(ch, '，' | ',' | '、' | '：' | ':' | '；' | ';'))
        })
}

fn volume_objective_and_ending_are_equivalent(objective: &str, ending_change: &str) -> bool {
    let objective = compact_authority_clause(objective);
    let ending_change = compact_authority_clause(ending_change);
    objective.chars().count() >= 8 && objective == ending_change
}

pub(super) fn outline_plan_text_is_placeholder(value: &str) -> bool {
    let compact = value.trim().replace(char::is_whitespace, "");
    if compact.is_empty() {
        return false;
    }
    compact == "本章目标"
        || compact == "预期转折"
        || compact == "不可逆变化"
        || compact == "章节目标"
        || compact == "事件目标"
        || compact == "第1章"
        || compact == "第一章"
        || [
            "阶段证据",
            "主线债务",
            "权威终局",
            "不可逆变化",
            "章末发生",
            "章末变化",
        ]
        .iter()
        .any(|marker| compact.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::writing::creation_contract_model::{
        ChapterSeedContract, CharacterContract, VolumeContract,
    };

    #[test]
    fn volume_title_rejects_outline_field_labels() {
        for title in ["卷尾变化", "卷尾转折", "不可逆变化", "预期转折"] {
            assert!(
                volume_title_is_not_contract_title(title),
                "outline field label must not become a volume title: {title}"
            );
        }
    }

    #[test]
    fn volume_title_rejects_dangling_or_sentence_like_fragments() {
        for title in [
            "的初识线索，到",
            "与官府周旋揭露伪证，最终在",
            "决战中，以旧伤爆发为代价斩杀幕后黑手并当众揭露全部旧案真相",
        ] {
            assert!(
                volume_title_is_not_contract_title(title),
                "sentence fragment must not become a volume title: {title}"
            );
        }
        for title in ["深入盐帮腹地", "盐道初霜", "在水一方"] {
            assert!(
                !volume_title_is_not_contract_title(title),
                "natural volume title must remain valid: {title}"
            );
        }
    }

    #[test]
    fn natural_urban_volume_plan_is_not_reported_as_polluted() {
        let volumes = [
            (
                "折叠初醒",
                "梁晏桥掌握基础折叠技巧，结识程砚棠，首次引起祝谨言注意",
                "梁晏桥被迫交出核心折叠点坐标，被迫与程砚棠结成临时同盟",
            ),
            (
                "缝隙追踪",
                "程砚棠查明父亲失踪真相，梁晏桥财富积累至顶层门槛",
                "祝谨言封锁主要商业折叠点，梁晏桥转入地下网络",
            ),
            (
                "地下暗涌",
                "发现城市巨大裂缝，整合底层反抗势力",
                "梁晏桥牺牲个人最大财富来源，换取裂缝入口控制权",
            ),
            (
                "终局重塑",
                "最终决战，折叠巨大裂缝，重塑资源格局",
                "裂缝永久闭合，城市资源重新分配，梁晏桥成为守护者",
            ),
        ];

        for (title, objective, ending_change) in volumes {
            assert!(!outline_text_is_polluted(title), "title: {title}");
            assert!(
                !outline_text_is_polluted(objective),
                "objective: {objective}"
            );
            assert!(
                !outline_text_is_polluted(ending_change),
                "ending: {ending_change}"
            );
            assert!(
                !outline_text_has_duplicate_plan_clause(objective),
                "objective duplicate: {objective}"
            );
            assert!(
                !outline_text_has_duplicate_plan_clause(ending_change),
                "ending duplicate: {ending_change}"
            );
            assert!(
                !volume_objective_and_ending_are_equivalent(objective, ending_change),
                "objective and ending must differ: {title}"
            );
            assert!(!volume_title_is_not_contract_title(title), "title: {title}");
        }
    }

    #[test]
    fn natural_cyberpunk_opening_chapter_plan_is_not_reported_as_polluted() {
        let chapters = [
            (
                "记忆修复师在诊所修复工人记忆，发现其中包含公共记忆云的加密标记",
                "他打破惯例私自保留记忆副本，引起雇主不满并留下违规记录",
            ),
            (
                "修复师回家发现妹妹梦中重复陌生旋律，醒来后情绪异常平静",
                "他检测到妹妹神经接口有外部写入痕迹，决定追踪数据流向",
            ),
            (
                "修复师潜入城市边缘的废弃服务器节点，首次连接公共记忆云",
                "他发现妹妹记忆被替换为无忧模式，确认真相并非丢失而是篡改",
            ),
            (
                "修复师追踪数据源头至算法企业总部外围，遭遇安保无人机追踪",
                "他在险境中结识黑客同伴并获得进入云端的临时权限密钥",
            ),
            (
                "修复师利用密钥潜入云端底层，观察记忆被清洗后重新分配给高价值用户",
                "他发现妹妹原始记忆被用于优化城市情绪指数，决定继续追查核心替换者",
            ),
        ];

        for (goal, expected_turn) in chapters {
            assert!(!outline_text_is_polluted(goal), "goal: {goal}");
            assert!(
                !outline_text_is_polluted(expected_turn),
                "expected turn: {expected_turn}"
            );
            assert!(!outline_plan_text_is_placeholder(goal), "goal: {goal}");
            assert!(
                !outline_plan_text_is_placeholder(expected_turn),
                "expected turn: {expected_turn}"
            );
        }
    }

    #[test]
    fn outline_plan_rejects_meta_level_event_placeholders() {
        for value in [
            "建立冲突并取得阶段证据",
            "获得证据但留下主线债务",
            "偿还主线债务并推进终局",
            "完成权威终局的不可逆变化",
            "章末发生新的不可逆变化",
            "章末变化并留下后续主线债务",
        ] {
            assert!(
                outline_plan_text_is_placeholder(value),
                "meta-level outline placeholder must be rejected: {value}"
            );
        }
        for value in [
            "叶予序从剑冢取出记录宗门旧账的断剑",
            "断剑吞噬叶予序一截剑骨后显出前任剑主姓名",
            "沈照川销毁渡口账册，主角只抢出一页货单",
        ] {
            assert!(
                !outline_plan_text_is_placeholder(value),
                "concrete story event must remain valid: {value}"
            );
        }
    }

    #[test]
    fn natural_lexical_words_ending_in_connector_characters_are_not_dangling() {
        for text in [
            "他决定继续追踪数据流向",
            "她开始共同参与",
            "审计员重新核对统计总和",
            "调查范围涉及全部节点",
            "两条证据链完成合并",
            "系统记录了这次异常行为",
        ] {
            assert!(!text_ends_with_dangling_connector(text), "{text}");
        }
        assert!(text_ends_with_dangling_connector("主角核对伤口与账册，从"));
    }

    #[test]
    fn outline_surface_blocks_dangling_connector_and_repeated_volume_change() {
        assert!(outline_text_is_polluted(
            "主角沿盐道追查旧案，逐层核对伤口与账册，从"
        ));

        let mut contract = NovelCreationContract::default();
        contract.outline.volumes.push(VolumeContract {
            title: "盐道初霜".to_string(),
            objective: "主角查清第一批异常剑伤的来源".to_string(),
            ending_change: "主角查清第一批异常剑伤的来源".to_string(),
        });
        let mut issues = ContractIssueList::default();
        validate_outline_surface(
            &contract,
            &mut issues,
            ContractReadinessScope::DisplayContract,
        );

        assert!(issues
            .iter()
            .any(|issue| issue.contains("分卷规划含有结构污染或无效卷名")));
    }

    #[test]
    fn outline_surface_blocks_dangling_conjunction_particle_but_allows_participation_phrase() {
        assert!(outline_text_is_polluted("阮照弦发现K-7的节律与的心跳同步"));
        assert!(!outline_text_is_polluted("阮照弦回顾自己参与的全部实验"));
    }

    #[test]
    fn outline_surface_blocks_self_renaming_and_internal_identity_notes() {
        assert!(outline_text_is_polluted(
            "落魄千金叶承言（原叶承言）为救家族进入闻氏集团"
        ));
        assert!(outline_text_is_polluted(
            "叶承言放弃旧名‘叶承言’，正式更名为‘叶承言’"
        ));
        assert!(outline_text_is_polluted(
            "叶承言签下协议（注：原大纲姓名冲突，此处统一角色名）"
        ));
        assert!(
            !outline_text_is_polluted("沈旧宁放弃旧名‘旧宁’，正式更名为‘沈旧宁’"),
            "a genuine change between distinct identities is not self-renaming residue"
        );
    }

    #[test]
    fn outline_title_authority_allows_quoted_story_artifact_terms() {
        let mut contract = NovelCreationContract::default();
        contract.title.canonical_title = "废墟之上斩新神".to_string();
        contract.premise =
            "主角季栖遥获得上古残卷《混沌经》，发现唯有吞噬道源方可延寿。".to_string();
        contract.main_causal_spine =
            "主角获《混沌经》开启变数->旧神苏醒->主角联合盟友斩神".to_string();
        contract.outline.raw_outline =
            "第一卷：主角季栖遥获得上古残卷《混沌经》，卷入宗门权斗。".to_string();

        let mut issues = ContractIssueList::default();
        validate_outline_title_authority(&contract, &mut issues);

        assert!(
            issues.is_empty(),
            "story artifact terms must not be treated as competing book titles: {issues:?}"
        );
    }

    #[test]
    fn outline_title_authority_still_blocks_unanchored_quoted_titles() {
        let mut contract = NovelCreationContract::default();
        contract.title.canonical_title = "废墟之上斩新神".to_string();
        contract.premise = "主角季栖遥获得残卷，发现唯有吞噬道源方可延寿。".to_string();
        contract.outline.raw_outline = "第一卷：主角误入《旧书名》的主线。".to_string();

        let mut issues = ContractIssueList::default();
        validate_outline_title_authority(&contract, &mut issues);

        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("与权威书名不一致")),
            "unanchored quoted titles should still be blocked: {issues:?}"
        );
    }

    #[test]
    fn outline_primary_role_authority_blocks_non_primary_character_as_protagonist() {
        let mut contract = NovelCreationContract::default();
        contract.characters.push(CharacterContract {
            canonical_name: "唐晴声".to_string(),
            role: "主角".to_string(),
            ..Default::default()
        });
        contract.characters.push(CharacterContract {
            canonical_name: "祝栖序".to_string(),
            role: "对手".to_string(),
            ..Default::default()
        });
        contract.outline.raw_outline =
            "前提：主角祝栖序名普通的灵网线路检修员，意外获得修补裂缝能力。".to_string();

        let mut issues = ContractIssueList::default();
        validate_outline_primary_role_authority(&contract, &mut issues);

        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("把 `祝栖序` 标成主角")),
            "outline protagonist marker must obey character authority: {issues:?}"
        );
    }

    #[test]
    fn outline_primary_role_authority_allows_primary_identity_sentence() {
        let mut contract = NovelCreationContract::default();
        contract.characters.push(CharacterContract {
            canonical_name: "唐晴声".to_string(),
            role: "主角".to_string(),
            ..Default::default()
        });
        contract.outline.raw_outline =
            "前提：主角唐晴声是一名普通的灵网线路检修员，意外获得修补裂缝能力。".to_string();

        let mut issues = ContractIssueList::default();
        validate_outline_primary_role_authority(&contract, &mut issues);

        assert!(
            issues.is_empty(),
            "primary identity sentence should not be blocked: {issues:?}"
        );
    }

    #[test]
    fn locked_contract_blocks_near_chapters_without_opening_seed() {
        let mut contract = NovelCreationContract::default();
        contract.outline.near_chapters = vec![
            ChapterSeedContract {
                number: Some(2),
                goal: "主角第一次反击旧公司审查。".to_string(),
                expected_turn: "反击带来公开追捕。".to_string(),
            },
            ChapterSeedContract {
                number: Some(3),
                goal: "主角找到能力代价的第一条证据。".to_string(),
                expected_turn: "他确认胜利不是无代价的。".to_string(),
            },
        ];

        let mut issues = ContractIssueList::default();
        validate_outline_surface(
            &contract,
            &mut issues,
            ContractReadinessScope::LockedAuthorityContract,
        );

        assert!(
            issues.iter().any(|issue| issue.contains("缺少第1章目标")),
            "locked contract must not become ready without chapter 1 seed: {issues:?}"
        );
    }

    #[test]
    fn locked_contract_requires_both_volume_plan_and_near_chapter_window() {
        let mut only_near = NovelCreationContract::default();
        only_near.outline.near_chapters = vec![ChapterSeedContract {
            number: Some(1),
            goal: "主角核对第一份被改写的赈灾名册。".to_string(),
            expected_turn: "同一墨迹也出现在第二代名册上。".to_string(),
        }];
        let mut near_issues = ContractIssueList::default();
        validate_outline_surface(
            &only_near,
            &mut near_issues,
            ContractReadinessScope::LockedAuthorityContract,
        );
        assert!(
            near_issues
                .iter()
                .any(|issue| issue.code == "contract.outline.volumes"),
            "near chapters alone must not replace the long-form volume boundary: {near_issues:?}"
        );

        let mut only_volume = NovelCreationContract::default();
        only_volume.outline.volumes = vec![VolumeContract {
            title: "旧志疑墨".to_string(),
            objective: "主角确认三代名册都被同一种墨迹改写。".to_string(),
            ending_change: "主角锁定旧堤工程图是下一阶段的关键证据。".to_string(),
        }];
        let mut volume_issues = ContractIssueList::default();
        validate_outline_surface(
            &only_volume,
            &mut volume_issues,
            ContractReadinessScope::LockedAuthorityContract,
        );
        assert!(
            volume_issues
                .iter()
                .any(|issue| issue.code == "contract.outline.near_chapters"),
            "volume stages alone must not replace the opening chapter window: {volume_issues:?}"
        );
    }

    #[test]
    fn locked_contract_does_not_treat_raw_outline_words_as_typed_plan() {
        let mut contract = NovelCreationContract::default();
        contract.outline.raw_outline =
            "书名候选：断枢；第一章将从工匠入城开始，但分卷和章节字段尚未生成。".to_string();

        let mut issues = ContractIssueList::default();
        validate_outline_surface(
            &contract,
            &mut issues,
            ContractReadinessScope::LockedAuthorityContract,
        );

        assert!(
            issues
                .iter()
                .any(|issue| issue.code == "contract.outline.volumes")
                && issues
                    .iter()
                    .any(|issue| issue.code == "contract.outline.near_chapters"),
            "untyped raw text must not satisfy the locked contract plan gate: {issues:?}"
        );
    }

    #[test]
    fn locked_contract_blocks_non_contiguous_near_chapter_numbers() {
        let mut contract = NovelCreationContract::default();
        contract.outline.near_chapters = vec![
            ChapterSeedContract {
                number: Some(1),
                goal: "主角核对第一份事故记录。".to_string(),
                expected_turn: "记录暴露第一处时间戳异常。".to_string(),
            },
            ChapterSeedContract {
                number: Some(3),
                goal: "主角走访第二名证人。".to_string(),
                expected_turn: "证词指向被隐藏的设备日志。".to_string(),
            },
        ];

        let mut issues = ContractIssueList::default();
        validate_outline_surface(
            &contract,
            &mut issues,
            ContractReadinessScope::LockedAuthorityContract,
        );

        assert!(
            issues.iter().any(|issue| issue.contains("连续递增")),
            "near chapter numbering must stay stable before writing begins: {issues:?}"
        );
    }

    #[test]
    fn outline_blocks_repeated_plan_clauses_and_stale_explicit_ending() {
        let mut contract = NovelCreationContract::default();
        contract.ending.desired_resolution =
            "岑闻棠不再逃避自己的特异能力，主动修补世界裂痕".to_string();
        contract.outline.raw_outline = "起势阶段进入核心冲突。结局：岑闻棠再逃避自己的特异能力，主动修补世界裂痕卷尾变化；预期转折：主角确认这不是偶然事件；预期转折：主角确认这不是偶然事件".to_string();

        let mut issues = ContractIssueList::default();
        validate_outline_surface(
            &contract,
            &mut issues,
            ContractReadinessScope::DisplayContract,
        );

        assert!(
            issues.iter().any(|issue| issue.contains("重复规划子句")),
            "duplicate planning clauses must not pass: {issues:?}"
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("semantic.ending_equivalence")),
            "stale outline endings must require semantic review before overriding ending authority: {issues:?}"
        );
    }

    #[test]
    fn outline_role_labels_and_relationships_obey_character_authority() {
        let mut contract = NovelCreationContract::default();
        contract.characters = vec![
            CharacterContract {
                canonical_name: "阮栖澜".to_string(),
                role: "主角".to_string(),
                ..Default::default()
            },
            CharacterContract {
                canonical_name: "孟泊声".to_string(),
                role: "关键对手".to_string(),
                ..Default::default()
            },
        ];
        contract.outline.raw_outline =
            "证据把主要对手阮栖澜推到台前；阮栖澜与阮栖澜形成初步对立。".to_string();

        let mut issues = ContractIssueList::default();
        validate_outline_surface(
            &contract,
            &mut issues,
            ContractReadinessScope::DisplayContract,
        );

        assert!(
            issues.iter().any(|issue| issue.contains("标成 `对手`")),
            "role labels must match the character authority table: {issues:?}"
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("与自身的关系变化")),
            "self-relationships must not enter the outline: {issues:?}"
        );
    }

    #[test]
    fn repeated_name_after_a_valid_relationship_is_not_a_self_edge() {
        let mut contract = NovelCreationContract::default();
        contract.characters = vec![
            CharacterContract {
                canonical_name: "林深".to_string(),
                role: "主角".to_string(),
                ..Default::default()
            },
            CharacterContract {
                canonical_name: "程昭安".to_string(),
                role: "关键对手".to_string(),
                ..Default::default()
            },
        ];
        contract.outline.near_chapters = vec![ChapterSeedContract {
            number: Some(15),
            goal: "林深与程昭安在核心舱对峙，程昭安揭示筛选的最终目的".to_string(),
            expected_turn: "林深必须在保留证据与阻止清洗之间选择".to_string(),
        }];

        let mut issues = ContractIssueList::default();
        validate_outline_surface(
            &contract,
            &mut issues,
            ContractReadinessScope::DisplayContract,
        );

        assert!(
            !issues
                .iter()
                .any(|issue| issue.contains("与自身的关系变化")),
            "a valid A-B relationship followed by another B action is not a B-B edge: {issues:?}"
        );
    }

    #[test]
    fn longform_opening_window_cannot_complete_the_locked_ending() {
        let mut contract = NovelCreationContract::default();
        contract.target_units = Some(100_000);
        contract.chapter_unit_target = Some(2_500);
        contract.ending.desired_resolution =
            "温景衡拒绝资源垄断并独立运营研究所，许听安放弃收购权成为长期合作伙伴".to_string();
        contract.outline.near_chapters = vec![ChapterSeedContract {
            number: Some(8),
            goal: "温景衡筹集第一笔独立资金".to_string(),
            expected_turn: "许听安放弃收购权，两人转为长期合作伙伴".to_string(),
        }];

        let mut issues = ContractIssueList::default();
        validate_longform_plan_position(&contract, &mut issues);

        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("提前完成权威终局") && issue.contains("约40章")),
            "opening window must retain longform story debt: {issues:?}"
        );
    }

    #[test]
    fn nonfinal_volume_cannot_finish_terminal_conflict_before_an_epilogue_volume() {
        let mut contract = NovelCreationContract::default();
        contract.target_units = Some(100_000);
        contract.chapter_unit_target = Some(2_500);
        contract.ending.desired_resolution = "钟栖序拒绝将核心交给垄断巨头，而是将其嵌入废弃的高铁枢纽，激活覆盖全境的清洁能源网，迫使所有势力接受共享规则".to_string();
        contract.ending.final_state =
            "旧世界的垄断能源制度瓦解，废土进入清洁能源共享的新秩序时代".to_string();
        contract.outline.volumes = vec![
            VolumeContract {
                title: "锈蚀初醒".to_string(),
                objective: "发现地核核心并踏上旅程".to_string(),
                ending_change: "天穹集团派出追兵".to_string(),
            },
            VolumeContract {
                title: "枢纽决战".to_string(),
                objective: "抵达废弃高铁枢纽，与垄断集团主力展开最终决战".to_string(),
                ending_change: "核心激活，垄断地位崩塌，各方接受新秩序规则".to_string(),
            },
            VolumeContract {
                title: "绿野新生".to_string(),
                objective: "清理余波并描写新生活".to_string(),
                ending_change: "众人进入长期稳定状态".to_string(),
            },
        ];

        let mut issues = ContractIssueList::default();
        validate_longform_plan_position(&contract, &mut issues);

        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("非末卷提前完成权威终局")),
            "a terminal battle followed only by an epilogue volume must be rejected: {issues:?}"
        );
    }

    #[test]
    fn final_epilogue_cannot_skip_the_authoritative_terminal_action() {
        let mut contract = NovelCreationContract::default();
        contract.target_units = Some(100_000);
        contract.chapter_unit_target = Some(2_500);
        contract.characters.push(CharacterContract {
            canonical_name: "许照野".to_string(),
            role: "女主".to_string(),
            ..Default::default()
        });
        contract.ending.desired_resolution = "许照野利用观测站的旧式通讯阵列，将关键工程数据发送回三十年前的施工队，修正轨道支架的焊接缺陷".to_string();
        contract.ending.final_state =
            "地月运输网恢复稳定运行，许照野建立新的科研与回收联合体".to_string();
        contract.outline.volumes = vec![
            VolumeContract {
                title: "静海残响".to_string(),
                objective: "许照野发现未来求救信号并确认工程事故源头".to_string(),
                ending_change: "许照野取得被掩盖的事故数据".to_string(),
            },
            VolumeContract {
                title: "深渊凝视".to_string(),
                objective: "许照野夺取原始焊接缺陷证据".to_string(),
                ending_change: "对手包围观测站，通讯阵列因过载受损".to_string(),
            },
            VolumeContract {
                title: "静海黎明".to_string(),
                objective: "展示新时间线下许照野的稳定生活".to_string(),
                ending_change: "许照野看着恢复繁忙的地月航线并开始规划未来".to_string(),
            },
        ];

        let mut issues = ContractIssueList::default();
        validate_longform_plan_position(&contract, &mut issues);

        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("outline.terminal_coverage")),
            "an epilogue cannot replace the missing authoritative terminal action: {issues:?}"
        );
    }

    #[test]
    fn final_volume_accepts_synonymous_irreversible_transformation_language() {
        let mut contract = NovelCreationContract::default();
        contract.target_units = Some(100_000);
        contract.chapter_unit_target = Some(2_500);
        contract.characters.push(CharacterContract {
            canonical_name: "许照野".to_string(),
            role: "主角".to_string(),
            ..Default::default()
        });
        contract.ending.desired_resolution = "许照野化身信标照亮失联航路并恢复全境通讯".to_string();
        contract.outline.volumes = vec![
            VolumeContract {
                title: "静默航线".to_string(),
                objective: "许照野确认全境通讯断裂的源头".to_string(),
                ending_change: "许照野取得修复信标所需的核心".to_string(),
            },
            VolumeContract {
                title: "长夜信标".to_string(),
                objective: "许照野以自身为代价完成从搜救者到信标化身的转变".to_string(),
                ending_change: "许照野舍身化作信标，照亮失联航路并使全境通讯复苏".to_string(),
            },
        ];

        let mut issues = ContractIssueList::default();
        validate_longform_plan_position(&contract, &mut issues);

        assert!(
            !issues
                .iter()
                .any(|issue| issue.contains("outline.terminal_coverage")),
            "equivalent irreversible transformation verbs must preserve terminal coverage: {issues:?}"
        );
    }

    #[test]
    fn unrelated_final_volume_cannot_skip_terminal_action_without_epilogue_markers() {
        let mut contract = NovelCreationContract::default();
        contract.target_units = Some(100_000);
        contract.chapter_unit_target = Some(2_500);
        contract.ending.desired_resolution =
            "周见川潜入中央水塔关闭毒化阀门，并公开账本终止财团对全城水源的控制".to_string();
        contract.outline.volumes = vec![
            VolumeContract {
                title: "旱城账本".to_string(),
                objective: "周见川取得财团秘密账本".to_string(),
                ending_change: "水塔坐标与守卫轮班暴露".to_string(),
            },
            VolumeContract {
                title: "北境迁徙".to_string(),
                objective: "周见川护送居民穿越盐碱荒原寻找新聚落".to_string(),
                ending_change: "迁徙队伍抵达北境并建立临时营地".to_string(),
            },
        ];

        let mut issues = ContractIssueList::default();
        validate_longform_plan_position(&contract, &mut issues);

        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("outline.terminal_coverage")),
            "terminal coverage must be checked for every final volume, not only volumes labeled as an epilogue: {issues:?}"
        );
    }

    #[test]
    fn raw_outline_terminal_summary_cannot_replace_final_volume_execution() {
        let mut contract = NovelCreationContract::default();
        contract.target_units = Some(100_000);
        contract.chapter_unit_target = Some(2_500);
        contract.ending.desired_resolution = "姜照舟将心核嵌入灯塔核心并逆转骨海潮汐".to_string();
        contract.outline.raw_outline = "姜照舟最终将心核嵌入灯塔核心并逆转骨海潮汐。".to_string();
        contract.outline.volumes = vec![
            VolumeContract {
                title: "骨海边缘".to_string(),
                objective: "姜照舟取得心核并逃离追兵".to_string(),
                ending_change: "船队驶入沉船回廊".to_string(),
            },
            VolumeContract {
                title: "回廊深处".to_string(),
                objective: "姜照舟解开第一段灯塔线索".to_string(),
                ending_change: "追兵突破回廊防线，灯塔仍未抵达".to_string(),
            },
        ];

        let mut issues = ContractIssueList::default();
        validate_longform_plan_position(&contract, &mut issues);

        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("outline.terminal_coverage")),
            "a raw summary cannot execute the missing final-volume action: {issues:?}"
        );
    }

    #[test]
    fn final_volume_stopping_before_the_final_cannot_pass_by_shared_event_words() {
        let mut contract = NovelCreationContract::default();
        contract.target_units = Some(100_000);
        contract.chapter_unit_target = Some(2_500);
        contract.ending.desired_resolution =
            "沈清宁在全国大赛决赛最后一跳越过纪录线并夺冠".to_string();
        contract.outline.volumes = vec![
            VolumeContract {
                title: "港口风云".to_string(),
                objective: "沈清宁掌握跃迁技术".to_string(),
                ending_change: "沈清宁取得省队试训资格".to_string(),
            },
            VolumeContract {
                title: "逆风破晓".to_string(),
                objective: "沈清宁在省队立足".to_string(),
                ending_change: "沈清宁克服伤病，在全国大赛决赛前确立技术优势".to_string(),
            },
        ];

        let mut issues = ContractIssueList::default();
        validate_longform_plan_position(&contract, &mut issues);

        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("outline.terminal_coverage")),
            "pre-final preparation cannot execute the terminal event: {issues:?}"
        );
    }

    #[test]
    fn epilogue_cannot_use_arc_echo_to_excuse_an_already_finished_main_terminal() {
        let mut contract = NovelCreationContract::default();
        contract.target_units = Some(100_000);
        contract.chapter_unit_target = Some(2_500);
        contract.characters.push(CharacterContract {
            canonical_name: "宋维安".to_string(),
            role: "女主".to_string(),
            ..Default::default()
        });
        contract.ending.desired_resolution = "宋维安将自身记忆作为密钥接入中央服务器，强制覆盖所有市民的过滤算法，销毁垄断集团核心记忆库，使被篡改的记忆回归原始状态".to_string();
        contract.protagonist_arc =
            "宋维安从漠不关心的拾荒者成长为承担全城记忆重构的觉醒者".to_string();
        contract.outline.volumes = vec![
            VolumeContract {
                title: "数据洪流".to_string(),
                objective: "宋维安取得中央服务器的物理密钥".to_string(),
                ending_change: "核心记忆库防御瓦解，宋维安准备执行全城覆盖".to_string(),
            },
            VolumeContract {
                title: "零号重构".to_string(),
                objective: "宋维安强制覆盖所有市民的过滤算法，销毁垄断集团核心记忆库，使城市记忆回归原始状态".to_string(),
                ending_change: "记忆垄断终结，宋维安完成牺牲与觉醒".to_string(),
            },
            VolumeContract {
                title: "余波".to_string(),
                objective: "展示记忆重构后的城市变化".to_string(),
                ending_change: "宋维安确认自由并非终点，觉醒者开始新的生活".to_string(),
            },
        ];

        let mut issues = ContractIssueList::default();
        validate_longform_plan_position(&contract, &mut issues);

        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("非末卷提前完成权威终局")),
            "an arc echo must not stand in for unresolved main-terminal debt: {issues:?}"
        );
    }

    #[test]
    fn self_declared_post_terminal_volume_cannot_repeat_one_ending_component() {
        let mut contract = NovelCreationContract::default();
        contract.target_units = Some(100_000);
        contract.chapter_unit_target = Some(2_500);
        contract.ending.desired_resolution = "两人正式结婚，女主完成事业与关系选择".to_string();
        contract.protagonist_arc = "女主成为独立企业家并主动选择爱情".to_string();
        contract.outline.volumes = vec![
            VolumeContract {
                title: "破茧".to_string(),
                objective: "女主完成股份制改造并偿清家庭债务".to_string(),
                ending_change: "工厂恢复盈利，新品牌进入筹备阶段".to_string(),
            },
            VolumeContract {
                title: "春暖".to_string(),
                objective: "女主成立新品牌并与伴侣共同打开市场".to_string(),
                ending_change: "两人正式结婚，女主完成事业与关系选择".to_string(),
            },
            VolumeContract {
                title: "尾声".to_string(),
                objective: "展现终局后的稳定生活并呼应开篇".to_string(),
                ending_change: "两人在旧厂区举行婚礼".to_string(),
            },
        ];

        let mut issues = ContractIssueList::default();
        validate_longform_plan_position(&contract, &mut issues);

        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("非末卷提前完成权威终局")),
            "a self-declared post-terminal volume cannot excuse an early ending: {issues:?}"
        );
    }

    #[test]
    fn nonfinal_volume_cannot_complete_same_terminal_event_with_different_outcome_verbs() {
        let mut contract = NovelCreationContract::default();
        contract.target_units = Some(100_000);
        contract.chapter_unit_target = Some(2_500);
        contract.characters = vec![
            CharacterContract {
                canonical_name: "程屿原".to_string(),
                role: "女主".to_string(),
                ..Default::default()
            },
            CharacterContract {
                canonical_name: "秦晏声".to_string(),
                role: "同伴".to_string(),
                ..Default::default()
            },
            CharacterContract {
                canonical_name: "韩谨宁".to_string(),
                role: "对手".to_string(),
                ..Default::default()
            },
        ];
        contract.ending.desired_resolution = "程屿原利用修补后的完整账册，在朝堂辩论中揭露韩谨宁的罪证，秦晏声以武力控制粮仓，迫使皇帝重新核定赈灾名单".to_string();
        contract.outline.volumes = vec![
            VolumeContract {
                title: "残册疑云".to_string(),
                objective: "查明地方粮仓粮食被换的真相".to_string(),
                ending_change: "两人决定深入追查漕运路线".to_string(),
            },
            VolumeContract {
                title: "漕运暗流".to_string(),
                objective: "追踪粮仓与京城之间的利益输送管道".to_string(),
                ending_change: "主角取得关键账册副本".to_string(),
            },
            VolumeContract {
                title: "朝堂博弈".to_string(),
                objective: "整理修补后的完整账册，在朝堂辩论中揭露韩谨宁罪证".to_string(),
                ending_change: "韩谨宁罪证确凿，被迫退位，皇帝重新核定赈灾名单".to_string(),
            },
            VolumeContract {
                title: "尘埃落定".to_string(),
                objective: "处理案件余波".to_string(),
                ending_change: "众人进入新的生活".to_string(),
            },
        ];

        let mut issues = ContractIssueList::default();
        validate_longform_plan_position(&contract, &mut issues);

        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("非末卷提前完成权威终局")),
            "strongly shared terminal events must not escape the gate merely because their outcome verbs differ: {issues:?}"
        );
    }

    #[test]
    fn nonfinal_volume_can_prepare_for_terminal_conflict_without_completing_it() {
        let mut contract = NovelCreationContract::default();
        contract.target_units = Some(100_000);
        contract.chapter_unit_target = Some(2_500);
        contract.ending.desired_resolution =
            "商景言在京城决战中击败阮星桥并摧毁天网核心".to_string();
        contract.outline.volumes = vec![
            VolumeContract {
                title: "朝堂风云".to_string(),
                objective: "商景言潜入京城，利用天网内部矛盾制造混乱，为最终决战铺路".to_string(),
                ending_change: "阮星桥启动全面清洗，商景言身份暴露并陷入绝境".to_string(),
            },
            VolumeContract {
                title: "孤灯长明".to_string(),
                objective: "商景言在京城决战中击败阮星桥并摧毁天网核心".to_string(),
                ending_change: "天网瓦解，江湖门派重获自治权".to_string(),
            },
        ];

        let mut issues = ContractIssueList::default();
        validate_longform_plan_position(&contract, &mut issues);

        assert!(
            !issues
                .iter()
                .any(|issue| issue.contains("提前完成权威终局")),
            "preparation for a future terminal conflict must retain its story debt: {issues:?}"
        );
    }

    #[test]
    fn penultimate_climax_can_leave_a_distinct_terminal_debt_for_final_volume() {
        let mut contract = NovelCreationContract::default();
        contract.target_units = Some(100_000);
        contract.chapter_unit_target = Some(2_500);
        contract.characters = vec![
            CharacterContract {
                canonical_name: "谢云安".to_string(),
                role: "女主".to_string(),
                ..Default::default()
            },
            CharacterContract {
                canonical_name: "姜维桥".to_string(),
                role: "同伴".to_string(),
                ..Default::default()
            },
        ];
        contract.ending.desired_resolution = "谢云安拒绝收购方的高价买断，坚持公开真实的生产数据，虽然短期股价下跌，但赢得核心技工团队支持，成功引入战略投资者".to_string();
        contract.ending.final_state = "企业由传统制造转型为智能维保服务商，老工人成为数据标注师，工厂从亏损转为盈利，谢云安成为新一代技术管理者".to_string();
        contract.outline.volumes = vec![
            VolumeContract {
                title: "锈蚀与代码".to_string(),
                objective: "完成生产线数据摸底并启动数字化改造".to_string(),
                ending_change: "谢云安发现历史产量数据存在系统性偏差".to_string(),
            },
            VolumeContract {
                title: "断点与回滚".to_string(),
                objective: "在效率与工人安置之间建立可审计的试点流程".to_string(),
                ending_change: "第三方审计正式介入".to_string(),
            },
            VolumeContract {
                title: "风暴与真相".to_string(),
                objective: "谢云安拒绝高价买断并公开真实生产数据".to_string(),
                ending_change: "短期股价下跌，但姜维桥团队决定共同完成最后订单".to_string(),
            },
            VolumeContract {
                title: "重生与锚点".to_string(),
                objective: "凭借真实数据引入战略投资者，完成智能维保服务转型".to_string(),
                ending_change: "工厂恢复盈利，老工人转为数据标注师，谢云安成为技术管理者"
                    .to_string(),
            },
        ];

        let mut issues = ContractIssueList::default();
        validate_longform_plan_position(&contract, &mut issues);

        assert!(
            !issues
                .iter()
                .any(|issue| issue.contains("提前完成权威终局")),
            "a penultimate moral climax is valid when the final volume still resolves a different concrete terminal debt: {issues:?}"
        );
    }

    #[test]
    fn penultimate_disclosure_can_leave_physical_and_institutional_terminal_debt() {
        let mut contract = NovelCreationContract::default();
        contract.target_units = Some(100_000);
        contract.chapter_unit_target = Some(2_500);
        contract.ending.desired_resolution = "女主公开重力税账，切断王庭对底层岛屿的重力抽取，导致空中都城高度下降但稳固，底层岛屿停止失重".to_string();
        contract.ending.final_state =
            "重力分配制度由王庭独享改为浮岛群共享，空中都城与底层岛屿建立新的物理连接".to_string();
        contract.outline.volumes = vec![
            VolumeContract {
                title: "失重之痕".to_string(),
                objective: "确认岛屿失重真相并取得重力枢机入口线索".to_string(),
                ending_change: "主角被王庭通缉，旧地图证实抽取点位于空中都城".to_string(),
            },
            VolumeContract {
                title: "驳船网络".to_string(),
                objective: "潜入外围节点并收集第一手流失数据".to_string(),
                ending_change: "枢机入口打开，但核心账本仍在王庭手中".to_string(),
            },
            VolumeContract {
                title: "枢机之心".to_string(),
                objective: "夺回税账并揭露王庭抽取重力的代价".to_string(),
                ending_change: "重力税账公开，空中都城开始缓慢下降，底层岛屿失重速度减缓但未停止"
                    .to_string(),
            },
            VolumeContract {
                title: "重塑浮岛".to_string(),
                objective: "重新校准重力锚链并完成最终秩序重构".to_string(),
                ending_change: "空中都城稳固于新高度，底层岛屿停止失重，重力分配改为浮岛群共享"
                    .to_string(),
            },
        ];

        let mut issues = ContractIssueList::default();
        validate_longform_plan_position(&contract, &mut issues);

        assert!(
            !issues
                .iter()
                .any(|issue| issue.contains("提前完成权威终局")),
            "a penultimate disclosure is valid when the final volume resolves distinct physical and institutional debt: {issues:?}"
        );
    }

    #[test]
    fn nonfinal_stage_victory_over_same_faction_is_not_the_terminal_victory() {
        let mut contract = NovelCreationContract::default();
        contract.target_units = Some(327_500);
        contract.chapter_unit_target = Some(2_500);
        contract.characters = vec![CharacterContract {
            canonical_name: "顾启弦".to_string(),
            role: "男主".to_string(),
            ..Default::default()
        }];
        contract.ending.desired_resolution =
            "顾启弦在决战中击败世家联盟首席，当众折断武林令，终结旧秩序".to_string();
        contract.outline.volumes = vec![
            VolumeContract {
                title: "残谱初现".to_string(),
                objective: "顾启弦重返江湖外围，击败世家外门执事".to_string(),
                ending_change: "顾启弦脱离乱葬岗，进入江湖序列".to_string(),
            },
            VolumeContract {
                title: "武林终结".to_string(),
                objective: "顾启弦在决战中击败世家联盟首席，当众折断武林令".to_string(),
                ending_change: "旧秩序终结，江湖进入自由的新纪元".to_string(),
            },
        ];

        let mut issues = ContractIssueList::default();
        validate_longform_plan_position(&contract, &mut issues);

        assert!(
            !issues
                .iter()
                .any(|issue| issue.contains("提前完成权威终局")),
            "a smaller stage victory against the same faction is not the final victory: {issues:?}"
        );
    }

    #[test]
    fn nonfinal_limited_scope_cutoff_is_not_the_citywide_terminal_cutoff() {
        let mut contract = NovelCreationContract::default();
        contract.target_units = Some(100_000);
        contract.chapter_unit_target = Some(2_500);
        contract.characters = vec![CharacterContract {
            canonical_name: "程晏舟".to_string(),
            role: "女主".to_string(),
            ..Default::default()
        }];
        contract.ending.desired_resolution =
            "程晏舟切断企业对市民神经接口的垄断控制，并将芯片数据开源".to_string();
        contract.outline.volumes = vec![
            VolumeContract {
                title: "街头突围".to_string(),
                objective: "带领同伴突破企业封锁，前往中央塔".to_string(),
                ending_change: "切断企业对部分街区的监控，主角团取得进入中央塔的主动权".to_string(),
            },
            VolumeContract {
                title: "秩序重构".to_string(),
                objective: "在中央塔顶完成最终决战".to_string(),
                ending_change: "切断企业对市民神经接口的垄断控制，芯片数据开源".to_string(),
            },
        ];

        let mut issues = ContractIssueList::default();
        validate_longform_plan_position(&contract, &mut issues);

        assert!(
            !issues
                .iter()
                .any(|issue| issue.contains("提前完成权威终局")),
            "a limited district-level cutoff must retain the citywide terminal debt: {issues:?}"
        );
    }

    #[test]
    fn grammatical_quantifier_alone_does_not_mark_an_event_as_limited_scope() {
        assert!(
            !clause_is_explicitly_limited_stage_event(
                "主角击败一名世家联盟首席并终结旧秩序",
                "主角击败世家联盟首席并终结旧秩序"
            ),
            "ordinary grammatical quantifiers are not evidence of a smaller narrative scope"
        );
    }

    #[test]
    fn nonfinal_initial_identity_change_is_not_the_terminal_character_arc() {
        let mut contract = NovelCreationContract::default();
        contract.target_units = Some(100_000);
        contract.chapter_unit_target = Some(2_500);
        contract.protagonist_arc = "秦昭宁从最初只相信数据与逻辑、情感淡漠的计算机器，经历被背叛后的信任危机，最终成长为兼具理性锋芒与领袖魅力的商业统帅，完成从执行者到统治者的身份跃迁".to_string();
        contract.outline.volumes = vec![
            VolumeContract {
                title: "蛰伏与猎杀".to_string(),
                objective: "组建并购基金，低价截获被低估的核心专利".to_string(),
                ending_change:
                    "秦昭宁从失业首席财务官转变为小型基金合伙人，完成从打工者到操盘手的身份初步转换"
                        .to_string(),
            },
            VolumeContract {
                title: "终局与加冕".to_string(),
                objective: "完成终极并购与控制权接管".to_string(),
                ending_change: "秦昭宁完成从执行者到统治者的身份跃迁".to_string(),
            },
        ];

        let mut issues = ContractIssueList::default();
        validate_longform_plan_position(&contract, &mut issues);

        assert!(
            !issues
                .iter()
                .any(|issue| issue.contains("提前完成权威终局")),
            "an explicitly initial identity change must retain the terminal character arc: {issues:?}"
        );
    }

    #[test]
    fn opening_can_seek_a_terminal_transformation_without_completing_it() {
        let mut contract = NovelCreationContract::default();
        contract.target_units = Some(100_000);
        contract.chapter_unit_target = Some(2_500);
        contract.ending.desired_resolution =
            "谢云宁将自身的生物骨骼替换为高纯度神经簇并接入城市核心".to_string();
        contract.outline.near_chapters = vec![ChapterSeedContract {
            number: Some(3),
            goal: "谢云宁寻找将自身的生物骨骼替换为神经簇的手术方案".to_string(),
            expected_turn: "她取得第一份神经簇适配报告".to_string(),
        }];

        let mut issues = ContractIssueList::default();
        validate_longform_plan_position(&contract, &mut issues);

        assert!(
            !issues
                .iter()
                .any(|issue| issue.contains("提前完成权威终局")),
            "seeking a future transformation must not count as completing it: {issues:?}"
        );
    }

    #[test]
    fn opening_can_locate_terminal_destination_while_retaining_unresolved_conflict() {
        let mut contract = NovelCreationContract::default();
        contract.target_units = Some(100_000);
        contract.chapter_unit_target = Some(2_500);
        contract.characters = vec![
            CharacterContract {
                canonical_name: "程星舟".to_string(),
                role: "女主".to_string(),
                ..Default::default()
            },
            CharacterContract {
                canonical_name: "祝知序".to_string(),
                role: "对手".to_string(),
                ..Default::default()
            },
        ];
        contract.ending.desired_resolution = "程星舟利用篡改地图中的隐藏坐标，引导车队驶入一个未被官方记录的深层地下空洞稳定区，并在此处建立独立于官方控制的新聚落".to_string();
        contract.outline.near_chapters = vec![
            ChapterSeedContract {
                number: Some(7),
                goal: "车队进入地下空洞入口，开辟临时通道".to_string(),
                expected_turn: "发现恒温苔藓生态，但入口随时可能坍塌".to_string(),
            },
            ChapterSeedContract {
                number: Some(8),
                goal: "程星舟发现官方迁移图的原始底图，确认绝对安全区的范围".to_string(),
                expected_turn: "确认新聚落的位置，但祝知序的追兵即将到达，车队必须建立防御工事"
                    .to_string(),
            },
        ];
        contract.outline.volumes = vec![
            VolumeContract {
                title: "地下空洞".to_string(),
                objective: "深入地下空洞建立临时庇护所，锁定深层安全区入口".to_string(),
                ending_change: "确认独立生态闭环，但入口即将被祝知序的追兵引爆".to_string(),
            },
            VolumeContract {
                title: "新秩序".to_string(),
                objective: "击退追兵并建立独立于官方控制的新聚落".to_string(),
                ending_change: "车队在深层稳定区建立新秩序".to_string(),
            },
        ];

        let mut issues = ContractIssueList::default();
        validate_longform_plan_position(&contract, &mut issues);

        assert!(
            !issues
                .iter()
                .any(|issue| issue.contains("提前完成权威终局")),
            "locating the terminal destination while an armed conflict remains is setup, not completion: {issues:?}"
        );
    }

    #[test]
    fn early_completion_of_a_different_event_is_not_the_terminal_completion() {
        let mut contract = NovelCreationContract::default();
        contract.target_units = Some(100_000);
        contract.chapter_unit_target = Some(2_500);
        contract.characters = vec![
            CharacterContract {
                canonical_name: "秦泊桥".to_string(),
                role: "女主".to_string(),
                ..Default::default()
            },
            CharacterContract {
                canonical_name: "梁星衡".to_string(),
                role: "男主".to_string(),
                ..Default::default()
            },
        ];
        contract.ending.desired_resolution = "秦泊桥利用最后一次广播完整播放整理好的口述档案，梁星衡在毕业典礼上完成最后一圈奔跑，学校正式挂牌合并，但老校区记忆馆成立".to_string();
        contract.outline.volumes = vec![
            VolumeContract {
                title: "杂音与信号".to_string(),
                objective: "组建调查小组，完成首批口述档案的采集与整理".to_string(),
                ending_change: "校方切断广播站电源，但秦泊桥通过备用电池完成第一档特别节目的播出，老校区学生开始自发收集档案".to_string(),
            },
            VolumeContract {
                title: "最后一次广播".to_string(),
                objective: "在合并挂牌前完成最终档案核对".to_string(),
                ending_change: "完整播放全部档案，记忆馆正式挂牌".to_string(),
            },
        ];

        let mut issues = ContractIssueList::default();
        validate_longform_plan_position(&contract, &mut issues);

        assert!(
            !issues
                .iter()
                .any(|issue| issue.contains("提前完成权威终局")),
            "completing an early broadcast is not the same event as completing the terminal race: {issues:?}"
        );
    }

    #[test]
    fn partial_opening_activation_is_not_the_complete_terminal_activation() {
        let mut contract = NovelCreationContract::default();
        contract.target_units = Some(1_000_000);
        contract.chapter_unit_target = Some(5_000);
        contract.ending.desired_resolution =
            "主角完整激活隐藏引擎并校准全域能量流，使局部宇宙从热寂中复苏".to_string();
        contract.outline.volumes = vec![
            VolumeContract {
                title: "裂痕初现".to_string(),
                objective: "发现隐藏节点并建立与核心数据的初步连接".to_string(),
                ending_change:
                    "节点首次激活，引发局部能量逆流并出现第一道裂痕，但仍处于半激活不稳定状态"
                        .to_string(),
            },
            VolumeContract {
                title: "终局校准".to_string(),
                objective: "进入核心完成全域能量校准".to_string(),
                ending_change: "主角完整激活隐藏引擎，使局部宇宙从热寂中复苏".to_string(),
            },
        ];
        contract.outline.near_chapters = vec![ChapterSeedContract {
            number: Some(5),
            goal: "完成第一次能量脉冲尝试，封印出现裂纹".to_string(),
            expected_turn: "节点进入半激活状态，但输出仍不稳定".to_string(),
        }];

        let mut issues = ContractIssueList::default();
        validate_longform_plan_position(&contract, &mut issues);

        assert!(
            !issues
                .iter()
                .any(|issue| issue.contains("提前完成权威终局")),
            "a partial unstable activation must retain the terminal activation debt: {issues:?}"
        );
    }

    #[test]
    fn nonfinal_volume_cannot_complete_primary_identity_arc_before_epilogue() {
        let mut contract = NovelCreationContract::default();
        contract.target_units = Some(100_000);
        contract.chapter_unit_target = Some(2_500);
        contract.protagonist_arc =
            "唐听桥从唯利是图的底层中介，成长为愿意平衡各方利益的城市资源整合者，完成身份跃迁"
                .to_string();
        contract.outline.volumes = vec![
            VolumeContract {
                title: "幽灵户型".to_string(),
                objective: "发现产权漏洞".to_string(),
                ending_change: "巨头启动断水计划".to_string(),
            },
            VolumeContract {
                title: "暴雨夜".to_string(),
                objective: "促成各方谈判".to_string(),
                ending_change: "唐听桥完成从中介到城市资源整合者的身份转换".to_string(),
            },
            VolumeContract {
                title: "连接与回响".to_string(),
                objective: "处理暴雨余波".to_string(),
                ending_change: "故事在平稳生活中落幕".to_string(),
            },
        ];

        let mut issues = ContractIssueList::default();
        validate_longform_plan_position(&contract, &mut issues);

        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("非末卷提前完成权威终局")),
            "a completed protagonist identity arc belongs in the final volume: {issues:?}"
        );
    }

    #[test]
    fn raw_outline_rejects_unscoped_volume_change_fragments() {
        let polluted = "两人从利益博弈转向合作卷尾变化：核心资源被托管；卷尾变化：对手退出竞争";
        let scoped = "第一卷《托管危机》：两人被迫合作；卷尾变化：核心资源被托管。第二卷《独立之路》：解决收购战；卷尾变化：对手退出竞争。";

        assert!(outline_text_is_polluted(polluted));
        assert!(!outline_text_is_polluted(scoped));
    }

    #[test]
    fn typed_outline_fields_reject_contract_section_heading_residue() {
        assert!(outline_text_is_polluted("陆承言公开监视名单。分卷规划"));
        assert!(outline_text_is_polluted(
            "爆炸证据被公开，唐星原找到哥哥。近期章节包"
        ));
        assert!(!outline_text_is_polluted("主角重新设计分卷规划"));
    }
}
