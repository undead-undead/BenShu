use serde_json::json;
use std::collections::{BTreeSet, HashMap};

use super::chapter_state::{chapter_is_approved, first_unapproved_chapter_number};
use super::quality_checks::{
    manifest_character_anchors, project_title_registry_warnings, title_has_enough_signal,
};
use super::{NovelProjectManifest, ACTIVE_CONTINUITY_CHAPTER_LIMIT};
use crate::tool::writing::{novel_bible, policy};

pub(super) fn audit_manifest(manifest: &NovelProjectManifest) -> serde_json::Value {
    let mut blockers = Vec::new();
    let mut warnings = Vec::new();
    if manifest.sources.is_empty() {
        warnings.push("No source material has been attached to this project.".to_string());
    }
    if manifest.contract.is_none() {
        blockers.push("Story contract is missing. Set premise, characters/rules, and outline before long-form drafting.".to_string());
    } else if let Some(contract) = &manifest.contract {
        blockers.extend(novel_bible::story_contract_blockers(contract));
    }
    blockers.extend(character_authority_graph_blockers(manifest));
    let (bible_blockers, bible_warnings) =
        novel_bible::story_bible_audit(manifest.story_bible.as_ref());
    blockers.extend(bible_blockers);
    warnings.extend(bible_warnings);
    if manifest.truth_files.is_empty() {
        warnings.push("No truth/control files have been recorded yet.".to_string());
    }
    let mut numbers = BTreeSet::new();
    for chapter in &manifest.chapters {
        if !numbers.insert(chapter.number) {
            blockers.push(format!("Duplicate chapter number: {}", chapter.number));
        }
        if chapter.summary.trim().is_empty() {
            warnings.push(format!("Chapter {} has no summary.", chapter.number));
        }
        if chapter.title.trim().is_empty() {
            blockers.push(format!("Chapter {} has no title.", chapter.number));
        }
        if chapter.status.trim().eq_ignore_ascii_case("needs_revision") {
            warnings.push(format!("Chapter {} needs revision.", chapter.number));
        }
        if manifest
            .reviews
            .iter()
            .all(|review| review.chapter_number != chapter.number)
        {
            warnings.push(format!("Chapter {} has no review record.", chapter.number));
        }
        if manifest
            .truth_validations
            .iter()
            .all(|record| record.chapter_number != chapter.number)
        {
            warnings.push(format!(
                "Chapter {} has no truth validation record.",
                chapter.number
            ));
        }
        if manifest
            .truth_validations
            .iter()
            .rev()
            .find(|record| record.chapter_number == chapter.number)
            .map(|record| record.verdict != "passed")
            .unwrap_or(false)
        {
            warnings.push(format!(
                "Chapter {} truth validation needs attention.",
                chapter.number
            ));
        }
    }
    warnings.extend(project_title_registry_warnings(manifest));
    for plan in &manifest.chapter_plans {
        if manifest
            .chapter_contracts
            .iter()
            .all(|record| record.number != plan.number)
        {
            warnings.push(format!(
                "Chapter plan {} has no control contract.",
                plan.number
            ));
        }
        if manifest
            .context_packages
            .iter()
            .all(|record| record.number != plan.number)
        {
            warnings.push(format!(
                "Chapter plan {} has no context package.",
                plan.number
            ));
        }
        if manifest
            .chapters
            .iter()
            .all(|chapter| chapter.number != plan.number)
        {
            warnings.push(format!(
                "Chapter plan {} has not been drafted yet.",
                plan.number
            ));
        }
    }
    for architecture in &manifest.chapter_architectures {
        if manifest
            .chapter_plans
            .iter()
            .all(|plan| plan.number != architecture.number)
        {
            warnings.push(format!(
                "Chapter architecture {} has no matching planner record.",
                architecture.number
            ));
        }
        if manifest
            .chapters
            .iter()
            .all(|chapter| chapter.number != architecture.number)
        {
            warnings.push(format!(
                "Chapter architecture {} has not been drafted yet.",
                architecture.number
            ));
        }
    }
    json!({
        "passed": blockers.is_empty(),
        "blockers": blockers,
        "warnings": warnings,
        "writing_policy": policy::fiction_project_policy(
            manifest.sources.len(),
            manifest.contract.is_some(),
            manifest.truth_files.len(),
            manifest.chapter_plans.len(),
            manifest.chapter_architectures.len(),
            manifest.chapters.len(),
            manifest
                .chapters
                .iter()
                .max_by_key(|chapter| chapter.number)
                .map(|chapter| chapter.status.trim().eq_ignore_ascii_case("needs_revision"))
                .unwrap_or(false),
        )
    })
}

pub(super) fn governed_project_readiness_blockers(manifest: &NovelProjectManifest) -> Vec<String> {
    let mut blockers = Vec::new();
    match manifest.contract.as_ref() {
        Some(contract) => blockers.extend(novel_bible::story_contract_blockers(contract)),
        None => blockers.push("Story contract is missing.".to_string()),
    }
    let (bible_blockers, _) = novel_bible::story_bible_audit(manifest.story_bible.as_ref());
    blockers.extend(bible_blockers);
    blockers.extend(character_authority_graph_blockers(manifest));
    blockers.sort();
    blockers.dedup();
    blockers
}

fn character_authority_graph_blockers(manifest: &NovelProjectManifest) -> Vec<String> {
    let mut blockers = Vec::new();
    let mut ids = BTreeSet::new();
    let mut names = BTreeSet::new();
    for character in &manifest.character_ledger {
        if character.id.trim().is_empty() {
            blockers.push(format!(
                "Character authority `{}` has no stable id.",
                character.canonical_name
            ));
        } else if !ids.insert(character.id.clone()) {
            blockers.push(format!("Character authority reuses id `{}`.", character.id));
        }
        if character.canonical_name.trim().is_empty() {
            blockers.push("Character authority has an empty canonical name.".to_string());
        } else if !names.insert(character.canonical_name.clone()) {
            blockers.push(format!(
                "Character authority repeats canonical name `{}`.",
                character.canonical_name
            ));
        }
    }
    for relationship in &manifest.structured_contract_v2.relationship_ledger {
        if relationship.characters.len() != relationship.character_ids.len() {
            blockers
                .push("Relationship authority has unresolved character id references.".to_string());
            continue;
        }
        for id in &relationship.character_ids {
            if !ids.contains(id) {
                blockers.push(format!(
                    "Relationship authority references unknown character id `{id}`."
                ));
            }
        }
    }
    blockers
}

pub(super) fn analytics_report(manifest: &NovelProjectManifest) -> serde_json::Value {
    let mut status_counts: HashMap<String, usize> = HashMap::new();
    for chapter in &manifest.chapters {
        *status_counts.entry(chapter.status.clone()).or_default() += 1;
    }
    let mut issue_counts: HashMap<String, usize> = HashMap::new();
    for review in &manifest.reviews {
        for issue in &review.issues {
            *issue_counts.entry(issue.clone()).or_default() += 1;
        }
    }
    let total_units: usize = manifest
        .chapters
        .iter()
        .map(|chapter| chapter.unit_count)
        .sum();
    let average_units = if manifest.chapters.is_empty() {
        None
    } else {
        Some(total_units as f64 / manifest.chapters.len() as f64)
    };
    json!({
        "status_counts": status_counts,
        "issue_counts": issue_counts,
        "total_units": total_units,
        "average_chapter_units": average_units,
        "review_pass_rate": review_pass_rate(manifest),
        "longest_chapter": manifest.chapters.iter().max_by_key(|chapter| chapter.unit_count),
        "shortest_chapter": manifest.chapters.iter().min_by_key(|chapter| chapter.unit_count)
    })
}

pub(super) fn writing_governance_report(manifest: &NovelProjectManifest) -> serde_json::Value {
    let approved_chapters = manifest
        .chapters
        .iter()
        .filter(|chapter| chapter_is_approved(chapter))
        .count();
    let has_story_bible = manifest.story_bible.is_some();
    let story_bible = manifest.story_bible.as_ref();
    let has_ending_contract = story_bible
        .map(|bible| {
            !bible.ending_contract.desired_resolution.trim().is_empty()
                && !bible.ending_contract.final_state.trim().is_empty()
        })
        .unwrap_or(false);
    let has_world_database = story_bible
        .map(|bible| !bible.world_database.rules.is_empty())
        .unwrap_or(false);
    let has_character_anchors = !manifest_character_anchors(manifest).is_empty();
    let has_volume_graph = !manifest.volumes.is_empty()
        && story_bible
            .map(|bible| !bible.narrative_graph.volume_arcs.is_empty())
            .unwrap_or(false);
    let has_context_packages = approved_chapters == 0
        || manifest.context_packages.iter().any(|context| {
            manifest
                .chapters
                .iter()
                .any(|chapter| chapter.number == context.number && chapter_is_approved(chapter))
        });
    let has_quality_gate = approved_chapters == 0
        || manifest.reviews.iter().any(|review| {
            manifest.chapters.iter().any(|chapter| {
                chapter.number == review.chapter_number && chapter_is_approved(chapter)
            })
        });
    let approval_truth_clean = first_unapproved_chapter_number(manifest).is_none()
        || !manifest.truth_files.iter().any(|truth| {
            truth.section.eq_ignore_ascii_case("chapter_summaries")
                && first_unapproved_chapter_number(manifest)
                    .is_some_and(|number| truth.path.contains(&number.to_string()))
        });
    let has_hook_debt = approved_chapters == 0 || !manifest.hook_debt_reports.is_empty();
    let has_archive_budget = manifest.chapters.len() <= ACTIVE_CONTINUITY_CHAPTER_LIMIT
        || manifest
            .archives
            .iter()
            .any(|archive| archive.kind.eq_ignore_ascii_case("arc"))
        || manifest
            .archives
            .iter()
            .any(|archive| archive.kind.eq_ignore_ascii_case("volume"));
    let naming_warnings = project_title_registry_warnings(manifest);
    let naming_governed = title_has_enough_signal(&manifest.title)
        && manifest
            .chapters
            .iter()
            .filter(|chapter| chapter_is_approved(chapter))
            .all(|chapter| title_has_enough_signal(&chapter.title))
        && naming_warnings.is_empty();
    let axes = vec![
        governance_axis(
            "story_contract",
            "Project-level story contract",
            manifest.contract.is_some() && governed_project_readiness_blockers(manifest).is_empty(),
            json!({
                "has_contract": manifest.contract.is_some(),
                "readiness_blockers": governed_project_readiness_blockers(manifest)
            }),
        ),
        governance_axis(
            "ending_first_design",
            "Ending-first reverse design",
            has_story_bible && has_ending_contract,
            json!({
                "has_story_bible": has_story_bible,
                "has_ending_contract": has_ending_contract
            }),
        ),
        governance_axis(
            "world_and_character_authority",
            "World database and character anchors",
            has_world_database && has_character_anchors,
            json!({
                "world_rules": story_bible.map(|bible| bible.world_database.rules.len()).unwrap_or(0),
                "character_anchors": manifest_character_anchors(manifest)
            }),
        ),
        governance_axis(
            "volume_graph",
            "Volume-level narrative graph",
            has_volume_graph,
            json!({
                "manifest_volumes": manifest.volumes.len(),
                "story_bible_volume_arcs": story_bible.map(|bible| bible.narrative_graph.volume_arcs.len()).unwrap_or(0)
            }),
        ),
        governance_axis(
            "chapter_context_packages",
            "Per-chapter context packaging",
            has_context_packages,
            json!({
                "context_packages": manifest.context_packages.len(),
                "approved_chapters": approved_chapters
            }),
        ),
        governance_axis(
            "quality_gate",
            "Rule-first quality and review gate",
            has_quality_gate,
            json!({
                "reviews": manifest.reviews.len(),
                "truth_validations": manifest.truth_validations.len()
            }),
        ),
        governance_axis(
            "approval_truth_commit",
            "Truth/summary/hooks commit only after approval",
            approval_truth_clean,
            json!({
                "first_unapproved_chapter": first_unapproved_chapter_number(manifest),
                "truth_files": manifest.truth_files.len()
            }),
        ),
        governance_axis(
            "hook_debt",
            "Hook debt tracking",
            has_hook_debt,
            json!({
                "hook_debt_reports": manifest.hook_debt_reports.len(),
                "approved_chapters": approved_chapters
            }),
        ),
        governance_axis(
            "longform_budget",
            "Longform summary/archive budget",
            has_archive_budget,
            json!({
                "chapters": manifest.chapters.len(),
                "archives": manifest.archives.len(),
                "active_tail_limit": ACTIVE_CONTINUITY_CHAPTER_LIMIT
            }),
        ),
        governance_axis(
            "naming_governance",
            "Book/volume/chapter naming authority",
            naming_governed,
            json!({
                "title": manifest.title,
                "volume_titles": manifest.volumes.iter().map(|volume| volume.title.clone()).collect::<Vec<_>>(),
                "warnings": naming_warnings
            }),
        ),
    ];
    let blockers = axes
        .iter()
        .filter(|axis| {
            !axis
                .get("passed")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
        })
        .filter_map(|axis| {
            axis.get("id")
                .and_then(|value| value.as_str())
                .map(ToString::to_string)
        })
        .collect::<Vec<_>>();
    json!({
        "schema_version": "benshu.novel_governance.v1",
        "passed": blockers.is_empty(),
        "blockers": blockers,
        "axes": axes
    })
}

fn governance_axis(
    id: &str,
    label: &str,
    passed: bool,
    evidence: serde_json::Value,
) -> serde_json::Value {
    json!({
        "id": id,
        "label": label,
        "passed": passed,
        "evidence": evidence
    })
}

fn review_pass_rate(manifest: &NovelProjectManifest) -> Option<f64> {
    if manifest.reviews.is_empty() {
        return None;
    }
    let passed = manifest
        .reviews
        .iter()
        .filter(|review| review.verdict == "passed")
        .count();
    Some(passed as f64 / manifest.reviews.len() as f64)
}
