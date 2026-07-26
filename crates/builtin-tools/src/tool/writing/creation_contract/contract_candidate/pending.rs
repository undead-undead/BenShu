use super::*;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ContractCandidateRank {
    user_authority_preserved: i64,
    confirmed_fields_preserved: i64,
    blocker_count: i64,
    blocker_vector: [i64; 6],
    cross_field_contradictions: i64,
    protected_fields_preserved: i64,
    filled_field_score: i64,
}

pub(crate) fn creation_draft_with_pending_contract_applied(
    draft: &SessionCreationDraftState,
) -> SessionCreationDraftState {
    creation_draft_and_contract_with_pending_applied(draft).0
}

pub(crate) fn creation_draft_and_contract_with_pending_applied(
    draft: &SessionCreationDraftState,
) -> (SessionCreationDraftState, NovelCreationContract) {
    let mut visible = draft.clone();
    visible.pending_contract_candidate = None;
    let visible_rank = draft_contract_validation_rank(&visible, &visible);
    let visible_contract = super::strong_novel_contract_from_visible_creation_draft(&visible);

    let mut effective = visible.clone();
    let Some(mut contract) = pending_normalized_contract(draft) else {
        return (visible, visible_contract);
    };
    seed_new_pending_character_name_authority(&mut effective, &contract);
    apply_strong_novel_contract_to_creation_draft(&mut effective, &mut contract);
    normalize_fiction_creation_draft_after_contract_change(&mut effective);
    sanitize_creation_draft_control_noise(&mut effective);
    effective.current_contract = None;
    effective.pending_contract_candidate = None;
    if draft_contract_validation_rank(&effective, &visible) < visible_rank {
        return (visible, visible_contract);
    }
    let effective_contract = super::strong_novel_contract_from_visible_creation_draft(&effective);
    (effective, effective_contract)
}

fn seed_new_pending_character_name_authority(
    draft: &mut SessionCreationDraftState,
    contract: &NovelCreationContract,
) {
    let mut existing = draft
        .fiction_characters
        .iter()
        .filter(|line| character_line_has_locked_name_authority(draft, line))
        .map(|line| draft_character_line_to_contract(line))
        .collect::<Vec<_>>();
    for pending in &contract.characters {
        let pending_line = pending.to_draft_line();
        if !character_line_has_locked_name_authority(draft, &pending_line)
            || existing.iter().any(|known| {
                (!value_missing(&known.character_id)
                    && known.character_id.trim() == pending.character_id.trim())
                    || known.canonical_name.trim() == pending.canonical_name.trim()
                    || patch::character_contract_roles_match(known, pending)
            })
        {
            continue;
        }
        let authority = CharacterContract {
            character_id: pending.character_id.clone(),
            canonical_name: pending.canonical_name.clone(),
            name_source: pending.name_source.clone(),
            role: pending.role.clone(),
            previous_names: pending.previous_names.clone(),
            ..Default::default()
        };
        draft.fiction_characters.push(authority.to_draft_line());
        existing.push(authority);
    }
}

fn draft_contract_validation_rank(
    draft: &SessionCreationDraftState,
    authority: &SessionCreationDraftState,
) -> ContractCandidateRank {
    let contract = super::strong_novel_contract_from_creation_draft(draft);
    let issues = contract
        .validate_for_scope(ContractReadinessScope::LockedAuthorityContract)
        .issues;
    let issue_messages = issues.messages();
    let mut blocker_vector = [0_i64; 6];
    for issue in issues.iter() {
        let index = match issue.kind {
            super::issue::ContractIssueKind::Skeleton => 0,
            super::issue::ContractIssueKind::Characters => 1,
            super::issue::ContractIssueKind::Plot => 2,
            super::issue::ContractIssueKind::Governance => 3,
            super::issue::ContractIssueKind::Diagnostic => 4,
            super::issue::ContractIssueKind::Other => 5,
        };
        blocker_vector[index] -= 1;
    }
    ContractCandidateRank {
        user_authority_preserved: -contract_user_authority_regressions(authority, &contract),
        confirmed_fields_preserved: -contract_confirmed_field_regressions(authority, &contract),
        blocker_count: -(issues.len() as i64),
        blocker_vector,
        cross_field_contradictions: -(issues
            .iter()
            .filter(|issue| contract_issue_is_cross_field_contradiction(issue))
            .count() as i64),
        protected_fields_preserved: -contract_protected_field_regressions(authority, &contract),
        filled_field_score: pending_contract_candidate_field_score(&serde_json::json!({
            "normalized": contract,
            "issues": issue_messages,
        })),
    }
}

fn contract_issue_is_cross_field_contradiction(issue: &super::issue::ContractIssue) -> bool {
    issue.code.contains("authority")
        || issue.code.contains("reference")
        || issue.code.contains("primary_role")
        || issue.code.starts_with("semantic.")
}

fn contract_user_authority_regressions(
    authority: &SessionCreationDraftState,
    candidate: &NovelCreationContract,
) -> i64 {
    let mut regressions = 0;
    if authority.target_units_user_specified && candidate.target_units != authority.target_units {
        regressions += 1;
    }
    if authority.chapter_unit_target_user_specified
        && candidate.chapter_unit_target != authority.chapter_unit_target
    {
        regressions += 1;
    }
    let authority_contract = super::strong_novel_contract_from_visible_creation_draft(authority);
    if matches!(
        authority_contract.title.source,
        crate::tool::writing::creation_contract_model::TitleSource::User
    ) && authority_contract.title.canonical_title.trim()
        != candidate.title.canonical_title.trim()
    {
        regressions += 1;
    }
    for character in authority_contract.characters.iter().filter(|character| {
        character.name_source.trim() == "user" && !value_missing(&character.canonical_name)
    }) {
        let preserved = candidate.characters.iter().any(|incoming| {
            (!value_missing(&character.character_id)
                && incoming.character_id.trim() == character.character_id.trim()
                || patch::character_contract_roles_match(character, incoming))
                && incoming.canonical_name.trim() == character.canonical_name.trim()
        });
        if !preserved {
            regressions += 1;
        }
    }
    regressions
}

fn contract_confirmed_field_regressions(
    authority: &SessionCreationDraftState,
    candidate: &NovelCreationContract,
) -> i64 {
    let Some(confirmed) = authority
        .current_contract
        .as_ref()
        .and_then(|value| NovelCreationContract::parse_json_boundary(&value.to_string()))
    else {
        return 0;
    };
    let pending_kinds = super::pending_explicit_contract_revision_findings(authority)
        .iter()
        .map(|issue| issue.kind)
        .collect::<BTreeSet<_>>();
    let mut regressions = 0;
    if !pending_kinds.contains(&super::issue::ContractIssueKind::Skeleton)
        && contract_skeleton_value(&confirmed) != contract_skeleton_value(candidate)
    {
        regressions += 1;
    }
    if !pending_kinds.contains(&super::issue::ContractIssueKind::Characters)
        && serde_json::to_value(&confirmed.characters).ok()
            != serde_json::to_value(&candidate.characters).ok()
    {
        regressions += 1;
    }
    if !pending_kinds.contains(&super::issue::ContractIssueKind::Plot)
        && serde_json::to_value(&confirmed.outline).ok()
            != serde_json::to_value(&candidate.outline).ok()
    {
        regressions += 1;
    }
    if !pending_kinds.contains(&super::issue::ContractIssueKind::Governance)
        && contract_governance_value(&confirmed) != contract_governance_value(candidate)
    {
        regressions += 1;
    }
    regressions
}

fn contract_protected_field_regressions(
    authority: &SessionCreationDraftState,
    candidate: &NovelCreationContract,
) -> i64 {
    let existing = super::strong_novel_contract_from_visible_creation_draft(authority);
    let mut regressions = 0;
    for (old, new) in [
        (existing.language.as_str(), candidate.language.as_str()),
        (existing.genre.as_str(), candidate.genre.as_str()),
        (existing.brief.as_str(), candidate.brief.as_str()),
    ] {
        if !value_missing(old) && value_missing(new) {
            regressions += 1;
        }
    }
    for character in existing.characters.iter().filter(|character| {
        !value_missing(&character.canonical_name)
            && character_line_has_locked_name_authority(authority, &character.to_draft_line())
    }) {
        if !candidate
            .characters
            .iter()
            .any(|incoming| incoming.canonical_name.trim() == character.canonical_name.trim())
        {
            regressions += 1;
        }
    }
    regressions
}

fn contract_skeleton_value(contract: &NovelCreationContract) -> Value {
    serde_json::json!({
        "title": contract.title,
        "language": contract.language,
        "genre": contract.genre,
        "brief": contract.brief,
        "target_units": contract.target_units,
        "chapter_unit_target": contract.chapter_unit_target,
        "max_chapters_per_turn": contract.max_chapters_per_turn,
        "premise": contract.premise,
        "ending": contract.ending,
        "protagonist_arc": contract.protagonist_arc,
        "world_imagery": contract.world_imagery,
        "main_causal_spine": contract.main_causal_spine,
    })
}

fn contract_governance_value(contract: &NovelCreationContract) -> Value {
    serde_json::json!({
        "themes": contract.themes,
        "world_rules": contract.world_rules,
        "style_rules": contract.style_rules,
        "must_avoid": contract.must_avoid,
        "structured": contract.structured,
    })
}

pub(crate) fn pending_normalized_contract(
    draft: &SessionCreationDraftState,
) -> Option<NovelCreationContract> {
    let normalized = draft
        .pending_contract_candidate
        .as_ref()
        .and_then(|value| value.get("normalized"))?;
    let text = serde_json::to_string(normalized).ok()?;
    NovelCreationContract::parse_json_boundary(&text)
}

pub(crate) fn contract_boundary_quality_issues(
    draft: &SessionCreationDraftState,
    contract_text: &str,
) -> Vec<String> {
    let mut issues = Vec::new();
    let language = draft.language.to_ascii_lowercase();
    let expects_chinese = language.starts_with("zh") || draft.language.contains("中文");
    if expects_chinese {
        if let Some(fragment) = unexpected_non_cjk_script_fragment(contract_text) {
            issues.push(format!("中文合同混入非中文脚本残片：{fragment}"));
        }
        if let Some(fragment) = latex_or_escape_residue_fragment(contract_text) {
            issues.push(format!("中文合同混入转义或 LaTeX 残片：{fragment}"));
        }
        if let Some(fragment) = cjk_underscore_fragment(contract_text) {
            issues.push(format!("中文合同混入异常下划线残片：{fragment}"));
        }
        if let Some(fragment) = malformed_contract_bullet_prefix_fragment(contract_text) {
            issues.push(format!("中文合同混入异常列表前缀：{fragment}"));
        }
    }
    if let Some(fragment) = degenerate_repetition_fragment(contract_text) {
        issues.push(format!("合同出现连续重复退化片段：{fragment}"));
    }
    if surface_sanitizer::contains_legal_contract_residue(contract_text) {
        issues.push("创作蓝图输出混入法律合同/委托协议字段，不能作为小说设定草案".to_string());
    }
    if let Some(fragment) = malformed_numeric_fragment(contract_text) {
        issues.push(format!("合同数字格式异常：{fragment}"));
    }
    if let Some(fragment) = malformed_contract_name_fragment(contract_text) {
        issues.push(format!("合同命名字段异常：{fragment}"));
    }
    if let Some(fragment) = assistant_surface_noise_fragment(contract_text) {
        issues.push(format!("合同混入面板说明或上一轮草案提示：{fragment}"));
    }
    issues.sort();
    issues.dedup();
    issues
}

pub(crate) fn record_pending_contract_candidate(
    draft: &mut SessionCreationDraftState,
    raw_contract_text: &str,
    normalized_value: Option<Value>,
    issues: &[String],
) -> bool {
    let mut candidate = serde_json::Map::new();
    candidate.insert(
        "raw_preview".to_string(),
        Value::String(preview_text(raw_contract_text, 1600)),
    );
    if let Some(value) = normalized_value {
        candidate.insert("normalized".to_string(), value);
    }
    candidate.insert(
        "issues".to_string(),
        Value::Array(issues.iter().cloned().map(Value::String).collect()),
    );
    candidate.insert(
        "created_at".to_string(),
        Value::String(chrono::Utc::now().to_rfc3339()),
    );
    let candidate = Value::Object(candidate);
    let should_replace = draft
        .pending_contract_candidate
        .as_ref()
        .map(|existing| {
            match (
                normalized_pending_candidate_validation_rank(draft, &candidate),
                normalized_pending_candidate_validation_rank(draft, existing),
            ) {
                (Some(candidate_rank), Some(existing_rank)) => candidate_rank >= existing_rank,
                _ => {
                    pending_contract_candidate_quality_rank(&candidate)
                        >= pending_contract_candidate_quality_rank(existing)
                }
            }
        })
        .unwrap_or(true);
    if should_replace {
        draft.pending_contract_candidate = Some(candidate);
        if !issues.is_empty() {
            draft.diagnostics = merge_list(
                &draft.diagnostics,
                &[format!(
                    "合同候选未进入可确认草案：{}",
                    public_contract_candidate_issue_summary(issues)
                )],
            );
        }
    }
    should_replace
}

fn normalized_pending_candidate_validation_rank(
    draft: &SessionCreationDraftState,
    candidate: &Value,
) -> Option<ContractCandidateRank> {
    let normalized = candidate.as_object()?.get("normalized")?;
    let mut contract = NovelCreationContract::parse_json_boundary(&normalized.to_string())?;
    let mut candidate_draft = draft.clone();
    candidate_draft.current_contract = None;
    candidate_draft.pending_contract_candidate = None;
    apply_strong_novel_contract_to_creation_draft(&mut candidate_draft, &mut contract);
    normalize_fiction_creation_draft_after_contract_change(&mut candidate_draft);
    sanitize_creation_draft_control_noise(&mut candidate_draft);
    Some(draft_contract_validation_rank(&candidate_draft, draft))
}

fn public_contract_candidate_issue_summary(issues: &[String]) -> String {
    let mut summary = Vec::new();
    if issues.iter().any(|issue| issue.contains("书名")) {
        summary.push("书名还没有通过质量门，需要重新提供来自终局、大纲和故事锚点的候选");
    }
    if issues
        .iter()
        .any(|issue| issue.contains("角色") || issue.contains("主角"))
    {
        summary.push("角色权威表还不完整，需要补齐稳定姓名、欲望、恐惧和底线");
    }
    if issues
        .iter()
        .any(|issue| issue.contains("分卷") || issue.contains("近期章节"))
    {
        summary.push("分卷规划或近期章节规划还不完整");
    }
    if issues
        .iter()
        .any(|issue| issue.contains("世界观") || issue.contains("规则"))
    {
        summary.push("世界观规则还不够可执行");
    }
    if summary.is_empty() {
        summary.push("合同字段还没有补齐或格式仍需修复");
    }
    summary.join("；")
}

fn pending_contract_candidate_quality_rank(candidate: &Value) -> (i64, i64, i64, i64, i64) {
    let has_normalized = candidate
        .as_object()
        .and_then(|object| object.get("normalized"))
        .is_some() as i64;
    (
        has_normalized,
        -pending_contract_candidate_hard_issue_penalty(candidate),
        -pending_contract_candidate_issue_count(candidate),
        pending_contract_candidate_field_score(candidate),
        -pending_contract_candidate_issue_penalty(candidate),
    )
}

fn pending_contract_candidate_issue_count(candidate: &Value) -> i64 {
    candidate
        .as_object()
        .and_then(|object| object.get("issues"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|issue| issue.as_str().is_some())
                .count() as i64
        })
        .unwrap_or(0)
}

fn pending_contract_candidate_hard_issue_penalty(candidate: &Value) -> i64 {
    let Some(object) = candidate.as_object() else {
        return i64::MAX / 4;
    };
    object
        .get("issues")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .filter(|issue| {
                    issue.contains("不能解析")
                        || issue.contains("schema")
                        || issue.contains("JSON")
                        || issue.contains("没有形成可归位")
                        || issue.contains("混入")
                        || issue.contains("残片")
                        || issue.contains("法律合同")
                        || issue.contains("连续重复退化")
                })
                .map(contract_candidate_issue_penalty)
                .sum()
        })
        .unwrap_or(0)
}

fn pending_contract_candidate_issue_penalty(candidate: &Value) -> i64 {
    let Some(object) = candidate.as_object() else {
        return i64::MAX / 4;
    };
    object
        .get("issues")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(contract_candidate_issue_penalty)
                .sum()
        })
        .unwrap_or(0)
}

pub(crate) fn contract_candidate_issue_penalty(issue: &str) -> i64 {
    let mut penalty = 12;
    if issue.contains("不能解析")
        || issue.contains("schema")
        || issue.contains("JSON")
        || issue.contains("没有形成可归位")
    {
        penalty += 24;
    }
    if issue.contains("混入")
        || issue.contains("污染")
        || issue.contains("残片")
        || issue.contains("法律合同")
        || issue.contains("连续重复退化")
    {
        penalty += 36;
    }
    if issue.contains("书名") || issue.contains("角色") || issue.contains("主角") {
        penalty += 10;
    }
    if issue.contains("缺少") || issue.contains("尚未形成") {
        penalty += 16;
    }
    penalty
}

fn pending_contract_candidate_field_score(candidate: &Value) -> i64 {
    let Some(object) = candidate.as_object() else {
        return i64::MIN / 2;
    };
    let mut score = 0_i64;
    if let Some(contract) = object
        .get("normalized")
        .and_then(|value| NovelCreationContract::parse_json_boundary(&value.to_string()))
    {
        score += 100;
        score += contract_field_score(&contract.title.canonical_title, 12);
        score += contract_field_score(&contract.title.rationale, 8);
        score += contract_field_score(&contract.language, 4);
        score += contract_field_score(&contract.genre, 4);
        score += contract_field_score(&contract.brief, 4);
        score += contract_field_score(&contract.premise, 8);
        score += contract_field_score(&contract.ending.desired_resolution, 10);
        score += contract_field_score(&contract.ending.final_state, 8);
        score += contract_field_score(&contract.protagonist_arc, 8);
        score += contract_field_score(&contract.world_imagery, 8);
        score += contract_field_score(&contract.main_causal_spine, 8);
        score += contract_world_rule_score(&contract.world_rules, 3);
        score += (contract.themes.len().min(3) as i64) * 2;
        for character in contract.characters.iter().take(5) {
            score += contract_field_score(&character.canonical_name, 6);
            score += contract_field_score(&character.role, 2);
            score += contract_character_anchor_score(&character.desire, 2);
            score += contract_character_anchor_score(&character.fear, 2);
            score += contract_character_anchor_score(&character.bottom_line, 2);
            score += contract_character_anchor_score(&character.arc_start, 2);
            score += contract_character_anchor_score(&character.arc_end, 2);
        }
        score += contract_field_score(&contract.structured.emotional_contract.primary_emotion, 4);
        score += contract_field_score(&contract.structured.emotional_contract.emotional_promise, 4);
        score += contract_field_score(
            &contract
                .structured
                .emotional_contract
                .ending_emotional_state,
            4,
        );
        score += (contract.structured.relationship_ledger.len().min(4) as i64) * 5;
        score += (contract.structured.emotional_state_ledger.len().min(4) as i64) * 3;
        for volume in contract.outline.volumes.iter().take(4) {
            score += contract_field_score(&volume.title, 3);
            score += contract_field_score(&volume.objective, 3);
            score += contract_field_score(&volume.ending_change, 3);
        }
        for chapter in contract.outline.near_chapters.iter().take(8) {
            score += contract_field_score(&chapter.goal, 3);
            score += contract_field_score(&chapter.expected_turn, 4);
        }
        score -= contract_structured_surface_noise_penalty(&contract);
    }
    score
}

fn contract_field_score(value: &str, weight: i64) -> i64 {
    if value_missing(value) {
        0
    } else {
        weight
    }
}

fn contract_world_rule_score(values: &[String], weight: i64) -> i64 {
    values
        .iter()
        .filter(|value| {
            !value_missing(value)
                && !crate::tool::writing::typed_contract_gate::world_rule_looks_truncated_or_not_actionable(value)
        })
        .take(4)
        .count() as i64
        * weight
}

fn contract_character_anchor_score(value: &str, weight: i64) -> i64 {
    if value_missing(value)
        || crate::tool::writing::typed_contract_gate::character_anchor_uses_generic_placeholder(value)
        || crate::tool::writing::typed_contract_gate::character_anchor_looks_like_storyline_or_truncated_surface(value)
    {
        0
    } else {
        weight
    }
}

fn contract_structured_surface_noise_penalty(contract: &NovelCreationContract) -> i64 {
    let Ok(value) = serde_json::to_value(&contract.structured) else {
        return 0;
    };
    value_structured_surface_noise_penalty(&value).min(160)
}

fn value_structured_surface_noise_penalty(value: &Value) -> i64 {
    match value {
        Value::String(text) => {
            let mut penalty = 0;
            if crate::tool::writing::surface_sanitizer::contains_excessive_repeated_cjk_surface_noise(text) {
                penalty += 48;
            }
            if crate::tool::writing::surface_sanitizer::contains_legal_contract_residue(text) {
                penalty += 48;
            }
            if crate::tool::writing::surface_sanitizer::contains_generic_contract_placeholder_residue(text) {
                penalty += 16;
            }
            penalty
        }
        Value::Array(items) => items
            .iter()
            .map(value_structured_surface_noise_penalty)
            .sum(),
        Value::Object(map) => map
            .values()
            .map(value_structured_surface_noise_penalty)
            .sum(),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_rank_never_trades_away_user_numeric_authority_for_field_density() {
        let authority = build_initial_creation_draft(
            "pending-user-numeric-authority",
            "fiction",
            "写一部都市小说，每章5000字，一共100万字。",
        )
        .expect("draft");
        let preserved = strong_novel_contract_from_visible_creation_draft(&authority);
        let mut regressed = preserved.clone();
        regressed.target_units = Some(100_000);
        regressed.chapter_unit_target = Some(2_500);

        let mut preserved_draft = authority.clone();
        let mut preserved_contract = preserved;
        apply_strong_novel_contract_to_creation_draft(
            &mut preserved_draft,
            &mut preserved_contract,
        );
        let mut regressed_draft = authority.clone();
        apply_strong_novel_contract_to_creation_draft(&mut regressed_draft, &mut regressed);
        regressed_draft.target_units = Some(100_000);
        regressed_draft.chapter_unit_target = Some(2_500);

        assert!(
            draft_contract_validation_rank(&preserved_draft, &authority)
                > draft_contract_validation_rank(&regressed_draft, &authority)
        );
    }

    #[test]
    fn confirmed_scope_is_protected_until_user_explicitly_reopens_that_scope() {
        let mut authority = build_initial_creation_draft(
            "pending-confirmed-scope-authority",
            "fiction",
            "写一部都市小说，每章2500字，一共10万字。",
        )
        .expect("draft");
        authority.fiction_premise = "审计员发现旧城资源账本被系统性篡改。".to_string();
        let confirmed = strong_novel_contract_from_visible_creation_draft(&authority);
        authority.current_contract =
            Some(serde_json::to_value(&confirmed).expect("confirmed contract"));
        let mut changed = confirmed.clone();
        changed.premise = "审计员改为调查一宗无关的校园案件。".to_string();

        assert_eq!(
            contract_confirmed_field_regressions(&authority, &changed),
            1
        );
        authority.planning_notes.push(
            super::super::draft_lifecycle::pending_explicit_contract_revision_note(
                CreationContractPatchType::Skeleton,
                "把故事前提改为校园案件",
            ),
        );
        assert_eq!(
            contract_confirmed_field_regressions(&authority, &changed),
            0
        );
    }

    #[test]
    fn partial_visible_authority_does_not_regovern_locked_pending_names() {
        let mut draft = build_initial_creation_draft(
            "pending-partial-character-authority",
            "fiction",
            "写一部都市悬疑小说，每章2500字，一共10万字。",
        )
        .expect("draft");
        draft.fiction_characters = vec![CharacterContract {
            character_id: "primary".to_string(),
            canonical_name: "闻庭安".to_string(),
            name_source: "generated_by_writing_tool_policy".to_string(),
            role: "主角".to_string(),
            desire: "公开事故记录".to_string(),
            fear: "证据再次被销毁".to_string(),
            bottom_line: "绝不伪造原始记录".to_string(),
            arc_start: "独自调查".to_string(),
            arc_end: "公开完整证据链".to_string(),
            planned_entry: "第1卷进入主线".to_string(),
            planned_exit: "第4卷公开完整证据链".to_string(),
            ..Default::default()
        }
        .to_draft_line()];
        let mut repaired_primary = draft_character_line_to_contract(&draft.fiction_characters[0]);
        repaired_primary.bottom_line = "绝不伪造原始记录，也不删改不利于自己的复核误差".to_string();
        repaired_primary.planned_exit = "第4卷公开完整证据链；持续至第3卷终局".to_string();
        let pending = NovelCreationContract {
            characters: vec![
                repaired_primary,
                CharacterContract {
                    character_id: "companion".to_string(),
                    canonical_name: "沈星岚".to_string(),
                    name_source: "generated_by_writing_tool_policy".to_string(),
                    role: "关键同伴".to_string(),
                    desire: "复核事故样本".to_string(),
                    fear: "样本被调包".to_string(),
                    bottom_line: "绝不隐瞒复核误差".to_string(),
                    arc_start: "只相信实验数据".to_string(),
                    arc_end: "愿意共同公开证据".to_string(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        draft.pending_contract_candidate = Some(serde_json::json!({
            "normalized": pending,
            "issues": ["ContractBlocker: 小说合同故事骨架尚待补齐"]
        }));

        let effective = creation_draft_with_pending_contract_applied(&draft);
        let names = effective
            .fiction_characters
            .iter()
            .map(|line| draft_character_line_to_contract(line).canonical_name)
            .collect::<BTreeSet<_>>();

        assert!(names.contains("闻庭安"), "{names:?}");
        assert!(names.contains("沈星岚"), "{names:?}");
        let primary = effective
            .fiction_characters
            .iter()
            .map(|line| draft_character_line_to_contract(line))
            .find(|character| character.canonical_name == "闻庭安")
            .expect("locked primary authority");
        assert_eq!(
            primary.bottom_line, "绝不伪造原始记录，也不删改不利于自己的复核误差",
            "pending repaired fields must survive locked-name authority alignment"
        );
    }

    #[test]
    fn partially_repaired_core_field_ranks_above_completely_missing_field() {
        let missing = serde_json::json!({
            "normalized": {"world_rules": []},
            "issues": ["ContractBlocker: 小说合同缺少世界规则"]
        });
        let partial = serde_json::json!({
            "normalized": {
                "world_rules": [
                    "潮汐闸门每次改写水位都会消耗岛屿地下淡水储备。",
                    "规则残片"
                ]
            },
            "issues": [
                "ContractBlocker: 小说合同世界规则[1]不像可执行规则、代价或限制，疑似截断主线或角色锚点"
            ]
        });

        assert!(
            pending_contract_candidate_quality_rank(&partial)
                > pending_contract_candidate_quality_rank(&missing)
        );
    }

    #[test]
    fn newly_filled_plot_is_not_discarded_when_it_exposes_latent_volume_references() {
        let missing_plot = serde_json::json!({
            "normalized": {
                "genre": "言情",
                "target_units": 100000,
                "chapter_unit_target": 2500,
                "characters": [{
                    "canonical_name": "顾维舟",
                    "role": "女主",
                    "planned_entry": "第一卷",
                    "planned_exit": "第四十卷"
                }],
                "outline": {"volumes": [], "near_chapters": []}
            },
            "issues": [
                "ContractBlocker: 小说合同缺少分卷/阶段安排或近期章节包",
                "ContractBlocker: 小说合同缺少分卷/阶段安排或近期章节包，不能进入写作确认",
                "小说合同尚未形成逐章规划或分卷/阶段大纲"
            ]
        });
        let filled_plot_with_exposed_reference = serde_json::json!({
            "normalized": {
                "genre": "言情",
                "target_units": 100000,
                "chapter_unit_target": 2500,
                "characters": [{
                    "canonical_name": "顾维舟",
                    "role": "女主",
                    "planned_entry": "第一卷",
                    "planned_exit": "第四十卷"
                }],
                "outline": {
                    "volumes": [
                        {"title": "旧信重启", "objective": "顾维舟修复第一封旧信", "ending_change": "顾维舟确认书局收购另有隐情"},
                        {"title": "共同守护", "objective": "顾维舟与祝屿川建立同盟", "ending_change": "两人取得保存书局的关键证据"}
                    ],
                    "near_chapters": [
                        {"number": 1, "goal": "顾维舟接下旧信修复委托", "expected_turn": "委托信上出现祝家的旧印"},
                        {"number": 2, "goal": "顾维舟核对旧印来历", "expected_turn": "祝屿川带着收购文件来到书局"},
                        {"number": 3, "goal": "顾维舟拒绝立即签署收购文件", "expected_turn": "祝屿川同意暂缓签约并共同核验旧信"}
                    ]
                }
            },
            "issues": [
                "ContractBlocker: 角色 `顾维舟` 的计划离场锚点引用第40卷，但合同只有2卷；必须按实际分卷重写，不能把预计章节数当成卷数"
            ]
        });

        assert!(
            pending_contract_candidate_quality_rank(&filled_plot_with_exposed_reference)
                > pending_contract_candidate_quality_rank(&missing_plot)
        );
    }

    #[test]
    fn newly_filled_plot_is_not_discarded_when_its_story_fields_still_need_repair() {
        let missing_plot = serde_json::json!({
            "normalized": {
                "genre": "都市",
                "target_units": 100000,
                "chapter_unit_target": 2500,
                "outline": {"volumes": [], "near_chapters": []}
            },
            "issues": [
                "ContractBlocker: 小说合同缺少分卷/阶段安排或近期章节包",
                "小说合同尚未形成逐章规划或分卷/阶段大纲"
            ]
        });
        let filled_plot_with_repairable_story_fields = serde_json::json!({
            "normalized": {
                "genre": "都市",
                "target_units": 100000,
                "chapter_unit_target": 2500,
                "outline": {
                    "volumes": [
                        {"title": "卷一：起步", "objective": "主角接手工厂并恢复生产", "ending_change": "首批订单完成交付"},
                        {"title": "卷二：扩张", "objective": "主角建立自主品牌", "ending_change": "竞争对手公开发起价格战"}
                    ],
                    "near_chapters": [
                        {"number": 1, "goal": "主角核对工厂债务", "expected_turn": "发现原料账目被人为改写"},
                        {"number": 2, "goal": "主角追查异常账目", "expected_turn": "仓库管理员交出隐藏出库单"},
                        {"number": 3, "goal": "主角用出库单恢复第一条供货线", "expected_turn": "竞争对手开始截断运输渠道"}
                    ]
                }
            },
            "issues": [
                "ContractBlocker: 小说合同分卷规划含有结构污染或无效卷名",
                "ContractBlocker: 小说合同近期章节包含有结构污染或占位目标"
            ]
        });

        assert!(
            pending_contract_candidate_quality_rank(&filled_plot_with_repairable_story_fields)
                > pending_contract_candidate_quality_rank(&missing_plot)
        );
    }

    #[test]
    fn candidate_with_fewer_contract_blockers_wins_before_raw_field_density() {
        let dense_but_blocked = serde_json::json!({
            "normalized": {
                "title": {"canonical_title": "边城账册", "rationale": "账册连接案件与旧怨"},
                "genre": "古代言情",
                "target_units": 100000,
                "chapter_unit_target": 2500,
                "characters": [
                    {
                        "canonical_name": "阮承弦",
                        "role": "女主",
                        "desire": "查清旧战役真相",
                        "fear": "边城百姓被案件牵连",
                        "bottom_line": "绝不牺牲无辜者",
                        "arc_start": "谨慎自保",
                        "arc_end": "主动承担共同选择的代价"
                    }
                ]
            },
            "issues": [
                "ContractBlocker: 小说合同缺少世界规则",
                "ContractBlocker: 小说合同缺少叙事风格",
                "ContractBlocker: 小说合同缺少必须避免",
                "ContractBlocker: 小说合同缺少核心主题"
            ]
        });
        let sparse_but_repaired = serde_json::json!({
            "normalized": {
                "title": {"canonical_title": "边城账册"},
                "genre": "古代言情",
                "target_units": 100000,
                "chapter_unit_target": 2500,
                "world_rules": [
                    "边贸货物必须经驿站、军仓和州府三方留档，任一缺页都会触发停运复核",
                    "军粮调拨依赖双印封签，伪造其中一印会暴露对应经手人的权限来源",
                    "边城冬季封路七日，错过补给窗口会直接造成驻军断粮"
                ],
                "themes": ["共同选择只有在共同承担代价时才能转化为信任"],
                "must_avoid": ["不得用替身误会或强制关系替代案件证据与人物选择"]
            },
            "issues": [
                "ContractBlocker: 小说合同缺少叙事风格"
            ]
        });

        assert!(
            pending_contract_candidate_quality_rank(&sparse_but_repaired)
                > pending_contract_candidate_quality_rank(&dense_but_blocked),
            "a candidate with fewer actionable blockers must not be discarded merely because an older candidate has denser unrelated fields"
        );
    }

    #[test]
    fn character_prerequisite_patch_accumulates_before_outline_exists() {
        let mut draft = build_initial_creation_draft(
            "pending-character-before-outline",
            "fiction",
            "写一部都市言情小说，每章2500字，一共10万字。",
        )
        .expect("draft");
        let skeleton = r#"{
            "patch_type":"skeleton_patch",
            "title":{"canonical_title":"玻璃幕墙后的雨","rationale":"来自雨夜建筑项目与终局公开选择"},
            "genre":"都市言情",
            "brief":"失忆科技新贵与建筑设计师在共同项目中重新建立信任",
            "target_units":100000,
            "chapter_unit_target":2500,
            "max_chapters_per_turn":1,
            "premise":"男主林深因实验事故失忆，女主苏念为保护事务所与他签订契约。",
            "ending":{"desired_resolution":"两人公开真相并共同保住事务所","final_state":"关系与事业都完成重建"},
            "protagonist_arc":"从拒绝信任到共同承担选择代价",
            "world_imagery":"玻璃幕墙、雨夜霓虹、旧唱片店",
            "main_causal_spine":"失忆触发合作，项目危机揭开真相，终局共同承担代价"
        }"#;
        submit_generated_contract_candidate_to_draft(&mut draft, skeleton);

        let characters = r#"{
            "patch_type":"character_patch",
            "characters":[
                {"canonical_name":"模型男主","role":"男主","desire":"查明失忆真相","fear":"再次失去选择能力","bottom_line":"不伪造记忆证据","arc_start":"拒绝依赖他人","arc_end":"主动承担共同选择","planned_entry":"第1卷进入主线","planned_exit":"持续至第4卷终局"},
                {"canonical_name":"模型女主","role":"女主","desire":"保住建筑事务所","fear":"家族秘密毁掉团队","bottom_line":"不牺牲员工掩盖秘密","arc_start":"独自承担压力","arc_end":"愿意公开真相并接受支持","planned_entry":"第1卷进入主线","planned_exit":"持续至第4卷终局"},
                {"canonical_name":"模型对手","role":"关键对手","desire":"控制项目决策权","fear":"旧账被公开","bottom_line":"不让出核心项目","arc_start":"掌握资源优势","arc_end":"失去项目控制权","planned_entry":"第1卷施压","planned_exit":"第4卷接受调查"}
            ]
        }"#;
        submit_generated_contract_candidate_to_draft(&mut draft, characters);

        let contract = pending_normalized_contract(&draft).expect("pending contract");
        assert_eq!(contract.characters.len(), 3, "{contract:#?}");
        assert!(!contract.premise.contains("林深"), "{}", contract.premise);
        assert!(!contract.premise.contains("苏念"), "{}", contract.premise);
        assert!(contract.characters.iter().all(
            |character| character.planned_entry.is_empty() && character.planned_exit.is_empty()
        ));
    }
}
