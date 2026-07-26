use serde_json::json;

use super::model::{WritingDocumentManifest, WritingSectionRecord};

pub(super) const MAX_CONTEXT_SUMMARY_CHARS: usize = 12_000;
pub(super) const MAX_CONTEXT_SECTIONS: usize = 32;

pub(super) fn bounded_section_context(
    sections: &[WritingSectionRecord],
) -> (Vec<serde_json::Value>, usize) {
    let mut selected = Vec::new();
    let mut used_chars = 0usize;
    for section in sections.iter().rev() {
        let section_chars = section.title.chars().count()
            + section.summary.chars().count()
            + section
                .evidence_refs
                .iter()
                .map(|item| item.chars().count())
                .sum::<usize>();
        if selected.len() >= MAX_CONTEXT_SECTIONS
            || (!selected.is_empty()
                && used_chars.saturating_add(section_chars) > MAX_CONTEXT_SUMMARY_CHARS)
        {
            break;
        }
        used_chars = used_chars.saturating_add(section_chars);
        selected.push(json!({
            "id": section.id,
            "title": section.title,
            "summary": section.summary,
            "status": section.status,
            "revision": section.revision,
            "evidence_refs": section.evidence_refs
        }));
    }
    selected.reverse();
    let omitted = sections.len().saturating_sub(selected.len());
    (selected, omitted)
}

pub(super) fn mechanical_audit(
    manifest: &WritingDocumentManifest,
    section: &WritingSectionRecord,
    body: &str,
) -> Vec<String> {
    let mut issues = Vec::new();
    if body.trim().is_empty() {
        issues.push("section body is empty".to_string());
    }
    if section.title.trim().is_empty() {
        issues.push("section title is empty".to_string());
    }
    let Some(contract) = &manifest.contract else {
        return issues;
    };
    let body_lower = body.to_ascii_lowercase();
    for forbidden in &contract.forbidden_drift {
        let forbidden = forbidden.trim();
        if !forbidden.is_empty() && body_lower.contains(&forbidden.to_ascii_lowercase()) {
            issues.push(format!("forbidden drift marker appears: {forbidden}"));
        }
    }
    if !contract.evidence_rules.is_empty() && section.evidence_refs.is_empty() {
        issues.push("evidence rules exist but this section has no evidence_refs".to_string());
    }
    if let Some(target) = manifest.section_unit_target {
        if section.unit_count < target {
            issues.push(format!(
                "section has {} units, below the configured target of {target}",
                section.unit_count
            ));
        }
    }
    issues
}

pub(super) fn section_has_passed_audit(
    manifest: &WritingDocumentManifest,
    section_id: &str,
) -> bool {
    let Some(section) = manifest
        .sections
        .iter()
        .find(|section| section.id == section_id)
    else {
        return false;
    };
    manifest
        .audits
        .iter()
        .rev()
        .find(|audit| audit.section_id == section_id && audit.section_revision == section.revision)
        .is_some_and(|audit| {
            matches!(
                audit.verdict.trim().to_ascii_lowercase().as_str(),
                "pass" | "passed" | "approve" | "approved"
            )
        })
}
