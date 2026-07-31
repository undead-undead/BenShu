//! Shared quality-gate utilities for chapter checks.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ChapterFindingClass {
    Contract,
    Continuity,
    State,
    BodyIntegrity,
    Length,
    Metadata,
    Advisory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ChapterFindingDisposition {
    HardBlock,
    DeterministicRepair,
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FindingEvidenceGrade {
    DeterministicInvariant,
    EvidenceBackedSemantic,
    Heuristic,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AuthorityEvidenceRef {
    pub(crate) path: String,
    pub(crate) excerpt: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BodyEvidenceSpan {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) excerpt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ChapterFinding {
    pub(crate) code: String,
    pub(crate) class: ChapterFindingClass,
    pub(crate) disposition: ChapterFindingDisposition,
    pub(crate) evidence_grade: FindingEvidenceGrade,
    pub(crate) source: String,
    pub(crate) message: String,
    #[serde(default)]
    pub(crate) authority_evidence: Vec<AuthorityEvidenceRef>,
    #[serde(default)]
    pub(crate) body_evidence: Vec<BodyEvidenceSpan>,
    pub(crate) authority_fingerprint: String,
    pub(crate) body_fingerprint: String,
}

impl ChapterFinding {
    pub(crate) fn local(
        code: impl Into<String>,
        class: ChapterFindingClass,
        disposition: ChapterFindingDisposition,
        evidence_grade: FindingEvidenceGrade,
        source: impl Into<String>,
        message: impl Into<String>,
        authority_fingerprint: impl Into<String>,
        body: &str,
    ) -> Self {
        Self {
            code: code.into(),
            class,
            disposition,
            evidence_grade,
            source: source.into(),
            message: message.into(),
            authority_evidence: Vec::new(),
            body_evidence: Vec::new(),
            authority_fingerprint: authority_fingerprint.into(),
            body_fingerprint: chapter_body_fingerprint(body),
        }
    }

    pub(crate) fn hard_blocking(&self) -> bool {
        if self.disposition != ChapterFindingDisposition::HardBlock {
            return false;
        }
        match self.evidence_grade {
            FindingEvidenceGrade::DeterministicInvariant => true,
            FindingEvidenceGrade::EvidenceBackedSemantic => {
                !self.authority_fingerprint.is_empty()
                    && !self.body_fingerprint.is_empty()
                    && !self.authority_evidence.is_empty()
                    && !self.body_evidence.is_empty()
            }
            FindingEvidenceGrade::Heuristic => false,
        }
    }
}

pub(crate) fn chapter_body_fingerprint(body: &str) -> String {
    hex::encode(Sha256::digest(body.as_bytes()))
}

pub(crate) fn future_chapter_consumed_finding(
    chapter_number: usize,
    next_chapter_number: usize,
    next_authority_path: String,
    next_seed: String,
    body_excerpt: String,
    source: &str,
    authority_fingerprint: &str,
    body: &str,
) -> Option<ChapterFinding> {
    let start = body.find(&body_excerpt)?;
    Some(ChapterFinding {
        code: "future_chapter_consumed".to_string(),
        class: ChapterFindingClass::Continuity,
        disposition: ChapterFindingDisposition::HardBlock,
        evidence_grade: FindingEvidenceGrade::EvidenceBackedSemantic,
        source: source.to_string(),
        message: format!(
            "chapter {chapter_number} consumes the sealed chapter {next_chapter_number} boundary early"
        ),
        authority_evidence: vec![AuthorityEvidenceRef {
            path: next_authority_path,
            excerpt: next_seed,
        }],
        body_evidence: vec![BodyEvidenceSpan {
            start,
            end: start + body_excerpt.len(),
            excerpt: body_excerpt,
        }],
        authority_fingerprint: authority_fingerprint.to_string(),
        body_fingerprint: chapter_body_fingerprint(body),
    })
}

/// Counts shared two-character event fragments while ignoring generic story
/// glue. Contract outline validation and per-chapter execution-package
/// validation share this primitive so their event-boundary decisions cannot
/// drift into separate similarity mechanisms.
pub(crate) fn shared_distinctive_bigram_count(left: &str, right: &str) -> usize {
    let right_bigrams = adjacent_bigrams(right).into_iter().collect::<BTreeSet<_>>();
    let generic = [
        "主角", "角色", "故事", "事件", "最终", "终局", "完成", "实现", "达成", "进入", "成为",
        "开始", "进行", "通过", "为了", "以及", "他们", "两人", "一个",
    ];
    adjacent_bigrams(left)
        .into_iter()
        .filter(|bigram| !generic.contains(&bigram.as_str()) && right_bigrams.contains(bigram))
        .collect::<BTreeSet<_>>()
        .len()
}

/// Splits normalized text into adjacent two-character fragments. Domain
/// filters, deduplication and scoring remain with the owning validator.
pub(crate) fn adjacent_bigrams(value: &str) -> Vec<String> {
    value
        .chars()
        .collect::<Vec<_>>()
        .windows(2)
        .map(|window| window.iter().collect::<String>())
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum ChapterQualityStatus {
    Accepted,
    Warning,
    DeterministicRepair,
    MetadataRepair,
    BodyRevision,
    Blocked,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ChapterQualityDecision {
    pub(crate) status: ChapterQualityStatus,
    pub(crate) body_issues: Vec<String>,
    pub(crate) metadata_issues: Vec<String>,
    pub(crate) warnings: Vec<String>,
    pub(crate) can_preserve_body: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct ChapterQualityGate {
    pub(crate) passed: bool,
    #[serde(default)]
    pub(crate) findings: Vec<ChapterFinding>,
    pub(crate) issues: Vec<String>,
    pub(crate) repairable: Vec<String>,
    pub(crate) warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct ChapterMetadataGate {
    pub(crate) passed: bool,
    #[serde(default)]
    pub(crate) findings: Vec<ChapterFinding>,
    pub(crate) blocking: Vec<String>,
    pub(crate) repairable: Vec<String>,
    pub(crate) warnings: Vec<String>,
}

impl ChapterMetadataGate {
    pub(crate) fn from_findings(findings: Vec<ChapterFinding>) -> Self {
        let blocking = finalize_issues(
            findings
                .iter()
                .filter(|finding| finding.hard_blocking())
                .map(|finding| finding.message.clone())
                .collect(),
        );
        let repairable = finalize_issues(
            findings
                .iter()
                .filter(|finding| {
                    finding.disposition == ChapterFindingDisposition::DeterministicRepair
                })
                .map(|finding| finding.message.clone())
                .collect(),
        );
        let warnings = finalize_issues(
            findings
                .iter()
                .filter(|finding| finding.disposition == ChapterFindingDisposition::Warning)
                .map(|finding| finding.message.clone())
                .collect(),
        );
        Self {
            passed: blocking.is_empty() && repairable.is_empty(),
            findings,
            blocking,
            repairable,
            warnings,
        }
    }

    pub(crate) fn blocking(&self) -> bool {
        !self.blocking.is_empty()
    }

    pub(crate) fn needs_repair(&self) -> bool {
        !self.blocking.is_empty() || !self.repairable.is_empty()
    }
}

impl ChapterQualityGate {
    pub(crate) fn from_findings(findings: Vec<ChapterFinding>) -> Self {
        let issues = finalize_issues(
            findings
                .iter()
                .filter(|finding| finding.hard_blocking())
                .map(|finding| finding.message.clone())
                .collect(),
        );
        let repairable = finalize_issues(
            findings
                .iter()
                .filter(|finding| {
                    finding.disposition == ChapterFindingDisposition::DeterministicRepair
                })
                .map(|finding| finding.message.clone())
                .collect(),
        );
        let warnings = finalize_issues(
            findings
                .iter()
                .filter(|finding| finding.disposition == ChapterFindingDisposition::Warning)
                .map(|finding| finding.message.clone())
                .collect(),
        );
        Self {
            passed: issues.is_empty() && repairable.is_empty(),
            findings,
            issues,
            repairable,
            warnings,
        }
    }

    pub(crate) fn hard_blocking(&self) -> bool {
        self.findings.iter().any(ChapterFinding::hard_blocking)
    }

    /// Final approval runs only after the bounded revision/top-up controller has
    /// selected and durably bound its best candidate. At that boundary a small
    /// length shortfall is advisory; every other unresolved deterministic body
    /// repair and every evidence-backed hard finding still blocks approval.
    pub(crate) fn blocks_approval_after_bounded_recovery(&self) -> bool {
        self.findings.iter().any(|finding| {
            finding.hard_blocking()
                || (finding.disposition == ChapterFindingDisposition::DeterministicRepair
                    && finding.code != "length_below_target")
        })
    }

    pub(crate) fn extend_findings(&mut self, findings: Vec<ChapterFinding>) {
        if findings.is_empty() {
            return;
        }
        self.findings.extend(findings);
        *self = Self::from_findings(std::mem::take(&mut self.findings));
    }
}

pub(crate) fn chapter_quality_decision(
    quality: &ChapterQualityGate,
    metadata: &ChapterMetadataGate,
) -> ChapterQualityDecision {
    let body_issues = finalize_issues(
        quality
            .findings
            .iter()
            .filter(|finding| finding.class != ChapterFindingClass::Metadata)
            .filter(|finding| finding.disposition != ChapterFindingDisposition::Warning)
            .map(|finding| finding.message.clone())
            .collect(),
    );
    let metadata_issues = finalize_issues(
        quality
            .findings
            .iter()
            .filter(|finding| finding.class == ChapterFindingClass::Metadata)
            .filter(|finding| finding.disposition != ChapterFindingDisposition::Warning)
            .map(|finding| finding.message.clone())
            .chain(
                metadata
                    .blocking
                    .iter()
                    .chain(metadata.repairable.iter())
                    .cloned(),
            )
            .collect(),
    );
    let warnings = finalize_issues(
        quality
            .warnings
            .iter()
            .chain(metadata.warnings.iter())
            .cloned()
            .collect(),
    );
    let status = if quality.hard_blocking() {
        ChapterQualityStatus::BodyRevision
    } else if !body_issues.is_empty() {
        ChapterQualityStatus::DeterministicRepair
    } else if metadata.blocking()
        || quality.findings.iter().any(|finding| {
            finding.class == ChapterFindingClass::Metadata && finding.hard_blocking()
        })
    {
        ChapterQualityStatus::Blocked
    } else if metadata.needs_repair() || !metadata_issues.is_empty() {
        ChapterQualityStatus::MetadataRepair
    } else if !warnings.is_empty() {
        ChapterQualityStatus::Warning
    } else {
        ChapterQualityStatus::Accepted
    };
    let can_preserve_body = !quality.hard_blocking() && body_issues.is_empty();
    ChapterQualityDecision {
        status,
        body_issues,
        metadata_issues,
        warnings,
        can_preserve_body,
    }
}

pub(crate) fn finalize_issues(mut issues: Vec<String>) -> Vec<String> {
    issues.sort();
    issues.dedup();
    issues
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finding(
        class: ChapterFindingClass,
        disposition: ChapterFindingDisposition,
        message: &str,
    ) -> ChapterFinding {
        ChapterFinding::local(
            "test",
            class,
            disposition,
            FindingEvidenceGrade::DeterministicInvariant,
            "test",
            message,
            "",
            "body",
        )
    }

    #[test]
    fn body_failure_has_priority_over_metadata_blocker() {
        let quality = ChapterQualityGate::from_findings(vec![finding(
            ChapterFindingClass::BodyIntegrity,
            ChapterFindingDisposition::HardBlock,
            "chapter body is incomplete",
        )]);
        let metadata = ChapterMetadataGate::from_findings(vec![finding(
            ChapterFindingClass::Metadata,
            ChapterFindingDisposition::HardBlock,
            "chapter title is missing",
        )]);

        let decision = chapter_quality_decision(&quality, &metadata);

        assert_eq!(decision.status, ChapterQualityStatus::BodyRevision);
        assert!(!decision.can_preserve_body);
    }

    #[test]
    fn metadata_only_failure_preserves_body() {
        let quality = ChapterQualityGate::from_findings(Vec::new());
        let metadata = ChapterMetadataGate::from_findings(vec![finding(
            ChapterFindingClass::Metadata,
            ChapterFindingDisposition::DeterministicRepair,
            "chapter title needs repair",
        )]);

        let decision = chapter_quality_decision(&quality, &metadata);

        assert_eq!(decision.status, ChapterQualityStatus::MetadataRepair);
        assert!(decision.can_preserve_body);
    }

    #[test]
    fn semantic_finding_without_two_sided_evidence_cannot_block() {
        let finding = ChapterFinding::local(
            "timeline_conflict",
            ChapterFindingClass::Continuity,
            ChapterFindingDisposition::HardBlock,
            FindingEvidenceGrade::EvidenceBackedSemantic,
            "test",
            "claimed timeline conflict",
            "authority-fingerprint",
            "chapter body",
        );

        assert!(!finding.hard_blocking());
        assert!(ChapterQualityGate::from_findings(vec![finding]).passed);
    }

    #[test]
    fn semantic_finding_with_two_sided_evidence_blocks() {
        let mut finding = ChapterFinding::local(
            "timeline_conflict",
            ChapterFindingClass::Continuity,
            ChapterFindingDisposition::HardBlock,
            FindingEvidenceGrade::EvidenceBackedSemantic,
            "test",
            "grounded timeline conflict",
            "authority-fingerprint",
            "chapter body",
        );
        finding.authority_evidence.push(AuthorityEvidenceRef {
            path: "/truth_as_of_chapter/time".to_string(),
            excerpt: "凌晨".to_string(),
        });
        finding.body_evidence.push(BodyEvidenceSpan {
            start: 0,
            end: 7,
            excerpt: "chapter".to_string(),
        });

        assert!(finding.hard_blocking());
        assert!(!ChapterQualityGate::from_findings(vec![finding]).passed);
    }

    #[test]
    fn bounded_recovery_may_approve_only_the_soft_length_repair() {
        let mut length = finding(
            ChapterFindingClass::Length,
            ChapterFindingDisposition::DeterministicRepair,
            "small length shortfall",
        );
        length.code = "length_below_target".to_string();
        let gate = ChapterQualityGate::from_findings(vec![length]);

        assert!(!gate.passed);
        assert!(!gate.blocks_approval_after_bounded_recovery());
    }

    #[test]
    fn bounded_recovery_does_not_approve_other_unresolved_body_repairs() {
        let mut surface = finding(
            ChapterFindingClass::BodyIntegrity,
            ChapterFindingDisposition::DeterministicRepair,
            "body still needs deterministic cleanup",
        );
        surface.code = "body_surface_contamination".to_string();
        let gate = ChapterQualityGate::from_findings(vec![surface]);

        assert!(gate.blocks_approval_after_bounded_recovery());
    }
}
