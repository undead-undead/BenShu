//! Session creation-draft validation report.
//!
//! This module adapts typed/readiness checks into user-facing draft responses.
//! It does not own the underlying contract field rules.

use super::*;

#[derive(Debug, Clone)]
pub struct CreationContractSurfaceState {
    pub lifecycle: CreationDraftLifecycleStatus,
    pub confirmable: bool,
    pub issues: Vec<String>,
}

impl CreationContractSurfaceState {
    pub fn from_draft(draft: &SessionCreationDraftState) -> Self {
        let report = ContractValidationReport::for_draft_scope(
            draft,
            ContractReadinessScope::LockedAuthorityContract,
        );
        let lifecycle = draft.lifecycle_status();
        let confirmable = matches!(
            lifecycle,
            CreationDraftLifecycleStatus::Approved | CreationDraftLifecycleStatus::Writing
        ) || (lifecycle == CreationDraftLifecycleStatus::ContractReady
            && report.is_ready());
        Self {
            lifecycle,
            confirmable,
            issues: latest_contract_status_issues(draft, &report.issues),
        }
    }
}

pub fn creation_contract_draft_is_confirmable(draft: &SessionCreationDraftState) -> bool {
    CreationContractSurfaceState::from_draft(draft).confirmable
}

pub(crate) fn latest_contract_status_issues(
    draft: &SessionCreationDraftState,
    fallback: &[String],
) -> Vec<String> {
    let pending = draft.pending_contract_candidate.as_ref();
    let has_normalized_contract = pending
        .and_then(|candidate| candidate.get("normalized"))
        .is_some();
    let mut issues = if has_normalized_contract {
        fallback.to_vec()
    } else {
        pending
            .and_then(|candidate| candidate.get("issues"))
            .and_then(|issues| issues.as_array())
            .map(|issues| {
                issues
                    .iter()
                    .filter_map(|issue| issue.as_str())
                    .map(str::trim)
                    .filter(|issue| !issue.is_empty())
                    .map(ToOwned::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    if issues.is_empty() {
        issues = fallback.to_vec();
    }
    issues.sort();
    issues.dedup();
    issues
}

#[derive(Debug, Clone)]
pub struct ContractValidationReport {
    pub artifact_kind: String,
    pub issues: Vec<String>,
}

impl ContractValidationReport {
    #[cfg(test)]
    pub fn for_draft(draft: &SessionCreationDraftState) -> Self {
        Self::for_draft_scope(draft, ContractReadinessScope::LockedAuthorityContract)
    }

    pub fn for_draft_scope(
        draft: &SessionCreationDraftState,
        scope: ContractReadinessScope,
    ) -> Self {
        let mut issues = creation_draft_contract_blocking_issues_for_scope(draft, scope);
        issues.sort();
        issues.dedup();
        Self {
            artifact_kind: draft.artifact_kind.clone(),
            issues,
        }
    }

    pub fn is_ready(&self) -> bool {
        self.issues.is_empty()
    }

    pub fn user_response(
        &self,
        draft: &SessionCreationDraftState,
        latest_user: &str,
    ) -> CreationDraftUserResponse {
        if self.is_ready() {
            return CreationDraftUserResponse::new(
                creation_draft_planning_response_text(draft, latest_user),
                self.artifact_kind.clone(),
            );
        }

        let mut text = String::new();
        text.push_str("当前写作合同还不能进入正文写作，我不会把缺字段的草案交给 writer 硬写。\n\n");
        text.push_str("需要补齐：");
        text.push_str(&creation_contract_issue_summary(&self.issues));
        text.push('\n');
        text.push_str("\n你可以直接用自然语言补充或修改这些内容；系统会继续补齐合同。补齐后再说“开始写第一章”或“按这个开始”。");
        CreationDraftUserResponse::new(text, self.artifact_kind.clone())
    }
}
