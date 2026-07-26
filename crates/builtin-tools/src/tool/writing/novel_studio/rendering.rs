use super::{
    first_non_empty, render_list, sanitize_saved_prose, strip_redundant_leading_chapter_heading,
    surface_sanitizer, yaml_line, ChapterArchitectureRecord, ChapterPlanRecord, ChapterRecord,
    NovelContractV2, NovelProjectManifest, ReviewReceipt, StoryContract,
};
use crate::tool::writing::novel_contract_v2;

pub(super) fn render_project_readme(manifest: &NovelProjectManifest) -> String {
    let story_bible = manifest
        .story_bible
        .as_ref()
        .map(|bible| {
            format!(
                "\n## Story Bible\n\n- Ending contract: {}\n- Genre family: {}\n- Character anchors: {}\n- World rules: {}\n- Hooks: {}\n- Timeline entries: {}\n",
                if bible.ending_contract.desired_resolution.trim().is_empty() {
                    "missing"
                } else {
                    "present"
                },
                bible.genre_governance.genre_family,
                bible.character_ledger.len(),
                bible.world_database.rules.len(),
                bible.hook_ledger.len(),
                bible.timeline.len()
            )
        })
        .unwrap_or_default();
    let volume_block = if manifest.volumes.is_empty() {
        String::new()
    } else {
        format!(
            "\n## Volumes\n\n{}\n",
            manifest
                .volumes
                .iter()
                .map(|volume| format!(
                    "- {}: chapters {}-{}, objective={}",
                    volume.title,
                    volume.start_chapter,
                    volume
                        .end_chapter
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "open".to_string()),
                    first_non_empty(&[volume.objective.as_str(), "(unspecified)"])
                ))
                .collect::<Vec<_>>()
                .join("\n")
        )
    };
    format!(
        "# {}\n\n- Language: {}\n- Genre: {}\n- Target units: {}\n- Chapter target: {}\n\n## Brief\n\n{}\n{}{}",
        manifest.title,
        manifest.language,
        if manifest.genre.is_empty() {
            "(unspecified)"
        } else {
            &manifest.genre
        },
        manifest
            .target_units
            .map(|value| value.to_string())
            .unwrap_or_else(|| "(unspecified)".to_string()),
        manifest
            .chapter_unit_target
            .map(|value| value.to_string())
            .unwrap_or_else(|| "(unspecified)".to_string()),
        manifest.brief,
        volume_block,
        story_bible
    )
}

pub(super) fn render_source_file(
    title: &str,
    source_url: &str,
    notes: &str,
    content: &str,
) -> String {
    format!(
        "---\ntitle: {}\nsource_url: {}\nnotes: {}\n---\n\n{}\n",
        yaml_line(title),
        yaml_line(source_url),
        yaml_line(notes),
        content.trim()
    )
}

pub(super) fn render_contract(contract: &StoryContract) -> String {
    let premise = surface_sanitizer::sanitize_contract_surface_text(&contract.premise);
    let outline = surface_sanitizer::sanitize_contract_surface_text(&contract.outline);
    format!(
        "# Story Contract\n\n## Premise\n\n{}\n\n## Themes\n{}\n\n## Characters\n{}\n\n## World Rules\n{}\n\n## Style Rules\n{}\n\n## Must Avoid\n{}\n\n## Outline\n\n{}\n\n{}",
        premise,
        render_list(&contract.themes),
        render_list(&contract.characters),
        render_list(&contract.world_rules),
        render_list(&contract.style_rules),
        render_list(&contract.must_avoid),
        outline,
        render_contract_v2_markdown(&contract.structured_contract_v2)
    )
}

fn render_contract_v2_markdown(contract: &NovelContractV2) -> String {
    let summary = novel_contract_v2::summary_lines(contract);
    if summary.is_empty() && contract.field_requirements.is_empty() {
        return String::new();
    }
    let requirements = contract
        .field_requirements
        .iter()
        .map(|(key, value)| format!("- {key}: {value}"))
        .collect::<Vec<_>>()
        .join("\n");
    let summary = if summary.is_empty() {
        "- (not specified yet)".to_string()
    } else {
        summary
            .iter()
            .map(|line| format!("- {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "## Structured Contract v2\n\n### Field Requirements\n{}\n\n### Summary\n{}\n",
        if requirements.is_empty() {
            "- (not specified yet)"
        } else {
            requirements.as_str()
        },
        summary
    )
}

pub(super) fn render_plan_file(record: &ChapterPlanRecord, notes: &str) -> String {
    format!(
        "---\nnumber: {}\ntitle: {}\nstatus: {}\n---\n\n# Plan: {}\n\n{}\n\n## Notes\n\n{}\n",
        record.number,
        yaml_line(&record.title),
        yaml_line(&record.status),
        record.title,
        record.plan.trim(),
        if notes.trim().is_empty() {
            "(none)"
        } else {
            notes.trim()
        }
    )
}

pub(super) fn render_architecture_file(record: &ChapterArchitectureRecord, notes: &str) -> String {
    format!(
        "---\nnumber: {}\ntitle: {}\nstatus: {}\n---\n\n# Architecture: {}\n\n{}\n\n## Notes\n\n{}\n",
        record.number,
        yaml_line(&record.title),
        yaml_line(&record.status),
        record.title,
        record.architecture.trim(),
        if notes.trim().is_empty() {
            "(none)"
        } else {
            notes.trim()
        }
    )
}

pub(super) fn render_chapter_file(record: &ChapterRecord, content: &str) -> String {
    let content = sanitize_saved_prose(content);
    let content = strip_redundant_leading_chapter_heading(&content, &record.title);
    format!(
        "---\nnumber: {}\ntitle: {}\nvolume_id: {}\nvolume_title: {}\nstatus: {}\nunits: {}\nsummary: {}\n---\n\n# {}\n\n{}\n",
        record.number,
        yaml_line(&record.title),
        yaml_line(&record.volume_id),
        yaml_line(&record.volume_title),
        yaml_line(&record.status),
        record.unit_count,
        yaml_line(&record.summary),
        record.title,
        content.trim()
    )
}

pub(super) fn stable_chapter_path(number: usize) -> String {
    format!("chapters/{number:04}.md")
}

pub(super) fn render_review_file(review: &ReviewReceipt) -> String {
    format!(
        "# Chapter {} Review\n\n- Verdict: {}\n- Created: {}\n\n## Issues\n{}\n\n## Feedback\n\n{}\n",
        review.chapter_number,
        review.verdict,
        review.created_at,
        render_list(&review.issues),
        if review.feedback.trim().is_empty() {
            "(none)"
        } else {
            review.feedback.trim()
        }
    )
}

pub(super) fn render_truth_file(section: &str, content: &str) -> String {
    format!("# {}\n\n{}\n", section.trim(), content.trim())
}

pub(super) fn truth_file_body(section: &str, raw: &str) -> String {
    let section = section.trim();
    let mut lines = raw.trim().lines().peekable();
    while let Some(line) = lines.peek() {
        if line.trim().is_empty() {
            lines.next();
            continue;
        }
        break;
    }
    if let Some(first) = lines.peek().copied() {
        let title = first.trim().trim_start_matches('#').trim();
        if !section.is_empty() && title.eq_ignore_ascii_case(section) {
            lines.next();
            while let Some(line) = lines.peek() {
                if line.trim().is_empty() {
                    lines.next();
                    continue;
                }
                break;
            }
        }
    }
    lines.collect::<Vec<_>>().join("\n").trim().to_string()
}

pub(super) fn render_style_file(title: &str, notes: &str, content: &str) -> String {
    format!(
        "# {}\n\n## Notes\n\n{}\n\n## Style Profile\n\n{}\n",
        title.trim(),
        if notes.trim().is_empty() {
            "(none)"
        } else {
            notes.trim()
        },
        content.trim()
    )
}
