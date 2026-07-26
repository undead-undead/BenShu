use super::*;

pub(super) fn split_chapters(
    content: &str,
    split_pattern: &str,
) -> anyhow::Result<Vec<(String, String)>> {
    let pattern = if split_pattern.trim().is_empty() {
        r"(?m)^(?:\s*#+\s*)?(第[0-9０-９一二三四五六七八九十百千万零〇两]+[章节回卷部篇][^\n]*|Chapter\s+[0-9]+[^\n]*|CHAPTER\s+[0-9]+[^\n]*)\s*$"
    } else {
        split_pattern.trim()
    };
    let re = regex::Regex::new(pattern)?;
    let matches = re.find_iter(content).collect::<Vec<_>>();
    if matches.is_empty() {
        return Ok(vec![("Chapter 1".to_string(), content.trim().to_string())]);
    }

    let mut chapters = Vec::new();
    let prefix = content[..matches[0].start()].trim();
    if !prefix.is_empty() {
        chapters.push(("Preface".to_string(), prefix.to_string()));
    }
    for (idx, found) in matches.iter().enumerate() {
        let title = found.as_str().trim().trim_start_matches('#').trim();
        let body_start = found.end();
        let body_end = matches
            .get(idx + 1)
            .map(|next| next.start())
            .unwrap_or(content.len());
        let body = content[body_start..body_end].trim();
        let chapter_body = if body.is_empty() {
            title.to_string()
        } else {
            body.to_string()
        };
        chapters.push((title.to_string(), chapter_body));
    }
    Ok(chapters)
}

pub(super) async fn archive_chapter_file(
    project_dir: &Path,
    number: usize,
    chapter_path: &Path,
) -> anyhow::Result<()> {
    let existing = tokio::fs::read_to_string(chapter_path)
        .await
        .unwrap_or_default();
    if existing.trim().is_empty() {
        return Ok(());
    }
    archive_chapter_content(project_dir, number, existing).await
}

pub(super) async fn archive_chapter_content(
    project_dir: &Path,
    number: usize,
    content: String,
) -> anyhow::Result<()> {
    let revision_dir = project_dir.join("chapters").join(".revisions");
    tokio::fs::create_dir_all(&revision_dir).await?;
    let revision_path = revision_dir.join(format!("{number:04}-{}.md", safe_timestamp(&now_iso())));
    atomic_write_file(revision_path, content).await?;
    Ok(())
}

pub(super) async fn write_chapter_record(
    project_dir: &Path,
    record: &ChapterRecord,
    content: &str,
) -> anyhow::Result<()> {
    tokio::fs::create_dir_all(project_dir.join("chapters")).await?;
    atomic_write_file(
        project_dir.join(&record.path),
        render_chapter_file(record, content),
    )
    .await?;
    Ok(())
}

pub(super) async fn sync_chapter_record_file(
    project_dir: &Path,
    record: &ChapterRecord,
) -> anyhow::Result<()> {
    let chapter_path = project_dir.join(&record.path);
    let raw = tokio::fs::read_to_string(&chapter_path)
        .await
        .unwrap_or_default();
    if raw.trim().is_empty() {
        return Ok(());
    }
    let body = strip_frontmatter(&raw);
    write_chapter_record(project_dir, record, &body).await
}

pub(super) fn normalize_truth_section_content(
    section: &str,
    content: &str,
    language: &str,
) -> String {
    let mut content = truth_file_body(section, content);
    if content.is_empty() {
        return content;
    }
    if section.eq_ignore_ascii_case("current_state") {
        if let Ok(structured) = serde_json::from_str::<serde_json::Value>(&content) {
            return serde_json::to_string(&structured).unwrap_or_else(|_| "{}".to_string());
        }
        content = compact_chapter_summary(&content, language);
        return content;
    }
    if section.eq_ignore_ascii_case("pending_hooks") {
        return compact_truth_items(content.lines().map(ToString::to_string).collect(), 12)
            .join("\n")
            .chars()
            .take(TRUTH_HOOKS_MAX_CHARS)
            .collect::<String>()
            .trim()
            .to_string();
    }
    if section.eq_ignore_ascii_case("chapter_summaries") {
        let mut seen = BTreeSet::new();
        return content
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .filter(|line| !line.starts_with('#'))
            .filter_map(|line| {
                let line = truncate_compact_text(line, TRUTH_SUMMARY_LINE_MAX_CHARS);
                if seen.insert(line.clone()) {
                    Some(line)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    content
}

pub(super) fn compact_chapter_summary(summary: &str, language: &str) -> String {
    let summary = sanitize_contract_text(&strip_markdown_heading(&truth_file_body(
        "summary", summary,
    )));
    if summary.is_empty() {
        return summary;
    }
    let max_sentences = if runner::is_chinese_language(language) {
        3
    } else {
        2
    };
    let mut out = String::new();
    let mut sentence_count = 0usize;
    for ch in summary.chars() {
        out.push(ch);
        if matches!(ch, '。' | '！' | '？' | '.' | '!' | '?') {
            sentence_count += 1;
            if sentence_count >= max_sentences {
                break;
            }
        }
        if out.chars().count() >= CHAPTER_SUMMARY_MAX_CHARS {
            break;
        }
    }
    truncate_compact_text(&out, CHAPTER_SUMMARY_MAX_CHARS)
}

pub(super) fn compact_truth_items(values: Vec<String>, limit: usize) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for value in values {
        let value = truncate_compact_text(&sanitize_contract_text(&value), CHAPTER_FACT_MAX_CHARS);
        if value.is_empty() || !truth_item_has_informative_payload(&value) {
            continue;
        }
        if !seen.insert(value.clone()) {
            continue;
        }
        out.push(value);
        if out.len() >= limit {
            break;
        }
    }
    out
}

fn truth_item_has_informative_payload(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.chars().count() <= 2 {
        return false;
    }
    let cjk_count = trimmed.chars().filter(|ch| is_truth_cjk_char(*ch)).count();
    let non_space_count = trimmed.chars().filter(|ch| !ch.is_whitespace()).count();
    if cjk_count == non_space_count && non_space_count <= 3 {
        return false;
    }
    true
}

fn is_truth_cjk_char(ch: char) -> bool {
    matches!(
        ch,
        '\u{3400}'..='\u{9fff}' | '\u{f900}'..='\u{faff}' | '\u{20000}'..='\u{2a6df}'
    )
}

pub(super) fn truncate_compact_text(value: &str, max_chars: usize) -> String {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= max_chars {
        return compact.trim().to_string();
    }
    let mut out = compact
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    out.push('…');
    out.trim().to_string()
}

pub(super) async fn refresh_continuity_truth_file(
    project_dir: &Path,
    manifest: &mut NovelProjectManifest,
) -> anyhow::Result<()> {
    let section = "continuity_index";
    let content = render_continuity_index(manifest);
    tokio::fs::create_dir_all(project_dir.join("truth")).await?;
    let path = format!("truth/{}.md", slugify(section));
    atomic_write_file(
        project_dir.join(&path),
        render_truth_file(section, &content),
    )
    .await?;
    upsert_truth_record(
        manifest,
        TruthFileRecord {
            section: section.to_string(),
            path,
            unit_count: count_units(&content, &manifest.language),
            updated_at: now_iso(),
        },
    );
    Ok(())
}

pub(super) async fn compact_longform_state(
    project_dir: &Path,
    manifest: &mut NovelProjectManifest,
) -> anyhow::Result<()> {
    write_chapter_range_archives(
        project_dir,
        manifest,
        "arc",
        ARCHIVE_ARC_CHAPTER_SPAN,
        ACTIVE_CONTINUITY_CHAPTER_LIMIT,
    )
    .await?;
    write_chapter_range_archives(
        project_dir,
        manifest,
        "volume",
        ARCHIVE_VOLUME_CHAPTER_SPAN,
        ACTIVE_CONTINUITY_CHAPTER_LIMIT,
    )
    .await?;
    refresh_continuity_truth_file(project_dir, manifest).await?;
    Ok(())
}

pub(super) async fn write_chapter_range_archives(
    project_dir: &Path,
    manifest: &mut NovelProjectManifest,
    kind: &str,
    span: usize,
    active_tail: usize,
) -> anyhow::Result<()> {
    let approved = approved_chapters(manifest)
        .into_iter()
        .cloned()
        .collect::<Vec<_>>();
    let Some(latest_approved) = approved.iter().map(|chapter| chapter.number).max() else {
        return Ok(());
    };
    let archive_cutoff = latest_approved.saturating_sub(active_tail);
    if archive_cutoff == 0 {
        return Ok(());
    }
    let mut ranges = BTreeSet::new();
    for chapter in &approved {
        if chapter.number > archive_cutoff {
            continue;
        }
        let range_start = ((chapter.number - 1) / span) * span + 1;
        let range_end = range_start + span - 1;
        ranges.insert((range_start, range_end.min(archive_cutoff)));
    }
    if ranges.is_empty() {
        return Ok(());
    }
    tokio::fs::create_dir_all(project_dir.join("archives")).await?;
    for (range_start, range_end) in ranges {
        let chapters = approved
            .iter()
            .filter(|chapter| chapter.number >= range_start && chapter.number <= range_end)
            .cloned()
            .collect::<Vec<_>>();
        if chapters.is_empty() {
            continue;
        }
        let content = render_chapter_archive(kind, range_start, range_end, &chapters);
        let path = format!("archives/{kind}-{range_start:04}-{range_end:04}.md");
        atomic_write_file(project_dir.join(&path), content.clone()).await?;
        upsert_archive_record(
            manifest,
            LongformArchiveRecord {
                kind: kind.to_string(),
                range_start,
                range_end,
                path,
                unit_count: count_units(&content, &manifest.language),
                created_at: now_iso(),
                updated_at: now_iso(),
            },
        );
    }
    Ok(())
}

pub(super) fn render_chapter_archive(
    kind: &str,
    range_start: usize,
    range_end: usize,
    chapters: &[ChapterRecord],
) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "# {} continuity archive: chapters {}-{}\n\n",
        kind, range_start, range_end
    ));
    out.push_str(
        "This archive is a compact continuity layer. It stores summaries and facts, not chapter prose.\n\n",
    );
    for chapter in chapters {
        out.push_str(&format!(
            "## Chapter {}: {}\n\n- Status: {}\n- Units: {}\n- Summary: {}\n",
            chapter.number,
            chapter.title,
            chapter.status,
            chapter.unit_count,
            if chapter.summary.trim().is_empty() {
                "(missing)"
            } else {
                chapter.summary.trim()
            }
        ));
        if !chapter.key_facts.is_empty() {
            out.push_str("\nKey facts:\n");
            out.push_str(&render_list(&chapter.key_facts));
        }
        if !chapter.continuity_updates.is_empty() {
            out.push_str("\nContinuity updates:\n");
            out.push_str(&render_list(&chapter.continuity_updates));
        }
        out.push('\n');
    }
    out
}

pub(super) fn render_continuity_index(manifest: &NovelProjectManifest) -> String {
    let mut out = String::new();
    out.push_str("This file is maintained by novel_studio as a compact continuity ledger.\n\n");
    out.push_str("## Project\n\n");
    out.push_str(&format!(
        "- Title: {}\n- Language: {}\n- Genre: {}\n",
        manifest.title, manifest.language, manifest.genre
    ));
    if let Some(contract) = &manifest.contract {
        out.push_str("\n## Contract Anchors\n\n");
        out.push_str("### Characters\n");
        out.push_str(&render_list(&contract.characters));
        out.push_str("\n### World Rules\n");
        out.push_str(&render_list(&contract.world_rules));
        out.push_str("\n### Must Avoid\n");
        out.push_str(&render_list(&contract.must_avoid));
        out.push_str("\n");
    }
    if !manifest.archives.is_empty() {
        out.push_str("\n## Archived Continuity Layers\n");
        for archive in manifest.archives.iter().rev().take(CONTEXT_ARCHIVE_LIMIT) {
            out.push_str(&format!(
                "\n- {} chapters {}-{}: {}",
                archive.kind, archive.range_start, archive.range_end, archive.path
            ));
        }
        out.push('\n');
    }
    out.push_str("\n## Active Approved Chapters\n");
    let chapters = approved_chapters(manifest);
    for chapter in chapters
        .into_iter()
        .rev()
        .take(ACTIVE_CONTINUITY_CHAPTER_LIMIT)
        .rev()
    {
        out.push_str(&format!(
            "\n### Chapter {}: {}\n\n- Status: {}\n- Summary: {}\n",
            chapter.number,
            chapter.title,
            chapter.status,
            if chapter.summary.trim().is_empty() {
                "(missing)"
            } else {
                chapter.summary.trim()
            }
        ));
        if !chapter.key_facts.is_empty() {
            out.push_str("\nKey facts:\n");
            out.push_str(&render_list(&chapter.key_facts));
        }
        if !chapter.continuity_updates.is_empty() {
            out.push_str("\nContinuity updates:\n");
            out.push_str(&render_list(&chapter.continuity_updates));
        }
    }
    out
}

pub(super) fn approved_chapters(manifest: &NovelProjectManifest) -> Vec<&ChapterRecord> {
    manifest
        .chapters
        .iter()
        .filter(|chapter| chapter_is_approved(chapter))
        .collect()
}

pub(super) async fn append_continuity(
    project_dir: &Path,
    record: &ChapterRecord,
) -> anyhow::Result<()> {
    if !chapter_is_approved(record) {
        return Ok(());
    }
    let mut section = format!(
        "\n## Chapter {}: {}\n\nSummary: {}\n\n",
        record.number, record.title, record.summary
    );
    if !record.key_facts.is_empty() {
        section.push_str("Key facts:\n");
        section.push_str(&render_list(&record.key_facts));
        section.push('\n');
    }
    if !record.continuity_updates.is_empty() {
        section.push_str("Continuity updates:\n");
        section.push_str(&render_list(&record.continuity_updates));
        section.push('\n');
    }
    let path = project_dir.join("continuity.md");
    let existing = tokio::fs::read_to_string(&path).await.unwrap_or_default();
    atomic_write_file(path, format!("{existing}{section}")).await?;
    Ok(())
}

pub(super) fn safe_timestamp(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect()
}

pub(super) async fn copy_dir_recursive(source: &Path, target: &Path) -> anyhow::Result<()> {
    if !source.exists() {
        anyhow::bail!("source project does not exist: {}", source.display());
    }
    tokio::fs::create_dir_all(target).await?;
    let mut stack = vec![(source.to_path_buf(), target.to_path_buf())];
    while let Some((src_dir, dst_dir)) = stack.pop() {
        tokio::fs::create_dir_all(&dst_dir).await?;
        let mut entries = tokio::fs::read_dir(&src_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let src_path = entry.path();
            let dst_path = dst_dir.join(entry.file_name());
            let file_type = entry.file_type().await?;
            if file_type.is_dir() {
                stack.push((src_path, dst_path));
            } else if file_type.is_file() {
                if let Some(parent) = dst_path.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                tokio::fs::copy(src_path, dst_path).await?;
            }
        }
    }
    Ok(())
}

pub(super) async fn copy_file_if_exists(source: &Path, target: &Path) -> anyhow::Result<()> {
    if !source.exists() {
        return Ok(());
    }
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::copy(source, target).await?;
    Ok(())
}

pub(super) fn render_list(items: &[String]) -> String {
    let items = items
        .iter()
        .map(|item| super::surface_sanitizer::sanitize_contract_surface_text(item))
        .filter(|item| !item.trim().is_empty())
        .collect::<Vec<_>>();
    if items.is_empty() {
        return "- none\n".to_string();
    }
    items
        .iter()
        .map(|item| format!("- {item}\n"))
        .collect::<String>()
}

pub(super) fn yaml_line(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}
