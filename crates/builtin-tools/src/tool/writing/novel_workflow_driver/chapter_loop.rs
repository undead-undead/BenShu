use super::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct RevisionBudget {
    #[serde(default)]
    pub(super) local_cleanup_fingerprints: BTreeSet<String>,
    #[serde(default)]
    pub(super) length_topup_attempted: bool,
    #[serde(default)]
    pub(super) tail_completion_attempted: bool,
    #[serde(default)]
    pub(super) metadata_repair_attempts: usize,
    #[serde(default)]
    pub(super) semantic_attempts: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct RevisionState {
    #[serde(default)]
    pub(super) best_candidate_id: Option<String>,
    #[serde(default)]
    pub(super) best_candidate_path: Option<String>,
    #[serde(default)]
    pub(super) budget: RevisionBudget,
    #[serde(default)]
    pub(super) next_candidate_iteration: usize,
}

pub(super) struct BoundedRevisionCycle {
    pub(super) best_candidate: DraftCandidateRecord,
    pub(super) state: RevisionState,
    pub(super) next_iteration: usize,
}

impl RevisionBudget {
    pub(super) fn can_cleanup(&self, body_fingerprint: &str) -> bool {
        !self.local_cleanup_fingerprints.contains(body_fingerprint)
    }

    pub(super) fn can_attempt_semantic_revision(&self) -> bool {
        self.semantic_attempts < MAX_LLM_REVISION_ATTEMPTS
    }
}

pub(super) fn findings_from_results(
    write_result: &Value,
    audit: &Value,
) -> Vec<chapter_quality::ChapterFinding> {
    let mut findings = ["/quality_gate/findings", "/metadata_gate/findings"]
        .into_iter()
        .flat_map(|pointer| {
            write_result
                .pointer(pointer)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .chain(
            audit
                .pointer("/review/findings")
                .and_then(Value::as_array)
                .into_iter()
                .flatten(),
        )
        .filter_map(|finding| {
            serde_json::from_value::<chapter_quality::ChapterFinding>(finding.clone()).ok()
        })
        .collect::<Vec<_>>();
    findings.sort_by(|left, right| {
        (
            left.code.as_str(),
            left.source.as_str(),
            left.message.as_str(),
        )
            .cmp(&(
                right.code.as_str(),
                right.source.as_str(),
                right.message.as_str(),
            ))
    });
    findings.dedup_by(|left, right| {
        left.code == right.code && left.source == right.source && left.message == right.message
    });
    findings
}

pub(super) fn revision_quality_vector(
    authority: &SealedChapterAuthority,
    draft: &novel_runner::DraftOutput,
    findings: &[chapter_quality::ChapterFinding],
    parent: Option<&novel_runner::DraftOutput>,
    parent_findings: &[chapter_quality::ChapterFinding],
    chapter_unit_target: Option<usize>,
    language: &str,
) -> RevisionQualityVector {
    use chapter_quality::{ChapterFindingClass, ChapterFindingDisposition};

    let hard = findings
        .iter()
        .filter(|finding| finding.hard_blocking())
        .collect::<Vec<_>>();
    let parent_hard_codes = parent_findings
        .iter()
        .filter(|finding| finding.hard_blocking())
        .map(|finding| finding.code.as_str())
        .collect::<BTreeSet<_>>();
    let new_high_priority_blockers = hard
        .iter()
        .filter(|finding| {
            matches!(
                finding.class,
                ChapterFindingClass::Contract
                    | ChapterFindingClass::Continuity
                    | ChapterFindingClass::State
            ) && !parent_hard_codes.contains(finding.code.as_str())
        })
        .count();
    let required_outcomes_missing = hard
        .iter()
        .filter(|finding| {
            matches!(
                finding.code.as_str(),
                "chapter_goal_replaced"
                    | "required_outcome_missing"
                    | "required_reveal_missing"
                    | "required_hook_progress_missing"
            )
        })
        .count();
    let typed_conflicts = hard
        .iter()
        .filter(|finding| {
            matches!(
                finding.code.as_str(),
                "character_identity_conflict"
                    | "character_name_replacement"
                    | "relationship_state_conflict"
                    | "world_rule_conflict"
                    | "timeline_conflict"
                    | "ability_or_resource_conflict"
            )
        })
        .count();
    let protected_facts_lost = typed_conflicts
        + parent
            .map(|parent| protected_authority_anchors_lost(authority, parent, draft))
            .unwrap_or_default();
    let material_deletion_ratio = parent
        .map(|parent| {
            let before = count_chapter_units(&parent.content, language);
            let after = count_chapter_units(&draft.content, language);
            if before == 0 || after >= before {
                0
            } else {
                before
                    .saturating_sub(after)
                    .saturating_mul(1000)
                    .saturating_div(before)
                    .min(1000) as u16
            }
        })
        .unwrap_or(0);
    let units = count_chapter_units(&draft.content, language);
    let length_violation = chapter_unit_target
        .filter(|target| *target > 0)
        .map(|target| units.saturating_sub(longform_policy::chapter_tier_max_units(target)))
        .unwrap_or(0);
    let length_shortfall = chapter_unit_target
        .filter(|target| *target > 0)
        .map(|target| target.saturating_sub(units))
        .unwrap_or(0);
    let length_topup_eligible =
        chapter_unit_target
            .filter(|target| *target > 0)
            .is_none_or(|target| {
                length_shortfall == 0
                    || length_shortfall <= length_topup_shortfall_limit(target, language)
            });
    RevisionQualityVector {
        hard_blockers: hard.len(),
        authority_conflicts: hard
            .iter()
            .filter(|finding| {
                matches!(
                    finding.class,
                    ChapterFindingClass::Contract | ChapterFindingClass::Continuity
                )
            })
            .count(),
        state_conflicts: hard
            .iter()
            .filter(|finding| finding.class == ChapterFindingClass::State)
            .count(),
        required_outcomes_missing,
        protected_facts_lost,
        new_high_priority_blockers,
        material_deletion_ratio,
        incomplete_body: !chapter_body_completion_issue_list(&draft.content).is_empty(),
        contaminated_body: chapter_body_has_tool_or_json_residue(&draft.content),
        degenerate_repetition: chapter_body_has_degenerate_repetition(&draft.content, language),
        length_violation,
        length_shortfall,
        length_blockers: hard
            .iter()
            .filter(|finding| finding.code == "length_below_minimum")
            .count(),
        length_topup_eligible,
        deterministic_repairs: findings
            .iter()
            .filter(|finding| finding.disposition == ChapterFindingDisposition::DeterministicRepair)
            .count(),
    }
}

fn protected_authority_anchors_lost(
    authority: &SealedChapterAuthority,
    parent: &novel_runner::DraftOutput,
    candidate: &novel_runner::DraftOutput,
) -> usize {
    let chapter_scope = serde_json::to_string(&authority.chapter_contract).unwrap_or_default();
    let truth_scope = serde_json::to_string(&authority.truth_as_of_chapter).unwrap_or_default();
    let mut character_anchors = authority
        .character_registrations
        .iter()
        .map(|registration| registration.canonical_name.trim().to_string())
        .filter(|name| !name.is_empty())
        .collect::<BTreeSet<_>>();
    collect_character_authority_anchors(
        &authority.canonical_contract,
        false,
        &mut character_anchors,
    );
    // A rejected parent is not story truth. A character name is protected only when it is
    // already in sealed truth or is required by this chapter contract. Otherwise deleting an
    // accidentally introduced future character would be misclassified as continuity loss.
    character_anchors
        .retain(|anchor| character_anchor_is_protected(&chapter_scope, &truth_scope, anchor));
    let mut anchors = character_anchors;
    anchors.extend(
        authority
            .chapter_contract
            .hook_opened
            .iter()
            .chain(authority.chapter_contract.hook_paid_off.iter())
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
    );
    anchors
        .iter()
        .filter(|anchor| {
            parent.content.contains(anchor.as_str()) && !candidate.content.contains(anchor.as_str())
        })
        .count()
}

fn character_anchor_is_protected(chapter_scope: &str, truth_scope: &str, anchor: &str) -> bool {
    chapter_scope.contains(anchor) || truth_scope.contains(anchor)
}

fn collect_character_authority_anchors(
    value: &Value,
    in_character_scope: bool,
    anchors: &mut BTreeSet<String>,
) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                let key_lower = key.to_ascii_lowercase();
                let child_scope = in_character_scope
                    || key_lower.contains("character")
                    || matches!(key_lower.as_str(), "cast" | "protagonist" | "antagonist");
                if child_scope
                    && matches!(key_lower.as_str(), "name" | "canonical_name")
                    && child.as_str().is_some_and(|name| !name.trim().is_empty())
                {
                    anchors.insert(child.as_str().unwrap_or_default().trim().to_string());
                }
                collect_character_authority_anchors(child, child_scope, anchors);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_character_authority_anchors(item, in_character_scope, anchors);
            }
        }
        _ => {}
    }
}

pub(super) fn candidate_is_strict_improvement(
    current: &RevisionQualityVector,
    candidate: &RevisionQualityVector,
    provenance: CandidateProvenance,
) -> bool {
    if candidate.new_high_priority_blockers > 0
        || candidate.required_outcomes_missing > current.required_outcomes_missing
        || candidate.protected_facts_lost > current.protected_facts_lost
        || candidate.material_deletion_ratio > 350
        || candidate.incomplete_body
        || candidate.contaminated_body
        || candidate.degenerate_repetition
        || candidate.length_violation > 0
    {
        return false;
    }
    if candidate.hard_blockers < current.hard_blockers {
        return true;
    }
    if candidate.hard_blockers == current.hard_blockers
        && (candidate.authority_conflicts, candidate.state_conflicts)
            < (current.authority_conflicts, current.state_conflicts)
        && (candidate.length_blockers == 0 || candidate.length_topup_eligible)
    {
        // Contract/continuity/state conflicts can poison every later chapter.
        // A bounded, recoverable length blocker is therefore a strict net
        // improvement when it replaces one of those conflicts; the existing
        // length-top-up route can then repair the remaining shortfall.
        return true;
    }
    if candidate.hard_blockers == current.hard_blockers
        && candidate.required_outcomes_missing < current.required_outcomes_missing
        && (candidate.length_blockers == 0 || candidate.length_topup_eligible)
    {
        return true;
    }
    if current.hard_blockers > 0
        && current.hard_blockers == current.length_blockers
        && candidate.hard_blockers == candidate.length_blockers
        && candidate.hard_blockers == current.hard_blockers
        && candidate.length_shortfall < current.length_shortfall
    {
        return true;
    }
    matches!(
        provenance,
        CandidateProvenance::LocalCleanup
            | CandidateProvenance::LengthTopup
            | CandidateProvenance::TailCompletion
            | CandidateProvenance::MetadataRepair
    ) && candidate.hard_blockers == current.hard_blockers
        && candidate.deterministic_repairs < current.deterministic_repairs
}

pub(super) fn draft_candidate_record(
    authority: &SealedChapterAuthority,
    draft: novel_runner::DraftOutput,
    findings: Vec<chapter_quality::ChapterFinding>,
    quality_vector: RevisionQualityVector,
    provenance: CandidateProvenance,
    parent_candidate_id: Option<String>,
    accepted_as_best: bool,
) -> DraftCandidateRecord {
    let body_fingerprint = chapter_quality::chapter_body_fingerprint(&draft.content);
    let metadata_json = serde_json::json!({
        "title": &draft.title,
        "summary": &draft.summary,
        "key_facts": &draft.key_facts,
        "continuity_updates": &draft.continuity_updates,
    });
    let metadata_fingerprint = hex::encode(Sha256::digest(
        serde_json::to_vec(&metadata_json)
            .unwrap_or_default()
            .as_slice(),
    ));
    let mut candidate_digest = Sha256::new();
    candidate_digest.update(authority.authority_root_fingerprint.as_bytes());
    candidate_digest.update(body_fingerprint.as_bytes());
    candidate_digest.update(metadata_fingerprint.as_bytes());
    candidate_digest.update(format!("{provenance:?}").as_bytes());
    let candidate_id = hex::encode(candidate_digest.finalize());
    DraftCandidateRecord {
        candidate_id,
        parent_candidate_id,
        authority_fingerprint: authority.authority_root_fingerprint.clone(),
        body_fingerprint,
        metadata_fingerprint,
        draft,
        findings,
        quality_vector,
        provenance,
        accepted_as_best,
    }
}

pub(super) fn align_draft_with_studio_result(
    draft: &mut novel_runner::DraftOutput,
    result: &Value,
) {
    if let Some(content) = result
        .get("candidate_body")
        .or_else(|| result.get("content"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|content| !content.is_empty())
    {
        draft.content = content.to_string();
    }
    let Some(chapter) = result.get("chapter") else {
        return;
    };
    if let Some(title) = chapter
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|title| !title.is_empty())
    {
        draft.title = title.to_string();
    }
    if let Some(summary) = chapter.get("summary").and_then(Value::as_str) {
        draft.summary = summary.trim().to_string();
    }
    draft.key_facts = json_string_array(chapter.get("key_facts"));
    draft.continuity_updates = json_string_array(chapter.get("continuity_updates"));
}

impl NovelChapterRunner {
    pub(super) async fn run_bounded_revision_cycle(
        &self,
        authority: &SealedChapterAuthority,
        initial_draft: novel_runner::DraftOutput,
        initial_findings: &[chapter_quality::ChapterFinding],
        mut persisted_state: RevisionState,
        recovered_best: Option<DraftCandidateRecord>,
    ) -> anyhow::Result<BoundedRevisionCycle> {
        let chapter_number = authority.chapter_number;
        let vector = revision_quality_vector(
            authority,
            &initial_draft,
            initial_findings,
            None,
            &[],
            self.chapter_unit_target,
            &self.language,
        );
        let provenance = if initial_draft.degraded {
            CandidateProvenance::TruncatedRecovery
        } else {
            CandidateProvenance::InitialDraft
        };
        let best_candidate = if let Some(mut recovered) = recovered_best {
            recovered.draft = initial_draft;
            recovered.findings = initial_findings.to_vec();
            recovered.quality_vector = vector;
            recovered.accepted_as_best = true;
            recovered
        } else {
            let candidate = draft_candidate_record(
                authority,
                initial_draft,
                initial_findings.to_vec(),
                vector,
                provenance,
                None,
                true,
            );
            self.persist_draft_candidate(
                chapter_number,
                persisted_state.next_candidate_iteration,
                &candidate,
            )
            .await?;
            let best_path = self
                .persist_best_draft_candidate(chapter_number, &candidate)
                .await?;
            call_novel_studio_json(
                &self.tool,
                json!({
                    "action": "record_candidate_decision",
                    "project_path": self.project_path,
                    "chapter_number": chapter_number,
                    "attempt_kind": serde_json::to_value(provenance)?
                        .as_str()
                        .unwrap_or("initial_draft"),
                    "candidate_fingerprint": candidate.body_fingerprint.clone(),
                    "quality_vector": candidate.quality_vector.clone(),
                    "accepted_as_best": true,
                    "best_candidate_path": best_path.clone()
                }),
            )
            .await?;
            persisted_state.best_candidate_id = Some(candidate.candidate_id.clone());
            persisted_state.best_candidate_path = Some(best_path);
            persisted_state.next_candidate_iteration =
                persisted_state.next_candidate_iteration.saturating_add(1);
            candidate
        };
        let next_iteration = persisted_state.next_candidate_iteration;
        Ok(BoundedRevisionCycle {
            best_candidate,
            state: persisted_state,
            next_iteration,
        })
    }

    pub(super) async fn reconcile_submitted_candidate(
        &self,
        authority: &SealedChapterAuthority,
        chapter_number: usize,
        mut candidate_draft: novel_runner::DraftOutput,
        candidate_write_result: Value,
        candidate_audit: Value,
        provenance: CandidateProvenance,
        cycle: &mut BoundedRevisionCycle,
    ) -> anyhow::Result<(novel_runner::DraftOutput, Value, Value, bool, String)> {
        align_draft_with_studio_result(&mut candidate_draft, &candidate_write_result);
        let findings = findings_from_results(&candidate_write_result, &candidate_audit);
        let vector = revision_quality_vector(
            authority,
            &candidate_draft,
            &findings,
            Some(&cycle.best_candidate.draft),
            &cycle.best_candidate.findings,
            self.chapter_unit_target,
            &self.language,
        );
        let accepted_as_best = candidate_is_strict_improvement(
            &cycle.best_candidate.quality_vector,
            &vector,
            provenance,
        );
        let record = draft_candidate_record(
            authority,
            candidate_draft.clone(),
            findings,
            vector,
            provenance,
            Some(cycle.best_candidate.candidate_id.clone()),
            accepted_as_best,
        );
        let path = self
            .persist_draft_candidate(chapter_number, cycle.next_iteration, &record)
            .await?;
        cycle.next_iteration += 1;
        let best_candidate_path = if accepted_as_best {
            self.persist_best_draft_candidate(chapter_number, &record)
                .await?
        } else {
            cycle.state.best_candidate_path.clone().unwrap_or_default()
        };
        call_novel_studio_json(
            &self.tool,
            json!({
                "action": "record_candidate_decision",
                "project_path": self.project_path,
                "chapter_number": chapter_number,
                "attempt_kind": serde_json::to_value(provenance)?
                    .as_str()
                    .unwrap_or("candidate"),
                "candidate_fingerprint": record.body_fingerprint.clone(),
                "quality_vector": record.quality_vector.clone(),
                "accepted_as_best": accepted_as_best,
                "best_candidate_path": best_candidate_path
            }),
        )
        .await?;
        if accepted_as_best {
            cycle.state.best_candidate_id = Some(record.candidate_id.clone());
            cycle.state.best_candidate_path = Some(best_candidate_path.clone());
            cycle.best_candidate = record;
            let persisted_write = call_novel_studio_json(
                &self.tool,
                json!({
                    "action": "revise_draft",
                    "project_path": self.project_path,
                    "chapter_number": chapter_number,
                    "chapter_title": candidate_draft.title.clone(),
                    "content": candidate_draft.content.clone(),
                    "summary": candidate_draft.summary.clone(),
                    "key_facts": candidate_draft.key_facts.clone(),
                    "continuity_updates": candidate_draft.continuity_updates.clone()
                }),
            )
            .await?;
            let persisted_audit = self
                .rule_first_audit_or_full_audit(chapter_number, &persisted_write)
                .await?;
            return Ok((
                candidate_draft,
                persisted_write,
                persisted_audit,
                true,
                path,
            ));
        }

        let restored = cycle.best_candidate.draft.clone();
        let restored_write = call_novel_studio_json(
            &self.tool,
            json!({
                "action": "revise_draft",
                "candidate_only": true,
                "project_path": self.project_path,
                "chapter_number": chapter_number,
                "chapter_title": restored.title.clone(),
                "content": restored.content.clone(),
                "summary": restored.summary.clone(),
                "key_facts": restored.key_facts.clone(),
                "continuity_updates": restored.continuity_updates.clone()
            }),
        )
        .await?;
        let restored_audit = self
            .rule_first_audit_or_full_audit(chapter_number, &restored_write)
            .await?;
        Ok((restored, restored_write, restored_audit, false, path))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ChapterLoopDecision {
    Accept,
    MetadataRepair,
    TailCompletion,
    LocalCleanup,
    LengthTopup,
    LlmRevision,
    StopForFinalCleanup,
    BlockRevision,
}

pub(super) struct ChapterLoopDecisionInput<'a> {
    pub(super) write_result: &'a Value,
    pub(super) audit: &'a Value,
    pub(super) body_fingerprint: u64,
    pub(super) last_cleanup_fingerprint: Option<u64>,
    pub(super) attempted_tail_completion: bool,
    pub(super) attempted_length_topup: bool,
    pub(super) chapter_unit_target: Option<usize>,
    pub(super) language: &'a str,
}

pub(super) fn decide_chapter_loop_step(input: ChapterLoopDecisionInput<'_>) -> ChapterLoopDecision {
    if metadata_gate_blocks(input.write_result)
        || !json_array_is_empty(input.write_result.pointer("/truth_validation/issues"))
    {
        return ChapterLoopDecision::MetadataRepair;
    }

    if only_local_cleanup_issues(input.write_result, input.audit) {
        return if input.last_cleanup_fingerprint == Some(input.body_fingerprint) {
            ChapterLoopDecision::StopForFinalCleanup
        } else {
            ChapterLoopDecision::LocalCleanup
        };
    }

    if !input.attempted_tail_completion
        && revision_issues_include_tail_completion(input.write_result, input.audit)
    {
        return ChapterLoopDecision::TailCompletion;
    }

    if only_small_length_shortfall(
        input.write_result,
        input.audit,
        input.chapter_unit_target,
        input.language,
    ) {
        return if input.attempted_length_topup {
            ChapterLoopDecision::StopForFinalCleanup
        } else {
            ChapterLoopDecision::LengthTopup
        };
    }

    if metadata_gate_has_repairable(input.write_result) {
        return ChapterLoopDecision::MetadataRepair;
    }

    if !body_revision_required_after_audit(input.write_result, input.audit) {
        return ChapterLoopDecision::Accept;
    }

    if audit_next_action_blocked(input.audit)
        && !only_local_cleanup_issues(input.write_result, input.audit)
    {
        if input.attempted_tail_completion
            && revision_issues_include_tail_completion(input.write_result, input.audit)
        {
            return ChapterLoopDecision::BlockRevision;
        }
        return ChapterLoopDecision::LlmRevision;
    }

    ChapterLoopDecision::LlmRevision
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn studio_result_aligns_candidate_to_the_persisted_body_and_metadata() {
        let mut draft = novel_runner::DraftOutput {
            title: "第2章".to_string(),
            content: "第一段。\n\n第二段。".to_string(),
            summary: "旧摘要".to_string(),
            key_facts: vec!["旧事实".to_string()],
            continuity_updates: vec!["旧连续性".to_string()],
            degraded: false,
            degraded_reason: String::new(),
        };
        let result = json!({
            "candidate_body": "第一段。\n第二段。",
            "chapter": {
                "title": "剑指黑石镇",
                "summary": "新摘要",
                "key_facts": ["新事实"],
                "continuity_updates": ["新连续性"]
            }
        });

        align_draft_with_studio_result(&mut draft, &result);

        assert_eq!(draft.content, "第一段。\n第二段。");
        assert_eq!(draft.title, "剑指黑石镇");
        assert_eq!(draft.summary, "新摘要");
        assert_eq!(draft.key_facts, ["新事实"]);
        assert_eq!(draft.continuity_updates, ["新连续性"]);
    }

    #[test]
    fn semantic_budget_uses_the_shared_five_attempt_limit() {
        let mut budget = RevisionBudget::default();
        for attempt in 0..MAX_LLM_REVISION_ATTEMPTS {
            budget.semantic_attempts = attempt;
            assert!(budget.can_attempt_semantic_revision());
        }
        budget.semantic_attempts = MAX_LLM_REVISION_ATTEMPTS;
        assert!(!budget.can_attempt_semantic_revision());
    }

    #[test]
    fn candidate_requires_hard_blocker_net_improvement() {
        let current = RevisionQualityVector {
            hard_blockers: 2,
            authority_conflicts: 2,
            ..Default::default()
        };
        let improved = RevisionQualityVector {
            hard_blockers: 1,
            authority_conflicts: 1,
            ..Default::default()
        };
        assert!(candidate_is_strict_improvement(
            &current,
            &improved,
            CandidateProvenance::SemanticRevision,
        ));
        assert!(!candidate_is_strict_improvement(
            &improved,
            &improved,
            CandidateProvenance::SemanticRevision,
        ));
    }

    #[test]
    fn resolving_authority_conflict_can_hand_off_a_shortfall_to_existing_topup() {
        let current = RevisionQualityVector {
            hard_blockers: 1,
            authority_conflicts: 1,
            new_high_priority_blockers: 1,
            ..Default::default()
        };
        let candidate = RevisionQualityVector {
            hard_blockers: 1,
            length_blockers: 1,
            length_shortfall: 386,
            length_topup_eligible: true,
            material_deletion_ratio: 328,
            ..Default::default()
        };

        assert!(candidate_is_strict_improvement(
            &current,
            &candidate,
            CandidateProvenance::SemanticRevision,
        ));
    }

    #[test]
    fn authority_improvement_cannot_hand_off_an_unrecoverable_length_shortfall() {
        let current = RevisionQualityVector {
            hard_blockers: 1,
            authority_conflicts: 1,
            ..Default::default()
        };
        let candidate = RevisionQualityVector {
            hard_blockers: 1,
            length_blockers: 1,
            length_shortfall: 1_330,
            length_topup_eligible: false,
            material_deletion_ratio: 350,
            ..Default::default()
        };

        assert!(!candidate_is_strict_improvement(
            &current,
            &candidate,
            CandidateProvenance::SemanticRevision,
        ));
    }

    #[test]
    fn authority_improvement_still_rejects_excessive_material_deletion() {
        let current = RevisionQualityVector {
            hard_blockers: 1,
            authority_conflicts: 1,
            ..Default::default()
        };
        let candidate = RevisionQualityVector {
            hard_blockers: 1,
            length_blockers: 1,
            material_deletion_ratio: 351,
            ..Default::default()
        };

        assert!(!candidate_is_strict_improvement(
            &current,
            &candidate,
            CandidateProvenance::SemanticRevision,
        ));
    }

    #[test]
    fn rejected_candidate_cannot_make_a_future_character_a_protected_fact() {
        let chapter_scope = r#"{"goal":"姜清澜确认旧图失效"}"#;
        let sealed_truth = r#"{"characters":["姜清澜"]}"#;

        assert!(character_anchor_is_protected(
            chapter_scope,
            sealed_truth,
            "姜清澜"
        ));
        assert!(!character_anchor_is_protected(
            chapter_scope,
            sealed_truth,
            "岑启白"
        ));
    }

    #[test]
    fn candidate_cannot_win_by_deleting_material_or_losing_protected_facts() {
        let current = RevisionQualityVector {
            hard_blockers: 2,
            authority_conflicts: 2,
            ..Default::default()
        };
        let deleted = RevisionQualityVector {
            hard_blockers: 1,
            material_deletion_ratio: 351,
            ..Default::default()
        };
        let lost_fact = RevisionQualityVector {
            hard_blockers: 1,
            protected_facts_lost: 1,
            ..Default::default()
        };
        assert!(!candidate_is_strict_improvement(
            &current,
            &deleted,
            CandidateProvenance::SemanticRevision,
        ));
        assert!(!candidate_is_strict_improvement(
            &current,
            &lost_fact,
            CandidateProvenance::SemanticRevision,
        ));
    }

    #[test]
    fn candidate_can_win_by_reducing_the_only_remaining_length_shortfall() {
        let current = RevisionQualityVector {
            hard_blockers: 1,
            length_shortfall: 795,
            length_blockers: 1,
            ..Default::default()
        };
        let closer = RevisionQualityVector {
            hard_blockers: 1,
            length_shortfall: 106,
            length_blockers: 1,
            ..Default::default()
        };

        assert!(candidate_is_strict_improvement(
            &current,
            &closer,
            CandidateProvenance::SemanticRevision,
        ));
        assert!(!candidate_is_strict_improvement(
            &closer,
            &closer,
            CandidateProvenance::SemanticRevision,
        ));
    }

    #[test]
    fn shorter_length_gap_cannot_hide_an_unchanged_non_length_blocker() {
        let current = RevisionQualityVector {
            hard_blockers: 2,
            length_blockers: 1,
            authority_conflicts: 1,
            length_shortfall: 795,
            ..Default::default()
        };
        let longer_but_still_conflicting = RevisionQualityVector {
            hard_blockers: 2,
            length_blockers: 1,
            authority_conflicts: 1,
            length_shortfall: 106,
            ..Default::default()
        };

        assert!(!candidate_is_strict_improvement(
            &current,
            &longer_but_still_conflicting,
            CandidateProvenance::SemanticRevision,
        ));
    }

    #[test]
    fn loop_tops_up_small_length_shortfall_before_repairing_metadata() {
        let write_result = json!({
            "unit_count": 2394,
            "quality_gate": {
                "passed": false,
                "findings": [
                    {
                        "class": "length",
                        "code": "length_below_minimum",
                        "disposition": "hard_block"
                    },
                    {
                        "class": "metadata",
                        "code": "metadata_invalid",
                        "disposition": "deterministic_repair"
                    }
                ]
            },
            "metadata_gate": {
                "blocking": [],
                "repairable": []
            },
            "truth_validation": {
                "issues": []
            }
        });
        let audit = json!({
            "review": {
                "verdict": "needs_revision",
                "findings": []
            },
            "truth_validation": {
                "issues": []
            }
        });

        let decision = decide_chapter_loop_step(ChapterLoopDecisionInput {
            write_result: &write_result,
            audit: &audit,
            body_fingerprint: 11,
            last_cleanup_fingerprint: None,
            attempted_tail_completion: false,
            attempted_length_topup: false,
            chapter_unit_target: Some(2500),
            language: "zh-CN",
        });

        assert_eq!(decision, ChapterLoopDecision::LengthTopup);
    }

    #[test]
    fn loop_tops_up_length_only_draft_that_reaches_half_the_contract_target() {
        let write_result = json!({
            "unit_count": 1719,
            "quality_gate": {
                "passed": false,
                "findings": [{
                    "class": "length",
                    "code": "length_below_minimum",
                    "disposition": "hard_block"
                }]
            },
            "metadata_gate": {"blocking": [], "repairable": []},
            "truth_validation": {"issues": []}
        });
        let audit = json!({
            "review": {"verdict": "needs_revision", "findings": []},
            "truth_validation": {"issues": []}
        });

        let decision = decide_chapter_loop_step(ChapterLoopDecisionInput {
            write_result: &write_result,
            audit: &audit,
            body_fingerprint: 17,
            last_cleanup_fingerprint: None,
            attempted_tail_completion: false,
            attempted_length_topup: false,
            chapter_unit_target: Some(2500),
            language: "zh-CN",
        });

        assert_eq!(decision, ChapterLoopDecision::LengthTopup);
    }

    #[test]
    fn loop_accepts_soft_audit_even_when_write_result_keeps_stale_revision_verdict() {
        let write_result = json!({
            "quality_gate": {
                "passed": true,
                "issues": [],
                "warnings": [],
                "repairable": []
            },
            "truth_validation": {
                "issues": []
            },
            "review": {
                "verdict": "needs_revision"
            }
        });
        let audit = json!({
            "review": {
                "verdict": "needs_revision",
                "issues": [
                    "场景转换突兀：第17段结尾主角走出档案局大门，第18段开头他已在地下商业街，中间缺少过渡描写。"
                ]
            },
            "truth_validation": {
                "issues": []
            }
        });

        let decision = decide_chapter_loop_step(ChapterLoopDecisionInput {
            write_result: &write_result,
            audit: &audit,
            body_fingerprint: 7,
            last_cleanup_fingerprint: Some(7),
            attempted_tail_completion: false,
            attempted_length_topup: false,
            chapter_unit_target: Some(2500),
            language: "zh-CN",
        });

        assert_eq!(decision, ChapterLoopDecision::Accept);
    }

    #[test]
    fn loop_repairs_metadata_before_accepting_a_passed_body() {
        let write_result = json!({
            "quality_gate": {
                "passed": true,
                "issues": [],
                "repairable": []
            },
            "metadata_gate": {
                "blocking": [],
                "repairable": [
                    "chapter summary could be more specific; repair metadata only"
                ]
            },
            "truth_validation": {
                "issues": []
            }
        });
        let audit = json!({
            "review": {
                "verdict": "passed",
                "issues": []
            },
            "truth_validation": {
                "issues": []
            }
        });

        let decision = decide_chapter_loop_step(ChapterLoopDecisionInput {
            write_result: &write_result,
            audit: &audit,
            body_fingerprint: 9,
            last_cleanup_fingerprint: Some(9),
            attempted_tail_completion: false,
            attempted_length_topup: false,
            chapter_unit_target: Some(2500),
            language: "zh-CN",
        });

        assert_eq!(decision, ChapterLoopDecision::MetadataRepair);
    }

    #[test]
    fn free_text_surface_noise_cannot_trigger_local_cleanup() {
        let write_result = json!({
            "quality_gate": {
                "passed": true,
                "issues": [],
                "warnings": [],
                "repairable": []
            },
            "truth_validation": {
                "issues": []
            }
        });
        let audit = json!({
            "issues": [
                "对话略显说教：解释资产残值的段落稍显冗长。",
                "正文中存在明显的乱码/OCR残留字符：'皱巴巴的4纸'（出现三次），应为'A4纸'或'纸张'。"
            ],
            "next_action": "blocked",
            "verdict": "needs_revision"
        });

        let decision = decide_chapter_loop_step(ChapterLoopDecisionInput {
            write_result: &write_result,
            audit: &audit,
            body_fingerprint: 42,
            last_cleanup_fingerprint: None,
            attempted_tail_completion: false,
            attempted_length_topup: false,
            chapter_unit_target: Some(2500),
            language: "zh-CN",
        });

        assert_eq!(decision, ChapterLoopDecision::Accept);
    }

    #[test]
    fn free_text_orthography_comment_cannot_trigger_local_cleanup() {
        let write_result = json!({
            "quality_gate": {
                "passed": true,
                "issues": [],
                "warnings": [],
                "repairable": []
            },
            "truth_validation": {
                "issues": []
            }
        });
        let audit = json!({
            "issues": [
                "正文中'核心开发區'混用了繁体字'區'，与其余简体中文语境不一致"
            ],
            "next_action": "revise_draft",
            "verdict": "needs_revision"
        });

        let decision = decide_chapter_loop_step(ChapterLoopDecisionInput {
            write_result: &write_result,
            audit: &audit,
            body_fingerprint: 42,
            last_cleanup_fingerprint: None,
            attempted_tail_completion: false,
            attempted_length_topup: false,
            chapter_unit_target: Some(2500),
            language: "zh-CN",
        });

        assert_eq!(decision, ChapterLoopDecision::Accept);
    }

    #[test]
    fn free_text_script_comment_cannot_trigger_semantic_revision() {
        let write_result = json!({
            "quality_gate": {
                "passed": true,
                "issues": [],
                "warnings": [],
                "repairable": []
            },
            "truth_validation": {
                "issues": []
            }
        });
        let audit = json!({
            "issues": [
                "存在外文残片：倒数第二段末尾夹杂英文'unawarethatsomewhereinthecity,ahundredpaperfiguresarewakingup,theirpaperbonescreakingintherain,waitingfortheirmasterscommand.'",
                "语体风格不统一：结尾处突然插入英文叙述，与全文中文语境割裂，像未翻译的草稿或残留的元数据。"
            ],
            "next_action": "revise_draft",
            "verdict": "needs_revision"
        });

        let decision = decide_chapter_loop_step(ChapterLoopDecisionInput {
            write_result: &write_result,
            audit: &audit,
            body_fingerprint: 42,
            last_cleanup_fingerprint: None,
            attempted_tail_completion: false,
            attempted_length_topup: false,
            chapter_unit_target: Some(2500),
            language: "zh-CN",
        });

        assert_eq!(decision, ChapterLoopDecision::Accept);
    }

    #[test]
    fn free_text_unfinished_tail_cannot_trigger_tail_completion() {
        let write_result = json!({
            "quality_gate": {
                "passed": false,
                "issues": [
                    "quality gate: chapter body appears unfinished: final line has no terminal punctuation near `断魂崖前，一座摇摇欲坠的吊桥横跨在深渊`"
                ],
                "repairable": []
            },
            "truth_validation": {
                "issues": []
            }
        });
        let audit = json!({
            "review": {
                "verdict": "needs_revision",
                "issues": []
            },
            "truth_validation": {
                "issues": []
            }
        });

        let decision = decide_chapter_loop_step(ChapterLoopDecisionInput {
            write_result: &write_result,
            audit: &audit,
            body_fingerprint: 42,
            last_cleanup_fingerprint: None,
            attempted_tail_completion: false,
            attempted_length_topup: false,
            chapter_unit_target: Some(2500),
            language: "zh-CN",
        });

        assert_eq!(decision, ChapterLoopDecision::Accept);
    }

    #[test]
    fn free_text_structural_comment_cannot_block_revision() {
        let write_result = json!({
            "quality_gate": {
                "passed": true,
                "issues": [],
                "repairable": []
            },
            "truth_validation": {
                "issues": []
            }
        });
        let audit = json!({
            "review": {
                "verdict": "needs_revision",
                "issues": [
                    "空间逻辑混乱：角色已经跑出洞穴，但下一句又写身后的暗门关闭。"
                ]
            },
            "truth_validation": {
                "issues": []
            }
        });

        let decision = decide_chapter_loop_step(ChapterLoopDecisionInput {
            write_result: &write_result,
            audit: &audit,
            body_fingerprint: 42,
            last_cleanup_fingerprint: None,
            attempted_tail_completion: false,
            attempted_length_topup: false,
            chapter_unit_target: Some(2500),
            language: "zh-CN",
        });

        assert_eq!(decision, ChapterLoopDecision::Accept);
    }

    #[test]
    fn free_text_incomplete_sentence_cannot_trigger_tail_completion() {
        let write_result = json!({
            "quality_gate": {
                "passed": true,
                "issues": [],
                "repairable": []
            },
            "truth_validation": {
                "issues": []
            }
        });
        let audit = json!({
            "review": {
                "verdict": "needs_revision",
                "issues": [
                    "结尾句子不完整，缺少标点符号，应补完最后一句。",
                    "让它在这里后缺失谓语或宾语，属于截断残片。"
                ]
            },
            "review_cycle": {
                "next_action": "blocked"
            },
            "truth_validation": {
                "issues": []
            }
        });

        let decision = decide_chapter_loop_step(ChapterLoopDecisionInput {
            write_result: &write_result,
            audit: &audit,
            body_fingerprint: 42,
            last_cleanup_fingerprint: None,
            attempted_tail_completion: false,
            attempted_length_topup: false,
            chapter_unit_target: Some(2500),
            language: "zh-CN",
        });

        assert_eq!(decision, ChapterLoopDecision::Accept);
    }

    #[test]
    fn loop_does_not_repeat_tail_completion_forever() {
        let write_result = json!({
            "quality_gate": {
                "passed": false,
                "issues": [
                    "quality gate: chapter body appears unfinished: final line has no terminal punctuation near `断魂崖前，一座摇摇欲坠的吊桥横跨在深渊`"
                ],
                "repairable": []
            },
            "truth_validation": {
                "issues": []
            }
        });
        let audit = json!({
            "review": {
                "verdict": "needs_revision",
                "issues": []
            },
            "truth_validation": {
                "issues": []
            }
        });

        let decision = decide_chapter_loop_step(ChapterLoopDecisionInput {
            write_result: &write_result,
            audit: &audit,
            body_fingerprint: 42,
            last_cleanup_fingerprint: None,
            attempted_tail_completion: true,
            attempted_length_topup: false,
            chapter_unit_target: Some(2500),
            language: "zh-CN",
        });

        assert_ne!(decision, ChapterLoopDecision::TailCompletion);
    }

    #[test]
    fn loop_routes_truth_support_issues_to_metadata_repair_not_body_revision() {
        let write_result = json!({
            "quality_gate": {
                "passed": true,
                "issues": [],
                "warnings": [],
                "repairable": []
            },
            "metadata_gate": {
                "blocking": [],
                "repairable": []
            },
            "truth_validation": {
                "issues": [
                    "truth item lacks visible support in chapter body: 冲突从单线追踪升级为多方混战，局势更加复杂。"
                ]
            }
        });
        let audit = json!({
            "review": {
                "verdict": "needs_revision",
                "issues": []
            },
            "truth_validation": {
                "issues": [
                    "truth item lacks visible support in chapter body: 冲突从单线追踪升级为多方混战，局势更加复杂。"
                ]
            }
        });

        let decision = decide_chapter_loop_step(ChapterLoopDecisionInput {
            write_result: &write_result,
            audit: &audit,
            body_fingerprint: 42,
            last_cleanup_fingerprint: Some(42),
            attempted_tail_completion: false,
            attempted_length_topup: false,
            chapter_unit_target: Some(2500),
            language: "zh-CN",
        });

        assert_eq!(decision, ChapterLoopDecision::MetadataRepair);
    }

    #[test]
    fn free_text_tail_comment_cannot_bypass_typed_gate() {
        let write_result = json!({
            "quality_gate": {
                "passed": false,
                "issues": [
                    "quality gate: chapter body appears unfinished: final line has no terminal punctuation near `他知道，这`"
                ],
                "repairable": []
            }
        });
        let audit = json!({
            "review": {
                "verdict": "needs_revision",
                "issues": [
                    "文本在结尾处截断，最后一句“他知道，这”未完成，缺少标点符号。",
                    "局部逻辑需要补一句过渡。"
                ]
            },
            "review_cycle": {
                "next_action": "blocked"
            }
        });

        let decision = decide_chapter_loop_step(ChapterLoopDecisionInput {
            write_result: &write_result,
            audit: &audit,
            body_fingerprint: 42,
            last_cleanup_fingerprint: Some(42),
            attempted_tail_completion: false,
            attempted_length_topup: false,
            chapter_unit_target: Some(2500),
            language: "zh-CN",
        });

        assert_eq!(decision, ChapterLoopDecision::Accept);
    }

    #[test]
    fn free_text_sudden_interruption_cannot_trigger_tail_completion() {
        let write_result = json!({
            "quality_gate": {
                "passed": true,
                "issues": [],
                "repairable": []
            },
            "truth_validation": {
                "issues": []
            }
        });
        let audit = json!({
            "review": {
                "verdict": "needs_revision",
                "issues": [
                    "文本末尾突然中断，导致情节闭环缺失，不符合正式章节的完整性要求。",
                    "正文在结尾处截断，句子‘如果压力超过阈值，我就’未完结，属于明显残片。"
                ]
            },
            "review_cycle": {
                "next_action": "blocked"
            }
        });

        let decision = decide_chapter_loop_step(ChapterLoopDecisionInput {
            write_result: &write_result,
            audit: &audit,
            body_fingerprint: 42,
            last_cleanup_fingerprint: None,
            attempted_tail_completion: false,
            attempted_length_topup: false,
            chapter_unit_target: Some(2500),
            language: "zh-CN",
        });

        assert_eq!(decision, ChapterLoopDecision::Accept);
    }

    #[test]
    fn free_text_tail_issue_cannot_block_after_tail_attempt() {
        let write_result = json!({
            "quality_gate": {
                "passed": false,
                "issues": [
                    "quality gate: chapter body appears unfinished: final line has no terminal punctuation near `他知道，这`"
                ],
                "repairable": []
            }
        });
        let audit = json!({
            "review": {
                "verdict": "needs_revision",
                "issues": [
                    "文本在结尾处截断，最后一句“他知道，这”未完成，缺少标点符号。"
                ]
            },
            "review_cycle": {
                "next_action": "blocked"
            }
        });

        let decision = decide_chapter_loop_step(ChapterLoopDecisionInput {
            write_result: &write_result,
            audit: &audit,
            body_fingerprint: 42,
            last_cleanup_fingerprint: Some(42),
            attempted_tail_completion: true,
            attempted_length_topup: false,
            chapter_unit_target: Some(2500),
            language: "zh-CN",
        });

        assert_eq!(decision, ChapterLoopDecision::Accept);
    }

    #[test]
    fn free_text_tail_issue_cannot_block_when_budget_is_exhausted() {
        let write_result = json!({
            "quality_gate": {
                "passed": false,
                "issues": [
                    "quality gate: chapter body appears unfinished: final line has no terminal punctuation near `他知道，这`"
                ],
                "repairable": []
            }
        });
        let audit = json!({
            "review": {
                "verdict": "needs_revision",
                "issues": [
                    "文本在结尾处截断，最后一句“他知道，这”未完成，缺少标点符号。"
                ]
            },
            "review_cycle": {
                "next_action": "blocked"
            }
        });

        let decision = decide_chapter_loop_step(ChapterLoopDecisionInput {
            write_result: &write_result,
            audit: &audit,
            body_fingerprint: 42,
            last_cleanup_fingerprint: Some(42),
            attempted_tail_completion: true,
            attempted_length_topup: false,
            chapter_unit_target: Some(2500),
            language: "zh-CN",
        });

        assert_eq!(decision, ChapterLoopDecision::Accept);
    }

    #[test]
    fn free_text_cleanup_issue_cannot_stop_after_repeated_cleanup() {
        let write_result = json!({
            "quality_gate": {
                "passed": false,
                "issues": [
                    "quality gate: chapter body contains likely malformed CJK action-object-part boundary; missing punctuation or duplicated object near: 握着一柄断裂的长剑尖滴落的不是血"
                ],
                "repairable": []
            },
            "truth_validation": {
                "issues": []
            }
        });
        let audit = json!({
            "review": {
                "verdict": "needs_revision",
                "issues": []
            },
            "truth_validation": {
                "issues": []
            }
        });

        let decision = decide_chapter_loop_step(ChapterLoopDecisionInput {
            write_result: &write_result,
            audit: &audit,
            body_fingerprint: 42,
            last_cleanup_fingerprint: Some(42),
            attempted_tail_completion: true,
            attempted_length_topup: false,
            chapter_unit_target: Some(2500),
            language: "zh-CN",
        });

        assert_eq!(decision, ChapterLoopDecision::Accept);
    }
}
