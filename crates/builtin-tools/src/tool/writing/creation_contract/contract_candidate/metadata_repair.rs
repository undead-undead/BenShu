use super::*;

#[cfg(test)]
pub fn creation_contract_issues_contain_title_metadata(issues: &[String]) -> bool {
    issues
        .iter()
        .any(|issue| creation_contract_issue_is_title_metadata(issue))
}

pub fn creation_contract_issue_is_title_metadata(issue: &str) -> bool {
    let lowered = issue.to_ascii_lowercase();
    issue.contains("书名")
        || issue.contains("标题")
        || lowered.contains("title")
        || lowered.contains("canonical_title")
}

pub fn creation_contract_issues_are_contract_metadata_only(issues: &[String]) -> bool {
    !issues.is_empty()
        && issues
            .iter()
            .all(|issue| creation_contract_issue_is_metadata_repairable(issue))
}

fn creation_contract_issue_is_metadata_repairable(issue: &str) -> bool {
    let lowered = issue.to_ascii_lowercase();
    if lowered.contains("outline.longform_position")
        || lowered.contains("outline.terminal_coverage")
        || issue.contains("提前完成权威终局")
        || issue.contains("末卷没有执行权威终局")
        || issue.contains("终局放回末卷")
    {
        return false;
    }
    if issue.contains("缺少可锁定书名")
        || issue.contains("尚未形成可锁定书名")
        || issue.contains("缺少书名理由")
    {
        return false;
    }
    if issue.contains("书名")
        || issue.contains("标题")
        || lowered.contains("title")
        || lowered.contains("canonical_title")
    {
        return true;
    }
    if (issue.contains("近期章节") || lowered.contains("near_chapter"))
        && (issue.contains("预期转折")
            || issue.contains("转折")
            || issue.contains("目标")
            || issue.contains("章节名")
            || issue.contains("标题")
            || lowered.contains("goal")
            || lowered.contains("expected_turn")
            || lowered.contains("chapter"))
    {
        return true;
    }
    if issue.contains("分卷/阶段安排")
        || issue.contains("分卷规划")
        || issue.contains("近期章节包")
        || issue.contains("近期章节规划")
        || issue.contains("逐章规划")
        || issue.contains("分卷/阶段大纲")
        || lowered.contains("volume")
        || lowered.contains("outline")
        || lowered.contains("near_chapter")
    {
        return true;
    }
    if issue.contains("结构化字段") || lowered.contains("structured") {
        return true;
    }
    if issue.contains("世界规则") || lowered.contains("world_rules") {
        return true;
    }
    false
}

pub fn submit_pending_contract_title_metadata_repair(
    draft: &mut SessionCreationDraftState,
    raw_title_metadata: &str,
) -> Option<ContractSubmissionOutcome> {
    if let Some(outcome) = contract_boundary_rejection_outcome(draft, raw_title_metadata) {
        return Some(outcome);
    }
    let mut merged = merged_pending_contract_repair_base(draft)?;
    if let Some(outcome) =
        submit_pending_contract_title_patch_repair(draft, &merged, raw_title_metadata)
    {
        return Some(outcome);
    }
    let title_metadata = parse_title_metadata_repair(draft, raw_title_metadata)?;
    let object = merged.as_object_mut()?;
    object.insert("title".to_string(), title_metadata);
    Some(submit_premerged_contract_candidate_to_draft(
        draft,
        &merged.to_string(),
    ))
}

fn submit_pending_contract_title_patch_repair(
    draft: &mut SessionCreationDraftState,
    pending_normalized: &Value,
    raw_title_metadata: &str,
) -> Option<ContractSubmissionOutcome> {
    let mut base_contract =
        NovelCreationContract::parse_json_boundary(&pending_normalized.to_string())?;
    align_primary_name_authority(&mut base_contract);
    let mut candidate_draft = draft.clone();
    apply_strong_novel_contract_to_creation_draft(&mut candidate_draft, &mut base_contract);
    normalize_fiction_creation_draft_after_contract_change(&mut candidate_draft);
    sanitize_creation_draft_control_noise(&mut candidate_draft);

    let patch = normalize_creation_contract_patch_boundary(&candidate_draft, raw_title_metadata)?;
    let scope = patch.validate_scope(&candidate_draft);
    if !scope.ready() {
        return Some(title_patch_repair_needs_repair_outcome(scope.issues));
    }
    if !patch.has_repairable_title_for_draft(&candidate_draft) {
        return Some(title_patch_repair_needs_repair_outcome(
            patch.title_repair_failure_reasons_for_draft(&candidate_draft),
        ));
    }
    patch.apply_title_repair_to_draft(&mut candidate_draft);
    normalize_fiction_creation_draft_after_contract_change(&mut candidate_draft);
    sanitize_creation_draft_control_noise(&mut candidate_draft);
    if value_missing(&candidate_draft.title)
        || value_missing(&candidate_draft.fiction_title_rationale)
    {
        return Some(title_patch_repair_needs_repair_outcome(vec![
            "书名修复没有产生可锁定书名和具体命名理由".to_string(),
        ]));
    }

    let applied_contract =
        super::strong_novel_contract_from_visible_creation_draft(&candidate_draft);
    let mut repaired_contract = base_contract;
    patch.merge_applied_scope_into_contract(&mut repaired_contract, &applied_contract);
    align_primary_name_authority(&mut repaired_contract);
    repaired_contract.normalize();
    let normalized_value =
        serde_json::to_value(&repaired_contract).unwrap_or_else(|_| pending_normalized.clone());
    let normalized_text = serde_json::to_string(&repaired_contract).unwrap_or_default();
    Some(submit_novel_creation_contract_candidate(
        draft,
        raw_title_metadata,
        repaired_contract,
        normalized_text,
        normalized_value,
    ))
}

fn title_patch_repair_needs_repair_outcome(issues: Vec<String>) -> ContractSubmissionOutcome {
    ContractSubmissionOutcome {
        gate: ContractGateResult {
            status: ContractGateStatus::NeedsRepair,
            blocking_issues: Vec::new(),
            repairable_issues: if issues.is_empty() {
                vec!["书名 metadata 修复没有形成可审查候选".to_string()]
            } else {
                issues
            },
            warnings: Vec::new(),
        },
        committed: false,
    }
}

pub fn repair_pending_contract_metadata_locally(
    draft: &mut SessionCreationDraftState,
) -> Option<ContractSubmissionOutcome> {
    let mut merged = merged_pending_contract_repair_base(draft)?;
    let mut contract = NovelCreationContract::parse_json_boundary(&merged.to_string())?;
    let mut changed = false;
    if sanitize_structured_contract_surface(&mut contract) {
        changed = true;
    }
    if sanitize_structured_short_slot_pollution(&mut contract) {
        changed = true;
    }
    if align_primary_name_authority(&mut contract) {
        changed = true;
    }
    if reconcile_character_plan_anchors_with_outline(&mut contract) {
        changed = true;
    }
    if reconcile_outline_book_title_authority(&mut contract) {
        changed = true;
    }
    if sanitize_structured_world_rules_seed(&mut contract) {
        changed = true;
    }
    if sync_world_rules_from_structured_contract(&mut contract) {
        changed = true;
    }
    if sanitize_repeated_primary_name_in_near_chapter_goals(&mut contract) {
        changed = true;
    }
    if reconcile_relationship_ledger_authority(&mut contract) {
        changed = true;
    }
    if !changed {
        return None;
    }
    contract.normalize();
    merged = serde_json::to_value(&contract).ok()?;
    let normalized_text = serde_json::to_string(&contract).ok()?;
    Some(super::submit_novel_creation_contract_candidate(
        draft,
        &merged.to_string(),
        contract,
        normalized_text,
        merged,
    ))
}

fn reconcile_outline_book_title_authority(contract: &mut NovelCreationContract) -> bool {
    let canonical = contract.title.canonical_title.trim();
    if value_missing(canonical) || contract.outline.raw_outline.trim().is_empty() {
        return false;
    }
    let mut allowed_titles = contract
        .outline
        .volumes
        .iter()
        .map(|volume| volume.title.trim().to_string())
        .filter(|title| !value_missing(title))
        .collect::<Vec<_>>();
    allowed_titles.extend(typed_contract_gate::non_character_contract_terms(contract));
    for chapter in &contract.outline.near_chapters {
        allowed_titles.extend(quoted_book_title_like_segments(&format!(
            "{}\n{}",
            chapter.goal, chapter.expected_turn
        )));
    }
    let Some(repaired) = canonicalize_outline_book_title_quotes(
        &contract.outline.raw_outline,
        canonical,
        &allowed_titles,
    ) else {
        return false;
    };
    contract.outline.raw_outline = repaired;
    true
}

pub(crate) fn sanitize_structured_world_rules_seed(contract: &mut NovelCreationContract) -> bool {
    let levels = &mut contract.structured.power_progression.levels;
    let original_len = levels.len();
    levels.retain(|level| !structured_level_looks_like_outline_prose(level));
    levels.len() != original_len
}

fn sync_world_rules_from_structured_contract(contract: &mut NovelCreationContract) -> bool {
    if !contract.world_rules.is_empty() {
        return false;
    }
    let structured = &contract.structured;
    let rules = [
        structured.resource_economy.cost_examples.as_slice(),
        structured.resource_economy.scarcity_rules.as_slice(),
        structured.resource_economy.trade_rules.as_slice(),
        structured.power_progression.advancement_costs.as_slice(),
        structured.power_progression.bottlenecks.as_slice(),
        structured.power_progression.failure_consequences.as_slice(),
        structured
            .power_progression
            .anti_power_creep_rules
            .as_slice(),
        structured.social_order.exam_or_promotion_rules.as_slice(),
        structured.social_order.laws.as_slice(),
        structured.social_order.authority_conflicts.as_slice(),
        structured.geography_model.distance_rules.as_slice(),
        structured.geography_model.travel_constraints.as_slice(),
    ]
    .into_iter()
    .flat_map(|rules| rules.iter())
    .map(|rule| rule.trim().to_string())
    .filter(|rule| {
        !value_missing(rule)
            && !typed_contract_gate::world_rule_looks_truncated_or_not_actionable(rule)
    })
    .fold(Vec::<String>::new(), |mut out, rule| {
        if !out.iter().any(|known| known == &rule) {
            out.push(rule);
        }
        out
    });
    if rules.is_empty() {
        return false;
    }
    contract.world_rules = rules;
    true
}

fn sanitize_repeated_primary_name_in_near_chapter_goals(
    contract: &mut NovelCreationContract,
) -> bool {
    let primary_names = contract
        .characters
        .iter()
        .filter(|character| character.role_looks_primary())
        .map(|character| character.canonical_name.trim().to_string())
        .filter(|name| !value_missing(name) && name.chars().count() >= 2)
        .collect::<Vec<_>>();
    if primary_names.is_empty() {
        return false;
    }

    let mut changed = false;
    for chapter in &mut contract.outline.near_chapters {
        let mut goal = chapter.goal.clone();
        let mut chapter_changed = false;
        for name in &primary_names {
            if goal.replace(char::is_whitespace, "").matches(name).count() < 2 {
                continue;
            }
            let repaired = keep_first_primary_name_mention(&goal, name);
            if !value_missing(&repaired) && repaired != goal {
                goal = repaired;
                chapter_changed = true;
                changed = true;
            }
        }
        if chapter_changed {
            chapter.goal = cleanup_repaired_chapter_goal(&goal);
        }
    }
    changed
}

fn keep_first_primary_name_mention(goal: &str, name: &str) -> String {
    let mut out = String::with_capacity(goal.len());
    let mut last = 0usize;
    let mut seen = false;
    for (index, _) in goal.match_indices(name) {
        out.push_str(&goal[last..index]);
        if !seen {
            out.push_str(name);
            seen = true;
        }
        last = index + name.len();
    }
    out.push_str(&goal[last..]);
    out
}

fn cleanup_repaired_chapter_goal(goal: &str) -> String {
    let mut value = goal.trim().to_string();
    for (from, to) in [
        ("，，", "，"),
        ("，。", "。"),
        ("、，", "，"),
        ("和，", "，"),
        ("与，", "，"),
        ("对，", "，"),
        ("在，", "，"),
    ] {
        value = value.replace(from, to);
    }
    value
        .trim_matches(|ch: char| ch.is_whitespace())
        .trim_matches('，')
        .to_string()
}

fn structured_level_looks_like_outline_prose(value: &str) -> bool {
    let value = value.trim();
    if value_missing(value) {
        return true;
    }
    value.chars().count() > 32
        || (value.contains('第')
            && value.contains('章')
            && (value.contains("目标") || value.contains("转折") || value.contains('卷')))
        || value.contains("->")
        || (value.contains('：')
            && (value.contains("主线") || value.contains("结局") || value.contains("大纲")))
}

fn align_primary_name_authority(contract: &mut NovelCreationContract) -> bool {
    contract.align_primary_name_authority_surfaces()
}

fn reconcile_character_plan_anchors_with_outline(contract: &mut NovelCreationContract) -> bool {
    let volume_count = contract.outline.volumes.len();
    if volume_count == 0 {
        return false;
    }

    let mut changed = false;
    for character in &mut contract.characters {
        let is_primary = character.role_looks_primary();
        let normalized_entry =
            clamp_out_of_range_volume_references(&character.planned_entry, volume_count);
        if normalized_entry != character.planned_entry {
            character.planned_entry = normalized_entry;
            changed = true;
        }
        let normalized_exit =
            clamp_out_of_range_volume_references(&character.planned_exit, volume_count);
        if normalized_exit != character.planned_exit {
            character.planned_exit = normalized_exit;
            changed = true;
        }
        if typed_contract_gate::character_plan_anchor_needs_repair(
            &character.planned_entry,
            volume_count,
            is_primary,
            false,
        ) {
            character.planned_entry = if is_primary {
                "第1卷进入主线".to_string()
            } else {
                format!("第{volume_count}卷进入主线")
            };
            changed = true;
        }
        if typed_contract_gate::character_plan_anchor_needs_repair(
            &character.planned_exit,
            volume_count,
            is_primary,
            true,
        ) {
            character.planned_exit = format!("持续至第{volume_count}卷终局");
            changed = true;
        }
    }
    changed
}

fn clamp_out_of_range_volume_references(value: &str, volume_count: usize) -> String {
    let chars = value.char_indices().collect::<Vec<_>>();
    let ordinal_char = |ch: char| {
        ch.is_ascii_digit()
            || matches!(
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
    };
    let mut output = String::with_capacity(value.len());
    let mut copied_until = 0;
    let mut index = 0;
    while index < chars.len() {
        let (start_byte, marker) = chars[index];
        let (number_start, suffix_required) = match marker {
            '第' => (index + 1, true),
            '卷' => (index + 1, false),
            _ => {
                index += 1;
                continue;
            }
        };
        let mut number_end = number_start;
        while number_end < chars.len()
            && number_end < number_start + 9
            && ordinal_char(chars[number_end].1)
        {
            number_end += 1;
        }
        if number_end == number_start
            || (suffix_required && (number_end >= chars.len() || chars[number_end].1 != '卷'))
        {
            index += 1;
            continue;
        }
        let raw_number = chars[number_start..number_end]
            .iter()
            .map(|(_, ch)| *ch)
            .collect::<String>();
        let Some(ordinal) =
            super::super::super::longform_guard::LongformArtifactGuard::parse_step_ordinal(
                &raw_number,
            )
        else {
            index += 1;
            continue;
        };
        if ordinal <= volume_count {
            index += 1;
            continue;
        }
        let end_index = if suffix_required {
            number_end + 1
        } else {
            number_end
        };
        let end_byte = chars
            .get(end_index)
            .map(|(byte, _)| *byte)
            .unwrap_or(value.len());
        output.push_str(&value[copied_until..start_byte]);
        if suffix_required {
            output.push_str(&format!("第{volume_count}卷"));
        } else {
            output.push_str(&format!("卷{volume_count}"));
        }
        copied_until = end_byte;
        index = end_index;
    }
    if copied_until == 0 {
        return value.to_string();
    }
    output.push_str(&value[copied_until..]);
    output
}

fn reconcile_relationship_ledger_authority(contract: &mut NovelCreationContract) -> bool {
    let known_names = contract
        .characters
        .iter()
        .map(|character| character.canonical_name.trim())
        .filter(|name| !value_missing(name))
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if known_names.is_empty() {
        return false;
    }
    let mut changed = false;
    for relation in &mut contract.structured.relationship_ledger {
        let mut normalized = Vec::new();
        for name in &relation.characters {
            let name = name.trim();
            if value_missing(name) {
                continue;
            }
            let canonical = relationship_character_name_matching_authority(name, &known_names)
                .unwrap_or_else(|| name.to_string());
            if !normalized.iter().any(|existing| existing == &canonical) {
                normalized.push(canonical);
            }
        }
        if relation.characters != normalized {
            relation.characters = normalized;
            changed = true;
        }
    }
    changed
}

fn sanitize_structured_contract_surface(contract: &mut NovelCreationContract) -> bool {
    let Ok(mut value) = serde_json::to_value(&contract.structured) else {
        return false;
    };
    let changed = sanitize_structured_value_surface(&mut value);
    if !changed {
        return false;
    }
    let Ok(structured) = serde_json::from_value(value) else {
        return false;
    };
    contract.structured = structured;
    true
}

fn sanitize_structured_short_slot_pollution(contract: &mut NovelCreationContract) -> bool {
    let mut changed = false;

    if structured_short_slot_looks_like_story_summary(
        &contract.structured.resource_economy.value_scale,
        contract,
    ) {
        contract.structured.resource_economy.value_scale.clear();
        changed = true;
    }
    if structured_short_slot_looks_like_story_summary(
        &contract.structured.power_progression.system_name,
        contract,
    ) {
        contract.structured.power_progression.system_name.clear();
        changed = true;
    }
    if structured_short_slot_looks_like_story_summary(
        &contract.structured.social_order.rank_system,
        contract,
    ) {
        contract.structured.social_order.rank_system.clear();
        changed = true;
    }
    if structured_short_slot_looks_like_story_summary(
        &contract.structured.antagonist_pressure.primary_pressure,
        contract,
    ) {
        contract
            .structured
            .antagonist_pressure
            .primary_pressure
            .clear();
        changed = true;
    }

    let summary_anchors = structured_story_summary_anchors(contract);
    for antagonist in &mut contract.structured.antagonist_pressure.antagonists {
        for value in [
            &mut antagonist.goal,
            &mut antagonist.current_move,
            &mut antagonist.defeat_condition,
        ] {
            if structured_short_slot_looks_like_story_summary_against_anchors(
                value,
                &summary_anchors,
            ) {
                value.clear();
                changed = true;
            }
        }
        let original_len = antagonist.resources.len();
        antagonist.resources.retain(|value| {
            !structured_short_slot_looks_like_story_summary_against_anchors(value, &summary_anchors)
        });
        if antagonist.resources.len() != original_len {
            changed = true;
        }
    }

    changed
}

fn structured_short_slot_looks_like_story_summary(
    value: &str,
    contract: &NovelCreationContract,
) -> bool {
    structured_short_slot_looks_like_story_summary_against_anchors(
        value,
        &structured_story_summary_anchors(contract),
    )
}

fn structured_short_slot_looks_like_story_summary_against_anchors(
    value: &str,
    anchors: &[String],
) -> bool {
    let normalized_value = normalized_short_slot_text(value);
    if normalized_value.chars().count() < 18 {
        return false;
    }
    let looks_like_sentence_or_chain = value.contains('：')
        || value.contains(':')
        || value.contains("->")
        || value.contains('→')
        || value.contains('。')
        || value.contains('；')
        || value.contains('，');
    anchors.iter().any(|anchor| {
        anchor.chars().count() >= 12
            && (normalized_value == *anchor
                || normalized_value.contains(anchor)
                || (looks_like_sentence_or_chain
                    && anchor.contains(&normalized_value)
                    && normalized_value.chars().count() >= 24))
    }) && (looks_like_sentence_or_chain || normalized_value.chars().count() >= 28)
}

fn structured_story_summary_anchors(contract: &NovelCreationContract) -> Vec<String> {
    [
        contract.premise.as_str(),
        contract.main_causal_spine.as_str(),
        contract.protagonist_arc.as_str(),
        contract.world_imagery.as_str(),
        contract.ending.desired_resolution.as_str(),
        contract.ending.final_state.as_str(),
    ]
    .into_iter()
    .map(normalized_short_slot_text)
    .filter(|value| !value_missing(value))
    .collect()
}

fn normalized_short_slot_text(value: &str) -> String {
    value
        .trim()
        .replace(char::is_whitespace, "")
        .trim_matches(['。', '；', ';', '，', ','])
        .to_string()
}

fn sanitize_structured_value_surface(value: &mut serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(text) => {
            let cleaned =
                crate::tool::writing::surface_sanitizer::sanitize_contract_surface_text(text);
            let cleaned = if crate::tool::writing::surface_sanitizer::contains_excessive_repeated_cjk_surface_noise(&cleaned)
                || crate::tool::writing::surface_sanitizer::contains_generic_contract_placeholder_residue(&cleaned)
            {
                String::new()
            } else {
                cleaned
            };
            if *text != cleaned {
                *text = cleaned;
                return true;
            }
            false
        }
        serde_json::Value::Array(items) => {
            let mut changed = false;
            for item in items.iter_mut() {
                if sanitize_structured_value_surface(item) {
                    changed = true;
                }
            }
            let original_len = items.len();
            items.retain(|item| {
                !matches!(item, serde_json::Value::String(text) if value_missing(text))
                    && !matches!(item, serde_json::Value::Array(values) if values.is_empty())
            });
            changed || items.len() != original_len
        }
        serde_json::Value::Object(map) => {
            let mut changed = false;
            for item in map.values_mut() {
                if sanitize_structured_value_surface(item) {
                    changed = true;
                }
            }
            changed
        }
        _ => false,
    }
}

fn relationship_character_name_matching_authority(
    name: &str,
    known_names: &[String],
) -> Option<String> {
    let name = name.trim();
    for known in known_names {
        let known = known.trim();
        if known.is_empty() {
            continue;
        }
        if name == known {
            return Some(known.to_string());
        }
        if let Some(tail) = name.strip_prefix(known) {
            let tail_len = tail.chars().count();
            if (1..=2).contains(&tail_len) {
                return Some(known.to_string());
            }
        }
    }
    None
}

pub fn submit_pending_contract_metadata_repair(
    draft: &mut SessionCreationDraftState,
    raw_metadata: &str,
) -> Option<ContractSubmissionOutcome> {
    if let Some(outcome) = contract_boundary_rejection_outcome(draft, raw_metadata) {
        return Some(outcome);
    }
    let mut merged = merged_pending_contract_repair_base(draft)?;
    if let Some(outcome) =
        submit_pending_contract_metadata_patch_repair(draft, &merged, raw_metadata)
    {
        return Some(outcome);
    }
    let mut changed = false;
    if let Some(title_metadata) = parse_title_metadata_repair(draft, raw_metadata) {
        let object = merged.as_object_mut()?;
        object.insert("title".to_string(), title_metadata);
        changed = true;
    }
    if let Some(near_chapters) = parse_near_chapter_metadata_repair(raw_metadata) {
        let object = merged.as_object_mut()?;
        let outline = object
            .entry("outline".to_string())
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        let outline_object = outline.as_object_mut()?;
        outline_object.insert("near_chapters".to_string(), Value::Array(near_chapters));
        changed = true;
    }
    if let Some(world_rules) = parse_world_rules_metadata_repair(raw_metadata) {
        let object = merged.as_object_mut()?;
        object.insert("world_rules".to_string(), Value::Array(world_rules));
        changed = true;
    }
    changed.then(|| submit_premerged_contract_candidate_to_draft(draft, &merged.to_string()))
}

fn submit_pending_contract_metadata_patch_repair(
    draft: &mut SessionCreationDraftState,
    pending_normalized: &Value,
    raw_metadata: &str,
) -> Option<ContractSubmissionOutcome> {
    let mut base_contract =
        NovelCreationContract::parse_json_boundary(&pending_normalized.to_string())?;
    align_primary_name_authority(&mut base_contract);
    let mut candidate_draft = draft.clone();
    apply_strong_novel_contract_to_creation_draft(&mut candidate_draft, &mut base_contract);
    normalize_fiction_creation_draft_after_contract_change(&mut candidate_draft);
    sanitize_creation_draft_control_noise(&mut candidate_draft);

    let patch = normalize_creation_contract_patch_boundary(&candidate_draft, raw_metadata)?;
    let scope = patch.validate_scope(&candidate_draft);
    if !scope.ready() {
        return None;
    }
    patch.apply_to_draft(&mut candidate_draft);
    normalize_fiction_creation_draft_after_contract_change(&mut candidate_draft);
    sanitize_creation_draft_control_noise(&mut candidate_draft);

    let applied_contract =
        super::strong_novel_contract_from_visible_creation_draft(&candidate_draft);
    let mut repaired_contract = base_contract;
    patch.merge_applied_scope_into_contract(&mut repaired_contract, &applied_contract);
    align_primary_name_authority(&mut repaired_contract);
    repaired_contract.normalize();
    let normalized_value =
        serde_json::to_value(&repaired_contract).unwrap_or_else(|_| pending_normalized.clone());
    let normalized_text = serde_json::to_string(&repaired_contract).unwrap_or_default();
    Some(submit_novel_creation_contract_candidate(
        draft,
        raw_metadata,
        repaired_contract,
        normalized_text,
        normalized_value,
    ))
}

fn merged_pending_contract_repair_base(draft: &SessionCreationDraftState) -> Option<Value> {
    let effective = super::creation_draft_with_pending_contract_applied(draft);
    let mut base_contract = super::strong_novel_contract_from_creation_draft(&effective);
    base_contract.normalize();
    serde_json::to_value(&base_contract).ok()
}

fn parse_title_metadata_repair(_draft: &SessionCreationDraftState, raw: &str) -> Option<Value> {
    let normalized = creation_contract_normalizer::normalize_creation_contract_boundary(raw)?;
    let object = normalized.value.as_object()?;
    let title_value = metadata_value_aliases(object, &["title", "title_metadata", "titleMetadata"])
        .unwrap_or(&normalized.value);
    let title_object = title_value.as_object()?;
    let canonical_title = title_metadata_string(
        title_object,
        &[
            "canonical_title",
            "canonicalTitle",
            "title",
            "book_title",
            "bookTitle",
            "work_title",
            "workTitle",
            "书名",
            "标题",
        ],
    );
    let rationale = title_metadata_string(
        title_object,
        &[
            "rationale",
            "title_rationale",
            "titleRationale",
            "reason",
            "basis",
            "书名理由",
            "命名理由",
            "标题理由",
        ],
    );
    let candidates = title_metadata_candidates(title_object, &canonical_title);
    if value_missing(&rationale) {
        return None;
    }
    let selected_title = if !value_missing(&canonical_title) {
        canonical_title
    } else {
        candidates.first()?.clone()
    };
    let mut title = serde_json::Map::new();
    title.insert("canonical_title".to_string(), Value::String(selected_title));
    title.insert("rationale".to_string(), Value::String(rationale));
    title.insert(
        "candidates".to_string(),
        Value::Array(candidates.into_iter().map(Value::String).collect()),
    );
    title.insert(
        "source".to_string(),
        title_object
            .get("source")
            .and_then(Value::as_str)
            .filter(|source| !source.trim().is_empty())
            .map(|source| Value::String(source.trim().to_string()))
            .unwrap_or_else(|| Value::String("llm_contract".to_string())),
    );
    Some(Value::Object(title))
}

fn title_metadata_string(object: &serde_json::Map<String, Value>, keys: &[&str]) -> String {
    metadata_value_aliases(object, keys)
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
        .unwrap_or_default()
}

fn title_metadata_candidates(
    object: &serde_json::Map<String, Value>,
    canonical_title: &str,
) -> Vec<String> {
    let mut candidates = Vec::new();
    for item in object
        .get("candidates")
        .or_else(|| metadata_value_aliases(object, &["title_candidates", "titleCandidates"]))
        .or_else(|| metadata_value_aliases(object, &["书名候选", "标题候选", "候选书名"]))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(value) = item
            .as_str()
            .map(str::trim)
            .filter(|value| !value_missing(value))
        {
            candidates.push(value.to_string());
            continue;
        }
        let Some(candidate) = item.as_object() else {
            continue;
        };
        let title = title_metadata_string(
            candidate,
            &[
                "title",
                "canonical_title",
                "book_title",
                "书名",
                "标题",
                "作品名",
            ],
        );
        if value_missing(&title) {
            continue;
        }
        candidates.push(title);
    }
    if !value_missing(canonical_title) && !candidates.iter().any(|value| value == canonical_title) {
        candidates.insert(0, canonical_title.to_string());
    }
    candidates.sort();
    candidates.dedup();
    candidates
}

fn parse_near_chapter_metadata_repair(raw: &str) -> Option<Vec<Value>> {
    let normalized = creation_contract_normalizer::normalize_creation_contract_boundary(raw)?;
    let object = normalized.value.as_object()?;
    let chapters = object
        .get("outline")
        .and_then(Value::as_object)
        .and_then(|outline| outline.get("near_chapters"))
        .or_else(|| object.get("near_chapters"))
        .or_else(|| object.get("chapters"))
        .and_then(Value::as_array)?;
    let mut repaired = Vec::new();
    for item in chapters {
        let Some(chapter_object) = item.as_object() else {
            continue;
        };
        let number = chapter_object
            .get("number")
            .or_else(|| chapter_object.get("chapter"))
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok());
        let goal = metadata_string_aliases(
            chapter_object,
            &["goal", "chapter_goal", "objective", "summary"],
        );
        let expected_turn = metadata_string_aliases(
            chapter_object,
            &[
                "expected_turn",
                "turn",
                "change",
                "irreversible_change",
                "payoff",
            ],
        );
        if value_missing(&goal) && value_missing(&expected_turn) {
            continue;
        }
        let mut chapter = serde_json::Map::new();
        if let Some(number) = number {
            chapter.insert(
                "number".to_string(),
                Value::Number(serde_json::Number::from(number)),
            );
        }
        chapter.insert("goal".to_string(), Value::String(goal));
        chapter.insert("expected_turn".to_string(), Value::String(expected_turn));
        repaired.push(Value::Object(chapter));
    }
    if repaired.is_empty() {
        None
    } else {
        Some(repaired)
    }
}

fn parse_world_rules_metadata_repair(raw: &str) -> Option<Vec<Value>> {
    if let Some(rules) = super::field_pack_world_rules(raw) {
        let rules = rules
            .into_iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value_missing(value))
            .map(Value::String)
            .collect::<Vec<_>>();
        if !rules.is_empty() {
            return Some(rules);
        }
    }
    let normalized = creation_contract_normalizer::normalize_creation_contract_boundary(raw)?;
    let object = normalized.value.as_object()?;
    let rules = metadata_value_aliases(object, &["world_rules", "worldRules", "世界规则", "规则"])
        .or_else(|| {
            object
                .get("governance_patch")
                .and_then(Value::as_object)
                .and_then(|governance| {
                    metadata_value_aliases(
                        governance,
                        &["world_rules", "worldRules", "世界规则", "规则"],
                    )
                })
        })
        .and_then(Value::as_array)?
        .iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value_missing(value))
        .map(|value| Value::String(value.to_string()))
        .collect::<Vec<_>>();
    (!rules.is_empty()).then_some(rules)
}

fn metadata_string_aliases(object: &serde_json::Map<String, Value>, keys: &[&str]) -> String {
    metadata_value_aliases(object, keys)
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
        .unwrap_or_default()
}

fn metadata_value_aliases<'a>(
    object: &'a serde_json::Map<String, Value>,
    keys: &[&str],
) -> Option<&'a Value> {
    for key in keys {
        if let Some(value) = object.get(*key) {
            return Some(value);
        }
    }
    let normalized_keys = keys
        .iter()
        .map(|key| normalize_metadata_key(key))
        .collect::<Vec<_>>();
    object.iter().find_map(|(key, value)| {
        let normalized = normalize_metadata_key(key);
        normalized_keys
            .iter()
            .any(|candidate| candidate == &normalized)
            .then_some(value)
    })
}

fn normalize_metadata_key(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !matches!(ch, '_' | '-' | ' ' | '\t' | '\n' | '\r'))
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outline_planning_issues_are_metadata_repairable() {
        let issues = vec![
            "ContractBlocker: 小说合同缺少分卷规划，不能进入写作确认".to_string(),
            "ContractBlocker: 小说合同缺少近期章节规划，不能进入写作确认".to_string(),
            "小说合同尚未形成逐章规划或分卷/阶段大纲".to_string(),
        ];

        assert!(
            creation_contract_issues_are_contract_metadata_only(&issues),
            "outline planning blockers should route to the existing metadata repair fallback"
        );
    }

    #[test]
    fn longform_terminal_position_uses_plot_repair_instead_of_metadata_repair() {
        for issue in [
            "ContractBlocker[outline.longform_position]: 小说合同开篇窗口或非末卷提前完成权威终局/主角弧线，但全书预计约40章；必须保留后续主线债务并把终局放回末卷",
            "ContractBlocker[outline.terminal_coverage]: 小说合同末卷没有执行权威终局的核心解决事件；不能从尚未解决的主冲突直接跳到尾声或稳定生活，必须把终局行动、结果和不可逆变化写入实际末卷",
        ] {
            assert!(
                !creation_contract_issues_are_contract_metadata_only(&[issue.to_string()]),
                "moving or restoring a terminal event changes plot semantics and belongs to the existing Plot typed patch: {issue}"
            );
        }
    }

    #[test]
    fn near_chapter_goal_issue_routes_to_metadata_repair() {
        let issues = vec![
            "ContractBlocker: 近期章节2目标重复使用主角名，疑似合同槽位污染，必须改成清晰事件目标"
                .to_string(),
        ];

        assert!(
            creation_contract_issues_are_contract_metadata_only(&issues),
            "near chapter goal blockers should use the existing metadata repair path"
        );
    }

    #[test]
    fn world_rules_issue_routes_to_focused_metadata_repair() {
        let issues = vec![
            "ContractBlocker: 小说合同世界规则[1]不像可执行规则、代价或限制，疑似截断主线或角色锚点"
                .to_string(),
        ];

        assert!(
            creation_contract_issues_are_contract_metadata_only(&issues),
            "existing but flawed world rules can use focused metadata repair"
        );
    }

    #[test]
    fn relationship_authority_issue_uses_governance_patch_instead_of_metadata_repair() {
        let issues = vec![
            "ContractBlocker: 关系线角色 `工友群体` 不在角色权威表中".to_string(),
            "ContractBlocker: 关系账本[2]引用角色权威表之外的角色 `工友群体`".to_string(),
        ];

        assert!(
            !creation_contract_issues_are_contract_metadata_only(&issues),
            "relationship ledger belongs to the existing governance patch scope, not metadata repair"
        );
    }

    #[test]
    fn missing_foundational_fields_use_staged_completion_not_metadata_repair() {
        let issues = vec![
            "ContractBlocker: 小说合同缺少世界规则".to_string(),
            "ContractBlocker: 小说合同缺少可锁定书名".to_string(),
            "小说合同尚未形成可锁定书名".to_string(),
        ];

        assert!(
            !creation_contract_issues_are_contract_metadata_only(&issues),
            "missing foundational fields should be generated by staged typed patches, not metadata repair"
        );
    }

    #[test]
    fn mixed_title_surface_and_missing_world_rules_route_to_metadata_repair() {
        let issues = vec![
            "ContractBlocker: 小说合同书名包含符号残片、外文残片或不像作品名".to_string(),
            "ContractBlocker: 小说合同缺少世界规则".to_string(),
        ];

        assert!(
            creation_contract_issues_are_contract_metadata_only(&issues),
            "a malformed title plus missing world rules should use the existing focused metadata repair path instead of repeatedly running broad staged completion"
        );
    }

    #[test]
    fn title_metadata_repair_rejects_rationale_without_declared_candidate() {
        let mut draft = build_initial_creation_draft(
            "title-repair",
            "fiction",
            "写一部都市爽文小说，每章2500字，至少5万字。",
        )
        .expect("draft");
        draft.fiction_premise = "阮桥遥因被未婚妻和富二代陷害破产，意外觉醒透视异能。".to_string();
        draft.fiction_ending_direction =
            "终局时主角摧毁财阀根基，公开资本黑幕并确立新秩序。".to_string();
        draft.fiction_main_causal_spine =
            "破产陷害，觉醒透视，追查资本黑幕，摧毁财阀根基。".to_string();
        draft.fiction_world_imagery = "古玩市场、股市盘口、财阀旧楼。".to_string();

        let metadata = parse_title_metadata_repair(
            &draft,
            r#"{"title":{"rationale":"书名取自终局时主角摧毁财阀根基，确立新秩序的核心爽点。","source":"llm_contract"}}"#,
        );

        assert!(
            metadata.is_none(),
            "metadata repair must not invent a local title when the model did not declare a candidate"
        );
    }

    #[test]
    fn local_metadata_repair_removes_repeated_primary_name_from_chapter_goal() {
        let raw = r#"{
            "title": {"canonical_title": "旧桥灵证", "rationale": "旧桥是终局公开证据的地点，灵证是主角反转夜校规则的关键物。"},
            "language": "zh-CN",
            "genre": "都市玄幻",
            "brief": "夜校考试接入城市灵轨，普通学生要证明灵籍被篡改。",
            "premise": "夜校灵轨控制学生晋级资格，灵籍账册被校盟篡改。",
            "ending": {"desired_resolution": "主角在终局公开灵轨账册，切断校盟对夜校考试的垄断。"},
            "protagonist_arc": "从只想通过考试的旁听生，变成愿意公开证据的秩序修补者。",
            "world_imagery": "夜校、灵轨、考试钟、灵籍账册。",
            "main_causal_spine": "主角在夜校考试中发现灵轨账册异常，追查校盟夺籍真相，终局公开账册改写晋级规则。",
            "characters": [
                {"canonical_name": "许照桥", "role": "主角", "desire": "通过夜校考试并查清父亲旧案", "fear": "再次被规则抹掉姓名", "bottom_line": "不把同学当成晋级垫脚石", "arc_start": "只想自保的旁听生", "arc_end": "公开证据的规则修补者"},
                {"canonical_name": "商砚衡", "role": "关键对手", "desire": "维护校盟垄断", "fear": "账册被公开", "bottom_line": "不亲手毁掉考试系统", "arc_start": "幕后监考者", "arc_end": "被证据逼到台前"}
            ],
            "world_rules": ["夜校灵轨会记录每次考试借力。"],
            "outline": {"near_chapters": [{"number": 1, "goal": "许照桥目睹导师许照桥，确认灵轨账册异常。", "expected_turn": "主角确认账册被人篡改并失去退路。"}], "raw_outline": "第一卷追查夜校灵轨，终局公开灵籍账册。"}
        }"#;
        let mut contract = NovelCreationContract::parse_json_boundary(raw).expect("contract");

        assert!(sanitize_repeated_primary_name_in_near_chapter_goals(
            &mut contract
        ));
        assert_eq!(
            contract.outline.near_chapters[0].goal,
            "许照桥目睹导师，确认灵轨账册异常。"
        );
        assert_eq!(
            contract.outline.near_chapters[0]
                .goal
                .matches("许照桥")
                .count(),
            1
        );
    }

    #[test]
    fn local_metadata_repair_preserves_unresolved_relationship_for_typed_gate() {
        let mut contract = NovelCreationContract {
            characters: vec![
                CharacterContract {
                    canonical_name: "许照桥".to_string(),
                    role: "主角".to_string(),
                    ..Default::default()
                },
                CharacterContract {
                    canonical_name: "商砚衡".to_string(),
                    role: "对手".to_string(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        contract.structured.relationship_ledger = vec![
            RelationshipLedgerEntry {
                characters: vec!["许照桥".to_string(), "商砚衡".to_string()],
                relationship_type: "调查者与阻挠者".to_string(),
                ..Default::default()
            },
            RelationshipLedgerEntry {
                characters: vec!["许照桥".to_string(), "外部证人（旧名）".to_string()],
                relationship_type: "尚未完成身份映射的关系".to_string(),
                ..Default::default()
            },
        ];

        assert!(!reconcile_relationship_ledger_authority(&mut contract));
        assert_eq!(contract.structured.relationship_ledger.len(), 2);
    }

    #[test]
    fn local_metadata_repair_reconciles_character_plan_anchors_with_actual_outline() {
        let mut contract = NovelCreationContract {
            characters: vec![
                CharacterContract {
                    canonical_name: "许照桥".to_string(),
                    role: "主角".to_string(),
                    planned_entry: "第2卷发现灵籍账册".to_string(),
                    planned_exit: "第4卷公开第一批证据".to_string(),
                    ..Default::default()
                },
                CharacterContract {
                    canonical_name: "商砚衡".to_string(),
                    role: "对手".to_string(),
                    planned_entry: "第2卷开始阻挠调查".to_string(),
                    planned_exit: "第4卷失去校盟席位".to_string(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        contract.outline.volumes = (1..=5)
            .map(|number| VolumeContract {
                title: format!("第{number}卷"),
                ..Default::default()
            })
            .collect();

        assert!(reconcile_character_plan_anchors_with_outline(&mut contract));
        assert_eq!(contract.characters[0].planned_entry, "第1卷进入主线");
        assert_eq!(contract.characters[0].planned_exit, "持续至第5卷终局");
        assert!(!typed_contract_gate::character_plan_anchor_needs_repair(
            &contract.characters[0].planned_entry,
            contract.outline.volumes.len(),
            true,
            false,
        ));
        assert!(!typed_contract_gate::character_plan_anchor_needs_repair(
            &contract.characters[0].planned_exit,
            contract.outline.volumes.len(),
            true,
            true,
        ));
        assert_eq!(contract.characters[1].planned_entry, "第2卷开始阻挠调查");
        assert_eq!(contract.characters[1].planned_exit, "第4卷失去校盟席位");
        assert!(
            !reconcile_character_plan_anchors_with_outline(&mut contract),
            "the existing local repair pass must converge after one normalization"
        );
    }

    #[test]
    fn local_metadata_repair_reconciles_secondary_out_of_range_plan_anchors() {
        let mut contract = NovelCreationContract {
            characters: vec![CharacterContract {
                canonical_name: "商砚衡".to_string(),
                role: "对手".to_string(),
                planned_entry: "第4卷封锁调查入口".to_string(),
                planned_exit: "第4卷失去校盟席位".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };
        contract.outline.volumes = (1..=3)
            .map(|number| VolumeContract {
                title: format!("第{number}卷"),
                ..Default::default()
            })
            .collect();

        assert!(reconcile_character_plan_anchors_with_outline(&mut contract));
        assert_eq!(contract.characters[0].planned_entry, "第3卷封锁调查入口");
        assert_eq!(contract.characters[0].planned_exit, "第3卷失去校盟席位");
        assert!(!typed_contract_gate::character_plan_anchor_needs_repair(
            &contract.characters[0].planned_entry,
            contract.outline.volumes.len(),
            false,
            false,
        ));
        assert!(!typed_contract_gate::character_plan_anchor_needs_repair(
            &contract.characters[0].planned_exit,
            contract.outline.volumes.len(),
            false,
            true,
        ));
        assert!(!reconcile_character_plan_anchors_with_outline(
            &mut contract
        ));
    }

    #[test]
    fn mixed_title_and_plot_issues_still_expose_title_repair() {
        let issues = vec![
            "ContractBlocker: 书名理由含有合同槽位名占位，需要重新生成干净合同字段".to_string(),
            "ContractBlocker: 大纲含有合同槽位名占位，需要重新生成干净合同字段".to_string(),
            "ContractBlocker: 近期章节2转折含有合同槽位名占位，需要重新生成干净合同字段"
                .to_string(),
        ];

        assert!(
            creation_contract_issues_contain_title_metadata(&issues),
            "mixed blockers must allow title metadata repair to run before plot repair"
        );
        assert!(
            issues
                .iter()
                .any(|issue| !creation_contract_issue_is_title_metadata(issue)),
            "mixed blockers are not title-only and must continue to later repair stages"
        );
    }

    #[test]
    fn pending_plot_metadata_repair_merges_typed_scope_over_stale_current_contract() {
        let mut draft = build_initial_creation_draft(
            "pending-plot-metadata-repair",
            "fiction",
            "我想从零创作一本修仙长篇小说，总字数10万字，每章2500字。",
        )
        .expect("draft");
        let base = NovelCreationContract::parse_json_boundary(
            r#"{
              "title":{"canonical_title":"无灵天","rationale":"终局天地灵气消散，世界由此进入无灵纪元。"},
              "language":"zh-CN","genre":"修仙","brief":"无灵根少年以凡铁之道挑战宗门灵脉垄断。",
              "target_units":100000,"chapter_unit_target":2500,"max_chapters_per_turn":1,
              "premise":"宗门垄断日益枯竭的灵脉，无灵根少年叶昭安发现凡铁可以淬炼肉身。",
              "ending":{"desired_resolution":"叶昭安公开灵脉代价并斩断宗门主灵脉。","final_state":"灵气垄断终结，凡俗建立公开修行秩序。"},
              "protagonist_arc":"叶昭安从只求自保的杂役成长为承担新秩序代价的守护者。",
              "world_imagery":"枯竭灵泉、锈蚀矿井、浮空宗门与凡铁剑胚。",
              "main_causal_spine":"灵脉枯竭迫使宗门加重掠夺，叶昭安从矿井证据追到主灵脉，最终终结垄断。",
              "characters":[
                {"canonical_name":"叶昭安","role":"男主","desire":"终结灵脉垄断","fear":"凡人再次成为宗门耗材","bottom_line":"绝不吞食活人血肉换取力量","arc_start":"只求活命的杂役","arc_end":"承担新秩序代价的守护者"},
                {"canonical_name":"秦景棠","role":"导师","desire":"保存凡铁铸造传承","fear":"技艺随矿井封闭失传","bottom_line":"绝不把未开锋剑胚交给宗门","arc_start":"隐居矿井的铸剑师","arc_end":"公开传承并保护弟子"},
                {"canonical_name":"梁栖澜","role":"对手","desire":"维持宗门资源特权","fear":"灵脉账目被凡俗公开","bottom_line":"绝不允许凡人进入主灵脉","arc_start":"掌控外门资源的天才","arc_end":"失去特权并面对凡俗审判"}
              ],
              "themes":["力量必须承担可验证的代价。"],
              "world_rules":[
                "凡铁每强化一次肉身都会造成骨骼裂伤，必须休养七日才能再次使用。",
                "宗门每抽取一处支脉都会使对应凡城水源永久下降一成。",
                "主灵脉一旦切断，依赖其悬浮的建筑会在七日内落地且无法恢复。"
              ],
              "style_rules":["每章必须用具体行动推进证据链或人物选择。"],
              "must_avoid":["不得用突然出现的万能血脉绕过凡铁代价。"],
              "outline":{
                "raw_outline":"叶昭安从矿井异常追查宗门灵脉垄断，并在终局承担切断主灵脉的后果。",
                "volumes":[{"title":"铁骨初成","objective":"叶昭安取得矿井账册","ending_change":"叶昭安取得矿井账册"}],
                "near_chapters":[
                  {"number":1,"goal":"叶昭安保存矿井坍塌记录","expected_turn":"他发现坍塌时间与宗门抽脉一致"},
                  {"number":2,"goal":"叶昭安核对凡城水位","expected_turn":"秦景棠交出旧灵脉图"},
                  {"number":3,"goal":"叶昭安潜入外门库房","expected_turn":"梁栖澜下令封锁矿井"}
                ]
              },
              "structured":{"payoff_matrix":[{"promise":"旧灵脉图记录抽脉顺序","payoff_target":"终局用图纸证明宗门长期掠夺支脉","status":"planned"}]}
            }"#,
        )
        .expect("base contract");
        let issues = base
            .validate_for_scope(ContractReadinessScope::LockedAuthorityContract)
            .issues;
        assert!(issues
            .iter()
            .any(|issue| issue.contains("分卷规划含有结构污染或无效卷名")));
        draft.pending_contract_candidate = Some(serde_json::json!({
            "normalized": base,
            "issues": issues.messages(),
            "raw_preview": "stale candidate"
        }));
        let patch = r#"{
          "patch_type":"plot_patch",
          "outline":{
            "volumes":[{"title":"第一卷《铁骨初成》","objective":"叶昭安取得矿井账册并查明支脉抽取顺序。","ending_change":"梁栖澜封锁矿井，叶昭安失去退回杂役院的可能。"}],
            "near_chapters":[
              {"number":1,"goal":"叶昭安保存矿井坍塌记录","expected_turn":"他发现坍塌时间与宗门抽脉一致"},
              {"number":2,"goal":"叶昭安核对凡城水位","expected_turn":"秦景棠交出旧灵脉图"},
              {"number":3,"goal":"叶昭安潜入外门库房","expected_turn":"梁栖澜下令封锁矿井"}
            ]
          }
        }"#;

        let outcome = submit_pending_contract_metadata_repair(&mut draft, patch)
            .expect("metadata repair outcome");
        let repaired = pending_normalized_contract(&draft)
            .or_else(|| {
                draft.current_contract.as_ref().and_then(|value| {
                    NovelCreationContract::parse_json_boundary(&value.to_string())
                })
            })
            .unwrap_or_else(|| {
                panic!(
                    "repair produced neither pending nor ready contract: {:?}",
                    outcome.gate.actionable_issues()
                )
            });

        assert_eq!(repaired.outline.volumes[0].title, "铁骨初成");
        assert!(
            repaired.outline.volumes[0]
                .objective
                .contains("取得矿井账册并查明支脉抽取顺序"),
            "{:?}",
            repaired.outline.volumes
        );
        assert!(repaired.outline.volumes[0]
            .ending_change
            .contains("封锁矿井"));
        assert!(repaired.outline.volumes[0]
            .ending_change
            .contains("失去退回杂役院的可能"));
        assert_ne!(
            repaired.outline.volumes[0].objective,
            repaired.outline.volumes[0].ending_change
        );
        assert_eq!(repaired.outline.near_chapters.len(), 3);
        assert_eq!(
            repaired
                .outline
                .near_chapters
                .iter()
                .map(|chapter| chapter.number)
                .collect::<Vec<_>>(),
            vec![Some(1), Some(2), Some(3)]
        );
    }
}
