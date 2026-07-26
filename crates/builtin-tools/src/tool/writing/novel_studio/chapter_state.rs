use super::quality_checks::{
    chapter_would_reach_target, contains_new_open_hook_signal,
    line_contains_placeholder_or_omission_marker,
};
use super::quality_gate::{
    chapter_fact_metadata_issues, chapter_summary_metadata_issues,
    chapter_title_formality_metadata_issues, chapter_title_registry_issues,
};
use super::{
    normalize_chapter_body_for_record, strip_frontmatter, ChapterRecord, NovelProjectManifest,
};
use crate::tool::writing::novel_pipeline::lifecycle as chapter_lifecycle;
use crate::tool::writing::novel_pipeline::{self, NovelPipelineFacts, NovelTransitionDecision};

#[derive(Debug, Clone, Default)]
pub(super) struct DurableChapterProgress {
    pub(super) approved_prefix_chapters: usize,
    pub(super) approved_prefix_units: usize,
    pub(super) next_chapter: usize,
    pub(super) first_unapproved_chapter: Option<usize>,
    pub(super) blockers: Vec<String>,
    pub(super) latest_receipt_present: bool,
    pub(super) latest_receipt_matches_body: bool,
    pub(super) latest_receipt_matches_truth: bool,
    pub(super) latest_receipt_legacy: bool,
}

pub(super) fn count_units(content: &str, language: &str) -> usize {
    let countable = countable_content_without_placeholder_lines(content);
    if language.starts_with("en") {
        countable
            .split_whitespace()
            .filter(|part| !part.is_empty())
            .count()
    } else {
        countable.chars().filter(|ch| !ch.is_whitespace()).count()
    }
}

fn countable_content_without_placeholder_lines(content: &str) -> String {
    content
        .lines()
        .filter(|line| !line_contains_placeholder_or_omission_marker(line.trim()))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn first_unapproved_chapter_number(manifest: &NovelProjectManifest) -> Option<usize> {
    manifest
        .chapters
        .iter()
        .filter(|chapter| !chapter_is_approved(chapter))
        .map(|chapter| chapter.number)
        .min()
}

pub(super) async fn durable_chapter_progress(
    project_dir: &std::path::Path,
    manifest: &NovelProjectManifest,
) -> DurableChapterProgress {
    let mut records = std::collections::BTreeMap::<usize, &ChapterRecord>::new();
    let mut duplicate_numbers = std::collections::BTreeSet::new();
    for chapter in &manifest.chapters {
        if records.insert(chapter.number, chapter).is_some() {
            duplicate_numbers.insert(chapter.number);
        }
    }

    let highest_recorded = records.keys().next_back().copied().unwrap_or(0);
    let mut progress = DurableChapterProgress {
        next_chapter: 1,
        ..DurableChapterProgress::default()
    };

    loop {
        let expected = progress.approved_prefix_chapters + 1;
        let Some(chapter) = records.get(&expected).copied() else {
            progress.next_chapter = expected;
            if highest_recorded >= expected {
                progress.first_unapproved_chapter = Some(expected);
                progress.blockers.push(format!(
                    "chapter {expected} is missing from the manifest before later chapter records"
                ));
            }
            break;
        };

        if duplicate_numbers.contains(&expected) {
            progress.next_chapter = expected;
            progress.first_unapproved_chapter = Some(expected);
            progress.blockers.push(format!(
                "chapter {expected} has duplicate manifest records and cannot establish progress authority"
            ));
            break;
        }

        if !chapter_is_approved(chapter) {
            progress.next_chapter = expected;
            progress.first_unapproved_chapter = Some(expected);
            break;
        }

        let relative_path = std::path::Path::new(&chapter.path);
        if relative_path.is_absolute()
            || relative_path
                .components()
                .any(|part| matches!(part, std::path::Component::ParentDir))
        {
            progress.next_chapter = expected;
            progress.first_unapproved_chapter = Some(expected);
            progress.blockers.push(format!(
                "chapter {expected} has an unsafe body path and cannot establish progress authority"
            ));
            break;
        }

        let raw = match tokio::fs::read_to_string(project_dir.join(relative_path)).await {
            Ok(raw) => raw,
            Err(error) => {
                progress.next_chapter = expected;
                progress.first_unapproved_chapter = Some(expected);
                progress.blockers.push(format!(
                    "chapter {expected} body is unavailable on disk: {error}"
                ));
                break;
            }
        };
        let body = normalize_chapter_body_for_record(&strip_frontmatter(&raw), &chapter.title);
        let units = count_units(&body, &manifest.language);
        if body.trim().is_empty() || units == 0 {
            progress.next_chapter = expected;
            progress.first_unapproved_chapter = Some(expected);
            progress.blockers.push(format!(
                "chapter {expected} body is empty on disk and cannot establish progress authority"
            ));
            break;
        }
        match super::read_approval_receipt(project_dir, expected).await {
            Ok(Some(receipt)) => {
                progress.latest_receipt_present = true;
                progress.latest_receipt_matches_body = receipt.body_fingerprint
                    == super::chapter_quality::chapter_body_fingerprint(&body);
                progress.latest_receipt_matches_truth = !receipt.truth_fingerprint.is_empty()
                    && receipt.truth_fingerprint
                        == super::approval_transaction::approval_truth_fingerprint(manifest);
                progress.latest_receipt_legacy = receipt.legacy;
            }
            _ => {
                progress.latest_receipt_present = false;
                progress.latest_receipt_matches_body = false;
                progress.latest_receipt_matches_truth = false;
                progress.latest_receipt_legacy = false;
            }
        }

        progress.approved_prefix_chapters = expected;
        progress.approved_prefix_units = progress.approved_prefix_units.saturating_add(units);
        progress.next_chapter = expected + 1;
    }

    progress
}

pub(super) fn durable_project_target_reached(
    manifest: &NovelProjectManifest,
    progress: &DurableChapterProgress,
) -> bool {
    manifest
        .target_units
        .filter(|target| *target > 0)
        .is_some_and(|target| progress.approved_prefix_units >= target)
}

pub(super) fn durable_project_completion_blockers(
    manifest: &NovelProjectManifest,
    progress: &DurableChapterProgress,
) -> Vec<String> {
    let mut blockers = progress.blockers.clone();
    if !durable_project_target_reached(manifest, progress) {
        blockers.push("Approved chapter bodies on disk have not reached target_units.".to_string());
    }
    if progress.first_unapproved_chapter.is_some() {
        blockers.push(
            "The contiguous chapter sequence contains an unapproved or missing chapter."
                .to_string(),
        );
    }
    blockers.extend(super::reporting::governed_project_readiness_blockers(
        manifest,
    ));
    blockers.extend(
        crate::tool::writing::novel_bible::story_bible_completion_blockers(
            manifest.story_bible.as_ref(),
        ),
    );
    if progress.approved_prefix_chapters > 0
        && (!progress.latest_receipt_present
            || !progress.latest_receipt_matches_body
            || !progress.latest_receipt_matches_truth
            || progress.latest_receipt_legacy)
    {
        blockers.push(
            "Latest approved chapter lacks a current non-legacy approval receipt matching body and truth."
                .to_string(),
        );
    }
    blockers.sort();
    blockers.dedup();
    blockers
}

pub(super) fn apply_durable_chapter_progress(
    mut state: serde_json::Value,
    manifest: &NovelProjectManifest,
    progress: &DurableChapterProgress,
) -> serde_json::Value {
    let completion_blockers = durable_project_completion_blockers(manifest, progress);
    let target_reached = durable_project_target_reached(manifest, progress);
    state["approved_chapters"] = serde_json::json!(progress.approved_prefix_chapters);
    state["approved_units"] = serde_json::json!(progress.approved_prefix_units);
    state["first_unapproved_chapter"] = serde_json::json!(progress.first_unapproved_chapter);
    state["next_chapter"] = serde_json::json!(progress.next_chapter);
    state["durable_progress_blockers"] = serde_json::json!(progress.blockers);
    state["progress_authority"] = serde_json::json!("contiguous_approved_chapter_bodies_on_disk");
    state["latest_approval_receipt"] = serde_json::json!({
        "present": progress.latest_receipt_present,
        "body_matches": progress.latest_receipt_matches_body,
        "truth_matches": progress.latest_receipt_matches_truth,
        "legacy": progress.latest_receipt_legacy
    });
    state["target_reached"] = serde_json::json!(target_reached);
    state["project_completion_ready"] =
        serde_json::json!(target_reached && completion_blockers.is_empty());
    state["completion_blockers"] = serde_json::json!(completion_blockers);
    state["typed_completion_debts"] = serde_json::json!(
        crate::tool::writing::novel_bible::story_bible_completion_debts(
            manifest.story_bible.as_ref()
        )
    );
    state["progress_ratio"] = serde_json::json!(manifest
        .target_units
        .filter(|target| *target > 0)
        .map(|target| progress.approved_prefix_units as f64 / target as f64));
    state
}

pub(super) fn apply_durable_progress_to_audit(
    mut audit: serde_json::Value,
    manifest: &NovelProjectManifest,
    progress: &DurableChapterProgress,
) -> serde_json::Value {
    let mut blockers = audit
        .get("blockers")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    blockers.extend(
        progress
            .blockers
            .iter()
            .cloned()
            .map(serde_json::Value::String),
    );
    blockers.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
    blockers.dedup();
    audit["passed"] = serde_json::json!(blockers.is_empty());
    audit["blockers"] = serde_json::Value::Array(blockers);
    audit["completion_blockers"] =
        serde_json::json!(durable_project_completion_blockers(manifest, progress));
    audit["progress_authority"] = serde_json::json!("contiguous_approved_chapter_bodies_on_disk");
    audit
}

pub(super) fn next_planned_chapter_number(manifest: &NovelProjectManifest) -> usize {
    manifest
        .chapter_plans
        .iter()
        .map(|plan| plan.number)
        .chain(manifest.chapters.iter().map(|chapter| chapter.number))
        .max()
        .unwrap_or(0)
        + 1
}

pub(super) fn next_unarchitected_planned_chapter_number(manifest: &NovelProjectManifest) -> usize {
    manifest
        .chapter_plans
        .iter()
        .filter(|plan| {
            !manifest
                .chapter_architectures
                .iter()
                .any(|item| item.number == plan.number)
        })
        .map(|plan| plan.number)
        .min()
        .unwrap_or_else(|| next_planned_chapter_number(manifest))
}

pub(super) fn latest_chapter_number(manifest: &NovelProjectManifest) -> Option<usize> {
    manifest.chapters.iter().map(|chapter| chapter.number).max()
}

pub(super) fn chapter_is_approved(chapter: &ChapterRecord) -> bool {
    chapter_lifecycle::status_is_approved(&chapter.status)
}

pub(super) fn project_pipeline_transition(
    manifest: &NovelProjectManifest,
    chapter_number: usize,
    settlement_ready: bool,
    target_reached: bool,
    export_ready: bool,
) -> NovelTransitionDecision {
    let chapter = manifest
        .chapters
        .iter()
        .find(|chapter| chapter.number == chapter_number);
    let chapter_status = chapter.map(|chapter| chapter.status.as_str()).unwrap_or("");
    let audit_passed = chapter
        .is_some_and(|chapter| chapter_review_allows_approval(manifest, chapter_number, chapter));
    let truth_validated = chapter_truth_validation_allows_approval(manifest, chapter_number);
    let chapter_approved = chapter.is_some_and(chapter_is_approved);
    let snapshot_requested =
        chapter_approved && super::snapshot::should_write_auto_chapter_snapshot(chapter_number);
    let snapshot_id = format!("chapter-{chapter_number:04}-approved");

    novel_pipeline::next_transition(&NovelPipelineFacts {
        source_intake_requested: false,
        source_ready: true,
        contract_ready: manifest.contract.is_some(),
        context_ready: manifest
            .context_packages
            .iter()
            .any(|record| record.number == chapter_number),
        execution_package_ready: manifest
            .chapter_contracts
            .iter()
            .any(|record| record.number == chapter_number),
        chapter_exists: chapter.is_some(),
        chapter_needs_revision: chapter_status.eq_ignore_ascii_case("needs_revision")
            || chapter_lifecycle::status_is_rejected(chapter_status),
        chapter_state_repair_required: chapter_lifecycle::status_requires_state_repair(
            chapter_status,
        ),
        audit_passed,
        settlement_ready,
        truth_validated,
        chapter_approved,
        snapshot_requested,
        snapshot_ready: manifest
            .snapshots
            .iter()
            .any(|snapshot| snapshot.id == snapshot_id),
        export_requested: chapter_approved && manifest.export_when_complete && target_reached,
        export_ready,
    })
}

pub(super) fn chapter_ready_for_approval(
    manifest: &NovelProjectManifest,
    chapter_number: usize,
    chapter: &ChapterRecord,
) -> bool {
    chapter_review_allows_approval(manifest, chapter_number, chapter)
        && chapter_truth_validation_allows_approval(manifest, chapter_number)
        && chapter_metadata_allows_approval(manifest, chapter)
}

pub(super) fn chapter_review_allows_approval(
    manifest: &NovelProjectManifest,
    chapter_number: usize,
    _chapter: &ChapterRecord,
) -> bool {
    chapter_has_passed_review(manifest, chapter_number)
}

pub(super) fn chapter_metadata_allows_approval(
    manifest: &NovelProjectManifest,
    chapter: &ChapterRecord,
) -> bool {
    chapter_title_registry_issues(manifest, chapter).is_empty()
        && chapter_summary_metadata_issues(manifest, chapter).is_empty()
        && chapter_fact_metadata_issues(manifest, chapter).is_empty()
        && chapter_title_formality_metadata_issues(manifest, chapter).is_empty()
        && (!chapter_would_reach_target(manifest, chapter)
            || !contains_new_open_hook_signal(&format!(
                "{}\n{}\n{}",
                chapter.summary,
                chapter.key_facts.join("\n"),
                chapter.continuity_updates.join("\n")
            )))
}

pub(super) fn chapter_has_passed_review(
    manifest: &NovelProjectManifest,
    chapter_number: usize,
) -> bool {
    let Some(current_fingerprint) = manifest
        .truth_validations
        .iter()
        .rev()
        .find(|record| record.chapter_number == chapter_number)
        .map(|record| record.chapter_fingerprint.as_str())
        .filter(|fingerprint| !fingerprint.is_empty())
    else {
        return false;
    };
    let Some(authority_fingerprint) = manifest
        .context_packages
        .iter()
        .find(|record| record.number == chapter_number && record.sealed)
        .map(|record| record.authority_root_fingerprint.as_str())
        .filter(|fingerprint| !fingerprint.is_empty())
    else {
        return false;
    };
    manifest
        .reviews
        .iter()
        .rev()
        .find(|review| review.chapter_number == chapter_number)
        .map(|review| {
            review.verdict == "passed"
                && review.locally_validated
                && review.chapter_fingerprint == current_fingerprint
                && review.authority_fingerprint == authority_fingerprint
                && review
                    .findings
                    .iter()
                    .all(|finding| !finding.hard_blocking())
        })
        .unwrap_or(false)
}

pub(super) fn chapter_truth_validation_allows_approval(
    manifest: &NovelProjectManifest,
    chapter_number: usize,
) -> bool {
    manifest
        .truth_validations
        .iter()
        .rev()
        .find(|record| record.chapter_number == chapter_number)
        .map(|record| {
            record.verdict == "passed"
                && record.issues.is_empty()
                && !record.chapter_fingerprint.is_empty()
        })
        .unwrap_or(false)
}

pub(super) fn chapter_blocks_export(chapter: &ChapterRecord) -> bool {
    chapter.status.trim().eq_ignore_ascii_case("needs_revision")
        || chapter_lifecycle::status_requires_state_repair(&chapter.status)
        || chapter_lifecycle::status_is_rejected(&chapter.status)
}

pub(super) fn ensure_export_ready(manifest: &NovelProjectManifest) -> anyhow::Result<()> {
    if manifest.contract.is_none() {
        anyhow::bail!("cannot export governed novel before story contract is set");
    }
    let blocked = manifest
        .chapters
        .iter()
        .filter(|chapter| chapter_blocks_export(chapter))
        .map(|chapter| format!("{}:{}", chapter.number, chapter.status))
        .collect::<Vec<_>>();
    if !blocked.is_empty() {
        anyhow::bail!(
            "cannot export governed novel while chapters need attention: {}",
            blocked.join(", ")
        );
    }
    Ok(())
}
