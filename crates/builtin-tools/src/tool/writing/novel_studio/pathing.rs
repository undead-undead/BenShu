use serde_json::json;
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use super::NovelStudioArgs;

pub(super) fn reject_parent_components(path: &Path) -> anyhow::Result<()> {
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        anyhow::bail!("path traversal is not allowed: {}", path.display());
    }
    Ok(())
}

pub(super) fn invalid_draft_path_as_project_path_result(
    args: &NovelStudioArgs,
) -> Option<serde_json::Value> {
    if !novel_action_requires_existing_project_path(&args.action) {
        return None;
    }
    if !project_path_looks_like_draft_file(&args.project_path) {
        return None;
    }
    Some(json!({
        "success": false,
        "recoverable": true,
        "error_kind": "project_path_is_creation_draft_file",
        "action": args.action,
        "project_path": args.project_path.trim(),
        "draft_path": args.project_path.trim(),
        "next_step_hint": "This path is a creation draft JSON, not a novel project directory. Call approve_draft with this draft_path first, then use the returned project_path for project/chapter actions."
    }))
}

pub(super) fn project_path_points_to_draft_file(project_path: &str, draft_path: &Path) -> bool {
    let trimmed = project_path.trim();
    if trimmed.is_empty() {
        return false;
    }
    let normalized = trimmed.replace('\\', "/").to_ascii_lowercase();
    if project_path_looks_like_draft_file(&normalized) {
        return true;
    }
    let draft_text = draft_path.to_string_lossy().replace('\\', "/");
    normalized == draft_text.to_ascii_lowercase()
}

pub(super) fn project_path_looks_like_draft_file(project_path: &str) -> bool {
    let normalized = project_path.trim().replace('\\', "/").to_ascii_lowercase();
    normalized.ends_with(".json") && normalized.contains("/drafts/")
}

pub(super) fn canonical_or_self(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

pub(super) fn canonical_parent_join(path: &Path) -> anyhow::Result<PathBuf> {
    if path.exists() {
        return Ok(path.canonicalize()?);
    }
    let Some(parent) = path.parent() else {
        return Ok(path.to_path_buf());
    };
    let file_name = path
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("invalid path: {}", path.display()))?;
    let parent = if parent.exists() {
        parent.canonicalize()?
    } else {
        parent.to_path_buf()
    };
    Ok(parent.join(file_name))
}

fn novel_action_requires_existing_project_path(action: &str) -> bool {
    !matches!(
        action,
        "list_projects"
            | "draft_project"
            | "update_draft"
            | "show_draft"
            | "approve_draft"
            | "discard_draft"
            | "init_project"
            | "clone_project"
    )
}

pub(super) fn slugify(value: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = value.chars().collect();
    for (idx, ch) in chars.iter().copied().enumerate() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if ('\u{4e00}'..='\u{9fff}').contains(&ch) {
            out.push(ch);
        } else if ch.is_whitespace()
            || matches!(
                ch,
                '-' | '_'
                    | '.'
                    | ':'
                    | '：'
                    | ','
                    | '，'
                    | '、'
                    | '/'
                    | '\\'
                    | '|'
                    | '—'
                    | '–'
                    | '·'
            )
        {
            if slug_separator_needed(&chars, idx) && !out.ends_with('-') {
                out.push('-');
            }
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        format!("novel-{}", uuid::Uuid::new_v4().simple())
    } else {
        trimmed.chars().take(64).collect()
    }
}

fn slug_separator_needed(chars: &[char], idx: usize) -> bool {
    let prev = chars[..idx].iter().rev().copied().find(is_slug_token_char);
    let next = chars[idx + 1..].iter().copied().find(is_slug_token_char);
    match (prev, next) {
        (Some(prev), Some(next)) => {
            !is_cjk_slug_char(prev) && !is_cjk_slug_char(next) && !prev.is_ascii_digit()
        }
        _ => false,
    }
}

fn is_slug_token_char(ch: &char) -> bool {
    ch.is_ascii_alphanumeric() || is_cjk_slug_char(*ch)
}

fn is_cjk_slug_char(ch: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&ch)
}

pub(super) fn normalize_project_lookup_key(value: &str) -> String {
    value
        .chars()
        .filter_map(|ch| {
            if ch.is_ascii_alphanumeric() {
                Some(ch.to_ascii_lowercase())
            } else if ('\u{4e00}'..='\u{9fff}').contains(&ch) {
                Some(ch)
            } else {
                None
            }
        })
        .collect()
}

pub(super) fn find_existing_title_conflicts(
    root: &Path,
    requested_title: &str,
) -> Vec<serde_json::Value> {
    let requested_key = normalize_project_lookup_key(requested_title);
    if requested_key.is_empty() {
        return Vec::new();
    }

    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };
    let mut conflicts = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let manifest_path = path.join("project.json");
        if !manifest_path.is_file() {
            continue;
        }
        let Some(existing_title) = std::fs::read_to_string(&manifest_path)
            .ok()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
            .and_then(|value| {
                value
                    .get("title")
                    .and_then(|title| title.as_str())
                    .map(str::to_string)
            })
        else {
            continue;
        };
        let existing_key = normalize_project_lookup_key(&existing_title);
        let folder_key = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(normalize_project_lookup_key)
            .unwrap_or_default();
        if existing_key.is_empty() && folder_key.is_empty() {
            continue;
        }
        let similarity = title_similarity(&requested_key, &existing_key)
            .max(title_similarity(&requested_key, &folder_key));
        if similarity >= 0.82 {
            conflicts.push(json!({
                "title": existing_title,
                "path": path.to_string_lossy(),
                "similarity": similarity
            }));
        }
    }
    conflicts.sort_by(|left, right| {
        let left_score = left
            .get("similarity")
            .and_then(|value| value.as_f64())
            .unwrap_or(0.0);
        let right_score = right
            .get("similarity")
            .and_then(|value| value.as_f64())
            .unwrap_or(0.0);
        right_score
            .partial_cmp(&left_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    conflicts.truncate(5);
    conflicts
}

pub(super) fn title_similarity(left: &str, right: &str) -> f64 {
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    if left == right {
        return 1.0;
    }
    let left_core = leading_cjk_title_core(left);
    let right_core = leading_cjk_title_core(right);
    if left_core.chars().count() >= 3 && left_core == right_core {
        return 0.95;
    }
    let shorter_core_len = left_core.chars().count().min(right_core.chars().count());
    if shorter_core_len >= 3
        && (left_core.starts_with(&right_core) || right_core.starts_with(&left_core))
    {
        return 0.95;
    }
    if left.chars().count() >= 3
        && right.chars().count() >= 3
        && (left.contains(right) || right.contains(left))
    {
        return 0.95;
    }
    let left_units = title_units(left);
    let right_units = title_units(right);
    if left_units.is_empty() || right_units.is_empty() {
        return 0.0;
    }
    let intersection = left_units.intersection(&right_units).count();
    let union = left_units.union(&right_units).count();
    if union == 0 {
        return 0.0;
    }
    intersection as f64 / union as f64
}

fn leading_cjk_title_core(value: &str) -> String {
    let mut core = String::new();
    let mut started = false;
    for ch in value.chars() {
        if ('\u{4e00}'..='\u{9fff}').contains(&ch) {
            core.push(ch);
            started = true;
            continue;
        }
        if started {
            break;
        }
    }
    core
}

fn title_units(value: &str) -> BTreeSet<String> {
    let chars = value.chars().collect::<Vec<_>>();
    let mut units = BTreeSet::new();
    for ch in &chars {
        units.insert(ch.to_string());
    }
    for pair in chars.windows(2) {
        units.insert(pair.iter().collect::<String>());
    }
    units
}

pub(super) fn unique_child_path(root: &Path, preferred_name: &str) -> PathBuf {
    let base = if preferred_name.trim().is_empty() {
        format!("novel-{}", uuid::Uuid::new_v4().simple())
    } else {
        preferred_name.trim().to_string()
    };
    let candidate = root.join(&base);
    if !candidate.exists() {
        return candidate;
    }
    for index in 2..1000 {
        let candidate = root.join(format!("{base}-{index}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    root.join(format!("{base}-{}", uuid::Uuid::new_v4().simple()))
}
