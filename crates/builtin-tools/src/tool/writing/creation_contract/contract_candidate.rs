use super::*;

mod field_pack;
mod metadata_repair;
mod pending;

pub(crate) use field_pack::*;
pub(crate) use metadata_repair::*;
pub(crate) use pending::*;

pub fn submit_generated_contract_candidate_to_draft(
    draft: &mut SessionCreationDraftState,
    raw_contract_text: &str,
) -> ContractSubmissionOutcome {
    submit_generated_contract_candidate_with_policy(draft, raw_contract_text, false, false)
}

pub(crate) fn submit_character_role_authority_repair_candidate_to_draft(
    draft: &mut SessionCreationDraftState,
    raw_contract_text: &str,
) -> ContractSubmissionOutcome {
    submit_generated_contract_candidate_with_policy(draft, raw_contract_text, false, true)
}

fn submit_premerged_contract_candidate_to_draft(
    draft: &mut SessionCreationDraftState,
    raw_contract_text: &str,
) -> ContractSubmissionOutcome {
    submit_generated_contract_candidate_with_policy(draft, raw_contract_text, true, false)
}

fn submit_generated_contract_candidate_with_policy(
    draft: &mut SessionCreationDraftState,
    raw_contract_text: &str,
    allow_existing_contract_full_merge: bool,
    allow_character_role_authority_repair: bool,
) -> ContractSubmissionOutcome {
    if !draft.can_accept_contract_candidate() {
        return ContractSubmissionOutcome {
            gate: if draft.lifecycle_status() == CreationDraftLifecycleStatus::ContractReady {
                ContractGateResult::ready()
            } else {
                ContractGateResult {
                    status: ContractGateStatus::NeedsRepair,
                    blocking_issues: Vec::new(),
                    repairable_issues: vec![
                        "当前草案状态不接受后台合同候选；请先按用户指令修改草案或重新进入合同阶段"
                            .to_string(),
                    ],
                    warnings: Vec::new(),
                }
            },
            committed: false,
        };
    }

    let sanitized = sanitize_generated_contract_surface(draft, raw_contract_text);
    if let Some(outcome) = contract_boundary_rejection_outcome(draft, &sanitized) {
        return outcome;
    }
    let explicit_patch = (contract_text_looks_like_explicit_patch(raw_contract_text)
        || contract_text_looks_like_explicit_patch(&sanitized))
    .then(|| {
        normalize_creation_contract_patch_boundary(draft, raw_contract_text)
            .or_else(|| normalize_creation_contract_patch_boundary(draft, &sanitized))
    })
    .flatten();
    let normalized =
        creation_contract_normalizer::normalize_creation_contract_boundary(raw_contract_text)
            .or_else(|| {
                creation_contract_normalizer::normalize_creation_contract_boundary(&sanitized)
            });
    let inferred_partial_patch = explicit_patch.is_none()
        && normalized
            .as_ref()
            .map(|normalized| contract_value_is_single_scope_partial(&normalized.value))
            .unwrap_or(false);
    let inferred_patch = inferred_partial_patch
        .then(|| {
            normalize_creation_contract_patch_boundary(draft, raw_contract_text)
                .or_else(|| normalize_creation_contract_patch_boundary(draft, &sanitized))
        })
        .flatten();
    let (contract, _, _) = if let Some(patch) = explicit_patch.or(inferred_patch) {
        if draft.current_contract.is_some()
            && !allow_existing_contract_full_merge
            && patch.is_multi_scope_batch()
        {
            return existing_contract_replacement_rejection_outcome(
                draft,
                &sanitized,
                "已有合同的修复输出只能修改当前质量问题所属的单一 typed scope；多作用域 contract_batch 不能绕过完整合同覆盖保护，如需更换整个故事必须先由用户明确重置故事合同",
            );
        }
        return submit_creation_contract_patch_candidate(
            draft,
            &sanitized,
            patch,
            allow_character_role_authority_repair,
        );
    } else if let Some(normalized) = normalized {
        if draft.current_contract.is_some() && !allow_existing_contract_full_merge {
            return existing_contract_replacement_rejection_outcome(
                draft,
                &sanitized,
                "已有合同的修复输出必须是限定作用域的 typed patch；完整合同候选不能覆盖已锁定的故事权威，如需更换整个故事必须先由用户明确重置故事合同",
            );
        }
        let Some(contract) = NovelCreationContract::parse_json_boundary(&normalized.json) else {
            return contract_candidate_schema_repair_outcome(
                draft,
                &sanitized,
                Some(normalized.value),
            );
        };
        (contract, normalized.json, normalized.value)
    } else if let Some(contract) = novel_creation_contract_from_field_pack(draft, &sanitized) {
        if draft.current_contract.is_some() && !allow_existing_contract_full_merge {
            return existing_contract_replacement_rejection_outcome(
                draft,
                &sanitized,
                "已有合同的修复输出必须是限定作用域的 typed patch；完整合同候选不能覆盖已锁定的故事权威，如需更换整个故事必须先由用户明确重置故事合同",
            );
        }
        let value = serde_json::to_value(&contract).unwrap_or_else(|_| serde_json::json!({}));
        let text = serde_json::to_string(&contract).unwrap_or_default();
        (contract, text, value)
    } else if let Some(patch) = normalize_creation_contract_patch_boundary(draft, raw_contract_text)
        .or_else(|| normalize_creation_contract_patch_boundary(draft, &sanitized))
    {
        return submit_creation_contract_patch_candidate(
            draft,
            &sanitized,
            patch,
            allow_character_role_authority_repair,
        );
    } else {
        let issue = "合同输出不能解析为 JSON，也没有形成可归位的合同字段包".to_string();
        record_contract_repair_candidate(draft, &sanitized, None, &[issue.clone()]);
        draft.set_lifecycle_status(CreationDraftLifecycleStatus::DraftingContract);
        draft.updated_at = chrono::Utc::now().to_rfc3339();
        return ContractSubmissionOutcome {
            gate: ContractGateResult {
                status: ContractGateStatus::NeedsRepair,
                blocking_issues: Vec::new(),
                repairable_issues: vec![issue],
                warnings: Vec::new(),
            },
            committed: false,
        };
    };
    let normalized_text = serde_json::to_string(&contract).unwrap_or_default();
    let normalized_value =
        serde_json::to_value(&contract).unwrap_or_else(|_| serde_json::json!({}));

    submit_novel_creation_contract_candidate(
        draft,
        &sanitized,
        contract,
        normalized_text,
        normalized_value,
    )
}

fn existing_contract_replacement_rejection_outcome(
    draft: &mut SessionCreationDraftState,
    sanitized: &str,
    issue: &str,
) -> ContractSubmissionOutcome {
    let issue = issue.to_string();
    record_contract_repair_candidate(draft, sanitized, None, &[issue.clone()]);
    draft.set_lifecycle_status(CreationDraftLifecycleStatus::DraftingContract);
    draft.updated_at = chrono::Utc::now().to_rfc3339();
    ContractSubmissionOutcome {
        gate: ContractGateResult {
            status: ContractGateStatus::NeedsRepair,
            blocking_issues: Vec::new(),
            repairable_issues: vec![issue],
            warnings: Vec::new(),
        },
        committed: false,
    }
}

fn contract_value_is_single_scope_partial(value: &serde_json::Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    let has_any = |keys: &[&str]| keys.iter().any(|key| object.contains_key(*key));
    let scopes = [
        has_any(&["title"]),
        has_any(&[
            "language",
            "genre",
            "brief",
            "target_units",
            "chapter_unit_target",
            "max_chapters_per_turn",
            "premise",
            "ending",
            "protagonist_arc",
            "world_imagery",
            "main_causal_spine",
        ]),
        has_any(&[
            "characters",
            "character_authority",
            "character_ledger",
            "relationship_ledger",
            "emotional_state_ledger",
        ]),
        has_any(&[
            "outline",
            "volumes",
            "volume_arcs",
            "near_chapters",
            "chapter_plan",
            "chapters",
            "raw_outline",
            "payoff_matrix",
        ]),
        has_any(&[
            "themes",
            "world_rules",
            "style_rules",
            "must_avoid",
            "structured",
            "emotional_contract",
            "resource_economy",
            "power_progression",
            "social_order",
            "geography_model",
            "time_model",
            "artifact_ledger",
            "antagonist_pressure",
            "narration_contract",
            "scene_type_mix",
            "character_voice_ledger",
            "reader_promise",
            "chapter_ending_rotation",
            "conflict_pressure_curve",
            "motif_ledger",
            "reveal_schedule",
            "relationship_interaction_quotas",
        ]),
    ];
    scopes.into_iter().filter(|present| *present).count() == 1
}

fn normalize_candidate_creative_surface(contract: &mut NovelCreationContract) {
    if value_missing(&contract.title.canonical_title) {
        let inferred = super::patch_normalizer::infer_book_title_from_rationale_text(
            &contract.title.rationale,
        );
        if !value_missing(&inferred) {
            contract.title.canonical_title = inferred.clone();
            if contract.title.candidates.is_empty() {
                contract.title.candidates.push(inferred);
            }
        }
    }
    contract.brief = sanitize_creation_brief_value(&contract.brief);
    contract.premise = sanitize_creation_brief_value(&contract.premise);
    contract.outline.raw_outline =
        strip_contract_section_heading_residue(&contract.outline.raw_outline);
    for volume in &mut contract.outline.volumes {
        super::patch_normalizer::normalize_volume_contract_surface(volume);
    }
    for chapter in &mut contract.outline.near_chapters {
        chapter.goal = strip_contract_section_heading_residue(&chapter.goal);
        chapter.expected_turn = strip_contract_section_heading_residue(&chapter.expected_turn);
    }
    for entry in &mut contract.structured.payoff_matrix {
        entry.promise = strip_contract_section_heading_residue(&entry.promise);
        entry.payoff_target = strip_contract_section_heading_residue(&entry.payoff_target);
    }
}

#[cfg(test)]
mod creative_surface_tests {
    use super::*;

    #[test]
    fn unified_candidate_normalization_removes_section_residue_from_all_plot_fields() {
        let mut contract = NovelCreationContract::default();
        contract.outline.raw_outline = "主角公开监视名单。分卷规划".to_string();
        contract.outline.volumes = vec![VolumeContract {
            title: "第2卷《余烬》".to_string(),
            objective: "本卷目标：主角取得爆炸证据".to_string(),
            ending_change: "监视名单被公开。近期章节包".to_string(),
        }];
        contract.outline.near_chapters = vec![ChapterSeedContract {
            number: Some(1),
            goal: "主角核对名单。章节规划".to_string(),
            expected_turn: "名单指向旧哨塔。伏笔矩阵".to_string(),
        }];
        contract.structured.payoff_matrix = vec![PayoffMatrixEntry {
            promise: "旧哨塔藏有原始名单。伏笔矩阵".to_string(),
            payoff_target: "主角在终局公开原始名单。质量合同".to_string(),
            ..Default::default()
        }];

        normalize_candidate_creative_surface(&mut contract);

        assert_eq!(contract.outline.raw_outline, "主角公开监视名单。");
        assert_eq!(contract.outline.volumes[0].title, "余烬");
        assert_eq!(contract.outline.volumes[0].objective, "主角取得爆炸证据");
        assert_eq!(
            contract.outline.volumes[0].ending_change,
            "监视名单被公开。"
        );
        assert_eq!(contract.outline.near_chapters[0].goal, "主角核对名单。");
        assert_eq!(
            contract.outline.near_chapters[0].expected_turn,
            "名单指向旧哨塔。"
        );
        assert_eq!(
            contract.structured.payoff_matrix[0].promise,
            "旧哨塔藏有原始名单。"
        );
        assert_eq!(
            contract.structured.payoff_matrix[0].payoff_target,
            "主角在终局公开原始名单。"
        );
    }

    #[test]
    fn unified_candidate_normalization_reuses_existing_title_rationale_inference() {
        let mut contract = NovelCreationContract::default();
        contract.title.rationale =
            "最终书名“晋升陷阱”来自主角登顶后成为新规则守门人的终局变化".to_string();

        normalize_candidate_creative_surface(&mut contract);

        assert_eq!(contract.title.canonical_title, "晋升陷阱");
        assert_eq!(contract.title.candidates, vec!["晋升陷阱"]);
    }
}

fn contract_text_looks_like_explicit_patch(text: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    lowered.contains("patch_type")
        || lowered.contains("patchtype")
        || lowered.contains("patch type")
        || lowered.contains("title_patch")
        || lowered.contains("skeleton_patch")
        || lowered.contains("character_patch")
        || lowered.contains("characters_patch")
        || lowered.contains("plot_patch")
        || lowered.contains("governance_patch")
        || lowered.contains("metadata_patch")
}

fn submit_creation_contract_patch_candidate(
    draft: &mut SessionCreationDraftState,
    sanitized: &str,
    patch: CreationContractPatch,
    allow_character_role_authority_repair: bool,
) -> ContractSubmissionOutcome {
    if let Some(outcome) = contract_boundary_rejection_outcome(draft, sanitized) {
        return outcome;
    }
    if let CreationContractPatch::Batch(items) = patch {
        return submit_creation_contract_patch_batch_candidate(
            draft,
            sanitized,
            items,
            allow_character_role_authority_repair,
        );
    }

    let patch_type = patch.patch_type();
    let (mut candidate_draft, mut contract) =
        creation_draft_and_contract_with_pending_applied(draft);
    let scope_report = patch.validate_scope(&candidate_draft);
    if !scope_report.ready() {
        let mut issues = vec![format!("typed patch 作用域校验未通过：{patch_type:?}")];
        issues.extend(scope_report.issues);
        record_contract_repair_candidate(draft, sanitized, None, &issues);
        draft.set_lifecycle_status(CreationDraftLifecycleStatus::DraftingContract);
        draft.updated_at = chrono::Utc::now().to_rfc3339();
        return ContractSubmissionOutcome {
            gate: ContractGateResult {
                status: ContractGateStatus::NeedsRepair,
                blocking_issues: Vec::new(),
                repairable_issues: issues,
                warnings: Vec::new(),
            },
            committed: false,
        };
    }

    if matches!(patch, CreationContractPatch::Title(_)) {
        return submit_title_only_contract_patch_candidate(draft, &sanitized, patch);
    }

    patch.apply_to_draft_with_role_repair_policy(
        &mut candidate_draft,
        allow_character_role_authority_repair,
    );
    normalize_fiction_creation_draft_after_contract_change(&mut candidate_draft);
    sanitize_creation_draft_control_noise(&mut candidate_draft);
    let applied_contract =
        super::strong_novel_contract_from_visible_creation_draft(&candidate_draft);
    patch.merge_applied_scope_into_contract_with_role_repair_policy(
        &mut contract,
        &applied_contract,
        allow_character_role_authority_repair,
    );
    contract.normalize();
    let normalized_value =
        serde_json::to_value(&contract).unwrap_or_else(|_| serde_json::json!({}));
    let normalized_text = serde_json::to_string(&contract).unwrap_or_default();
    let outcome = submit_novel_creation_contract_candidate_from_preapplied_draft(
        draft,
        candidate_draft,
        sanitized,
        contract,
        normalized_text,
        normalized_value,
    );
    if outcome.committed {
        clear_applied_explicit_contract_revisions(draft, patch_type);
    }
    outcome
}

fn submit_title_only_contract_patch_candidate(
    draft: &mut SessionCreationDraftState,
    sanitized: &str,
    patch: CreationContractPatch,
) -> ContractSubmissionOutcome {
    let (mut candidate_draft, mut contract) =
        creation_draft_and_contract_with_pending_applied(draft);
    patch.apply_title_repair_to_draft(&mut candidate_draft);
    normalize_fiction_creation_draft_after_contract_change(&mut candidate_draft);
    sanitize_creation_draft_control_noise(&mut candidate_draft);

    let applied_contract =
        super::strong_novel_contract_from_visible_creation_draft(&candidate_draft);
    patch.merge_applied_scope_into_contract(&mut contract, &applied_contract);
    contract.normalize();
    let canonical_contract_text = serde_json::to_string(&contract).unwrap_or_default();
    let canonical_contract_value =
        serde_json::to_value(&contract).unwrap_or_else(|_| serde_json::json!({}));

    let readiness_issues =
        contract_readiness_issues_for_candidate_draft(&candidate_draft, &canonical_contract_text);
    if !readiness_issues.is_empty() {
        let gate = contract_gate_from_issues(draft, &canonical_contract_text, readiness_issues);
        let actionable = gate.actionable_issues();
        record_contract_repair_candidate(
            draft,
            sanitized,
            Some(canonical_contract_value),
            &actionable,
        );
        draft.set_lifecycle_status(CreationDraftLifecycleStatus::DraftingContract);
        draft.updated_at = chrono::Utc::now().to_rfc3339();
        return ContractSubmissionOutcome {
            gate,
            committed: false,
        };
    }

    commit_ready_contract_draft(draft, candidate_draft, canonical_contract_value)
}

fn submit_creation_contract_patch_batch_candidate(
    draft: &mut SessionCreationDraftState,
    sanitized: &str,
    items: Vec<CreationContractPatch>,
    allow_character_role_authority_repair: bool,
) -> ContractSubmissionOutcome {
    let (mut candidate_draft, mut contract) =
        creation_draft_and_contract_with_pending_applied(draft);
    let mut valid_items = Vec::new();
    let mut invalid_issues = Vec::new();

    for item in items {
        let patch_type = item.patch_type();
        let scope_report = item.validate_scope(&candidate_draft);
        if scope_report.ready() {
            valid_items.push(item);
        } else {
            invalid_issues.push(format!("typed patch 作用域校验未通过：{patch_type:?}"));
            invalid_issues.extend(scope_report.issues);
        }
    }

    if valid_items.is_empty() {
        let issues = if invalid_issues.is_empty() {
            vec!["合同补丁批次没有形成可审查字段".to_string()]
        } else {
            invalid_issues
        };
        record_contract_repair_candidate(draft, sanitized, None, &issues);
        draft.set_lifecycle_status(CreationDraftLifecycleStatus::DraftingContract);
        draft.updated_at = chrono::Utc::now().to_rfc3339();
        return ContractSubmissionOutcome {
            gate: ContractGateResult {
                status: ContractGateStatus::NeedsRepair,
                blocking_issues: Vec::new(),
                repairable_issues: issues,
                warnings: Vec::new(),
            },
            committed: false,
        };
    }

    for item in &valid_items {
        item.apply_to_draft_with_role_repair_policy(
            &mut candidate_draft,
            allow_character_role_authority_repair,
        );
    }
    normalize_fiction_creation_draft_after_contract_change(&mut candidate_draft);
    sanitize_creation_draft_control_noise(&mut candidate_draft);
    let mut applied_patch_types = Vec::new();
    for patch_type in valid_items.iter().map(CreationContractPatch::patch_type) {
        if !applied_patch_types.contains(&patch_type) {
            applied_patch_types.push(patch_type);
        }
    }
    let applied_contract =
        super::strong_novel_contract_from_visible_creation_draft(&candidate_draft);
    for item in &valid_items {
        item.merge_applied_scope_into_contract_with_role_repair_policy(
            &mut contract,
            &applied_contract,
            allow_character_role_authority_repair,
        );
    }
    contract.normalize();
    let normalized_value =
        serde_json::to_value(&contract).unwrap_or_else(|_| serde_json::json!({}));
    let normalized_text = serde_json::to_string(&contract).unwrap_or_default();

    let mut outcome = submit_novel_creation_contract_candidate_from_preapplied_draft(
        draft,
        candidate_draft,
        sanitized,
        contract,
        normalized_text,
        normalized_value,
    );
    if outcome.committed {
        for patch_type in applied_patch_types {
            clear_applied_explicit_contract_revisions(draft, patch_type);
        }
    }
    if !outcome.is_ready() && !invalid_issues.is_empty() && !outcome.committed {
        outcome.gate.repairable_issues.extend(invalid_issues);
        outcome.gate.repairable_issues.sort();
        outcome.gate.repairable_issues.dedup();
    }
    outcome
}

fn contract_candidate_schema_repair_outcome(
    draft: &mut SessionCreationDraftState,
    sanitized: &str,
    normalized_value: Option<serde_json::Value>,
) -> ContractSubmissionOutcome {
    let issue = "合同 JSON 结构不符合写作合同 schema，不能写入可确认草案".to_string();
    record_contract_repair_candidate(draft, &sanitized, normalized_value, &[issue.clone()]);
    draft.set_lifecycle_status(CreationDraftLifecycleStatus::DraftingContract);
    draft.updated_at = chrono::Utc::now().to_rfc3339();
    ContractSubmissionOutcome {
        gate: ContractGateResult {
            status: ContractGateStatus::NeedsRepair,
            blocking_issues: Vec::new(),
            repairable_issues: vec![issue],
            warnings: Vec::new(),
        },
        committed: false,
    }
}

fn contract_boundary_rejection_outcome(
    draft: &mut SessionCreationDraftState,
    candidate_text: &str,
) -> Option<ContractSubmissionOutcome> {
    let issues = contract_boundary_quality_issues(draft, candidate_text);
    if issues.is_empty() {
        return None;
    }
    record_contract_repair_candidate(draft, candidate_text, None, &issues);
    draft.set_lifecycle_status(CreationDraftLifecycleStatus::DraftingContract);
    draft.updated_at = chrono::Utc::now().to_rfc3339();
    Some(ContractSubmissionOutcome {
        gate: ContractGateResult {
            status: ContractGateStatus::NeedsRepair,
            blocking_issues: Vec::new(),
            repairable_issues: issues,
            warnings: Vec::new(),
        },
        committed: false,
    })
}

fn submit_novel_creation_contract_candidate(
    draft: &mut SessionCreationDraftState,
    sanitized: &str,
    contract: NovelCreationContract,
    normalized_text: String,
    normalized_value: serde_json::Value,
) -> ContractSubmissionOutcome {
    submit_novel_creation_contract_candidate_from_preapplied_draft(
        draft,
        draft.clone(),
        sanitized,
        contract,
        normalized_text,
        normalized_value,
    )
}

fn submit_novel_creation_contract_candidate_from_preapplied_draft(
    draft: &mut SessionCreationDraftState,
    mut candidate_draft: SessionCreationDraftState,
    sanitized: &str,
    mut contract: NovelCreationContract,
    normalized_text: String,
    normalized_value: serde_json::Value,
) -> ContractSubmissionOutcome {
    normalize_candidate_creative_surface(&mut contract);
    contract.normalize();
    let normalized_contract_text =
        serde_json::to_string(&contract).unwrap_or_else(|_| normalized_text.clone());
    let boundary_issues = contract_boundary_quality_issues(draft, &normalized_contract_text);
    if !boundary_issues.is_empty() {
        let boundary_pending_value =
            serde_json::to_value(&contract).unwrap_or_else(|_| normalized_value.clone());
        record_contract_repair_candidate(
            draft,
            sanitized,
            Some(boundary_pending_value),
            &boundary_issues,
        );
        draft.set_lifecycle_status(CreationDraftLifecycleStatus::DraftingContract);
        draft.updated_at = chrono::Utc::now().to_rfc3339();
        return ContractSubmissionOutcome {
            gate: ContractGateResult {
                status: ContractGateStatus::NeedsRepair,
                blocking_issues: Vec::new(),
                repairable_issues: boundary_issues,
                warnings: Vec::new(),
            },
            committed: false,
        };
    }

    let mut candidate_contract = contract.clone();
    apply_strong_novel_contract_to_creation_draft(&mut candidate_draft, &mut candidate_contract);
    candidate_draft.pending_contract_candidate = None;
    normalize_fiction_creation_draft_after_contract_change(&mut candidate_draft);
    sanitize_creation_draft_control_noise(&mut candidate_draft);
    let mut canonical_contract = candidate_contract;
    canonical_contract.normalize();
    if candidate_draft.fiction_world_rules.is_empty() && !canonical_contract.world_rules.is_empty()
    {
        candidate_draft.fiction_world_rules = canonical_contract.world_rules.clone();
    }
    let canonical_contract_text =
        serde_json::to_string(&canonical_contract).unwrap_or_else(|_| normalized_text.clone());
    let canonical_contract_value =
        serde_json::to_value(&canonical_contract).unwrap_or_else(|_| normalized_value.clone());
    candidate_draft.current_contract = Some(canonical_contract_value.clone());
    let readiness_issues =
        contract_readiness_issues_for_candidate_draft(&candidate_draft, &canonical_contract_text);
    if !readiness_issues.is_empty() {
        let gate = contract_gate_from_issues(draft, &normalized_text, readiness_issues);
        let actionable = gate.actionable_issues();
        let pending_value = canonical_contract_value;
        record_contract_repair_candidate(draft, &sanitized, Some(pending_value), &actionable);
        draft.set_lifecycle_status(CreationDraftLifecycleStatus::DraftingContract);
        draft.updated_at = chrono::Utc::now().to_rfc3339();
        return ContractSubmissionOutcome {
            gate,
            committed: false,
        };
    }

    commit_ready_contract_draft(draft, candidate_draft, canonical_contract_value)
}

fn commit_ready_contract_draft(
    draft: &mut SessionCreationDraftState,
    mut candidate_draft: SessionCreationDraftState,
    canonical_contract_value: serde_json::Value,
) -> ContractSubmissionOutcome {
    let before = serde_json::to_value(&*draft).ok();
    if let Some(contract) =
        NovelCreationContract::parse_json_boundary(&canonical_contract_value.to_string())
    {
        candidate_draft.fiction_outline = super::strong_contract_outline_summary_text(&contract);
    }
    candidate_draft.current_contract = Some(canonical_contract_value);
    candidate_draft.pending_contract_candidate = None;
    clear_contract_quality_blocker_diagnostic(&mut candidate_draft);
    candidate_draft.set_lifecycle_status(CreationDraftLifecycleStatus::ContractReady);
    candidate_draft.updated_at = chrono::Utc::now().to_rfc3339();
    *draft = candidate_draft;
    let changed = before != serde_json::to_value(&*draft).ok();
    ContractSubmissionOutcome {
        gate: ContractGateResult::ready(),
        committed: changed || draft.current_contract.is_some(),
    }
}

fn record_contract_repair_candidate(
    draft: &mut SessionCreationDraftState,
    raw_contract_text: &str,
    normalized_value: Option<serde_json::Value>,
    issues: &[String],
) {
    record_pending_contract_candidate(draft, raw_contract_text, normalized_value, issues);
}

fn contract_readiness_issues_for_candidate_draft(
    draft: &SessionCreationDraftState,
    canonical_contract_text: &str,
) -> Vec<String> {
    let mut issues = creation_draft_contract_blocking_issues_for_scope(
        draft,
        ContractReadinessScope::LockedAuthorityContract,
    );
    issues.extend(contract_boundary_quality_issues(
        draft,
        canonical_contract_text,
    ));
    issues.extend(forbidden_naming_contract_issues(
        draft,
        canonical_contract_text,
    ));
    issues.sort();
    issues.dedup();
    issues
}

fn forbidden_naming_contract_issues(
    draft: &SessionCreationDraftState,
    canonical_contract_text: &str,
) -> Vec<String> {
    let forbidden = super::forbidden_naming_authority(draft);
    if forbidden.titles.is_empty() && forbidden.character_names.is_empty() {
        return Vec::new();
    }
    let Some(contract) = NovelCreationContract::parse_json_boundary(canonical_contract_text) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::to_value(&contract) else {
        return Vec::new();
    };
    let Some(fields) = value.as_object() else {
        return Vec::new();
    };
    let mut issues = Vec::new();
    let canonical_title = contract.title.canonical_title.trim();
    for title in forbidden.titles {
        if !canonical_title.is_empty()
            && canonical_title.replace(char::is_whitespace, "")
                == title.trim().replace(char::is_whitespace, "")
        {
            issues.push(format!(
                "ContractBlocker: 小说合同书名仍复用用户明确禁用书名 `{title}`"
            ));
        }
    }
    for name in forbidden.character_names {
        for (label, keys) in [
            (
                "故事前提、终局方向等故事骨架",
                &[
                    "brief",
                    "premise",
                    "ending",
                    "protagonist_arc",
                    "world_imagery",
                    "main_causal_spine",
                ][..],
            ),
            ("角色权威表", &["characters"][..]),
            ("大纲、分卷或近期章节", &["outline"][..]),
            (
                "结构化治理字段",
                &[
                    "themes",
                    "world_rules",
                    "style_rules",
                    "must_avoid",
                    "structured",
                ][..],
            ),
        ] {
            if keys.iter().any(|key| {
                fields.get(*key).is_some_and(|field| {
                    json_story_surface_contains_forbidden_name(field, &name, Some(key))
                })
            }) {
                issues.push(format!(
                    "ContractBlocker: 小说合同{label}仍复用用户明确禁用命名 `{name}`"
                ));
            }
        }
    }
    issues
}

fn json_story_surface_contains_forbidden_name(
    value: &serde_json::Value,
    name: &str,
    field_name: Option<&str>,
) -> bool {
    match value {
        serde_json::Value::String(text) => {
            !matches!(
                field_name,
                Some("previous_names" | "previous names" | "历史姓名" | "旧名")
            ) && text.contains(name)
        }
        serde_json::Value::Array(items) => items
            .iter()
            .any(|item| json_story_surface_contains_forbidden_name(item, name, field_name)),
        serde_json::Value::Object(fields) => fields
            .iter()
            .any(|(key, item)| json_story_surface_contains_forbidden_name(item, name, Some(key))),
        _ => false,
    }
}

#[cfg(test)]
mod patch_base_selection_tests {
    use super::*;

    fn volume(number: usize) -> VolumeContract {
        VolumeContract {
            title: format!("第{number}卷"),
            objective: format!("主角完成第{number}阶段的航路调查并取得新的证据"),
            ending_change: format!("第{number}阶段的结果不可逆地改变后续航路"),
        }
    }

    fn character(name: &str, role: &str) -> CharacterContract {
        CharacterContract {
            canonical_name: name.to_string(),
            role: role.to_string(),
            desire: "查明移动群岛的航路规律".to_string(),
            fear: "错误航图让无辜船队覆灭".to_string(),
            bottom_line: "绝不伪造航路测量数据".to_string(),
            arc_start: "只相信静态测绘结果".to_string(),
            arc_end: "学会在变化中持续校准航路".to_string(),
            planned_entry: "第一卷".to_string(),
            planned_exit: "第四卷终局".to_string(),
            ..Default::default()
        }
    }

    fn visible_four_volume_contract() -> NovelCreationContract {
        NovelCreationContract {
            title: TitleContract {
                canonical_title: "移动群岛图".to_string(),
                rationale: "书名来自贯穿全书并在终局完成的航图".to_string(),
                ..Default::default()
            },
            language: "zh-CN".to_string(),
            genre: "异界冒险".to_string(),
            brief: "测绘员误入移动群岛并用测绘知识阻止航路战争".to_string(),
            target_units: Some(100_000),
            chapter_unit_target: Some(2_500),
            premise: "测绘员误入持续移动的群岛世界，必须重新测定航路才能生存".to_string(),
            ending: EndingContract {
                desired_resolution: "主角完成动态航图并阻止航路战争".to_string(),
                final_state: "各岛群共同维护动态航图，主角决定留下继续测绘".to_string(),
                must_resolve: vec!["移动规律与航路战争的因果必须解决".to_string()],
                ..Default::default()
            },
            protagonist_arc: "从依赖静态数据成长为能持续校准变化的测绘者".to_string(),
            world_imagery: "漂移群岛、潮汐灯塔与不断改写的航图".to_string(),
            main_causal_spine: "误入群岛->测定航路->发现战争根因->完成动态航图->阻止战争"
                .to_string(),
            themes: vec!["在变化中建立可信秩序".to_string()],
            characters: vec![
                character("岑遥", "主角"),
                character("闻舟", "同伴"),
                character("顾衡", "对手"),
            ],
            world_rules: vec!["岛屿每天按潮汐改变位置，旧航图会在日落后失效".to_string()],
            style_rules: vec!["用行动与测量结果推进冲突".to_string()],
            must_avoid: vec!["不得让未测量的航路凭空安全".to_string()],
            outline: OutlineContract {
                volumes: (1..=4).map(volume).collect(),
                near_chapters: vec![
                    ChapterSeedContract {
                        number: Some(1),
                        goal: "主角测量最初登陆点".to_string(),
                        expected_turn: "旧航图在眼前失效".to_string(),
                    },
                    ChapterSeedContract {
                        number: Some(2),
                        goal: "主角复核潮位数据".to_string(),
                        expected_turn: "确认岛屿正在移动".to_string(),
                    },
                    ChapterSeedContract {
                        number: Some(3),
                        goal: "主角绘制临时安全线".to_string(),
                        expected_turn: "第一支船队改用新航线".to_string(),
                    },
                ],
                raw_outline: "四个阶段连续推进动态航图与航路战争主线".to_string(),
            },
            ..Default::default()
        }
    }

    #[test]
    fn typed_patch_uses_the_same_best_candidate_for_draft_and_merge_base() {
        let mut draft = build_initial_creation_draft(
            "patch-base-selection",
            "fiction",
            "写一部异界冒险小说，总字数10万字，每章2500字",
        )
        .expect("draft");
        let mut visible = visible_four_volume_contract();
        apply_strong_novel_contract_to_creation_draft(&mut draft, &mut visible);

        let mut stale_pending = visible.clone();
        stale_pending.outline.volumes.truncate(3);
        stale_pending.title.canonical_title.clear();
        stale_pending.premise.clear();
        draft.pending_contract_candidate = Some(serde_json::json!({
            "normalized": stale_pending,
            "issues": [
                "ContractBlocker: 角色计划离场锚点引用第4卷，但合同只有3卷"
            ]
        }));

        let effective = creation_draft_with_pending_contract_applied(&draft);
        assert_eq!(
            strong_novel_contract_from_visible_creation_draft(&effective)
                .outline
                .volumes
                .len(),
            4,
            "the visible four-volume draft must outrank the stale pending candidate"
        );

        let patch = serde_json::json!({
            "patch_type": "skeleton_patch",
            "premise": "测绘员误入持续移动的群岛世界，必须持续校准航路才能阻止战争"
        });
        let _ = submit_generated_contract_candidate_to_draft(&mut draft, &patch.to_string());

        let merged = draft
            .pending_contract_candidate
            .as_ref()
            .and_then(|candidate| candidate.get("normalized"))
            .or(draft.current_contract.as_ref())
            .and_then(|value| NovelCreationContract::parse_json_boundary(&value.to_string()))
            .expect("merged contract");
        assert_eq!(
            merged.outline.volumes.len(),
            4,
            "the patch merge base must be the same four-volume candidate selected for the draft"
        );
    }

    #[test]
    fn full_contract_cannot_replace_existing_authority_during_scoped_repair() {
        let mut draft = build_initial_creation_draft(
            "existing-authority-rejects-full-replacement",
            "fiction",
            "写一部异界冒险小说，总字数10万字，每章2500字",
        )
        .expect("draft");
        let mut locked = visible_four_volume_contract();
        apply_strong_novel_contract_to_creation_draft(&mut draft, &mut locked);
        draft.current_contract = Some(serde_json::to_value(&locked).expect("locked contract"));
        draft.set_lifecycle_status(CreationDraftLifecycleStatus::ContractReady);
        apply_message_to_creation_draft(
            &mut draft,
            "请检查并修复当前合同，保持题材、人物和故事不变。",
        );
        let before = draft.current_contract.clone();

        let mut unrelated = visible_four_volume_contract();
        unrelated.title.canonical_title = "镜中雪原".to_string();
        unrelated.brief = "守镜人进入雪原寻找一面会吞噬记忆的古镜".to_string();
        unrelated.premise = "守镜人追踪古镜并进入不断重置记忆的雪原".to_string();
        unrelated.characters[0].canonical_name = "沈砚".to_string();
        let outcome = submit_generated_contract_candidate_to_draft(
            &mut draft,
            &serde_json::to_string(&unrelated).expect("replacement contract"),
        );

        assert!(!outcome.committed);
        assert_eq!(draft.current_contract, before);
        assert!(outcome
            .gate
            .actionable_issues()
            .iter()
            .any(|issue| issue.contains("typed patch") && issue.contains("不能覆盖")));
        assert!(draft
            .pending_contract_candidate
            .as_ref()
            .and_then(|candidate| candidate.get("normalized"))
            .is_none());
    }

    #[test]
    fn multi_scope_contract_batch_cannot_replace_existing_authority() {
        let mut draft = build_initial_creation_draft(
            "existing-authority-rejects-contract-batch",
            "fiction",
            "写一部异界冒险小说，总字数10万字，每章2500字",
        )
        .expect("draft");
        let mut locked = visible_four_volume_contract();
        apply_strong_novel_contract_to_creation_draft(&mut draft, &mut locked);
        draft.current_contract = Some(serde_json::to_value(&locked).expect("locked contract"));
        draft.set_lifecycle_status(CreationDraftLifecycleStatus::DraftingContract);
        let before = draft.current_contract.clone();
        let raw = "patch_type: contract_batch\n题材：校园悬疑\n简述：学生调查失踪案\n故事前提：学生在旧校舍发现失踪档案\n终局方向：学生公开校方隐瞒的真相\n主角弧线：从旁观到公开作证\n世界观意象：封闭旧校舍\n总主线因果链：发现档案引发追查并公开真相\n全书大纲：学生调查失踪档案。\n第1卷《旧校舍》：本卷目标：取得档案；卷尾变化：确认档案被篡改。\n第1章《封门》：本章目标：进入旧校舍；预期转折：发现失踪者笔记。";

        let outcome = submit_generated_contract_candidate_to_draft(&mut draft, raw);

        assert!(!outcome.committed);
        assert_eq!(draft.current_contract, before);
        assert!(outcome
            .gate
            .actionable_issues()
            .iter()
            .any(|issue| issue.contains("contract_batch") && issue.contains("不能绕过")));
    }
}

#[cfg(test)]
mod forbidden_naming_tests {
    use super::*;

    #[test]
    fn forbidden_incidental_name_blocks_outline_without_rejecting_history_record() {
        let mut draft = build_initial_creation_draft(
            "session-forbidden-incidental-name",
            "fiction",
            "写一部赛博朋克小说，每章2500字，共10万字",
        )
        .expect("draft");
        draft
            .planning_notes
            .push(format!("{FORBIDDEN_CHARACTER_NAMING_PREFIX}林默"));
        let contract = NovelCreationContract {
            characters: vec![CharacterContract {
                canonical_name: "顾星安".to_string(),
                role: "女主".to_string(),
                previous_names: vec!["林默".to_string()],
                ..Default::default()
            }],
            outline: OutlineContract {
                raw_outline: "顾星安追查工伤记录。".to_string(),
                near_chapters: vec![ChapterSeedContract {
                    number: Some(2),
                    goal: "顾星安前往备份所有者林默的旧居。".to_string(),
                    expected_turn: "她找到被切断的原始晶片。".to_string(),
                }],
                ..Default::default()
            },
            ..Default::default()
        };

        let issues = forbidden_naming_contract_issues(
            &draft,
            &serde_json::to_string(&contract).expect("contract"),
        );

        assert!(issues
            .iter()
            .any(|issue| issue.contains("大纲、分卷或近期章节") && issue.contains("林默")));
        assert!(issues
            .iter()
            .all(|issue| !issue.contains("历史姓名") && !issue.contains("previous_names")));
    }

    #[test]
    fn forbidden_title_is_scoped_to_the_canonical_title() {
        let mut draft = build_initial_creation_draft(
            "session-forbidden-title-scope",
            "fiction",
            "写一部都市小说，每章2500字，共10万字",
        )
        .expect("draft");
        draft
            .planning_notes
            .push("失败合同禁用书名：林默".to_string());
        let mut contract = NovelCreationContract {
            title: TitleContract {
                canonical_title: "旧城回声".to_string(),
                ..Default::default()
            },
            characters: vec![CharacterContract {
                canonical_name: "林默".to_string(),
                role: "主角".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };

        let character_only_issues = forbidden_naming_contract_issues(
            &draft,
            &serde_json::to_string(&contract).expect("contract"),
        );
        assert!(
            character_only_issues.is_empty(),
            "{character_only_issues:?}"
        );

        contract.title.canonical_title = "林默".to_string();
        let title_issues = forbidden_naming_contract_issues(
            &draft,
            &serde_json::to_string(&contract).expect("contract"),
        );
        assert!(title_issues
            .iter()
            .any(|issue| issue.contains("禁用书名") && issue.contains("林默")));
    }
}
