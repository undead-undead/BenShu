use serde_json::json;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncWrite, AsyncWriteExt, BufWriter};

use super::project_cache::{ProjectCache, TextScanReport};
use super::{
    atomic_write_file, canonical_project_title, chapter_is_approved, count_units, now_iso,
    strip_markdown_heading, volume_for_chapter, NovelProjectManifest,
};
use crate::tool::writing::text_sanitizer::{self, WritingSanitizeStage};

#[derive(Debug, Clone)]
pub(crate) struct ReadableTxtExport {
    pub(crate) current_path: PathBuf,
    pub(crate) latest_chapter_path: PathBuf,
    pub(crate) collection_path: PathBuf,
    pub(crate) unit_count: usize,
    pub(crate) collection_unit_count: usize,
}

impl ReadableTxtExport {
    pub(crate) fn to_json(&self) -> serde_json::Value {
        json!({
            "format": "txt",
            "current_path": self.current_path.to_string_lossy(),
            "latest_chapter_path": self.latest_chapter_path.to_string_lossy(),
            "collection_path": self.collection_path.to_string_lossy(),
            "unit_count": self.unit_count,
            "collection_unit_count": self.collection_unit_count,
            "preferred_for_chat": true
        })
    }
}

pub(crate) fn normalize_export_format(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "md" | "markdown" => "md".to_string(),
        _ => "txt".to_string(),
    }
}

async fn write_export_to_writer<W>(
    project_dir: &Path,
    manifest: &NovelProjectManifest,
    format: &str,
    approved_only: bool,
    writer: &mut W,
) -> anyhow::Result<TextScanReport>
where
    W: AsyncWrite + Unpin,
{
    let mut scan = TextScanReport::default();
    let title = canonical_project_title(manifest);
    let heading = if format == "md" {
        format!("# {title}\n\n")
    } else {
        format!("{title}\n\n")
    };
    writer.write_all(heading.as_bytes()).await?;
    scan.add_text(&heading, &manifest.language);

    let cache = ProjectCache::from_manifest(manifest, approved_only);
    let mut current_volume_id = String::new();
    for entry in &cache.chapter_index {
        if let Some(volume) = volume_for_chapter(manifest, entry.number) {
            if volume.id != current_volume_id {
                let volume_heading = if format == "md" {
                    format!("## {}\n\n", volume.title)
                } else {
                    format!("{}\n\n", volume.title)
                };
                writer.write_all(volume_heading.as_bytes()).await?;
                scan.add_text(&volume_heading, &manifest.language);
                current_volume_id = volume.id.clone();
            }
        }
        let Some(path) = cache.chapter_path(project_dir, entry.number) else {
            continue;
        };
        let raw = tokio::fs::read_to_string(path).await?;
        let body = raw
            .split_once("\n---\n")
            .map(|(_, body)| body.trim())
            .unwrap_or(raw.trim());
        let body = sanitize_readable_chapter_body(body, &manifest.language);
        let rendered = if format == "md" {
            format!("{body}\n\n")
        } else {
            format!("{}\n\n", strip_markdown_heading(&body))
        };
        writer.write_all(rendered.as_bytes()).await?;
        scan.add_text(&rendered, &manifest.language);
    }
    writer.flush().await?;
    Ok(scan)
}

pub(crate) async fn write_export_to_path(
    project_dir: &Path,
    manifest: &NovelProjectManifest,
    format: &str,
    approved_only: bool,
    output_path: &Path,
) -> anyhow::Result<TextScanReport> {
    if let Some(parent) = output_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let file = tokio::fs::File::create(output_path).await?;
    let mut writer = BufWriter::new(file);
    write_export_to_writer(project_dir, manifest, format, approved_only, &mut writer).await
}

pub(crate) async fn sync_readable_txt_export(
    project_dir: &Path,
    manifest: &NovelProjectManifest,
) -> anyhow::Result<ReadableTxtExport> {
    let export_dir = project_dir.join("exports");
    tokio::fs::create_dir_all(&export_dir).await?;
    let approved_only = manifest.approved_only;
    let latest_rendered = render_latest_chapter_export(project_dir, manifest, "txt", false)
        .await?
        .unwrap_or_else(|| canonical_project_title(manifest).to_string());
    let current_path = export_dir.join("current.txt");
    let latest_chapter_path = export_dir.join("latest_chapter.txt");
    let collection_path = export_dir.join("章节合集.txt");
    let collection_state_path = export_dir.join(".readable_collection_state.json");
    let chapter_index_path = export_dir.join("chapter_index.json");
    atomic_write_file(current_path.clone(), latest_rendered.clone()).await?;
    atomic_write_file(latest_chapter_path.clone(), latest_rendered.clone()).await?;
    let project_cache = ProjectCache::from_manifest(manifest, approved_only);
    let collection_signature = project_cache.signature();
    atomic_write_file(
        chapter_index_path.clone(),
        serde_json::to_string_pretty(&project_cache)?,
    )
    .await?;
    let prior_state = read_json_file(&collection_state_path).await;
    let collection_unit_count = if collection_path.exists()
        && prior_state
            .as_ref()
            .and_then(|state| state.get("signature"))
            == Some(&collection_signature)
    {
        prior_state
            .as_ref()
            .and_then(|state| state.get("collection_unit_count"))
            .and_then(serde_json::Value::as_u64)
            .map(|value| value as usize)
            .or_else(|| {
                std::fs::read_to_string(&collection_path)
                    .ok()
                    .map(|content| count_units(&content, &manifest.language))
            })
            .unwrap_or(0)
    } else {
        let file = tokio::fs::File::create(&collection_path).await?;
        let mut writer = BufWriter::new(file);
        let scan = write_export_to_writer(project_dir, manifest, "txt", approved_only, &mut writer)
            .await?;
        let count = scan.units;
        let state = json!({
            "signature": collection_signature,
            "collection_unit_count": count,
            "scan": scan,
            "updated_at": now_iso()
        });
        atomic_write_file(
            collection_state_path.clone(),
            serde_json::to_string_pretty(&state)?,
        )
        .await?;
        count
    };
    Ok(ReadableTxtExport {
        current_path,
        latest_chapter_path,
        collection_path,
        unit_count: count_units(&latest_rendered, &manifest.language),
        collection_unit_count,
    })
}

async fn render_latest_chapter_export(
    project_dir: &Path,
    manifest: &NovelProjectManifest,
    format: &str,
    approved_only: bool,
) -> anyhow::Result<Option<String>> {
    let Some(chapter) = manifest
        .chapters
        .iter()
        .filter(|chapter| !approved_only || chapter_is_approved(chapter))
        .max_by_key(|chapter| chapter.number)
    else {
        return Ok(None);
    };
    let raw = tokio::fs::read_to_string(project_dir.join(&chapter.path)).await?;
    let body = raw
        .split_once("\n---\n")
        .map(|(_, body)| body.trim())
        .unwrap_or(raw.trim());
    let body = sanitize_readable_chapter_body(body, &manifest.language);
    let volume_heading = volume_for_chapter(manifest, chapter.number)
        .map(|volume| volume.title.as_str())
        .unwrap_or("");
    let rendered = if format == "md" {
        if volume_heading.is_empty() {
            format!("# {}\n\n{}", canonical_project_title(manifest), body)
        } else {
            format!(
                "# {}\n\n## {}\n\n{}",
                canonical_project_title(manifest),
                volume_heading,
                body
            )
        }
    } else if volume_heading.is_empty() {
        format!(
            "{}\n\n{}",
            canonical_project_title(manifest),
            strip_markdown_heading(&body)
        )
    } else {
        format!(
            "{}\n\n{}\n\n{}",
            canonical_project_title(manifest),
            volume_heading,
            strip_markdown_heading(&body)
        )
    };
    Ok(Some(rendered.trim().to_string()))
}

async fn read_json_file(path: &Path) -> Option<serde_json::Value> {
    let raw = tokio::fs::read_to_string(path).await.ok()?;
    serde_json::from_str(&raw).ok()
}

pub(super) fn sanitize_readable_chapter_body(content: &str, language: &str) -> String {
    let common = text_sanitizer::sanitize_common_surface_report(
        content,
        WritingSanitizeStage::ReadableExport,
    );
    if !readable_language_looks_chinese(language) {
        return common.text.trim().to_string();
    }
    collapse_suspicious_readable_cjk_stutters(&common.text)
        .trim()
        .to_string()
}

fn collapse_suspicious_readable_cjk_stutters(content: &str) -> String {
    let chars = content.chars().collect::<Vec<_>>();
    let mut out = String::with_capacity(content.len());
    let mut index = 0usize;
    while index < chars.len() {
        if index + 1 < chars.len()
            && chars[index] == chars[index + 1]
            && readable_is_cjk_char(chars[index])
            && readable_repeated_cjk_char_looks_like_stutter(&chars, index)
        {
            out.push(chars[index]);
            index += 2;
            continue;
        }
        out.push(chars[index]);
        index += 1;
    }
    out
}

fn readable_repeated_cjk_char_looks_like_stutter(chars: &[char], index: usize) -> bool {
    let ch = chars[index];
    let prev = index.checked_sub(1).and_then(|idx| chars.get(idx)).copied();
    let next = chars.get(index + 2).copied();
    if readable_repeated_cjk_boundary_looks_valid(prev, ch, next) {
        return false;
    }
    readable_repeated_cjk_boundary_looks_like_stutter(prev, ch, next)
}

fn readable_language_looks_chinese(language: &str) -> bool {
    let language = language.trim().to_ascii_lowercase();
    language.contains("zh")
        || language.contains("cn")
        || language.contains("chinese")
        || language.contains("中文")
        || language.contains("汉")
        || language.contains("漢")
}

fn readable_is_cjk_char(ch: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&ch)
}

fn readable_repeated_cjk_boundary_looks_valid(
    prev: Option<char>,
    ch: char,
    next: Option<char>,
) -> bool {
    matches!((prev, ch, next), (Some('结'), '构', Some('成')))
}

fn readable_repeated_cjk_boundary_looks_like_stutter(
    prev: Option<char>,
    ch: char,
    next: Option<char>,
) -> bool {
    matches!((prev, ch, next), (Some(_), '构', Some('成')))
}
