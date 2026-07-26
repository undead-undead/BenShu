use super::issue::{
    ContractIssue, ContractIssueEvidence, ContractIssueKind, ContractIssueList, ContractIssueSet,
};
use super::staged_prompts::{
    contract_completion_stage_output_budget_for_issues,
    final_prompt_from_staged_contract_completion_stage, select_contract_completion_stage_excluding,
    ContractCompletionStage,
};
use super::{
    contract_candidate_issue_penalty, creation_contract_draft_is_confirmable,
    creation_contract_issue_summary, creation_contract_issues_are_contract_metadata_only,
    creation_draft_contract_blocking_findings_for_scope,
    creation_draft_with_pending_contract_applied, final_prompt_from_contract_metadata_repair,
    final_prompt_from_title_metadata_repair, pending_explicit_contract_revision_findings,
    record_contract_quality_blocker_diagnostic, repair_pending_contract_metadata_locally,
    strong_novel_contract_from_creation_draft, submit_generated_contract_candidate_to_draft,
    submit_pending_contract_metadata_repair, submit_pending_contract_title_metadata_repair,
    ContractGateResult, ContractGateStatus, ContractReadinessScope, ContractSubmissionOutcome,
    CreationDraftLifecycleStatus, CreationDraftRuntime, SessionCreationDraftState,
};
use super::{
    creation_contract_quality_blocked_response, stabilize_creation_contract_user_response,
    CREATION_CONTRACT_QUALITY_BLOCKED_METADATA_KEY,
};
use crate::tool::writing::contract_semantic_review::{
    ending_equivalence_review_request, outline_character_authority_review_request,
    parse_semantic_review_finding, user_story_authority_review_request, SemanticReviewFinding,
    SemanticReviewVerdict,
};
use crate::tool::writing::creation_contract_model::value_missing;
use async_trait::async_trait;
use benshu_brain::agent::protocol::ChatOutcome;
use benshu_runtime_policy_core::{
    is_recoverable_provider_disconnect, provider_service_pause_reason,
};
use std::collections::BTreeMap;
use uuid::Uuid;

#[async_trait]
pub trait CreationContractRepairRuntime: CreationDraftRuntime {
    async fn generate_creation_contract_repair_text(
        &mut self,
        supervisor_task_id: Uuid,
        failure_label: &str,
        repair_prompt: &str,
        max_tokens: Option<u64>,
    ) -> anyhow::Result<Option<String>>;

    async fn record_creation_contract_checkpoint(
        &mut self,
        supervisor_task_id: Uuid,
        label: &str,
        detail: Option<String>,
    ) -> anyhow::Result<()>;
}

pub async fn submit_session_creation_contract_candidate<R>(
    runtime: &mut R,
    session_id: &str,
    contract_text: &str,
) -> anyhow::Result<Option<(SessionCreationDraftState, ContractSubmissionOutcome)>>
where
    R: CreationDraftRuntime + Send,
{
    let Some(mut draft) = runtime.load_draft(session_id).await? else {
        return Ok(None);
    };
    if !draft.can_accept_contract_candidate() {
        return Ok(Some((
            draft,
            ContractSubmissionOutcome {
                gate: ContractGateResult::ready(),
                committed: false,
            },
        )));
    }
    let outcome = submit_generated_contract_candidate_to_draft(&mut draft, contract_text);
    runtime.save_draft(&draft).await?;
    Ok(Some((draft, outcome)))
}

async fn submit_session_title_metadata_repair_candidate<R>(
    runtime: &mut R,
    session_id: &str,
    title_metadata_text: &str,
) -> anyhow::Result<Option<(SessionCreationDraftState, ContractSubmissionOutcome)>>
where
    R: CreationDraftRuntime + Send,
{
    let Some(mut draft) = runtime.load_draft(session_id).await? else {
        return Ok(None);
    };
    let Some(outcome) =
        submit_pending_contract_title_metadata_repair(&mut draft, title_metadata_text)
    else {
        return Ok(Some((
            draft,
            ContractSubmissionOutcome {
                gate: ContractGateResult {
                    status: ContractGateStatus::NeedsRepair,
                    blocking_issues: Vec::new(),
                    repairable_issues: vec![
                        "书名 metadata 修复输出没有形成可归位的 title 补丁或字段包".to_string(),
                    ],
                    warnings: Vec::new(),
                },
                committed: false,
            },
        )));
    };
    runtime.save_draft(&draft).await?;
    Ok(Some((draft, outcome)))
}

async fn submit_session_contract_metadata_repair_candidate<R>(
    runtime: &mut R,
    session_id: &str,
    metadata_text: &str,
) -> anyhow::Result<Option<(SessionCreationDraftState, ContractSubmissionOutcome)>>
where
    R: CreationDraftRuntime + Send,
{
    let Some(mut draft) = runtime.load_draft(session_id).await? else {
        return Ok(None);
    };
    let Some(outcome) = submit_pending_contract_metadata_repair(&mut draft, metadata_text) else {
        return Ok(Some((
            draft,
            ContractSubmissionOutcome {
                gate: ContractGateResult {
                    status: ContractGateStatus::NeedsRepair,
                    blocking_issues: Vec::new(),
                    repairable_issues: vec![
                        "合同 metadata 修复输出没有形成可归位的局部补丁或字段包".to_string(),
                    ],
                    warnings: Vec::new(),
                },
                committed: false,
            },
        )));
    };
    runtime.save_draft(&draft).await?;
    Ok(Some((draft, outcome)))
}

async fn repair_session_contract_metadata_locally<R>(
    runtime: &mut R,
    session_id: &str,
) -> anyhow::Result<Option<(SessionCreationDraftState, ContractSubmissionOutcome)>>
where
    R: CreationDraftRuntime + Send,
{
    let Some(mut draft) = runtime.load_draft(session_id).await? else {
        return Ok(None);
    };
    let Some(outcome) = repair_pending_contract_metadata_locally(&mut draft) else {
        return Ok(None);
    };
    runtime.save_draft(&draft).await?;
    Ok(Some((draft, outcome)))
}

struct MetadataRepairAttempt {
    outcome: ChatOutcome,
    draft: SessionCreationDraftState,
    issues: ContractIssueList,
    ready: bool,
}

enum ModelPatchAttempt {
    NotApplicable,
    NoOutput,
    Candidate(MetadataRepairAttempt),
}

fn user_story_authority(draft: &SessionCreationDraftState) -> Option<String> {
    let authorities = draft
        .planning_notes
        .iter()
        .filter_map(|note| note.strip_prefix("用户故事核心权威："))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    (!authorities.is_empty()).then(|| authorities.join("\n后续明确修订："))
}

async fn user_story_authority_semantic_issue<R>(
    runtime: &mut R,
    draft: &SessionCreationDraftState,
    supervisor_task_id: Uuid,
    reviewed_verdicts: &mut BTreeMap<String, SemanticReviewFinding>,
) -> anyhow::Result<Option<ContractIssue>>
where
    R: CreationContractRepairRuntime + Send,
{
    let Some(authority) = user_story_authority(draft) else {
        return Ok(None);
    };
    let effective_draft = creation_draft_with_pending_contract_applied(draft);
    let contract = strong_novel_contract_from_creation_draft(&effective_draft);
    let Some(request) = user_story_authority_review_request(&authority, &contract) else {
        return Ok(None);
    };
    let fingerprint = request.fingerprint();
    let finding = if let Some(finding) = reviewed_verdicts.get(&fingerprint).cloned() {
        finding
    } else {
        runtime
            .record_creation_contract_checkpoint(
                supervisor_task_id,
                "creation_contract:user_story_authority_review",
                Some(
                    "合同结构已完整，继续核对故事前提、主线因果和终局是否保留用户最初故事核心"
                        .to_string(),
                ),
            )
            .await?;
        let raw_verdict = runtime
            .generate_creation_contract_repair_text(
                supervisor_task_id,
                "creation_contract:user_story_authority_review_failed",
                &request.prompt(),
                Some(512),
            )
            .await?;
        let finding = match raw_verdict {
            Some(raw_verdict) => parse_semantic_review_finding(&raw_verdict),
            None => SemanticReviewFinding {
                verdict: SemanticReviewVerdict::Uncertain,
                rationale: String::new(),
                evidence: None,
            },
        };
        let finding = request.ground_finding(finding);
        reviewed_verdicts.insert(fingerprint, finding.clone());
        finding
    };
    let issue_kind = finding
        .evidence
        .as_ref()
        .map(|evidence| super::issue::user_story_semantic_issue_kind(&evidence.candidate_field))
        .unwrap_or(ContractIssueKind::Skeleton);
    Ok(semantic_authority_conflict_issue(
        &finding,
        "semantic.user_story_authority",
        issue_kind,
        "ContractBlocker[semantic.user_story_authority]: 故事前提、总主线因果、终局或大纲偏离用户故事核心权威，或含明显错字、词序损坏、截断和不可读拼接；必须按用户原始核心重写这些字段并同步大纲",
    ))
}

fn semantic_review_rationale_suffix(rationale: &str) -> String {
    let rationale = rationale.trim();
    if rationale.is_empty() {
        String::new()
    } else {
        format!("；裁判依据：{rationale}")
    }
}

fn semantic_authority_conflict_issue(
    finding: &SemanticReviewFinding,
    code: &str,
    kind: ContractIssueKind,
    conflict_message: &str,
) -> Option<ContractIssue> {
    let evidence = finding
        .evidence
        .as_ref()
        .filter(|evidence| evidence.is_exact());
    (matches!(finding.verdict, SemanticReviewVerdict::Conflict) && evidence.is_some()).then(|| {
        let evidence = evidence.expect("checked exact semantic conflict evidence");
        let text = format!(
            "{conflict_message}{}；权威证据 {}=`{}`；候选证据 {}=`{}`",
            semantic_review_rationale_suffix(&finding.rationale),
            evidence.authority_field.trim(),
            evidence.authority_quote.trim(),
            evidence.candidate_field.trim(),
            evidence.candidate_quote.trim(),
        );
        ContractIssue::new(
            code,
            kind,
            ContractIssueEvidence::new(
                evidence.candidate_field.trim(),
                format!(
                    "{} <> {}={}",
                    evidence.authority_quote.trim(),
                    evidence.candidate_field.trim(),
                    evidence.candidate_quote.trim()
                ),
            ),
            text,
        )
    })
}

async fn outline_character_authority_semantic_issue<R>(
    runtime: &mut R,
    draft: &SessionCreationDraftState,
    supervisor_task_id: Uuid,
    reviewed_verdicts: &mut BTreeMap<String, SemanticReviewFinding>,
) -> anyhow::Result<Option<ContractIssue>>
where
    R: CreationContractRepairRuntime + Send,
{
    let effective_draft = creation_draft_with_pending_contract_applied(draft);
    let contract = strong_novel_contract_from_creation_draft(&effective_draft);
    let Some(request) = outline_character_authority_review_request(&contract) else {
        return Ok(None);
    };
    let fingerprint = request.fingerprint();
    let finding = if let Some(finding) = reviewed_verdicts.get(&fingerprint).cloned() {
        finding
    } else {
        runtime
            .record_creation_contract_checkpoint(
                supervisor_task_id,
                "creation_contract:outline_character_authority_review",
                Some(
                    "合同结构已完整，继续核对角色权威、世界规则和终局是否在大纲与兑现矩阵中保持一致"
                        .to_string(),
                ),
            )
            .await?;
        let raw_verdict = runtime
            .generate_creation_contract_repair_text(
                supervisor_task_id,
                "creation_contract:outline_character_authority_review_failed",
                &request.prompt(),
                Some(512),
            )
            .await?;
        let finding = match raw_verdict {
            Some(raw_verdict) => parse_semantic_review_finding(&raw_verdict),
            None => SemanticReviewFinding {
                verdict: SemanticReviewVerdict::Uncertain,
                rationale: String::new(),
                evidence: None,
            },
        };
        let finding = request.ground_finding(finding);
        reviewed_verdicts.insert(fingerprint, finding.clone());
        finding
    };
    let issue_kind = finding
        .evidence
        .as_ref()
        .map(|evidence| super::issue::user_story_semantic_issue_kind(&evidence.candidate_field))
        .unwrap_or(ContractIssueKind::Plot);
    Ok(semantic_authority_conflict_issue(
        &finding,
        "semantic.outline_character_authority",
        issue_kind,
        "ContractBlocker[semantic.outline_character_authority]: 小说合同核心字段、大纲或兑现矩阵存在名称、角色权威、能力边界、事件语义或语言完整性冲突；必须按书名、角色权威表、世界规则和终局重写被点名的候选字段",
    ))
}

async fn reopen_draft_if_semantics_block<R>(
    runtime: &mut R,
    session_id: &str,
    draft: &mut SessionCreationDraftState,
    supervisor_task_id: Uuid,
    reviewed_verdicts: &mut BTreeMap<String, SemanticReviewFinding>,
) -> anyhow::Result<Option<ContractIssueList>>
where
    R: CreationContractRepairRuntime + Send,
{
    let mut review_draft = runtime
        .load_draft(session_id)
        .await?
        .unwrap_or_else(|| draft.clone());
    let pending_revisions = pending_explicit_contract_revision_findings(&review_draft);
    if !pending_revisions.is_empty() {
        review_draft.set_lifecycle_status(CreationDraftLifecycleStatus::DraftingContract);
        runtime.save_draft(&review_draft).await?;
        *draft = review_draft;
        return Ok(Some(pending_revisions));
    }
    let issue = if let Some(issue) = user_story_authority_semantic_issue(
        runtime,
        &review_draft,
        supervisor_task_id,
        reviewed_verdicts,
    )
    .await?
    {
        Some(ContractIssueList::from_issue(issue))
    } else {
        outline_character_authority_semantic_issue(
            runtime,
            &review_draft,
            supervisor_task_id,
            reviewed_verdicts,
        )
        .await?
        .map(ContractIssueList::from_issue)
    };
    let Some(issue) = issue else {
        *draft = review_draft;
        return Ok(None);
    };
    review_draft.set_lifecycle_status(super::CreationDraftLifecycleStatus::DraftingContract);
    runtime.save_draft(&review_draft).await?;
    *draft = review_draft;
    Ok(Some(issue))
}

async fn try_resolve_creation_contract_semantics<R>(
    runtime: &mut R,
    session_id: &str,
    current_outcome: &ChatOutcome,
    current_draft: &SessionCreationDraftState,
    supervisor_task_id: Uuid,
    reviewed_verdicts: &mut BTreeMap<String, SemanticReviewFinding>,
) -> anyhow::Result<Option<MetadataRepairAttempt>>
where
    R: CreationContractRepairRuntime + Send,
{
    let effective_draft = creation_draft_with_pending_contract_applied(current_draft);
    let contract = strong_novel_contract_from_creation_draft(&effective_draft);
    let Some(request) = ending_equivalence_review_request(&contract) else {
        return Ok(None);
    };
    let fingerprint = request.fingerprint();
    let finding = if let Some(finding) = reviewed_verdicts.get(&fingerprint).cloned() {
        finding
    } else {
        runtime
            .record_creation_contract_checkpoint(
                supervisor_task_id,
                "creation_contract:semantic_review",
                Some("大纲显式结局与已锁定终局文字不同，执行一次语义一致性裁判".to_string()),
            )
            .await?;
        let Some(raw_verdict) = runtime
            .generate_creation_contract_repair_text(
                supervisor_task_id,
                "creation_contract:semantic_review_failed",
                &request.prompt(),
                Some(512),
            )
            .await?
        else {
            return Ok(None);
        };
        let finding = request.ground_finding(parse_semantic_review_finding(&raw_verdict));
        reviewed_verdicts.insert(fingerprint, finding.clone());
        finding
    };

    match finding.verdict {
        SemanticReviewVerdict::Equivalent => {
            let patch = request.canonicalizing_plot_patch();
            let Some((render_draft, submission)) =
                submit_session_creation_contract_candidate(runtime, session_id, &patch).await?
            else {
                return Ok(None);
            };
            let mut repaired = current_outcome.clone();
            if submission.is_ready() {
                repaired.response =
                    stabilize_creation_contract_user_response(&render_draft, &repaired.response);
                return Ok(Some(MetadataRepairAttempt {
                    outcome: repaired,
                    draft: render_draft,
                    issues: ContractIssueList::default(),
                    ready: true,
                }));
            }
            let issues = next_creation_contract_repair_issues(&render_draft, &submission);
            Ok(Some(MetadataRepairAttempt {
                outcome: repaired,
                draft: render_draft,
                issues,
                ready: false,
            }))
        }
        SemanticReviewVerdict::Conflict => {
            runtime
                .record_creation_contract_checkpoint(
                    supervisor_task_id,
                    "creation_contract:semantic_conflict",
                    Some(
                        "语义裁判确认大纲结局与权威终局冲突；保留权威终局，交给 Plot typed patch 修正大纲"
                            .to_string(),
                    ),
                )
                .await?;
            Ok(None)
        }
        SemanticReviewVerdict::Uncertain => {
            runtime
                .record_creation_contract_checkpoint(
                    supervisor_task_id,
                    "creation_contract:semantic_uncertain",
                    Some(
                        "语义裁判无法确认两份结局等价；不修改权威状态，交给 Plot typed patch 重新明确大纲结局"
                            .to_string(),
                    ),
                )
                .await?;
            Ok(None)
        }
    }
}

async fn try_repair_creation_contract_locally<R>(
    runtime: &mut R,
    session_id: &str,
    current_outcome: &ChatOutcome,
    current_issues: &ContractIssueList,
    supervisor_task_id: Uuid,
) -> anyhow::Result<Option<MetadataRepairAttempt>>
where
    R: CreationContractRepairRuntime + Send,
{
    if creation_contract_issues_require_semantic_stage(current_issues) {
        return Ok(None);
    }
    let Some((render_draft, repaired_submission)) =
        repair_session_contract_metadata_locally(runtime, session_id).await?
    else {
        return Ok(None);
    };
    let mut repaired = current_outcome.clone();
    if repaired_submission.is_ready() {
        repaired.response =
            stabilize_creation_contract_user_response(&render_draft, &repaired.response);
        if let Some(run_trace) = repaired.run_trace.as_mut() {
            run_trace.metadata.insert(
                "creation_contract_local_repair_ready".to_string(),
                "true".to_string(),
            );
        }
        runtime
            .record_creation_contract_checkpoint(
                supervisor_task_id,
                "creation_contract:local_repair",
                Some("合同草案中的可确定字段已由写作工具本地修复并通过质量门".to_string()),
            )
            .await?;
        return Ok(Some(MetadataRepairAttempt {
            outcome: repaired,
            draft: render_draft,
            issues: ContractIssueList::default(),
            ready: true,
        }));
    }

    let issues = next_creation_contract_repair_issues(&render_draft, &repaired_submission);
    runtime
        .record_creation_contract_checkpoint(
            supervisor_task_id,
            "creation_contract:local_repair_partial",
            Some(format!(
                "合同草案中的可确定字段已先做本地修复，剩余问题继续交给分段补齐：{}",
                issues.join("；")
            )),
        )
        .await?;
    Ok(Some(MetadataRepairAttempt {
        outcome: repaired,
        draft: render_draft,
        issues,
        ready: false,
    }))
}

async fn try_repair_creation_contract_title_metadata<R>(
    runtime: &mut R,
    session_id: &str,
    current_outcome: &ChatOutcome,
    current_draft: &SessionCreationDraftState,
    current_issues: &ContractIssueList,
    supervisor_task_id: Uuid,
) -> anyhow::Result<Option<MetadataRepairAttempt>>
where
    R: CreationContractRepairRuntime + Send,
{
    let current_issue_messages = current_issues.messages();
    if !typed_creation_contract_issues_contain_title_metadata(current_issues) {
        return Ok(None);
    }
    let Some(repair_prompt) =
        final_prompt_from_title_metadata_repair(current_draft, &current_issue_messages)
    else {
        return Ok(None);
    };
    runtime
        .record_creation_contract_checkpoint(
            supervisor_task_id,
            "creation_contract:title_metadata_repair",
            Some(format!(
                "合同草案包含书名 metadata 问题，优先执行局部修复：{}",
                current_issues.join("；")
            )),
        )
        .await?;
    let Some(repaired_text) = runtime
        .generate_creation_contract_repair_text(
            supervisor_task_id,
            "creation_contract:title_metadata_repair_failed",
            &repair_prompt,
            Some(1024),
        )
        .await?
    else {
        return Ok(None);
    };
    let mut repaired = current_outcome.clone();
    repaired.response = repaired_text;
    let Some((render_draft, repaired_submission)) =
        submit_session_title_metadata_repair_candidate(runtime, session_id, &repaired.response)
            .await?
    else {
        return Ok(Some(MetadataRepairAttempt {
            outcome: repaired,
            draft: current_draft.clone(),
            issues: ContractIssueList::single(
                "contract.title.metadata_output",
                ContractIssueKind::Skeleton,
                "title",
                "书名 metadata 修复输出没有形成可审查候选",
            ),
            ready: false,
        }));
    };
    if repaired_submission.is_ready() {
        repaired.response =
            stabilize_creation_contract_user_response(&render_draft, &repaired.response);
        if let Some(run_trace) = repaired.run_trace.as_mut() {
            run_trace.metadata.insert(
                "creation_contract_title_metadata_repaired".to_string(),
                "true".to_string(),
            );
        }
        return Ok(Some(MetadataRepairAttempt {
            outcome: repaired,
            draft: render_draft,
            issues: ContractIssueList::default(),
            ready: true,
        }));
    }
    let issues = next_creation_contract_repair_issues(&render_draft, &repaired_submission);
    Ok(Some(MetadataRepairAttempt {
        outcome: repaired,
        draft: render_draft,
        issues,
        ready: false,
    }))
}

async fn try_repair_creation_contract_metadata<R>(
    runtime: &mut R,
    session_id: &str,
    current_outcome: &ChatOutcome,
    current_draft: &SessionCreationDraftState,
    current_issues: &ContractIssueList,
    supervisor_task_id: Uuid,
) -> anyhow::Result<ModelPatchAttempt>
where
    R: CreationContractRepairRuntime + Send,
{
    if creation_contract_issues_require_semantic_stage(current_issues) {
        return Ok(ModelPatchAttempt::NotApplicable);
    }
    if should_prioritize_title_metadata_repair(current_draft, current_issues) {
        return Ok(
            match try_repair_creation_contract_title_metadata(
                runtime,
                session_id,
                current_outcome,
                current_draft,
                current_issues,
                supervisor_task_id,
            )
            .await?
            {
                Some(attempt) => ModelPatchAttempt::Candidate(attempt),
                None => ModelPatchAttempt::NoOutput,
            },
        );
    }
    let current_issue_messages = current_issues.messages();
    if !creation_contract_issues_are_contract_metadata_only(&current_issue_messages) {
        return Ok(ModelPatchAttempt::NotApplicable);
    }
    let Some(repair_prompt) =
        final_prompt_from_contract_metadata_repair(current_draft, &current_issue_messages)
    else {
        return Ok(ModelPatchAttempt::NotApplicable);
    };
    runtime
        .record_creation_contract_checkpoint(
            supervisor_task_id,
            "creation_contract:metadata_repair",
            Some(format!(
                "合同草案只剩局部 metadata 问题，执行候选补丁修复：{}",
                current_issues.join("；")
            )),
        )
        .await?;
    let Some(repaired_text) = runtime
        .generate_creation_contract_repair_text(
            supervisor_task_id,
            "creation_contract:metadata_repair_failed",
            &repair_prompt,
            Some(1024),
        )
        .await?
    else {
        return Ok(ModelPatchAttempt::NoOutput);
    };
    let mut repaired = current_outcome.clone();
    repaired.response = repaired_text;
    let Some((render_draft, repaired_submission)) =
        submit_session_contract_metadata_repair_candidate(runtime, session_id, &repaired.response)
            .await?
    else {
        return Ok(ModelPatchAttempt::Candidate(MetadataRepairAttempt {
            outcome: repaired,
            draft: current_draft.clone(),
            issues: ContractIssueList::single(
                "contract.metadata.output",
                ContractIssueKind::Other,
                "metadata",
                "合同 metadata 修复输出没有形成可审查候选",
            ),
            ready: false,
        }));
    };
    if repaired_submission.is_ready() {
        repaired.response =
            stabilize_creation_contract_user_response(&render_draft, &repaired.response);
        if let Some(run_trace) = repaired.run_trace.as_mut() {
            run_trace.metadata.insert(
                "creation_contract_metadata_repaired".to_string(),
                "true".to_string(),
            );
        }
        return Ok(ModelPatchAttempt::Candidate(MetadataRepairAttempt {
            outcome: repaired,
            draft: render_draft,
            issues: ContractIssueList::default(),
            ready: true,
        }));
    }
    let issues = next_creation_contract_repair_issues(&render_draft, &repaired_submission);
    Ok(ModelPatchAttempt::Candidate(MetadataRepairAttempt {
        outcome: repaired,
        draft: render_draft,
        issues,
        ready: false,
    }))
}

pub async fn maybe_repair_creation_planning_outcome<R>(
    runtime: &mut R,
    session_id: &str,
    creation_planning_dialogue: bool,
    mut outcome: ChatOutcome,
    supervisor_task_id: Uuid,
) -> anyhow::Result<ChatOutcome>
where
    R: CreationContractRepairRuntime + Send,
{
    if !creation_planning_dialogue {
        return Ok(outcome);
    }
    let Some((mut draft, submission)) =
        submit_session_creation_contract_candidate(runtime, session_id, &outcome.response).await?
    else {
        return Ok(outcome);
    };
    let mut reviewed_semantic_verdicts = BTreeMap::new();
    let mut initial_semantic_issue = None;
    if submission.is_ready() {
        if let Some(issue) = reopen_draft_if_semantics_block(
            runtime,
            session_id,
            &mut draft,
            supervisor_task_id,
            &mut reviewed_semantic_verdicts,
        )
        .await?
        {
            runtime
                .record_creation_contract_checkpoint(
                    supervisor_task_id,
                    "creation_contract:user_story_authority_conflict",
                    Some(issue.join("；")),
                )
                .await?;
            initial_semantic_issue = Some(issue);
        } else {
            outcome.response = stabilize_creation_contract_user_response(&draft, &outcome.response);
            return Ok(outcome);
        }
    }

    let mut current_outcome = outcome;
    let mut current_draft = draft;
    let mut current_issues = if let Some(issue) = initial_semantic_issue {
        issue
    } else {
        next_creation_contract_repair_issues(&current_draft, &submission)
    };
    if current_issues.is_empty() && creation_contract_draft_is_confirmable(&current_draft) {
        if let Some(issue) = reopen_draft_if_semantics_block(
            runtime,
            session_id,
            &mut current_draft,
            supervisor_task_id,
            &mut reviewed_semantic_verdicts,
        )
        .await?
        {
            current_issues = issue;
        } else {
            current_outcome.response = stabilize_creation_contract_user_response(
                &current_draft,
                &current_outcome.response,
            );
            return Ok(current_outcome);
        }
    }
    if creation_contract_response_is_runtime_blocker(&current_outcome.response) {
        runtime
            .record_creation_contract_checkpoint(
                supervisor_task_id,
                "creation_contract:runtime_blocker",
                Some(
                    "合同生成遇到模型运行时阻塞，停止自动修复，避免把超时/断线当成可修复合同继续等待"
                        .to_string(),
                ),
            )
            .await?;
        current_outcome.response =
            render_creation_contract_runtime_blocker(&[current_outcome.response.clone()]);
        return Ok(current_outcome);
    }
    let mut model_patch_budget = ContractModelPatchBudget::default();
    let mut local_repair_attempted = false;
    let mut exhausted_stages = Vec::<ContractCompletionStage>::new();
    while model_patch_budget.can_attempt() {
        if let Some(semantic_repair) = try_resolve_creation_contract_semantics(
            runtime,
            session_id,
            &current_outcome,
            &current_draft,
            supervisor_task_id,
            &mut reviewed_semantic_verdicts,
        )
        .await?
        {
            current_outcome = semantic_repair.outcome;
            current_draft = semantic_repair.draft;
            current_issues = semantic_repair.issues;
            if semantic_repair.ready {
                if let Some(issue) = reopen_draft_if_semantics_block(
                    runtime,
                    session_id,
                    &mut current_draft,
                    supervisor_task_id,
                    &mut reviewed_semantic_verdicts,
                )
                .await?
                {
                    current_issues = issue;
                } else {
                    return Ok(current_outcome);
                }
            } else {
                continue;
            }
        }
        if !local_repair_attempted {
            local_repair_attempted = true;
            if let Some(local_repair) = try_repair_creation_contract_locally(
                runtime,
                session_id,
                &current_outcome,
                &current_issues,
                supervisor_task_id,
            )
            .await?
            {
                current_outcome = local_repair.outcome;
                current_draft = local_repair.draft;
                current_issues = local_repair.issues;
                if local_repair.ready {
                    if let Some(issue) = reopen_draft_if_semantics_block(
                        runtime,
                        session_id,
                        &mut current_draft,
                        supervisor_task_id,
                        &mut reviewed_semantic_verdicts,
                    )
                    .await?
                    {
                        current_issues = issue;
                    } else {
                        return Ok(current_outcome);
                    }
                }
            }
        }

        let before_patch = ContractRepairProgressSnapshot::new(&current_draft, &current_issues);
        match try_repair_creation_contract_metadata(
            runtime,
            session_id,
            &current_outcome,
            &current_draft,
            &current_issues,
            supervisor_task_id,
        )
        .await?
        {
            ModelPatchAttempt::NotApplicable => {}
            ModelPatchAttempt::NoOutput => {
                model_patch_budget.record(false);
                runtime
                    .record_creation_contract_checkpoint(
                        supervisor_task_id,
                        "creation_contract:model_patch_no_output",
                        Some(
                            "合同模型补丁没有返回候选；本轮未产生净提升，停止继续调用同一模型"
                                .to_string(),
                        ),
                    )
                    .await?;
                break;
            }
            ModelPatchAttempt::Candidate(metadata_repair) => {
                current_outcome = metadata_repair.outcome;
                current_draft = metadata_repair.draft;
                current_issues = metadata_repair.issues;
                let progressed =
                    ContractRepairProgressSnapshot::new(&current_draft, &current_issues)
                        .improves_on(&before_patch);
                model_patch_budget.record(progressed);
                if metadata_repair.ready {
                    if let Some(issue) = reopen_draft_if_semantics_block(
                        runtime,
                        session_id,
                        &mut current_draft,
                        supervisor_task_id,
                        &mut reviewed_semantic_verdicts,
                    )
                    .await?
                    {
                        current_issues = issue;
                    } else {
                        return Ok(current_outcome);
                    }
                }
                if !progressed {
                    runtime
                        .record_creation_contract_checkpoint(
                            supervisor_task_id,
                            "creation_contract:model_patch_no_progress",
                            Some(format!(
                                "合同模型补丁没有减少 blocker 或补齐关键字段，保留当前最佳候选并停止：{}",
                                current_issues.join("；")
                            )),
                        )
                        .await?;
                    break;
                }
                if model_patch_budget.can_attempt() {
                    continue;
                } else {
                    break;
                }
            }
        }

        let Some(current_stage) = select_contract_completion_stage_excluding(
            &current_draft,
            &current_issues,
            &exhausted_stages,
        ) else {
            runtime
                .record_creation_contract_checkpoint(
                    supervisor_task_id,
                    "creation_contract:auto_repair_exhausted",
                    Some(format!(
                        "有限合同补丁没有可继续处理的 typed stage，保留当前最佳候选：{}",
                        current_issues.join("；")
                    )),
                )
                .await?;
            break;
        };
        runtime
            .record_creation_contract_checkpoint(
                supervisor_task_id,
                "creation_contract:auto_repair",
                Some(format!(
                    "合同草案未通过质量门，按 {current_stage:?} 阶段执行第 {} 次有限模型补丁：{}",
                    model_patch_budget.next_attempt_number(),
                    current_issues.join("；")
                )),
            )
            .await?;
        let repair_prompt = final_prompt_from_staged_contract_completion_stage(
            &current_draft,
            &current_draft.brief,
            &current_issues,
            current_stage,
        );
        match runtime
            .generate_creation_contract_repair_text(
                supervisor_task_id,
                "creation_contract:auto_repair_failed",
                &repair_prompt,
                Some(contract_completion_stage_output_budget_for_issues(
                    current_stage,
                    &current_issues,
                )),
            )
            .await
        {
            Ok(Some(repaired_text)) => {
                let before_patch =
                    ContractRepairProgressSnapshot::new(&current_draft, &current_issues);
                let mut repaired = current_outcome.clone();
                repaired.response = repaired_text;
                let Some((render_draft, repaired_submission)) =
                    submit_session_creation_contract_candidate(
                        runtime,
                        session_id,
                        &repaired.response,
                    )
                    .await?
                else {
                    current_outcome = repaired;
                    current_issues.set_scope(
                        "contract.patch_output",
                        ContractIssueKind::Diagnostic,
                        "model_output",
                    );
                    current_issues.push("合同修复输出没有形成可审查合同候选");
                    current_issues.sort_dedup();
                    model_patch_budget.record(false);
                    break;
                };
                if repaired_submission.is_ready() {
                    let mut render_draft = render_draft;
                    model_patch_budget.record(true);
                    if let Some(issue) = reopen_draft_if_semantics_block(
                        runtime,
                        session_id,
                        &mut render_draft,
                        supervisor_task_id,
                        &mut reviewed_semantic_verdicts,
                    )
                    .await?
                    {
                        current_outcome = repaired;
                        current_draft = render_draft;
                        current_issues = issue;
                        if model_patch_budget.can_attempt() {
                            continue;
                        }
                        break;
                    }
                    repaired.response = stabilize_creation_contract_user_response(
                        &render_draft,
                        &repaired.response,
                    );
                    if let Some(run_trace) = repaired.run_trace.as_mut() {
                        run_trace.metadata.insert(
                            "creation_contract_auto_repaired".to_string(),
                            "true".to_string(),
                        );
                    }
                    return Ok(repaired);
                }
                current_outcome = repaired;
                current_draft = render_draft;
                current_issues = next_stage_creation_contract_repair_issues(
                    &current_issues,
                    &current_draft,
                    &repaired_submission,
                );
                if current_issues.is_empty()
                    && creation_contract_draft_is_confirmable(&current_draft)
                {
                    if let Some(issue) = reopen_draft_if_semantics_block(
                        runtime,
                        session_id,
                        &mut current_draft,
                        supervisor_task_id,
                        &mut reviewed_semantic_verdicts,
                    )
                    .await?
                    {
                        current_issues = issue;
                        model_patch_budget.record(true);
                        if model_patch_budget.can_attempt() {
                            continue;
                        }
                        break;
                    }
                    model_patch_budget.record(true);
                    current_outcome.response = stabilize_creation_contract_user_response(
                        &current_draft,
                        &current_outcome.response,
                    );
                    return Ok(current_outcome);
                }
                let progressed =
                    ContractRepairProgressSnapshot::new(&current_draft, &current_issues)
                        .improves_on(&before_patch);
                model_patch_budget.record(progressed);
                if !progressed {
                    let submission_diagnostics = repaired_submission.gate.actionable_issues();
                    let current_issue_messages = current_issues.messages();
                    let submission_diagnostic_suffix = contract_repair_submission_diagnostic_suffix(
                        &current_issue_messages,
                        &submission_diagnostics,
                    );
                    runtime
                        .record_creation_contract_checkpoint(
                            supervisor_task_id,
                            "creation_contract:auto_repair_no_progress",
                            Some(format!(
                                "{current_stage:?} 阶段没有产生新的结构化进展，保留当前最佳候选并停止：{}{}",
                                current_issues.join("；"),
                                submission_diagnostic_suffix
                            )),
                        )
                        .await?;
                    push_exhausted_contract_stage(&mut exhausted_stages, current_stage);
                    break;
                }
                exhausted_stages.clear();
                if !model_patch_budget.can_attempt() {
                    break;
                }
            }
            Ok(None) => {
                model_patch_budget.record(false);
                push_exhausted_contract_stage(&mut exhausted_stages, current_stage);
                break;
            }
            Err(error) => {
                let reason = error.to_string();
                if creation_contract_response_is_runtime_blocker(&reason) {
                    runtime
                        .record_creation_contract_checkpoint(
                            supervisor_task_id,
                            "creation_contract:runtime_blocker",
                            Some(
                                "合同自动补齐遇到模型运行时阻塞，暂停任务并交给外层 runtime 恢复链路"
                                    .to_string(),
                            ),
                        )
                        .await?;
                    current_outcome.response =
                        render_creation_contract_runtime_blocker(&[reason.to_string()]);
                    return Ok(current_outcome);
                }
                return Err(error);
            }
        }
    }

    creation_contract_quality_blocked_outcome(
        runtime,
        session_id,
        current_outcome,
        supervisor_task_id,
        &current_issues,
        &mut reviewed_semantic_verdicts,
    )
    .await
}

fn next_creation_contract_repair_issues(
    draft: &SessionCreationDraftState,
    submission: &ContractSubmissionOutcome,
) -> ContractIssueList {
    let effective_draft = creation_draft_with_pending_contract_applied(draft);
    let mut recomputed = creation_draft_contract_blocking_findings_for_scope(
        &effective_draft,
        ContractReadinessScope::LockedAuthorityContract,
    );
    for issue in submission.gate.actionable_issues() {
        if creation_contract_issue_is_patch_scope_noise(&issue) {
            recomputed.push_issue(ContractIssue::new(
                "contract.patch_scope",
                ContractIssueKind::Diagnostic,
                ContractIssueEvidence::new("typed_patch", issue.clone()),
                issue,
            ));
        } else if recomputed.is_empty() && !submission.is_ready() {
            recomputed.push_issue(ContractIssue::new(
                "contract.candidate_boundary",
                ContractIssueKind::Other,
                ContractIssueEvidence::new("candidate", issue.clone()),
                issue,
            ));
        }
    }
    recomputed.sort_dedup();
    if submission.is_ready() || recomputed.iter().any(|issue| !issue.kind.is_diagnostic()) {
        recomputed
    } else {
        ContractIssueList::single(
            "contract.readiness_unresolved",
            ContractIssueKind::Other,
            "contract",
            "合同候选已合并，但仍未达到可确认状态；需要继续补齐缺失的结构化合同字段",
        )
    }
}

fn next_stage_creation_contract_repair_issues(
    unresolved_issues: &ContractIssueList,
    draft: &SessionCreationDraftState,
    submission: &ContractSubmissionOutcome,
) -> ContractIssueList {
    let mut next = next_creation_contract_repair_issues(draft, submission);
    let submission_issues = submission.gate.actionable_issues();
    let patch_was_rejected = !submission.committed
        && !submission_issues.is_empty()
        && submission_issues
            .iter()
            .all(|issue| creation_contract_issue_is_patch_scope_noise(issue));
    if patch_was_rejected && !unresolved_issues.is_empty() {
        next.retain(|issue| issue.code != "contract.readiness_unresolved");
        next.extend_findings(unresolved_issues.iter().cloned());
        next.sort_dedup();
    }
    next
}

fn creation_contract_issues_require_semantic_stage(issues: &ContractIssueList) -> bool {
    issues
        .iter()
        .any(|issue| issue.code.starts_with("semantic."))
}

fn creation_contract_issue_is_patch_scope_noise(issue: &str) -> bool {
    issue.contains("typed patch 作用域校验未通过")
        || issue.contains("character_patch ")
        || issue.contains("skeleton_patch ")
        || issue.contains("plot_patch ")
        || issue.contains("governance_patch ")
        || issue.contains("metadata_patch ")
}

fn contract_repair_submission_diagnostic_suffix(
    authoritative_issues: &[String],
    submission_issues: &[String],
) -> String {
    let mut diagnostics = submission_issues
        .iter()
        .filter(|issue| !authoritative_issues.contains(issue))
        .cloned()
        .collect::<Vec<_>>();
    diagnostics.sort();
    diagnostics.dedup();
    if diagnostics.is_empty() {
        String::new()
    } else {
        format!("；本轮补丁反馈：{}", diagnostics.join("；"))
    }
}

fn push_exhausted_contract_stage(
    exhausted_stages: &mut Vec<ContractCompletionStage>,
    stage: ContractCompletionStage,
) {
    if !exhausted_stages.contains(&stage) {
        exhausted_stages.push(stage);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ContractRepairProgressSnapshot {
    fingerprint: String,
    issue_count: usize,
    issue_penalty: i64,
    filled_score: usize,
}

impl ContractRepairProgressSnapshot {
    const MIN_FILLED_SCORE_PROGRESS_DELTA: usize = 2;

    fn new(draft: &SessionCreationDraftState, issues: &ContractIssueList) -> Self {
        let mut issue_keys = issues
            .iter()
            .map(|issue| (issue.kind, issue.code.clone(), issue.evidence.clone()))
            .collect::<Vec<_>>();
        issue_keys.sort();
        issue_keys.dedup();
        Self {
            fingerprint: creation_contract_repair_progress_fingerprint(draft, issues),
            issue_count: issue_keys.len(),
            issue_penalty: creation_contract_repair_issue_penalty(issues),
            filled_score: creation_contract_repair_filled_score(draft),
        }
    }

    fn improves_on(&self, previous: &Self) -> bool {
        self.issue_count < previous.issue_count
            || self.issue_penalty < previous.issue_penalty
            || self.filled_score.saturating_sub(previous.filled_score)
                >= Self::MIN_FILLED_SCORE_PROGRESS_DELTA
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct ContractModelPatchBudget {
    completed_attempts: usize,
    second_attempt_allowed: bool,
}

impl ContractModelPatchBudget {
    const ABSOLUTE_MAX_ATTEMPTS: usize = 2;

    fn can_attempt(&self) -> bool {
        self.completed_attempts == 0
            || (self.completed_attempts == 1 && self.second_attempt_allowed)
    }

    fn next_attempt_number(&self) -> usize {
        self.completed_attempts + 1
    }

    fn record(&mut self, net_improvement: bool) {
        debug_assert!(self.can_attempt());
        self.completed_attempts = (self.completed_attempts + 1).min(Self::ABSOLUTE_MAX_ATTEMPTS);
        self.second_attempt_allowed = self.completed_attempts == 1 && net_improvement;
    }
}

fn creation_contract_repair_filled_score(draft: &SessionCreationDraftState) -> usize {
    let effective = creation_draft_with_pending_contract_applied(draft);
    let contract_v2 = effective.contract_v2();
    let mut score = 0usize;
    score += filled_text_score(&effective.title, 3);
    score += filled_text_score(&effective.fiction_title_rationale, 3);
    score += filled_text_score(&effective.fiction_premise, 4);
    score += filled_text_score(&effective.fiction_ending_direction, 4);
    score += filled_text_score(&effective.fiction_protagonist_arc, 3);
    score += filled_text_score(&effective.fiction_world_imagery, 3);
    score += filled_text_score(&effective.fiction_main_causal_spine, 4);
    score += filled_list_score(&effective.fiction_themes, 2);
    score += filled_world_rule_list_score(&effective.fiction_world_rules, 3);
    score += filled_list_score(&effective.fiction_style_rules, 2);
    score += filled_list_score(&effective.fiction_must_avoid, 1);
    score += filled_character_lines_score(&effective.fiction_characters, 4);
    score += filled_text_score(&effective.fiction_outline, 4);
    score += filled_text_score(&contract_v2.resource_economy.currency, 1);
    score += filled_text_score(&contract_v2.resource_economy.value_scale, 1);
    score += filled_text_score(&contract_v2.resource_economy.class_impact, 1);
    score += filled_text_score(&contract_v2.emotional_contract.primary_emotion, 2);
    score += filled_text_score(&contract_v2.emotional_contract.emotional_promise, 2);
    score += filled_text_score(&contract_v2.emotional_contract.ending_emotional_state, 2);
    score += filled_text_score(&contract_v2.power_progression.system_name, 2);
    score += contract_v2
        .power_progression
        .anti_power_creep_rules
        .iter()
        .filter(|value| filled_text_score(value, 1) > 0)
        .count()
        .min(3)
        * 2;
    score += filled_text_score(&contract_v2.social_order.rank_system, 1);
    score += filled_text_score(&contract_v2.antagonist_pressure.primary_pressure, 2);
    score += filled_text_score(&contract_v2.narration_contract.pov, 1);
    score += filled_text_score(&contract_v2.narration_contract.dialogue_style, 1);
    score += filled_text_score(&contract_v2.scene_type_mix.balance_rule, 1);
    score += filled_text_score(&contract_v2.reader_promise.core_hook, 3);
    score += filled_text_score(&contract_v2.reader_promise.curiosity_engine, 2);
    score += filled_text_score(&contract_v2.reader_promise.payoff_style, 2);
    score += filled_text_score(
        &contract_v2.chapter_ending_rotation.avoid_repetition_rule,
        1,
    );
    score += filled_text_score(&contract_v2.conflict_pressure_curve.release_strategy, 1);
    score += filled_text_score(&contract_v2.conflict_pressure_curve.peak_policy, 1);
    score += contract_v2.relationship_ledger.len().min(4) * 2;
    score += contract_v2.emotional_state_ledger.len().min(4) * 2;
    score += contract_v2.character_voice_ledger.len().min(4) * 2;
    score += contract_v2.motif_ledger.len().min(4);
    score += contract_v2.reveal_schedule.len().min(4);
    score += contract_v2.relationship_interaction_quotas.len().min(4);
    score += contract_v2
        .payoff_matrix
        .iter()
        .filter(|entry| {
            crate::tool::writing::typed_contract_gate::payoff_matrix_entry_is_complete(entry)
        })
        .count()
        .min(4);
    score
}

fn filled_text_score(value: &str, weight: usize) -> usize {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed == "未指定"
        || trimmed == "待补"
        || crate::tool::writing::surface_sanitizer::contains_generic_contract_placeholder_residue(
            trimmed,
        )
        || crate::tool::writing::surface_sanitizer::contains_legal_contract_residue(trimmed)
        || crate::tool::writing::surface_sanitizer::contains_excessive_repeated_cjk_surface_noise(
            trimmed,
        )
    {
        0
    } else {
        weight
    }
}

fn filled_list_score(values: &[String], weight: usize) -> usize {
    values
        .iter()
        .filter(|value| filled_text_score(value, 1) > 0)
        .count()
        * weight
}

fn filled_world_rule_list_score(values: &[String], weight: usize) -> usize {
    values
        .iter()
        .filter(|value| {
            filled_text_score(value, 1) > 0
                && !crate::tool::writing::typed_contract_gate::world_rule_looks_truncated_or_not_actionable(value)
        })
        .count()
        * weight
}

fn filled_character_lines_score(values: &[String], weight: usize) -> usize {
    values
        .iter()
        .filter(|line| character_line_is_repair_progress(line))
        .count()
        * weight
}

fn character_line_is_repair_progress(line: &str) -> bool {
    if filled_text_score(line, 1) == 0 {
        return false;
    }
    let character = super::draft_contract_bridge::draft_character_line_to_contract(line);
    if value_missing(&character.canonical_name)
        || value_missing(&character.role)
        || value_missing(&character.desire)
        || value_missing(&character.fear)
        || value_missing(&character.bottom_line)
        || value_missing(&character.arc_start)
        || value_missing(&character.arc_end)
    {
        return false;
    }
    let anchors = [
        character.desire.as_str(),
        character.fear.as_str(),
        character.bottom_line.as_str(),
        character.arc_start.as_str(),
        character.arc_end.as_str(),
    ];
    anchors.iter().all(|value| {
        !crate::tool::writing::typed_contract_gate::character_anchor_uses_generic_placeholder(value)
            && !crate::tool::writing::typed_contract_gate::character_anchor_looks_like_storyline_or_truncated_surface(value)
    })
}

fn creation_contract_repair_issue_penalty(issues: &ContractIssueList) -> i64 {
    issues
        .iter()
        .map(|issue| contract_candidate_issue_penalty(issue))
        .sum()
}

fn creation_contract_repair_progress_fingerprint(
    draft: &SessionCreationDraftState,
    issues: &ContractIssueList,
) -> String {
    let mut issue_keys = issues
        .iter()
        .map(|issue| {
            format!(
                "{:?}|{}|{}|{}",
                issue.kind, issue.code, issue.evidence.field, issue.evidence.observed
            )
        })
        .collect::<Vec<_>>();
    issue_keys.sort();
    issue_keys.dedup();
    let current_contract = draft
        .current_contract
        .as_ref()
        .and_then(|value| serde_json::to_string(value).ok())
        .unwrap_or_default();
    let pending_contract = stable_pending_contract_normalized_fingerprint(draft);
    format!(
        "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
        draft.title.trim(),
        draft.fiction_title_rationale.trim(),
        draft.fiction_premise.trim(),
        draft.fiction_ending_direction.trim(),
        draft.fiction_protagonist_arc.trim(),
        draft.fiction_world_imagery.trim(),
        draft.fiction_main_causal_spine.trim(),
        draft.fiction_themes.join("\n").trim(),
        draft.fiction_world_rules.join("\n").trim(),
        draft.fiction_style_rules.join("\n").trim(),
        draft.fiction_must_avoid.join("\n").trim(),
        draft.fiction_characters.join("\n"),
        draft.fiction_outline.trim(),
        current_contract,
        [pending_contract, issue_keys.join("\n")].join("\n")
    )
}

fn stable_pending_contract_normalized_fingerprint(draft: &SessionCreationDraftState) -> String {
    draft
        .pending_contract_candidate
        .as_ref()
        .and_then(|candidate| candidate.get("normalized"))
        .and_then(|value| serde_json::to_string(value).ok())
        .unwrap_or_default()
}

async fn creation_contract_quality_blocked_outcome<R>(
    runtime: &mut R,
    session_id: &str,
    mut outcome: ChatOutcome,
    supervisor_task_id: Uuid,
    issues: &ContractIssueList,
    reviewed_verdicts: &mut BTreeMap<String, SemanticReviewFinding>,
) -> anyhow::Result<ChatOutcome>
where
    R: CreationContractRepairRuntime + Send,
{
    let mut latest_issues = issues.clone();
    if let Some(mut draft) = runtime.load_draft(session_id).await? {
        latest_issues = creation_draft_contract_blocking_findings_for_scope(
            &draft,
            ContractReadinessScope::LockedAuthorityContract,
        );
        if latest_issues.is_empty() || creation_contract_draft_is_confirmable(&draft) {
            if let Some(issue) = reopen_draft_if_semantics_block(
                runtime,
                session_id,
                &mut draft,
                supervisor_task_id,
                reviewed_verdicts,
            )
            .await?
            {
                latest_issues = issue;
            } else {
                runtime
                    .record_creation_contract_checkpoint(
                        supervisor_task_id,
                        "creation_contract:metadata_repairable",
                        Some(format!(
                            "合同草案存在可修复元数据问题，但已进入可确认状态：{}",
                            issues.join("；")
                        )),
                    )
                    .await?;
                outcome.response =
                    stabilize_creation_contract_user_response(&draft, &outcome.response);
                outcome.response.push_str(
                    "\n\n提示：这版合同已可确认；如果你觉得书名、章节名或角色名不满意，可以直接自然语言要求修改。确认后回复“开始写第一章”或“按这个开始”。",
                );
                if let Some(run_trace) = outcome.run_trace.as_mut() {
                    run_trace.metadata.insert(
                        "creation_contract_metadata_repairable".to_string(),
                        "true".to_string(),
                    );
                }
                return Ok(outcome);
            }
        }
        draft.set_lifecycle_status(CreationDraftLifecycleStatus::Blocked);
        record_contract_quality_blocker_diagnostic(&mut draft, &latest_issues.messages());
        runtime.save_draft(&draft).await?;
    }

    runtime
        .record_creation_contract_checkpoint(
            supervisor_task_id,
            "creation_contract:quality_blocked",
            Some(format!(
                "合同草案未通过质量门，不能进入确认/写作：{}",
                latest_issues.join("；")
            )),
        )
        .await?;

    outcome.response = creation_contract_quality_blocked_response(&latest_issues.messages());
    if let Some(run_trace) = outcome.run_trace.as_mut() {
        run_trace.metadata.insert(
            CREATION_CONTRACT_QUALITY_BLOCKED_METADATA_KEY.to_string(),
            "true".to_string(),
        );
    }
    Ok(outcome)
}

fn creation_contract_response_is_runtime_blocker(response: &str) -> bool {
    let lowered = response.to_ascii_lowercase();
    is_recoverable_provider_disconnect(response)
        || lowered.contains("error_kind: llm_stream_timeout")
        || lowered.contains("llm_stream_timeout")
        || lowered.contains("provider_service_unavailable")
        || lowered.contains("provider disconnect")
        || lowered.contains("provider disconnected")
}

fn render_creation_contract_runtime_blocker(issues: &[String]) -> String {
    let issue_text = if issues.is_empty() {
        "模型本轮没有返回可解析的合同内容。".to_string()
    } else {
        issues.join("；")
    };
    let issue_summary = creation_contract_issue_summary(issues);
    let pause_reason = provider_service_pause_reason(&issue_text);
    format!(
        "合同草案还没有生成完成，暂时不能确认开始写作。\n\n\
         当前还需要处理：{issue_summary}。\n\n\
         这次阻塞来自模型运行时：模型输出超时、断线，或没有返回可解析合同。系统已停止自动修复，避免继续长时间等待。\n\n\
         当前诊断：{pause_reason}\n\n\
         你可以直接说“重试生成合同”，或补充更具体的题材、主角、结局方向后再继续。"
    )
}

fn should_prioritize_title_metadata_repair(
    draft: &SessionCreationDraftState,
    issues: &ContractIssueList,
) -> bool {
    if !typed_creation_contract_issues_contain_title_metadata(issues) {
        return false;
    }
    let issue_set = ContractIssueSet::new(issues);
    let non_title_kinds = issue_set
        .iter()
        .filter(|issue| !issue.code.starts_with("contract.title"))
        .map(|issue| issue.kind)
        .collect::<Vec<_>>();
    if non_title_kinds
        .iter()
        .any(|kind| matches!(kind, ContractIssueKind::Skeleton | ContractIssueKind::Plot))
    {
        return false;
    }
    if non_title_kinds
        .iter()
        .any(|kind| matches!(kind, ContractIssueKind::Governance))
    {
        let effective = creation_draft_with_pending_contract_applied(draft);
        return !value_missing(&effective.fiction_premise)
            && !value_missing(&effective.fiction_ending_direction)
            && !value_missing(&effective.fiction_main_causal_spine);
    }
    true
}

fn typed_creation_contract_issues_contain_title_metadata(issues: &ContractIssueList) -> bool {
    issues
        .iter()
        .any(|issue| issue.code.starts_with("contract.title"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_issue(code: &str, kind: ContractIssueKind, text: &str) -> ContractIssue {
        ContractIssue::new(code, kind, ContractIssueEvidence::new("test", text), text)
    }

    fn repair_test_draft() -> SessionCreationDraftState {
        crate::tool::writing::creation_contract::build_initial_creation_draft(
            "repair-progress-test",
            "fiction",
            "写都市玄幻小说，每章2500字，至少5万字",
        )
        .expect("fiction creation draft")
    }

    #[test]
    fn creation_contract_runtime_blocker_detects_stream_timeout() {
        assert!(creation_contract_response_is_runtime_blocker(
            "status: blocked\nerror_kind: llm_stream_timeout\nblockers: model stream did not finish"
        ));
        assert!(creation_contract_response_is_runtime_blocker(
            "Internal error: error sending request for url (http://127.0.0.1/v1/chat/completions)"
        ));
        assert!(!creation_contract_response_is_runtime_blocker(
            "{\"title\":{\"canonical_title\":\"灵城夜火\"}}"
        ));
    }

    #[test]
    fn story_authority_combines_initial_core_and_later_explicit_revisions() {
        let mut draft = repair_test_draft();
        draft.planning_notes = vec![
            "用户故事核心权威：寒门女官追查旧案".to_string(),
            "用户故事核心权威：终局为主动辞官，不是流放".to_string(),
            "普通展示笔记".to_string(),
        ];

        let authority = user_story_authority(&draft).expect("combined authority");

        assert!(authority.contains("寒门女官追查旧案"));
        assert!(authority.contains("后续明确修订：终局为主动辞官，不是流放"));
        assert!(!authority.contains("普通展示笔记"));
    }

    #[test]
    fn only_confirmed_semantic_conflict_blocks_authority() {
        let conflict = SemanticReviewFinding {
            verdict: SemanticReviewVerdict::Conflict,
            rationale: "用户终局是主动辞官，候选终局却写成被流放".to_string(),
            evidence: Some(
                crate::tool::writing::contract_semantic_review::SemanticConflictEvidence {
                    authority_field: "用户故事核心权威".to_string(),
                    authority_quote: "终局为主动辞官".to_string(),
                    candidate_field: "ending.desired_resolution".to_string(),
                    candidate_quote: "最终被流放".to_string(),
                },
            ),
        };
        let uncertain = SemanticReviewFinding {
            verdict: SemanticReviewVerdict::Uncertain,
            rationale: "模型输出无法解析".to_string(),
            evidence: None,
        };
        let equivalent = SemanticReviewFinding {
            verdict: SemanticReviewVerdict::Equivalent,
            rationale: String::new(),
            evidence: None,
        };

        let issue = semantic_authority_conflict_issue(
            &conflict,
            "semantic.user_story_authority",
            ContractIssueKind::Skeleton,
            "ContractBlocker[semantic.user_story_authority]: 权威冲突",
        )
        .expect("confirmed semantic conflict must block");
        assert!(issue.contains("权威冲突"));
        assert!(issue.contains("用户终局是主动辞官"));
        assert!(issue.contains("ending.desired_resolution"));
        assert_eq!(issue.evidence.field, "ending.desired_resolution");
        assert!(semantic_authority_conflict_issue(
            &uncertain,
            "semantic.test",
            ContractIssueKind::Skeleton,
            "unused"
        )
        .is_none());
        assert!(semantic_authority_conflict_issue(
            &equivalent,
            "semantic.test",
            ContractIssueKind::Skeleton,
            "unused"
        )
        .is_none());
    }

    #[test]
    fn mixed_contract_issues_without_story_evidence_do_not_prioritize_title_metadata() {
        let mut issues = ContractIssueList::from_issue(test_issue(
            "contract.world_rules",
            ContractIssueKind::Governance,
            "ContractBlocker: 小说合同缺少世界规则",
        ));
        issues.push_issue(test_issue(
            "contract.title",
            ContractIssueKind::Skeleton,
            "ContractBlocker: 小说合同缺少可锁定书名",
        ));

        assert!(!should_prioritize_title_metadata_repair(
            &repair_test_draft(),
            &issues
        ));
    }

    #[test]
    fn title_and_governance_issues_split_after_story_evidence_is_ready() {
        let mut draft = repair_test_draft();
        draft.fiction_premise = "旧社区面临拆迁，居民被迫共同经营公共食堂".to_string();
        draft.fiction_ending_direction =
            "居民保住社区并建立自治合作社，彼此从陌生人变成真正邻里".to_string();
        draft.fiction_main_causal_spine =
            "拆迁通知迫使居民合作，经营冲突暴露旧矛盾，最终共同对抗违规收购".to_string();
        let mut issues = ContractIssueList::from_issue(test_issue(
            "contract.world_rules",
            ContractIssueKind::Governance,
            "ContractBlocker: 小说合同缺少世界规则",
        ));
        issues.push_issue(test_issue(
            "contract.title",
            ContractIssueKind::Skeleton,
            "ContractBlocker: 小说合同缺少可锁定书名",
        ));

        assert!(should_prioritize_title_metadata_repair(&draft, &issues));
    }

    #[test]
    fn foundational_contract_issues_wait_before_title_metadata_repair() {
        let mut issues = ContractIssueList::from_issue(test_issue(
            "contract.skeleton",
            ContractIssueKind::Skeleton,
            "ContractBlocker: 小说合同缺少主角弧线",
        ));
        issues.push_issue(test_issue(
            "contract.title",
            ContractIssueKind::Skeleton,
            "ContractBlocker: 小说合同缺少可锁定书名",
        ));

        assert!(!should_prioritize_title_metadata_repair(
            &repair_test_draft(),
            &issues
        ));
    }

    #[test]
    fn next_repair_issues_preserve_patch_scope_diagnostics() {
        let draft = repair_test_draft();
        let submission = ContractSubmissionOutcome {
            gate: ContractGateResult {
                status: ContractGateStatus::NeedsRepair,
                blocking_issues: Vec::new(),
                repairable_issues: vec![
                    "typed patch 作用域校验未通过：Characters".to_string(),
                    "character_patch 必须恰好 1 个主角槽位，当前为 0".to_string(),
                ],
                warnings: Vec::new(),
            },
            committed: false,
        };

        let issues = next_creation_contract_repair_issues(&draft, &submission);

        assert!(issues
            .iter()
            .any(|issue| issue.contains("typed patch 作用域校验未通过")));
        assert!(issues
            .iter()
            .any(|issue| issue.contains("character_patch 必须恰好 1 个主角")));
        assert!(issues
            .iter()
            .any(|issue| issue.contains("小说合同角色权威表")));
    }

    #[test]
    fn rejected_stage_patch_does_not_erase_unresolved_semantic_issue() {
        let draft = repair_test_draft();
        let unresolved = ContractIssueList::single(
            "semantic.outline_character_authority",
            ContractIssueKind::Plot,
            "outline",
            "ContractBlocker[semantic.outline_character_authority]: 大纲把对手身份错误归给另一角色",
        );
        let submission = ContractSubmissionOutcome {
            gate: ContractGateResult {
                status: ContractGateStatus::NeedsRepair,
                blocking_issues: Vec::new(),
                repairable_issues: vec![
                    "typed patch 作用域校验未通过：Plot".to_string(),
                    "plot_patch 不能改写角色权威字段".to_string(),
                ],
                warnings: Vec::new(),
            },
            committed: false,
        };

        let issues = next_stage_creation_contract_repair_issues(&unresolved, &draft, &submission);

        assert!(issues
            .iter()
            .any(|issue| issue.contains("semantic.outline_character_authority")));
        assert!(creation_contract_issues_require_semantic_stage(&issues));
    }

    #[test]
    fn next_repair_issues_recompute_retained_normalized_candidate_instead_of_using_stale_text() {
        let mut draft = repair_test_draft();
        draft.pending_contract_candidate = Some(serde_json::json!({
            "normalized": {
                "genre": "生态灾难冒险",
                "world_rules": []
            },
            "issues": [
                "ContractBlocker: 小说合同缺少世界规则",
                "ContractBlocker: 小说合同分卷规划含有结构污染或无效卷名"
            ]
        }));
        let rejected_submission = ContractSubmissionOutcome {
            gate: ContractGateResult {
                status: ContractGateStatus::NeedsRepair,
                blocking_issues: Vec::new(),
                repairable_issues: vec![
                    "ContractBlocker: 小说合同兑现矩阵引用了角色权威表外角色 `错误残片`"
                        .to_string(),
                    "ContractBlocker: 小说合同大纲把主角标成对手".to_string(),
                ],
                warnings: Vec::new(),
            },
            committed: false,
        };

        let issues = next_creation_contract_repair_issues(&draft, &rejected_submission);

        assert!(issues.iter().any(|issue| issue.contains("缺少世界规则")));
        assert!(
            issues.iter().any(|issue| issue.contains("缺少可锁定书名")),
            "the normalized partial candidate must be validated as it exists now: {issues:?}"
        );
        assert!(
            !issues.iter().any(|issue| issue.contains("错误残片")),
            "a worse rejected submission must not replace the retained normalized candidate: {issues:?}"
        );
    }

    #[test]
    fn next_repair_issues_keep_patch_diagnostics_alongside_retained_candidate() {
        let mut draft = repair_test_draft();
        draft.pending_contract_candidate = Some(serde_json::json!({
            "normalized": {
                "genre": "硬科幻",
                "world_rules": []
            },
            "issues": ["ContractBlocker: 小说合同缺少世界规则"]
        }));
        let rejected_submission = ContractSubmissionOutcome {
            gate: ContractGateResult {
                status: ContractGateStatus::NeedsRepair,
                blocking_issues: Vec::new(),
                repairable_issues: vec![
                    "typed patch 作用域校验未通过：Characters".to_string(),
                    "character_patch 角色 陆衡 缺少欲望/恐惧/底线/弧线字段".to_string(),
                ],
                warnings: Vec::new(),
            },
            committed: false,
        };

        let issues = next_creation_contract_repair_issues(&draft, &rejected_submission);

        assert!(issues.iter().any(|issue| issue.contains("缺少世界规则")));
        assert!(issues
            .iter()
            .any(|issue| issue.contains("typed patch 作用域校验未通过")));
        assert!(issues
            .iter()
            .any(|issue| issue.contains("陆衡") && issue.contains("缺少")));
    }

    #[test]
    fn no_progress_checkpoint_preserves_rejected_submission_diagnostics() {
        let authoritative =
            vec!["ContractBlocker: 小说合同缺少分卷/阶段安排或近期章节包".to_string()];
        let submission = vec![
            "ContractBlocker: 小说合同缺少分卷/阶段安排或近期章节包".to_string(),
            "合同输出不能解析为 JSON，也没有形成可归位的合同字段包".to_string(),
        ];

        let suffix = contract_repair_submission_diagnostic_suffix(&authoritative, &submission);

        assert!(suffix.contains("本轮补丁反馈"));
        assert!(suffix.contains("不能解析为 JSON"));
        assert_eq!(
            suffix
                .matches("小说合同缺少分卷/阶段安排或近期章节包")
                .count(),
            0
        );
    }

    #[test]
    fn next_repair_issues_do_not_lose_pending_outline_when_character_authority_is_incomplete() {
        let mut draft = repair_test_draft();
        draft.pending_contract_candidate = Some(serde_json::json!({
            "normalized": {
                "title": {
                    "canonical_title": "回声档案室",
                    "rationale": "书名来自地下档案室保存的异常录音以及终局公开播放录音的行动。",
                    "source": "llm_contract"
                },
                "language": "zh-CN",
                "genre": "现实主义职场悬疑",
                "brief": "档案员追查广播乐团改制前夕的一段封存录音。",
                "target_units": 100000,
                "chapter_unit_target": 2500,
                "max_chapters_per_turn": 1,
                "premise": "档案员在乐团改制前夕发现一段能证明旧选拔被操纵的录音。",
                "ending": {
                    "desired_resolution": "档案员在考核会上公开原始录音并推动建立公开档案制度。",
                    "final_state": "旧式人情管理被可核验的公开制度取代。",
                    "must_resolve": ["发现录音->核验证据->公开录音->建立新制度"]
                },
                "protagonist_arc": "从只负责保存材料的档案员成长为敢于公开证据的监督者。",
                "world_imagery": "地下档案室、盘式录音带、广播塔与空旷排练厅。",
                "main_causal_spine": "改制触发档案清理，异常录音暴露旧案，阻挠迫使主角建立证据链，最终公开录音改变乐团制度。",
                "outline": {
                    "volumes": [{
                        "title": "尘封录音",
                        "objective": "确认录音内容与旧选拔事件的关系",
                        "ending_change": "主角截留原始借阅记录"
                    }],
                    "near_chapters": [{
                        "number": 1,
                        "goal": "在地下档案室修复异常录音带",
                        "expected_turn": "录音中出现指挥干预选拔的对话"
                    }]
                }
            },
            "issues": [
                "ContractBlocker: 小说合同缺少世界规则",
                "ContractBlocker: 小说合同角色权威表缺少明确主角"
            ]
        }));
        let rejected_submission = ContractSubmissionOutcome {
            gate: ContractGateResult {
                status: ContractGateStatus::NeedsRepair,
                blocking_issues: Vec::new(),
                repairable_issues: vec![
                    "ContractBlocker: 小说合同缺少分卷/阶段安排或近期章节包".to_string(),
                    "小说合同尚未形成逐章规划或分卷/阶段大纲".to_string(),
                ],
                warnings: Vec::new(),
            },
            committed: false,
        };

        let issues = next_creation_contract_repair_issues(&draft, &rejected_submission);

        assert!(
            !issues
                .iter()
                .any(|issue| issue.contains("分卷/阶段") || issue.contains("逐章规划")),
            "retained typed outline must remain authoritative even before characters are complete: {issues:?}"
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("角色") || issue.contains("主角")),
            "{issues:?}"
        );
    }

    #[test]
    fn title_only_contract_issues_prioritize_metadata_repair() {
        let issues = ContractIssueList::from_messages(
            "contract.title",
            ContractIssueKind::Skeleton,
            "title",
            vec![
                "ContractBlocker: 小说合同缺少可锁定书名".to_string(),
                "ContractBlocker: 小说合同书名未通过文字完整性和故事依据质量门".to_string(),
            ],
        );

        assert!(should_prioritize_title_metadata_repair(
            &repair_test_draft(),
            &issues
        ));
    }

    #[test]
    fn title_and_character_issues_repair_title_without_rebuilding_story_fields() {
        let mut issues = ContractIssueList::single(
            "contract.title",
            ContractIssueKind::Skeleton,
            "title",
            "ContractBlocker: 盐铁账: 书名像裸制度或账册名词",
        );
        issues.push_issue(test_issue(
            "contract.character_authority",
            ContractIssueKind::Characters,
            "ContractBlocker: 角色 `陶庭野`（同伴）的底线锚点缺少明确边界",
        ));

        assert!(should_prioritize_title_metadata_repair(
            &repair_test_draft(),
            &issues
        ));
    }

    #[test]
    fn contract_repair_snapshot_treats_filled_fields_as_progress() {
        let draft = repair_test_draft();
        let issues = ContractIssueList::single(
            "contract.skeleton",
            ContractIssueKind::Skeleton,
            "premise",
            "ContractBlocker: 小说合同缺少故事前提",
        );
        let before = ContractRepairProgressSnapshot::new(&draft, &issues);

        let mut improved = draft.clone();
        improved.fiction_premise = "底层维修工在灵能城市发现家族旧案与城市命脉相连。".to_string();
        let after = ContractRepairProgressSnapshot::new(&improved, &issues);

        assert!(after.improves_on(&before));
    }

    #[test]
    fn contract_model_patch_budget_stops_after_first_non_improving_attempt() {
        let mut budget = ContractModelPatchBudget::default();
        assert!(budget.can_attempt());
        assert_eq!(budget.next_attempt_number(), 1);

        budget.record(false);

        assert_eq!(budget.completed_attempts, 1);
        assert!(!budget.can_attempt());
    }

    #[test]
    fn contract_model_patch_budget_allows_only_one_follow_up_after_net_improvement() {
        let mut budget = ContractModelPatchBudget::default();
        budget.record(true);
        assert!(budget.can_attempt());
        assert_eq!(budget.next_attempt_number(), 2);

        budget.record(true);

        assert_eq!(
            budget.completed_attempts,
            ContractModelPatchBudget::ABSOLUTE_MAX_ATTEMPTS
        );
        assert!(!budget.can_attempt());
    }
}
