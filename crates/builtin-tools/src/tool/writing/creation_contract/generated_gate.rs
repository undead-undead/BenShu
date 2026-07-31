#[cfg(test)]
use super::boundary_text_gate::generated_contract_boundary_text_issues;
#[cfg(test)]
use super::contract_text::{
    assistant_surface_noise_fragment, chapter_plan_missing_goal_issue,
    chapter_plan_missing_title_issue, count_explicit_chapter_plan_lines,
    fiction_contract_mentions_core_identity, generated_title_is_contract_noise,
    generated_title_reuses_protagonist_name, malformed_chapter_plan_fragment,
    malformed_contract_name_fragment, malformed_goal_like_plan_line_issue,
    malformed_numeric_fragment, normalize_contract_numeric_surface,
};
use super::contract_text::{
    chapter_plan_invalid_title_issue, chapter_plan_title_diversity_issue,
    collect_explicit_chapter_plan_titles,
};
use super::issue::{ContractIssueDisposition, ContractIssueKind, ContractIssueList};
#[cfg(test)]
use super::planning_gate::generated_fiction_contract_planning_issues;
use super::{ContractGateResult, ContractGateStatus, SessionCreationDraftState};
use crate::tool::writing::longform_policy;

#[cfg(test)]
pub fn generated_contract_completion_quality_issues(
    draft: &SessionCreationDraftState,
    contract_text: &str,
) -> Vec<String> {
    let mut issues = generated_contract_quality_issues(draft, contract_text);
    issues.extend(generated_contract_semantic_quality_issues(
        draft,
        contract_text,
        true,
    ));
    issues.sort();
    issues.dedup();
    issues
}

#[cfg(test)]
pub fn generated_contract_quality_issues(
    draft: &SessionCreationDraftState,
    contract_text: &str,
) -> Vec<String> {
    let mut issues = generated_contract_boundary_text_issues(draft, contract_text);
    issues.extend(generated_contract_semantic_quality_issues(
        draft,
        contract_text,
        false,
    ));
    issues.sort();
    issues.dedup();
    issues
}

#[cfg(test)]
pub fn generated_contract_gate_result(
    draft: &SessionCreationDraftState,
    contract_text: &str,
    completion_gate: bool,
) -> ContractGateResult {
    let issues = if completion_gate {
        generated_contract_completion_quality_issues(draft, contract_text)
    } else {
        generated_contract_quality_issues(draft, contract_text)
    };
    if issues.is_empty() {
        return ContractGateResult::ready();
    }
    let mut findings = ContractIssueList::new(
        "contract.generated_quality",
        ContractIssueKind::Other,
        "generated_contract",
    );
    findings.extend_messages(issues);
    contract_gate_from_findings(draft, contract_text, findings)
}

#[cfg(test)]
fn generated_contract_semantic_quality_issues(
    draft: &SessionCreationDraftState,
    contract_text: &str,
    completion_gate: bool,
) -> Vec<String> {
    let mut issues = Vec::new();
    if draft.artifact_kind != "fiction" {
        return issues;
    }
    if let Some(fragment) = malformed_chapter_plan_fragment(contract_text) {
        issues.push(format!("章节规划编号格式异常：{fragment}"));
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
    if draft.title.trim().is_empty() {
        if let Some(issue) = generated_title_is_contract_noise(draft, contract_text) {
            issues.push(issue);
        }
        if let Some(issue) = generated_title_reuses_protagonist_name(draft, contract_text) {
            issues.push(issue);
        }
    } else if let Some(issue) = crate::tool::writing::naming::title_contract_basis_issue(
        &draft.title,
        "书名",
        &draft.fiction_title_rationale,
        contract_text,
    ) {
        issues.push(issue);
    }
    if let Some(target) = draft.chapter_unit_target {
        let normalized = normalize_contract_numeric_surface(contract_text);
        if !normalized.contains(&target.to_string()) {
            issues.push(format!("合同未保留每章目标档位：{target}"));
        }
    }
    if let Some(expected) =
        draft
            .target_units
            .zip(draft.chapter_unit_target)
            .and_then(|(total, per_chapter)| {
                longform_policy::expected_chapter_count(total, per_chapter)
            })
    {
        let minimum_recent = expected.min(3);
        let planned = count_explicit_chapter_plan_lines(contract_text);
        if expected >= 3 && planned > 0 && planned < minimum_recent {
            issues.push(format!(
                "近期章节包不足：至少需要 {minimum_recent} 章，当前识别到 {planned} 章"
            ));
        }
        if planned > 0 {
            if let Some(issue) = malformed_goal_like_plan_line_issue(contract_text) {
                issues.push(issue);
            }
            if let Some(issue) = chapter_plan_missing_title_issue(contract_text, expected) {
                issues.push(issue);
            }
            if let Some(issue) = chapter_plan_missing_goal_issue(contract_text, expected) {
                issues.push(issue);
            }
        }
    }
    if !fiction_contract_mentions_core_identity(contract_text) {
        issues.push("小说合同缺少明确主角、核心矛盾或结局承诺".to_string());
    }
    issues.extend(generated_fiction_contract_planning_issues(
        contract_text,
        completion_gate,
    ));
    issues.sort();
    issues.dedup();
    issues
}

pub fn generated_contract_advisory_issues(
    draft: &SessionCreationDraftState,
    contract_text: &str,
) -> Vec<String> {
    let mut issues = Vec::new();
    if draft.artifact_kind == "fiction" {
        if let Some(issue) = chapter_plan_invalid_title_issue(contract_text) {
            issues.push(issue);
        }
        let expected_chapters = draft
            .target_units
            .zip(draft.chapter_unit_target)
            .and_then(|(total, per_chapter)| {
                longform_policy::expected_chapter_count(total, per_chapter)
            })
            .unwrap_or_else(|| collect_explicit_chapter_plan_titles(contract_text).len());
        if expected_chapters >= 3 {
            if let Some(issue) =
                chapter_plan_title_diversity_issue(contract_text, expected_chapters)
            {
                issues.push(issue);
            }
        }
    }
    issues.sort();
    issues.dedup();
    issues
}

pub(super) fn contract_gate_from_findings(
    draft: &SessionCreationDraftState,
    contract_text: &str,
    issues: ContractIssueList,
) -> ContractGateResult {
    let mut blocking_issues = Vec::new();
    let mut repairable_issues = Vec::new();
    let mut warnings = Vec::new();
    let mut typed_issues = ContractIssueList::new(
        "contract.generated_advisory",
        ContractIssueKind::Other,
        "generated_contract",
    );
    typed_issues.set_disposition(ContractIssueDisposition::Advisory);
    typed_issues.extend_messages(generated_contract_advisory_issues(draft, contract_text));
    typed_issues.extend_findings(issues);
    typed_issues.sort_dedup();
    for issue in typed_issues {
        match issue.disposition {
            ContractIssueDisposition::HardBlock => blocking_issues.push(issue.text),
            ContractIssueDisposition::Repairable => repairable_issues.push(issue.text),
            ContractIssueDisposition::Advisory => warnings.push(issue.text),
            ContractIssueDisposition::Diagnostic => {}
        }
    }
    let status = if !blocking_issues.is_empty() {
        ContractGateStatus::Blocked
    } else if !repairable_issues.is_empty() {
        ContractGateStatus::NeedsRepair
    } else {
        ContractGateStatus::Ready
    };
    warnings.sort();
    warnings.dedup();
    ContractGateResult {
        status,
        blocking_issues,
        repairable_issues,
        warnings,
    }
}
