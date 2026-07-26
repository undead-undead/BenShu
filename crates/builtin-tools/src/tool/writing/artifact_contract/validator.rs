use super::{ArtifactQualityContract, ArtifactQualityReport};
use regex::Regex;
use std::collections::BTreeSet;
use std::sync::OnceLock;

pub(crate) fn quality_report_with_evidence(
    content: &str,
    contract: &ArtifactQualityContract,
    evidence: &str,
) -> ArtifactQualityReport {
    let normalized = content.trim();
    let char_count = normalized.chars().filter(|ch| !ch.is_whitespace()).count();
    let evidence_refs = artifact_reference_ids(evidence);
    let content_refs = artifact_reference_ids(normalized);
    let validated_refs = content_refs
        .intersection(&evidence_refs)
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut blockers = Vec::new();
    let mut repairable = Vec::new();
    let mut warnings = Vec::new();

    if normalized.is_empty() {
        blockers.push("artifact_body_is_empty".to_string());
    }
    if contains_provider_control_token(normalized) {
        blockers
            .push("provider_control_token_or_hidden_reasoning_leaked_into_artifact".to_string());
    }
    if contract.delivery_scope == super::ArtifactDeliveryScope::Final {
        if let Some(target) = contract.final_target_chars {
            if contract.max_chars.is_some_and(|maximum| target > maximum) {
                blockers.push(format!(
                    "artifact_contract_target_exceeds_maximum: target_chars={target} maximum_chars={}",
                    contract.max_chars.unwrap_or_default()
                ));
            }
        }
    }
    if char_count < contract.min_chars {
        repairable.push(format!(
            "content_depth_below_minimum: observed_chars={char_count} required_chars={}",
            contract.min_chars
        ));
    }
    if let Some(max_chars) = contract.max_chars {
        if char_count > max_chars {
            repairable.push(format!(
                "content_depth_above_maximum: observed_chars={char_count} maximum_chars={max_chars}"
            ));
        }
    }

    let mut required = contract.required_sections.clone();
    if contract.require_title && !required.iter().any(|section| section_is_title(section)) {
        required.insert(0, "标题".to_string());
    }
    let missing_sections = required
        .iter()
        .filter(|section| !section_present(normalized, section))
        .cloned()
        .collect::<Vec<_>>();
    if !missing_sections.is_empty() {
        repairable.push(format!(
            "missing_required_{}: {}",
            contract.required_section_label,
            missing_sections.join(", ")
        ));
    }

    if contract.min_citations > 0 {
        if evidence_refs.is_empty() {
            blockers.push(
                "evidence_receipt_has_no_verifiable_source_identifiers; citations cannot be validated"
                    .to_string(),
            );
        } else if validated_refs.len() < contract.min_citations {
            repairable.push(format!(
                "validated_citation_count_below_minimum: observed={} required={}",
                validated_refs.len(),
                contract.min_citations
            ));
        }
        let unverified = content_refs.difference(&evidence_refs).count();
        if unverified > 0 {
            warnings.push(format!(
                "artifact_contains_{unverified}_citation_identifier(s)_not_present_in_evidence_receipt"
            ));
        }
    }
    if declares_insufficient_evidence(normalized) && evidence_refs.is_empty() {
        blockers
            .push("declared_insufficient_evidence_without_actionable_source_receipt".to_string());
    }

    let required_sections_present = required.len().saturating_sub(missing_sections.len());
    ArtifactQualityReport {
        artifact_type: contract.artifact_type.clone(),
        passed: blockers.is_empty() && repairable.is_empty(),
        blockers,
        repairable,
        warnings,
        metrics: vec![
            ("chars".to_string(), char_count),
            ("validated_citations".to_string(), validated_refs.len()),
            (
                "required_sections_present".to_string(),
                required_sections_present,
            ),
        ],
        review_receipt_required: contract.require_review_receipt,
    }
}

fn section_present(content: &str, section: &str) -> bool {
    if section_is_title(section) {
        return artifact_has_title(content);
    }
    if section_is_body(section) {
        return artifact_has_body(content);
    }
    let aliases = section_aliases(section);
    content.lines().any(|line| {
        let heading = normalized_heading(line);
        !heading.is_empty()
            && aliases
                .iter()
                .any(|alias| heading.eq_ignore_ascii_case(alias))
    })
}

fn artifact_has_title(content: &str) -> bool {
    let mut lines = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    let Some(first) = lines.next() else {
        return false;
    };
    first.starts_with('#') || first.chars().count() <= 80
}

fn artifact_has_body(content: &str) -> bool {
    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with('#'))
        .map(|line| line.chars().filter(|ch| !ch.is_whitespace()).count())
        .sum::<usize>()
        >= 40
}

fn normalized_heading(line: &str) -> String {
    let mut value = line.trim().trim_start_matches('#').trim();
    if let Some((left, _)) = value.split_once(['：', ':']) {
        value = left.trim();
    }
    value
        .trim_start_matches(|ch: char| {
            ch.is_ascii_digit()
                || matches!(ch, '.' | '、' | ')' | '）' | '(' | '（' | '-' | '*' | ' ')
        })
        .trim()
        .to_string()
}

fn section_is_title(section: &str) -> bool {
    matches!(
        section.trim().to_ascii_lowercase().as_str(),
        "标题" | "title"
    )
}

fn section_is_body(section: &str) -> bool {
    matches!(
        section.trim().to_ascii_lowercase().as_str(),
        "正文" | "body"
    )
}

fn section_aliases(section: &str) -> Vec<String> {
    match section.trim().to_ascii_lowercase().as_str() {
        "摘要" | "abstract" | "summary" => {
            strings(&["摘要", "Abstract", "Summary", "Executive Summary"])
        }
        "引言" | "introduction" => strings(&["引言", "前言", "Introduction"]),
        "方法" | "methods" | "methodology" => {
            strings(&["方法", "研究方法", "Methods", "Methodology"])
        }
        "结果" | "results" | "findings" => strings(&["结果", "研究结果", "Results", "Findings"]),
        "讨论" | "discussion" => strings(&["讨论", "Discussion"]),
        "结论" | "conclusion" => strings(&["结论", "结语", "Conclusion", "Conclusions"]),
        "参考文献" | "references" => strings(&["参考文献", "References", "Bibliography"]),
        "背景" | "background" => strings(&["背景", "Background", "Context"]),
        "分析" | "analysis" => strings(&["分析", "Analysis"]),
        "建议" | "recommendations" => strings(&["建议", "Recommendations"]),
        value => vec![value.to_string()],
    }
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

fn declares_insufficient_evidence(content: &str) -> bool {
    let lowered = content.to_ascii_lowercase();
    content.contains("资料不足")
        || content.contains("证据不足")
        || lowered.contains("insufficient evidence")
        || lowered.contains("insufficient sources")
}

fn contains_provider_control_token(content: &str) -> bool {
    [
        "<|channel",
        "<channel|",
        "<|message|>",
        "<|start|>",
        "<|end|>",
        "<think>",
        "</think>",
    ]
    .iter()
    .any(|token| content.contains(token))
}

fn artifact_reference_ids(content: &str) -> BTreeSet<String> {
    let mut refs = BTreeSet::new();
    for found in doi_regex().find_iter(content) {
        let doi = found.as_str().trim_start_matches(|ch: char| {
            ch.eq_ignore_ascii_case(&'d')
                || ch.eq_ignore_ascii_case(&'o')
                || ch.eq_ignore_ascii_case(&'i')
                || matches!(ch, ':' | '：' | ' ')
        });
        refs.insert(format!(
            "doi:{}",
            trim_reference_tail(doi).to_ascii_lowercase()
        ));
    }
    for captures in pmid_regex().captures_iter(content) {
        if let Some(id) = captures.get(1) {
            refs.insert(format!("pmid:{}", id.as_str()));
        }
    }
    for found in url_regex().find_iter(content) {
        let url = trim_reference_tail(found.as_str()).to_ascii_lowercase();
        if let Some(doi) = url.split("doi.org/").nth(1) {
            refs.insert(format!("doi:{}", trim_reference_tail(doi)));
        } else {
            refs.insert(format!("url:{url}"));
        }
    }
    refs
}

fn trim_reference_tail(value: &str) -> &str {
    value.trim_end_matches(|ch: char| {
        matches!(
            ch,
            '.' | ',' | '，' | '。' | ';' | '；' | ')' | '）' | ']' | '】'
        )
    })
}

fn doi_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"(?i)(?:doi\s*[:：]?\s*)?10\.[0-9]{4,9}/[^\s，。；;]+")
            .expect("valid DOI regex")
    })
}

fn pmid_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"(?i)\b(?:pubmed\s*)?pmid\s*[:：]?\s*([0-9]{4,12})\b")
            .expect("valid PMID regex")
    })
}

fn url_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(r"https?://[^\s，。；;]+").expect("valid URL regex"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn research_contract() -> ArtifactQualityContract {
        ArtifactQualityContract::new(
            "research_paper",
            None,
            1,
            None,
            1,
            vec![
                "摘要".to_string(),
                "方法".to_string(),
                "参考文献".to_string(),
            ],
            "research_sections",
            true,
            true,
        )
    }

    #[test]
    fn section_gate_accepts_english_headings_for_chinese_contract_keys() {
        let body = "# A Study\n\n## Abstract\nSummary text.\n\n## Methods\nMethod text.\n\n## References\n[1] https://example.org/source";
        let report = quality_report_with_evidence(
            body,
            &research_contract(),
            "[1] https://example.org/source",
        );
        assert!(report.passed, "{:?}", report.actionable_issues());
    }

    #[test]
    fn citation_gate_requires_receipt_identity_match() {
        let body = "# 标题\n\n摘要\n正文内容足够形成一个段落并保持结构。\n\n方法\n方法内容。\n\n参考文献\n[1] https://fake.example/item";
        let report = quality_report_with_evidence(
            body,
            &research_contract(),
            "[2] https://verified.example/item",
        );
        assert!(!report.passed);
        assert!(report
            .repairable
            .iter()
            .any(|issue| issue.contains("validated_citation_count")));
    }

    #[test]
    fn citation_gate_does_not_count_a_number_and_url_as_two_sources() {
        let mut contract = research_contract();
        contract.min_citations = 2;
        let body = "# 标题\n\n摘要\n正文内容足够形成一个段落并保持结构。\n\n方法\n方法内容。\n\n参考文献\n[1] https://example.org/source";
        let report =
            quality_report_with_evidence(body, &contract, "[1] https://example.org/source");

        assert!(!report.passed);
        assert_eq!(
            report
                .metrics
                .iter()
                .find(|(name, _)| name == "validated_citations")
                .map(|(_, value)| *value),
            Some(1)
        );
    }

    #[test]
    fn missing_evidence_receipt_is_not_sent_into_a_futile_revision_loop() {
        let body = "# 标题\n\n摘要\n正文内容足够形成一个段落并保持结构。\n\n方法\n方法内容。\n\n参考文献\nDOI: 10.1000/example";
        let report = quality_report_with_evidence(body, &research_contract(), "");

        assert!(!report.passed);
        assert!(!report.should_attempt_revision());
    }

    #[test]
    fn self_review_is_a_receipt_not_a_required_document_section() {
        let body = "# 标题\n\n摘要\n正文内容足够形成一个段落并保持结构。\n\n方法\n方法内容。\n\n参考文献\n[1] https://example.org/source";
        let report = quality_report_with_evidence(
            body,
            &research_contract(),
            "[1] https://example.org/source",
        );
        assert!(report.passed);
        assert!(report.review_receipt_required);
        assert!(report
            .to_tool_result_section()
            .contains("quality_review_receipt: pass"));
    }
}
