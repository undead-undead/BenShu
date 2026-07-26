use std::ops::Deref;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ContractIssueKind {
    Skeleton,
    Characters,
    Plot,
    Governance,
    Diagnostic,
    Other,
}

impl ContractIssueKind {
    pub(crate) fn is_diagnostic(self) -> bool {
        matches!(self, Self::Diagnostic)
    }
}

pub(crate) fn user_story_semantic_issue_kind(candidate_field: &str) -> ContractIssueKind {
    let lowered = candidate_field.to_ascii_lowercase();
    if [
        "大纲",
        "分卷",
        "卷尾",
        "卷",
        "近期章节",
        "章节",
        "章",
        "兑现矩阵",
        "outline",
        "volume",
        "chapter",
        "payoff",
    ]
    .iter()
    .any(|marker| candidate_field.contains(marker) || lowered.contains(marker))
    {
        ContractIssueKind::Plot
    } else {
        ContractIssueKind::Skeleton
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ContractIssueEvidence {
    pub(crate) field: String,
    pub(crate) observed: String,
}

impl ContractIssueEvidence {
    pub(crate) fn new(field: impl Into<String>, observed: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            observed: observed.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ContractIssue {
    pub(crate) code: String,
    pub(crate) kind: ContractIssueKind,
    pub(crate) evidence: ContractIssueEvidence,
    pub(crate) text: String,
}

pub(crate) type ClassifiedContractIssue<'a> = &'a ContractIssue;

impl ContractIssue {
    pub(crate) fn new(
        code: impl Into<String>,
        kind: ContractIssueKind,
        evidence: ContractIssueEvidence,
        text: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            kind,
            evidence,
            text: text.into(),
        }
    }
}

impl Deref for ContractIssue {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.text
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContractIssueList {
    items: Vec<ContractIssue>,
    default_code: String,
    default_kind: ContractIssueKind,
    default_evidence_field: String,
}

impl Default for ContractIssueList {
    fn default() -> Self {
        Self::new("contract.unspecified", ContractIssueKind::Other, "contract")
    }
}

impl ContractIssueList {
    pub(crate) fn new(
        code: impl Into<String>,
        kind: ContractIssueKind,
        evidence_field: impl Into<String>,
    ) -> Self {
        Self {
            items: Vec::new(),
            default_code: code.into(),
            default_kind: kind,
            default_evidence_field: evidence_field.into(),
        }
    }

    pub(crate) fn single(
        code: impl Into<String>,
        kind: ContractIssueKind,
        evidence_field: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        let mut issues = Self::new(code, kind, evidence_field);
        issues.push(text);
        issues
    }

    #[cfg(test)]
    pub(crate) fn from_messages<I>(
        code: impl Into<String>,
        kind: ContractIssueKind,
        evidence_field: impl Into<String>,
        messages: I,
    ) -> Self
    where
        I: IntoIterator<Item = String>,
    {
        let mut issues = Self::new(code, kind, evidence_field);
        issues.extend_messages(messages);
        issues
    }

    pub(crate) fn from_issue(issue: ContractIssue) -> Self {
        let mut issues = Self::new(issue.code.clone(), issue.kind, issue.evidence.field.clone());
        issues.push_issue(issue);
        issues
    }

    pub(crate) fn set_scope(
        &mut self,
        code: impl Into<String>,
        kind: ContractIssueKind,
        evidence_field: impl Into<String>,
    ) {
        self.default_code = code.into();
        self.default_kind = kind;
        self.default_evidence_field = evidence_field.into();
    }

    pub(crate) fn push(&mut self, text: impl Into<String>) {
        let text = text.into();
        self.items.push(ContractIssue::new(
            self.default_code.clone(),
            self.default_kind,
            ContractIssueEvidence::new(self.default_evidence_field.clone(), text.clone()),
            text,
        ));
    }

    pub(crate) fn push_issue(&mut self, issue: ContractIssue) {
        self.items.push(issue);
    }

    pub(crate) fn extend_messages<I>(&mut self, messages: I)
    where
        I: IntoIterator<Item = String>,
    {
        for message in messages {
            self.push(message);
        }
    }

    pub(crate) fn extend<I>(&mut self, messages: I)
    where
        I: IntoIterator<Item = String>,
    {
        self.extend_messages(messages);
    }

    pub(crate) fn extend_findings<I>(&mut self, findings: I)
    where
        I: IntoIterator<Item = ContractIssue>,
    {
        self.items.extend(findings);
    }

    pub(crate) fn sort_dedup(&mut self) {
        self.items.sort();
        self.items.dedup();
    }

    pub(crate) fn retain(&mut self, predicate: impl FnMut(&ContractIssue) -> bool) {
        self.items.retain(predicate);
    }

    pub(crate) fn messages(&self) -> Vec<String> {
        self.items.iter().map(|issue| issue.text.clone()).collect()
    }

    pub(crate) fn join(&self, separator: &str) -> String {
        self.items
            .iter()
            .map(|issue| issue.text.as_str())
            .collect::<Vec<_>>()
            .join(separator)
    }
}

impl Deref for ContractIssueList {
    type Target = [ContractIssue];

    fn deref(&self) -> &Self::Target {
        &self.items
    }
}

impl IntoIterator for ContractIssueList {
    type Item = ContractIssue;
    type IntoIter = std::vec::IntoIter<ContractIssue>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ContractIssueSet<'a> {
    items: &'a [ContractIssue],
}

impl<'a> ContractIssueSet<'a> {
    pub(crate) fn new(issues: &'a ContractIssueList) -> Self {
        Self { items: issues }
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &'a ContractIssue> + '_ {
        self.items.iter()
    }

    pub(crate) fn actionable(&self) -> impl Iterator<Item = &'a ContractIssue> + '_ {
        self.iter().filter(|issue| !issue.kind.is_diagnostic())
    }

    pub(crate) fn has_actionable(&self, kind: ContractIssueKind) -> bool {
        self.actionable().any(|issue| issue.kind == kind)
    }
}

pub fn creation_contract_issue_summary(issues: &[String]) -> String {
    let mut groups = issues
        .iter()
        .map(|issue| contract_issue_surface_label(issue))
        .collect::<Vec<_>>();
    groups.sort_unstable();
    groups.dedup();
    if groups.is_empty() {
        "关键合同字段".to_string()
    } else {
        groups.join("、")
    }
}

fn contract_issue_surface_label(issue: &str) -> &'static str {
    if issue_matches_any(issue, &["书名", "命名理由", "读者钩子", "title rationale"]) {
        return "书名和命名理由";
    }
    if issue_matches_any(
        issue,
        &["世界观意象", "世界规则", "world_imagery", "world_rules"],
    ) {
        return "世界观规则";
    }
    if issue_matches_any(
        issue,
        &[
            "角色权威表",
            "角色名",
            "主角",
            "关系线",
            "关系账本",
            "情感线",
            "character",
            "protagonist",
            "relationship",
            "emotional",
        ],
    ) {
        return "角色权威表、关系线和情感线";
    }
    if issue_matches_any(
        issue,
        &[
            "大纲",
            "分卷",
            "近期章节",
            "章节目标",
            "章节转折",
            "伏笔",
            "outline",
            "chapter",
            "volume",
            "payoff",
        ],
    ) {
        return "分卷、章节规划和伏笔兑现";
    }
    if issue_matches_any(
        issue,
        &[
            "故事前提",
            "终局",
            "主线因果",
            "主角弧线",
            "premise",
            "ending",
            "causal_spine",
        ],
    ) {
        return "故事前提、终局和主线因果";
    }
    if issue_matches_any(
        issue,
        &[
            "叙事风格",
            "必须避免",
            "核心主题",
            "governance",
            "style_rules",
            "must_avoid",
        ],
    ) {
        return "世界规则和叙事治理";
    }
    "关键合同字段"
}

fn issue_matches_any(issue: &str, terms: &[&str]) -> bool {
    let lowered = issue.to_ascii_lowercase();
    terms
        .iter()
        .any(|term| issue.contains(term) || lowered.contains(&term.to_ascii_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_issue_owner_does_not_change_when_message_wording_changes() {
        let first = ContractIssue::new(
            "outline.character_role_conflict",
            ContractIssueKind::Plot,
            ContractIssueEvidence::new("outline.near_chapters", "主角被标成对手"),
            "近期章节把主角标成对手",
        );
        let reworded = ContractIssue::new(
            "outline.character_role_conflict",
            ContractIssueKind::Plot,
            ContractIssueEvidence::new("outline.near_chapters", "主角被标成对手"),
            "The near chapter assigns the protagonist an antagonist role",
        );
        assert_eq!(first.code, reworded.code);
        assert_eq!(first.kind, reworded.kind);
    }

    #[test]
    fn user_story_semantic_owner_follows_candidate_field() {
        assert_eq!(
            user_story_semantic_issue_kind("合同大纲-第2卷"),
            ContractIssueKind::Plot
        );
        assert_eq!(
            user_story_semantic_issue_kind("第1章转折"),
            ContractIssueKind::Plot
        );
        assert_eq!(
            user_story_semantic_issue_kind("ending.desired_resolution"),
            ContractIssueKind::Skeleton
        );
    }
}
