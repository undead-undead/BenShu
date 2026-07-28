use super::project_cache::TextScanReport;
use super::*;
use serde_json::{json, Value};
use std::path::Path;

pub(super) fn mechanical_chapter_issues(
    manifest: &NovelProjectManifest,
    chapter: &ChapterRecord,
    content: &str,
) -> Vec<String> {
    let scan = TextScanReport::scan(content, &manifest.language);
    mechanical_chapter_issues_with_scan(manifest, chapter, content, &scan)
}

pub(super) fn mechanical_chapter_issues_with_scan(
    manifest: &NovelProjectManifest,
    chapter: &ChapterRecord,
    content: &str,
    scan: &TextScanReport,
) -> Vec<String> {
    let mut issues = Vec::new();
    if chapter.title.trim().is_empty() {
        issues.push("chapter title is missing".to_string());
    }
    if chapter.summary.trim().is_empty() {
        issues.push("chapter summary is missing".to_string());
    }
    if chapter.key_facts.is_empty() {
        issues.push("chapter key facts are missing".to_string());
    }
    if chapter.continuity_updates.is_empty() {
        issues.push("chapter continuity updates are missing".to_string());
    }
    issues.extend(chapter_length_blocking_issues(manifest, chapter, scan));
    if let Some(contract) = &manifest.contract {
        if !stable_manifest_anchor_present(manifest, content) {
            issues.push(
                "chapter does not reference any stable contract character, rule, theme, or premise anchor"
                    .to_string(),
            );
        }
        for banned in &contract.must_avoid {
            let banned = banned.trim();
            if !banned.is_empty() && content.contains(banned) {
                issues.push(format!("chapter contains must_avoid phrase: {banned}"));
            }
        }
    } else {
        issues.push("story contract is missing for drift-controlled chapter audit".to_string());
    }
    issues
}

fn contract_must_avoid_issues(manifest: &NovelProjectManifest, content: &str) -> Vec<String> {
    let Some(contract) = &manifest.contract else {
        return Vec::new();
    };
    contract
        .must_avoid
        .iter()
        .filter_map(|banned| {
            let banned = banned.trim();
            (!banned.is_empty() && content.contains(banned))
                .then(|| format!("chapter contains must_avoid phrase: {banned}"))
        })
        .collect()
}

pub(super) fn chapter_quality_gate(
    manifest: &NovelProjectManifest,
    chapter: &ChapterRecord,
    content: &str,
    truth_issues: &[String],
) -> ChapterQualityGate {
    let scan = TextScanReport::scan(content, &manifest.language);
    chapter_quality_gate_with_scan(manifest, chapter, content, truth_issues, &scan)
}

pub(super) fn chapter_quality_gate_with_scan(
    manifest: &NovelProjectManifest,
    chapter: &ChapterRecord,
    content: &str,
    truth_issues: &[String],
    scan: &TextScanReport,
) -> ChapterQualityGate {
    let authority_fingerprint = manifest
        .context_packages
        .iter()
        .find(|record| record.number == chapter.number && record.sealed)
        .map(|record| record.authority_root_fingerprint.as_str())
        .unwrap_or("");
    let mut findings =
        chapter_length_findings(manifest, chapter, scan, authority_fingerprint, content);
    findings.extend(findings_from_messages(
        prose_surface_contamination_issues(content),
        "body_surface_contamination",
        chapter_quality::ChapterFindingClass::BodyIntegrity,
        chapter_quality::ChapterFindingDisposition::HardBlock,
        chapter_quality::FindingEvidenceGrade::DeterministicInvariant,
        "prose_surface_contamination",
        authority_fingerprint,
        content,
    ));
    let governance_leakage = contract_governance_leakage_report(manifest, content);
    findings.extend(findings_from_messages(
        governance_leakage.blocking,
        "body_surface_contamination",
        chapter_quality::ChapterFindingClass::BodyIntegrity,
        chapter_quality::ChapterFindingDisposition::HardBlock,
        chapter_quality::FindingEvidenceGrade::DeterministicInvariant,
        "contract_governance_leakage",
        authority_fingerprint,
        content,
    ));
    findings.extend(findings_from_messages(
        governance_leakage.warnings,
        "governance_surface_advisory",
        chapter_quality::ChapterFindingClass::Advisory,
        chapter_quality::ChapterFindingDisposition::Warning,
        chapter_quality::FindingEvidenceGrade::Heuristic,
        "contract_governance_leakage",
        authority_fingerprint,
        content,
    ));
    findings.extend(findings_from_messages(
        placeholder_or_omission_issues(content),
        "body_truncated",
        chapter_quality::ChapterFindingClass::BodyIntegrity,
        chapter_quality::ChapterFindingDisposition::HardBlock,
        chapter_quality::FindingEvidenceGrade::DeterministicInvariant,
        "placeholder_or_omission",
        authority_fingerprint,
        content,
    ));
    findings.extend(findings_from_messages(
        chapter_heading_issues(chapter, content),
        "body_surface_contamination",
        chapter_quality::ChapterFindingClass::BodyIntegrity,
        chapter_quality::ChapterFindingDisposition::DeterministicRepair,
        chapter_quality::FindingEvidenceGrade::DeterministicInvariant,
        "chapter_heading",
        authority_fingerprint,
        content,
    ));
    findings.extend(findings_from_messages(
        contract_character_anchor_issues(manifest, chapter, content),
        "character_anchor_advisory",
        chapter_quality::ChapterFindingClass::Advisory,
        chapter_quality::ChapterFindingDisposition::Warning,
        chapter_quality::FindingEvidenceGrade::Heuristic,
        "contract_character_anchor",
        authority_fingerprint,
        content,
    ));
    findings.extend(evidence_backed_character_findings(
        manifest,
        chapter,
        content,
        contract_character_drift_issues(manifest, chapter, content),
        "character_identity_conflict",
        chapter_quality::ChapterFindingClass::Contract,
        "contract_character_drift",
        authority_fingerprint,
    ));
    findings.extend(evidence_backed_character_findings(
        manifest,
        chapter,
        content,
        unregistered_character_candidate_issues(manifest, chapter),
        "unregistered_character",
        chapter_quality::ChapterFindingClass::Contract,
        "unregistered_character",
        authority_fingerprint,
    ));
    findings.extend(evidence_backed_character_findings(
        manifest,
        chapter,
        content,
        contract_character_pronoun_drift_issues(manifest, chapter, content),
        "character_pronoun_conflict",
        chapter_quality::ChapterFindingClass::Continuity,
        "character_pronoun_drift",
        authority_fingerprint,
    ));
    findings.extend(future_chapter_consumption_findings(
        manifest,
        chapter,
        content,
        authority_fingerprint,
    ));
    for (messages, source) in [
        (language_script_issues(manifest, content), "language_script"),
        (cjk_layout_issues(manifest, content), "cjk_layout"),
        (
            cjk_malformed_structural_phrase_issues(content),
            "cjk_malformed_structure",
        ),
        (
            anchor_malformed_predicate_issues(manifest, content),
            "anchor_malformed_predicate",
        ),
    ] {
        findings.extend(findings_from_messages(
            messages,
            "body_surface_contamination",
            chapter_quality::ChapterFindingClass::BodyIntegrity,
            chapter_quality::ChapterFindingDisposition::DeterministicRepair,
            chapter_quality::FindingEvidenceGrade::DeterministicInvariant,
            source,
            authority_fingerprint,
            content,
        ));
    }
    findings.extend(findings_from_messages(
        narrative_substance_issues(manifest, content),
        "narrative_substance_advisory",
        chapter_quality::ChapterFindingClass::Advisory,
        chapter_quality::ChapterFindingDisposition::Warning,
        chapter_quality::FindingEvidenceGrade::Heuristic,
        "narrative_substance",
        authority_fingerprint,
        content,
    ));
    findings.extend(findings_from_messages(
        chapter_progression_contract_issues(manifest, chapter, content),
        "chapter_progression_advisory",
        chapter_quality::ChapterFindingClass::Advisory,
        chapter_quality::ChapterFindingDisposition::Warning,
        chapter_quality::FindingEvidenceGrade::Heuristic,
        "chapter_progression_contract",
        authority_fingerprint,
        content,
    ));
    findings.extend(findings_from_messages(
        chapter_completion_mode_issues(manifest, chapter, content),
        "completion_mode_advisory",
        chapter_quality::ChapterFindingClass::Advisory,
        chapter_quality::ChapterFindingDisposition::Warning,
        chapter_quality::FindingEvidenceGrade::Heuristic,
        "chapter_completion_mode",
        authority_fingerprint,
        content,
    ));
    findings.extend(findings_from_messages(
        contract_must_avoid_issues(manifest, content),
        "world_rule_conflict",
        chapter_quality::ChapterFindingClass::Contract,
        chapter_quality::ChapterFindingDisposition::HardBlock,
        chapter_quality::FindingEvidenceGrade::DeterministicInvariant,
        "contract_must_avoid",
        authority_fingerprint,
        content,
    ));
    findings.extend(findings_from_messages(
        truth_issues
            .iter()
            .map(|issue| format!("truth validation: {issue}"))
            .collect(),
        "metadata_truth_unsupported",
        chapter_quality::ChapterFindingClass::Metadata,
        chapter_quality::ChapterFindingDisposition::DeterministicRepair,
        chapter_quality::FindingEvidenceGrade::DeterministicInvariant,
        "truth_validation",
        authority_fingerprint,
        content,
    ));
    findings.extend(findings_from_messages(
        mechanical_chapter_issues_with_scan(manifest, chapter, content, scan)
            .into_iter()
            .filter(|message| !findings.iter().any(|finding| finding.message == *message))
            .collect(),
        "mechanical_advisory",
        chapter_quality::ChapterFindingClass::Advisory,
        chapter_quality::ChapterFindingDisposition::Warning,
        chapter_quality::FindingEvidenceGrade::Heuristic,
        "mechanical_chapter_checks",
        authority_fingerprint,
        content,
    ));

    ChapterQualityGate::from_findings(findings)
}

fn manifest_chapter_seed(
    manifest: &NovelProjectManifest,
    chapter_number: usize,
) -> Option<(String, String)> {
    if let Some((index, seed)) = manifest
        .contract
        .as_ref()
        .and_then(|contract| contract.authority_contract.as_ref())
        .and_then(|contract| {
            contract
                .outline
                .near_chapters
                .iter()
                .enumerate()
                .find(|(_, seed)| seed.number == Some(chapter_number))
        })
    {
        let text = [seed.goal.as_str(), seed.expected_turn.as_str()]
            .into_iter()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join("；");
        if !text.is_empty() {
            return Some((
                format!("/canonical_contract/outline/near_chapters/{index}"),
                text,
            ));
        }
    }
    manifest
        .story_bible
        .as_ref()
        .and_then(|bible| {
            bible
                .narrative_graph
                .chapter_goals
                .iter()
                .enumerate()
                .find(|(_, goal)| goal.chapter_number == chapter_number)
        })
        .and_then(|(index, goal)| {
            let text = [goal.goal.as_str(), goal.moves_toward_ending.as_str()]
                .into_iter()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>()
                .join("；");
            (!text.is_empty()).then(|| {
                (
                    format!("/truth_as_of_chapter/narrative_graph/chapter_goals/{index}"),
                    text,
                )
            })
        })
}

fn future_chapter_consumption_findings(
    manifest: &NovelProjectManifest,
    chapter: &ChapterRecord,
    content: &str,
    authority_fingerprint: &str,
) -> Vec<chapter_quality::ChapterFinding> {
    let Some(next_number) = chapter.number.checked_add(1) else {
        return Vec::new();
    };
    let Some((_, current_seed)) = manifest_chapter_seed(manifest, chapter.number) else {
        return Vec::new();
    };
    let Some((next_path, next_seed)) = manifest_chapter_seed(manifest, next_number) else {
        return Vec::new();
    };
    let cjk = is_chinese_language(&manifest.language);
    let Some(excerpt) =
        governance::final_body_future_consumption_evidence(content, &current_seed, &next_seed, cjk)
    else {
        return Vec::new();
    };
    let Some(start) = content.find(&excerpt) else {
        return Vec::new();
    };
    vec![chapter_quality::ChapterFinding {
        code: "future_chapter_consumed".to_string(),
        class: chapter_quality::ChapterFindingClass::Continuity,
        disposition: chapter_quality::ChapterFindingDisposition::HardBlock,
        evidence_grade: chapter_quality::FindingEvidenceGrade::EvidenceBackedSemantic,
        source: "sealed_next_chapter_boundary".to_string(),
        message: format!(
            "chapter {} consumes the sealed chapter {} boundary early",
            chapter.number, next_number
        ),
        authority_evidence: vec![chapter_quality::AuthorityEvidenceRef {
            path: next_path,
            excerpt: next_seed,
        }],
        body_evidence: vec![chapter_quality::BodyEvidenceSpan {
            start,
            end: start + excerpt.len(),
            excerpt,
        }],
        authority_fingerprint: authority_fingerprint.to_string(),
        body_fingerprint: chapter_quality::chapter_body_fingerprint(content),
    }]
}

fn findings_from_messages(
    messages: Vec<String>,
    code: &str,
    class: chapter_quality::ChapterFindingClass,
    disposition: chapter_quality::ChapterFindingDisposition,
    evidence_grade: chapter_quality::FindingEvidenceGrade,
    source: &str,
    authority_fingerprint: &str,
    content: &str,
) -> Vec<chapter_quality::ChapterFinding> {
    chapter_quality::finalize_issues(messages)
        .into_iter()
        .map(|message| {
            chapter_quality::ChapterFinding::local(
                code,
                class,
                disposition,
                evidence_grade,
                source,
                message,
                authority_fingerprint,
                content,
            )
        })
        .collect()
}

fn evidence_backed_character_findings(
    manifest: &NovelProjectManifest,
    chapter: &ChapterRecord,
    content: &str,
    messages: Vec<String>,
    code: &str,
    class: chapter_quality::ChapterFindingClass,
    source: &str,
    authority_fingerprint: &str,
) -> Vec<chapter_quality::ChapterFinding> {
    let canonical_contract =
        serde_json::to_value(&manifest.structured_contract_v2).unwrap_or(Value::Null);
    let chapter_contract = manifest
        .chapter_contracts
        .iter()
        .find(|record| record.number == chapter.number);
    chapter_quality::finalize_issues(messages)
        .into_iter()
        .map(|message| {
            let terms = backtick_terms(&message);
            let authority_evidence = terms
                .iter()
                .find_map(|term| {
                    find_json_string_path(&canonical_contract, term, "").map(|path| {
                        chapter_quality::AuthorityEvidenceRef {
                            path: format!("/canonical_contract{path}"),
                            excerpt: term.clone(),
                        }
                    })
                })
                .or_else(|| {
                    (source == "unregistered_character").then(|| {
                        let registrations = chapter_contract
                            .map(|record| &record.character_registrations)
                            .cloned()
                            .unwrap_or_default();
                        chapter_quality::AuthorityEvidenceRef {
                            path: "/chapter_contract/character_registrations".to_string(),
                            excerpt: serde_json::to_string(&registrations)
                                .unwrap_or_else(|_| "[]".to_string()),
                        }
                    })
                });
            let body_evidence = terms
                .iter()
                .find_map(|term| body_evidence_around(content, term));
            let grounded = authority_evidence.is_some() && body_evidence.is_some();
            chapter_quality::ChapterFinding {
                code: code.to_string(),
                class: if grounded {
                    class
                } else {
                    chapter_quality::ChapterFindingClass::Advisory
                },
                disposition: if grounded {
                    chapter_quality::ChapterFindingDisposition::HardBlock
                } else {
                    chapter_quality::ChapterFindingDisposition::Warning
                },
                evidence_grade: if grounded {
                    chapter_quality::FindingEvidenceGrade::EvidenceBackedSemantic
                } else {
                    chapter_quality::FindingEvidenceGrade::Heuristic
                },
                source: source.to_string(),
                message,
                authority_evidence: authority_evidence.into_iter().collect(),
                body_evidence: body_evidence.into_iter().collect(),
                authority_fingerprint: authority_fingerprint.to_string(),
                body_fingerprint: chapter_quality::chapter_body_fingerprint(content),
            }
        })
        .collect()
}

fn backtick_terms(message: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut rest = message;
    while let Some(start) = rest.find('`') {
        let after_start = &rest[start + 1..];
        let Some(end) = after_start.find('`') else {
            break;
        };
        let term = after_start[..end].trim();
        if !term.is_empty() {
            terms.push(term.to_string());
        }
        rest = &after_start[end + 1..];
    }
    terms
}

fn find_json_string_path(value: &Value, needle: &str, path: &str) -> Option<String> {
    match value {
        Value::String(text) if text.contains(needle) => Some(path.to_string()),
        Value::Array(items) => items.iter().enumerate().find_map(|(index, item)| {
            find_json_string_path(item, needle, &format!("{path}/{index}"))
        }),
        Value::Object(items) => items.iter().find_map(|(key, item)| {
            let escaped = key.replace('~', "~0").replace('/', "~1");
            find_json_string_path(item, needle, &format!("{path}/{escaped}"))
        }),
        _ => None,
    }
}

fn body_evidence_around(content: &str, term: &str) -> Option<chapter_quality::BodyEvidenceSpan> {
    let term_start = content.find(term)?;
    let term_end = term_start + term.len();
    let start = content[..term_start]
        .char_indices()
        .rev()
        .find(|(_, ch)| matches!(ch, '。' | '！' | '？' | '\n'))
        .map(|(index, ch)| index + ch.len_utf8())
        .unwrap_or(0);
    let end = content[term_end..]
        .char_indices()
        .find(|(_, ch)| matches!(ch, '。' | '！' | '？' | '\n'))
        .map(|(index, ch)| term_end + index + ch.len_utf8())
        .unwrap_or(content.len());
    Some(chapter_quality::BodyEvidenceSpan {
        start,
        end,
        excerpt: content[start..end].to_string(),
    })
}

fn chapter_length_findings(
    manifest: &NovelProjectManifest,
    chapter: &ChapterRecord,
    scan: &TextScanReport,
    authority_fingerprint: &str,
    content: &str,
) -> Vec<chapter_quality::ChapterFinding> {
    let Some(target) = manifest.chapter_unit_target.filter(|target| *target > 0) else {
        return Vec::new();
    };
    let measured_units = chapter.unit_count.max(scan.units);
    let mut findings = Vec::new();
    if measured_units < target {
        findings.push(chapter_quality::ChapterFinding::local(
            "length_below_minimum",
            chapter_quality::ChapterFindingClass::Length,
            chapter_quality::ChapterFindingDisposition::HardBlock,
            chapter_quality::FindingEvidenceGrade::DeterministicInvariant,
            "chapter_length",
            format!(
                "chapter length is below minimum target: {} of {} units",
                measured_units, target
            ),
            authority_fingerprint,
            content,
        ));
    }
    let maximum = longform_policy::chapter_tier_max_units(target);
    if measured_units > maximum {
        findings.push(chapter_quality::ChapterFinding::local(
            "length_above_tier_maximum",
            chapter_quality::ChapterFindingClass::Length,
            chapter_quality::ChapterFindingDisposition::HardBlock,
            chapter_quality::FindingEvidenceGrade::DeterministicInvariant,
            "chapter_length",
            format!(
                "chapter length exceeds maximum for the selected tier: {} units; {}-unit chapters may not exceed {} units",
                measured_units, target, maximum
            ),
            authority_fingerprint,
            content,
        ));
    }
    findings
}

fn chapter_length_blocking_issues(
    manifest: &NovelProjectManifest,
    chapter: &ChapterRecord,
    scan: &TextScanReport,
) -> Vec<String> {
    chapter_length_findings(manifest, chapter, scan, "", "")
        .into_iter()
        .map(|finding| finding.message)
        .collect()
}

pub(super) fn chapter_metadata_gate(
    manifest: &NovelProjectManifest,
    chapter: &ChapterRecord,
    content: &str,
) -> ChapterMetadataGate {
    let authority_fingerprint = manifest
        .context_packages
        .iter()
        .find(|record| record.number == chapter.number && record.sealed)
        .map(|record| record.authority_root_fingerprint.as_str())
        .unwrap_or("");
    let checks = [
        (
            "chapter_title",
            chapter_title_repair_issues(manifest, chapter),
        ),
        (
            "chapter_title_registry",
            chapter_title_registry_issues(manifest, chapter),
        ),
        (
            "chapter_summary",
            chapter_summary_metadata_issues(manifest, chapter),
        ),
        (
            "chapter_summary_support",
            chapter_summary_content_support_issues(chapter, content, &manifest.language),
        ),
        (
            "chapter_fact_metadata",
            chapter_fact_metadata_issues(manifest, chapter),
        ),
        (
            "chapter_title_formality",
            chapter_title_formality_metadata_issues(manifest, chapter),
        ),
        (
            "chapter_title_body_fragment",
            chapter_title_body_fragment_metadata_issues(manifest, chapter, content),
        ),
        (
            "chapter_title_completion",
            chapter_title_completion_issues(manifest, chapter, content),
        ),
    ];
    let mut findings = checks
        .into_iter()
        .flat_map(|(source, messages)| {
            findings_from_messages(
                messages,
                "metadata_invalid",
                chapter_quality::ChapterFindingClass::Metadata,
                chapter_quality::ChapterFindingDisposition::DeterministicRepair,
                chapter_quality::FindingEvidenceGrade::DeterministicInvariant,
                source,
                authority_fingerprint,
                content,
            )
        })
        .collect::<Vec<_>>();
    findings.extend(findings_from_messages(
        chapter_title_fatigue_issues(manifest, chapter),
        "chapter_title_fatigue_advisory",
        chapter_quality::ChapterFindingClass::Advisory,
        chapter_quality::ChapterFindingDisposition::Warning,
        chapter_quality::FindingEvidenceGrade::Heuristic,
        "chapter_title_fatigue",
        authority_fingerprint,
        content,
    ));
    ChapterMetadataGate::from_findings(findings)
}

pub(super) fn chapter_outcome_status(
    quality_gate: &ChapterQualityGate,
    metadata_gate: &ChapterMetadataGate,
) -> &'static str {
    if !quality_gate.passed {
        "needs_revision"
    } else if metadata_gate.blocking() {
        "metadata_blocked"
    } else if metadata_gate.needs_repair() {
        "metadata_repair"
    } else {
        "accepted"
    }
}

pub(super) fn chapter_completion_gate_json(
    accepted: bool,
    outcome_status: &str,
) -> serde_json::Value {
    json!({
        "passed": accepted,
        "outcome_status": outcome_status,
        "can_finalize_answer": accepted,
        "requires_followup": !accepted,
        "reason": if accepted {
            "chapter body and metadata are ready for the next governed step"
        } else {
            "chapter still requires revision, metadata repair, or approval before it can feed later context"
        }
    })
}

fn chapter_title_repair_issues(
    manifest: &NovelProjectManifest,
    chapter: &ChapterRecord,
) -> Vec<String> {
    let title = chapter.title.trim();
    let mut issues = Vec::new();
    if title.is_empty() {
        issues.push("chapter title is empty".to_string());
    }
    if title_is_default_chapter_heading(title, chapter.number, &manifest.language) {
        issues.push("chapter title is still the default chapter heading".to_string());
    }
    if !title_has_enough_signal(title) {
        issues.push("chapter title has too little signal".to_string());
    }
    if chapter_title_is_generic_stage_label(title) {
        issues.push("chapter title is a generic stage or workflow label".to_string());
    }
    if is_chinese_language(&manifest.language) {
        if let Some(issue) = chinese_title_language_issues(title) {
            issues.push(format!(
                "Chinese-language chapter title violates language contract: {issue}"
            ));
        }
    }
    if title_matches_project_or_volume(manifest, title) {
        issues.push("chapter title repeats the project title or volume title".to_string());
    }
    chapter_quality::finalize_issues(issues)
}

pub(super) fn chapter_summary_metadata_issues(
    manifest: &NovelProjectManifest,
    chapter: &ChapterRecord,
) -> Vec<String> {
    let summary = chapter.summary.trim();
    if summary.is_empty() {
        return vec!["chapter summary is empty; repair metadata from the body".to_string()];
    }
    if summary.chars().count() < 12 {
        return vec![
            "chapter summary is too short to guide continuity; repair metadata from the body"
                .to_string(),
        ];
    }
    if chapter_summary_looks_like_prose_fragment(summary, &manifest.language) {
        return vec![
            "chapter summary looks like a prose/dialogue fragment; repair metadata from the body"
                .to_string(),
        ];
    }
    if !chapter_summary_has_authority_anchor(manifest, summary) {
        return vec![
            "chapter summary does not identify an authoritative character; repair metadata from the body"
                .to_string(),
        ];
    }
    Vec::new()
}

pub(super) fn chapter_summary_content_support_issues(
    chapter: &ChapterRecord,
    content: &str,
    language: &str,
) -> Vec<String> {
    if chapter.summary.trim().is_empty()
        || chapter.summary.trim().chars().count() < 12
        || chapter_summary_looks_like_prose_fragment(&chapter.summary, language)
    {
        return Vec::new();
    }

    if !chapter_summary_supported_by_content(&chapter.summary, content, language) {
        return vec![
            "chapter summary is not supported by this chapter body; repair metadata from the body"
                .to_string(),
        ];
    }

    let truth_text = chapter
        .key_facts
        .iter()
        .chain(chapter.continuity_updates.iter())
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("\n");
    let summarizes_truth = chapter_summary_supported_by_truth_items(chapter, language)
        || (!truth_text.trim().is_empty()
            && chapter_summary_supported_by_content(&chapter.summary, &truth_text, language));
    if !summarizes_truth {
        return vec![
            "chapter summary describes a body fragment but does not cover the chapter's key facts or continuity change; repair metadata from the current chapter outcome"
                .to_string(),
        ];
    }
    Vec::new()
}

pub(super) fn chapter_fact_metadata_issues(
    manifest: &NovelProjectManifest,
    chapter: &ChapterRecord,
) -> Vec<String> {
    let mut issues = Vec::new();
    if chapter.key_facts.is_empty() {
        issues.push("chapter key_facts are empty; repair metadata from the body".to_string());
    }
    if chapter.continuity_updates.is_empty() {
        issues.push(
            "chapter continuity_updates are empty; repair metadata from the body".to_string(),
        );
    }
    if metadata_items_are_fully_reused_from_prior_approved_chapter(
        manifest,
        chapter,
        &chapter.key_facts,
    ) {
        issues.push(
            "chapter key_facts are fully reused from a prior approved chapter; repair metadata from the current body"
                .to_string(),
        );
    }
    if metadata_items_are_fully_reused_from_prior_approved_chapter(
        manifest,
        chapter,
        &chapter.continuity_updates,
    ) {
        issues.push(
            "chapter continuity_updates are fully reused from a prior approved chapter; repair metadata from the current body"
                .to_string(),
        );
    }
    issues
}

fn metadata_items_are_fully_reused_from_prior_approved_chapter(
    manifest: &NovelProjectManifest,
    chapter: &ChapterRecord,
    current: &[String],
) -> bool {
    let current = current
        .iter()
        .map(|item| normalize_duplicate_probe_text(item))
        .filter(|item| !item.is_empty())
        .collect::<BTreeSet<_>>();
    if current.len() < 2 {
        return false;
    }
    manifest.chapters.iter().any(|other| {
        if other.number == chapter.number || !chapter_lifecycle::status_is_approved(&other.status) {
            return false;
        }
        let prior = other
            .key_facts
            .iter()
            .chain(other.continuity_updates.iter())
            .map(|item| normalize_duplicate_probe_text(item))
            .filter(|item| !item.is_empty())
            .collect::<BTreeSet<_>>();
        current.iter().all(|item| prior.contains(item))
    })
}

pub(super) fn chapter_title_formality_metadata_issues(
    manifest: &NovelProjectManifest,
    chapter: &ChapterRecord,
) -> Vec<String> {
    if !is_chinese_language(&manifest.language) {
        return Vec::new();
    }
    naming::title_formality_issue(&chapter.title, "章节标题")
        .map(|issue| {
            format!(
                "{issue}；请根据本章已写正文的不可逆事件、关键地点、物件、选择或关系变化只修标题，不重写正文"
            )
        })
        .into_iter()
        .collect()
}

pub(super) fn chapter_title_body_fragment_metadata_issues(
    manifest: &NovelProjectManifest,
    chapter: &ChapterRecord,
    content: &str,
) -> Vec<String> {
    naming::title_body_fragment_issue(&manifest.language, &chapter.title, content)
        .map(|issue| format!("{issue}; repair metadata only and keep the approved body unchanged"))
        .into_iter()
        .collect()
}

pub(super) fn extend_quality_gate_issues(
    gate: &mut ChapterQualityGate,
    manifest: &NovelProjectManifest,
    chapter: &ChapterRecord,
    content: &str,
    issues: Vec<String>,
) {
    let authority_fingerprint = manifest
        .context_packages
        .iter()
        .find(|record| record.number == chapter.number && record.sealed)
        .map(|record| record.authority_root_fingerprint.as_str())
        .unwrap_or("");
    gate.extend_findings(findings_from_messages(
        issues,
        "body_surface_contamination",
        chapter_quality::ChapterFindingClass::BodyIntegrity,
        chapter_quality::ChapterFindingDisposition::DeterministicRepair,
        chapter_quality::FindingEvidenceGrade::DeterministicInvariant,
        "pre_sanitized_content",
        authority_fingerprint,
        content,
    ));
}

pub(super) async fn cross_chapter_duplicate_issues(
    project_dir: &Path,
    manifest: &NovelProjectManifest,
    chapter: &ChapterRecord,
    content: &str,
) -> Vec<chapter_quality::ChapterFinding> {
    let mut findings = Vec::new();
    let authority_fingerprint =
        read_sealed_chapter_authority(project_dir, manifest, chapter.number)
            .await
            .map(|authority| authority.authority_root_fingerprint)
            .unwrap_or_else(|_| {
                governance::authority_fingerprint(&canonical_project_contract_projection(manifest))
            });
    let advisory = |message: String| {
        chapter_quality::ChapterFinding::local(
            "cross_chapter_similarity",
            chapter_quality::ChapterFindingClass::Advisory,
            chapter_quality::ChapterFindingDisposition::Warning,
            chapter_quality::FindingEvidenceGrade::Heuristic,
            "cross_chapter_duplicate",
            message,
            authority_fingerprint.clone(),
            content,
        )
    };
    let current_body =
        normalize_duplicate_probe_text(&normalize_chapter_body_for_record(content, &chapter.title));
    let current_summary = normalize_duplicate_probe_text(&chapter.summary);
    let mut prior = manifest
        .chapters
        .iter()
        .filter(|other| other.number != chapter.number)
        .cloned()
        .collect::<Vec<_>>();
    prior.sort_by_key(|other| std::cmp::Reverse(other.number));
    for other in prior.into_iter().take(8) {
        if !current_summary.is_empty() {
            let summary_score = text_shingle_similarity(
                &current_summary,
                &normalize_duplicate_probe_text(&other.summary),
            );
            if summary_score >= 0.86 {
                findings.push(advisory(format!(
                    "chapter summary is too similar to chapter {}",
                    other.number
                )));
            } else if chapter_is_completion_mode_candidate(manifest, chapter)
                && summary_score >= 0.55
            {
                findings.push(advisory(format!(
                    "completion tail summary repeats chapter {} too closely instead of closing a distinct unresolved debt",
                    other.number
                )));
            }
        }

        if current_body.chars().count() < 500 {
            continue;
        }
        let other_raw = tokio::fs::read_to_string(project_dir.join(&other.path))
            .await
            .unwrap_or_default();
        let other_body = normalize_duplicate_probe_text(&normalize_chapter_body_for_record(
            &strip_frontmatter(&other_raw),
            &other.title,
        ));
        if other_body.chars().count() < 500 {
            continue;
        }
        if current_body == other_body {
            findings.push(chapter_quality::ChapterFinding::local(
                "cross_chapter_exact_duplicate",
                chapter_quality::ChapterFindingClass::BodyIntegrity,
                chapter_quality::ChapterFindingDisposition::HardBlock,
                chapter_quality::FindingEvidenceGrade::DeterministicInvariant,
                "cross_chapter_duplicate",
                format!("chapter body is identical to chapter {}", other.number),
                authority_fingerprint.clone(),
                content,
            ));
            continue;
        }
        let body_score = text_shingle_similarity(&current_body, &other_body);
        if body_score >= 0.72 {
            findings.push(advisory(format!(
                "chapter body is too similar to chapter {}",
                other.number
            )));
        } else if chapter_is_completion_mode_candidate(manifest, chapter) && body_score >= 0.45 {
            findings.push(advisory(format!(
                "completion tail body repeats chapter {} too closely; close remaining story debts instead of rephrasing the same aftermath",
                other.number
            )));
        }
    }
    findings.sort_by(|left, right| (&left.code, &left.message).cmp(&(&right.code, &right.message)));
    findings.dedup_by(|left, right| left.code == right.code && left.message == right.message);
    findings
}

pub(super) fn chapter_title_registry_issues(
    manifest: &NovelProjectManifest,
    chapter: &ChapterRecord,
) -> Vec<String> {
    naming::chapter_title_registry_issues(
        chapter.number,
        &chapter.title,
        manifest
            .chapters
            .iter()
            .map(|other| (other.number, other.title.clone())),
    )
}

pub(super) fn chapter_title_fatigue_issues(
    manifest: &NovelProjectManifest,
    chapter: &ChapterRecord,
) -> Vec<String> {
    naming::chapter_title_fatigue_issues(
        &manifest.language,
        chapter.number,
        &chapter.title,
        manifest
            .chapters
            .iter()
            .filter(|other| chapter_is_title_reference_candidate(other))
            .map(|other| (other.number, other.title.clone())),
    )
}

fn chapter_title_completion_issues(
    manifest: &NovelProjectManifest,
    chapter: &ChapterRecord,
    content: &str,
) -> Vec<String> {
    let evidence = naming::ChapterTitleEvidence::new(
        manifest.language.clone(),
        chapter.summary.clone(),
        chapter.key_facts.clone(),
        chapter.continuity_updates.clone(),
        content.to_string(),
    );
    let decision = naming::evaluate_chapter_title_candidate(
        naming::ChapterTitleCandidate::new(&chapter.title),
        &evidence,
    );
    if decision.accepted {
        return decision
            .warnings
            .into_iter()
            .map(|warning| format!("章节标题语义证据告警：{warning}"))
            .collect();
    }

    let mut detail = decision.reasons.join("；");
    if detail.trim().is_empty() {
        detail = "chapter title is not grounded in chapter evidence".to_string();
    }
    if !decision.warnings.is_empty() {
        detail.push_str("；");
        detail.push_str(&decision.warnings.join("；"));
    }
    let repair_hint = if decision.repairable {
        "请写完正文后用本章实际内容重取标题"
    } else {
        "请检查标题元数据"
    };
    vec![format!(
        "章节标题没有被本章摘要、关键事实或正文事件支撑；命名 authority 诊断：{detail}；{repair_hint}"
    )]
}
