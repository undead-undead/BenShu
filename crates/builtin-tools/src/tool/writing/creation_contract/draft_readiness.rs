use super::issue::{
    ContractIssueDisposition, ContractIssueKind, ContractIssueList, ContractIssueSet,
};
use super::ContractReadinessScope;
use super::SessionCreationDraftState;

#[cfg(test)]
pub fn creation_draft_contract_blocking_issues(draft: &SessionCreationDraftState) -> Vec<String> {
    creation_draft_contract_blocking_issues_for_scope(
        draft,
        ContractReadinessScope::LockedAuthorityContract,
    )
}

pub fn creation_draft_contract_blocking_issues_for_scope(
    draft: &SessionCreationDraftState,
    scope: ContractReadinessScope,
) -> Vec<String> {
    let findings = creation_draft_contract_blocking_findings_for_scope(draft, scope);
    ContractIssueSet::new(&findings)
        .actionable()
        .map(|issue| issue.text.clone())
        .collect()
}

pub(crate) fn creation_draft_contract_blocking_findings_for_scope(
    draft: &SessionCreationDraftState,
    scope: ContractReadinessScope,
) -> ContractIssueList {
    let effective = super::creation_draft_with_pending_contract_applied(draft);
    let mut issues = ContractIssueList::new(
        "contract.draft_readiness",
        ContractIssueKind::Other,
        "creation_draft",
    );
    if draft.artifact_kind != "fiction" {
        return issues;
    }

    if crate::tool::writing::typed_contract_gate::contract_outline_text_is_polluted(
        &effective.fiction_outline,
    ) {
        issues.set_scope(
            "contract.outline.pollution",
            ContractIssueKind::Plot,
            "outline",
        );
        issues.set_disposition(ContractIssueDisposition::HardBlock);
        issues.push("ContractBlocker: 小说合同大纲含有结构污染、工作流说明或控制面文本");
        issues.set_disposition(ContractIssueDisposition::Repairable);
    }

    let typed_contract = super::strong_novel_contract_from_creation_draft(&effective);
    issues.extend_findings(typed_contract.validate_for_scope(scope).issues);

    issues.sort_dedup();
    issues
}
