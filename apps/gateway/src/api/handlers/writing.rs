use axum::{extract::State, Json};
use benshu_builtin_tools::tool::NovelStudioTool;
use benshu_infra::Tool;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use crate::api::state::{AppError, AppState};

#[derive(Debug, Clone, Serialize)]
pub struct NovelProjectDto {
    pub id: String,
    pub title: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    pub language: String,
    pub genre: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_units: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chapter_unit_target: Option<u64>,
    pub chapter_count: usize,
    pub approved_chapters: usize,
    pub drafted_chapters: usize,
    pub needs_revision_chapters: usize,
    pub total_units: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_export_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NovelProjectsResponse {
    pub root: String,
    pub projects: Vec<NovelProjectDto>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NovelExportRequest {
    pub project_path: String,
    #[serde(default)]
    pub format: String,
    #[serde(default)]
    pub approved_only: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct NovelExportResponse {
    pub exported: bool,
    pub project_path: String,
    pub output_path: Option<String>,
    pub format: String,
    pub chapter_count: usize,
    pub total_units: u64,
    pub message: String,
}

fn novels_root(state: &AppState) -> PathBuf {
    writing_data_dir(state).join("generated").join("novels")
}

fn writing_data_dir(state: &AppState) -> PathBuf {
    state
        .config_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf()
}

fn as_u64(value: &Value, key: &str) -> Option<u64> {
    value.get(key).and_then(Value::as_u64)
}

fn as_string(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn option_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
}

fn project_id(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("novel-project")
        .to_string()
}

async fn latest_export_path(project_dir: &Path) -> Result<Option<String>, AppError> {
    let exports_dir = project_dir.join("exports");
    let mut entries = match tokio::fs::read_dir(&exports_dir).await {
        Ok(entries) => entries,
        Err(_) => return Ok(None),
    };
    let mut newest: Option<(std::time::SystemTime, String)> = None;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let modified = entry
            .metadata()
            .await
            .and_then(|metadata| metadata.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        let path_text = path.to_string_lossy().to_string();
        if newest
            .as_ref()
            .map(|(current, _)| modified > *current)
            .unwrap_or(true)
        {
            newest = Some((modified, path_text));
        }
    }
    Ok(newest.map(|(_, path)| path))
}

async fn read_project(
    root: &Path,
    project_dir: PathBuf,
) -> Result<Option<NovelProjectDto>, AppError> {
    let manifest_path = project_dir.join("project.json");
    if !manifest_path.is_file() {
        return Ok(None);
    }
    let canonical_root = match tokio::fs::canonicalize(root).await {
        Ok(path) => path,
        Err(_) => return Ok(None),
    };
    let canonical_project = tokio::fs::canonicalize(&project_dir).await?;
    if !canonical_project.starts_with(&canonical_root) {
        return Ok(None);
    }

    let raw = tokio::fs::read_to_string(&manifest_path).await?;
    let manifest: Value = serde_json::from_str(&raw)?;
    let chapters = manifest
        .get("chapters")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let chapter_count = chapters.len();
    let mut total_units = 0_u64;
    let mut approved_chapters = 0_usize;
    let mut drafted_chapters = 0_usize;
    let mut needs_revision_chapters = 0_usize;
    for chapter in chapters {
        total_units += chapter
            .get("unit_count")
            .and_then(Value::as_u64)
            .unwrap_or_default();
        match chapter
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "approved" => approved_chapters += 1,
            "needs_revision" | "rejected" => needs_revision_chapters += 1,
            _ => drafted_chapters += 1,
        }
    }

    Ok(Some(NovelProjectDto {
        id: project_id(&canonical_project),
        title: as_string(&manifest, "title"),
        path: canonical_project.to_string_lossy().to_string(),
        created_at: option_string(&manifest, "created_at"),
        updated_at: option_string(&manifest, "updated_at"),
        language: as_string(&manifest, "language"),
        genre: as_string(&manifest, "genre"),
        target_units: as_u64(&manifest, "target_units"),
        chapter_unit_target: as_u64(&manifest, "chapter_unit_target"),
        chapter_count,
        approved_chapters,
        drafted_chapters,
        needs_revision_chapters,
        total_units,
        latest_export_path: latest_export_path(&canonical_project).await?,
    }))
}

fn resolve_project_path(root: &Path, requested: &str) -> Result<PathBuf, AppError> {
    let trimmed = requested.trim();
    if trimmed.is_empty() {
        return Err(anyhow::anyhow!("project_path is required").into());
    }
    let candidate = {
        let path = PathBuf::from(trimmed);
        if path.is_absolute() {
            path
        } else {
            root.join(path)
        }
    };
    let canonical_root = std::fs::canonicalize(root)?;
    let canonical_project = std::fs::canonicalize(candidate)?;
    if !canonical_project.starts_with(canonical_root) {
        return Err(anyhow::anyhow!("project_path is outside the local novel project root").into());
    }
    if !canonical_project.join("project.json").is_file() {
        return Err(
            anyhow::anyhow!("project_path does not contain a novel project manifest").into(),
        );
    }
    Ok(canonical_project)
}

pub async fn list_novel_projects(
    State(state): State<AppState>,
) -> Result<Json<NovelProjectsResponse>, AppError> {
    let root = novels_root(&state);
    let mut projects = Vec::new();
    let mut entries = match tokio::fs::read_dir(&root).await {
        Ok(entries) => entries,
        Err(_) => {
            return Ok(Json(NovelProjectsResponse {
                root: root.to_string_lossy().to_string(),
                projects,
            }))
        }
    };

    while let Some(entry) = entries.next_entry().await? {
        if let Some(project) = read_project(&root, entry.path()).await? {
            projects.push(project);
        }
    }
    projects.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));

    Ok(Json(NovelProjectsResponse {
        root: root.to_string_lossy().to_string(),
        projects,
    }))
}

pub async fn export_novel_project(
    State(state): State<AppState>,
    Json(request): Json<NovelExportRequest>,
) -> Result<Json<NovelExportResponse>, AppError> {
    let root = novels_root(&state);
    let project_dir = resolve_project_path(&root, &request.project_path)?;
    let format = match request.format.trim() {
        "" | "txt" => "txt",
        "md" => "md",
        other => return Err(anyhow::anyhow!("unsupported export format: {other}").into()),
    };

    let before = read_project(&root, project_dir.clone())
        .await?
        .ok_or_else(|| anyhow::anyhow!("project manifest is unavailable"))?;
    let tool = NovelStudioTool::new(writing_data_dir(&state), "panel")
        .with_artifact_manager(state.kernel.state_artifact().clone());
    let args = json!({
        "action": "export",
        "project_path": project_dir.to_string_lossy(),
        "format": format,
        "approved_only": request.approved_only
    });
    let result_text = tool.call(&args.to_string()).await?;
    let result: Value = serde_json::from_str(&result_text)?;
    let success = result
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !success {
        return Ok(Json(NovelExportResponse {
            exported: false,
            project_path: project_dir.to_string_lossy().to_string(),
            output_path: None,
            format: format.to_string(),
            chapter_count: before.chapter_count,
            total_units: before.total_units,
            message: result_text,
        }));
    }

    Ok(Json(NovelExportResponse {
        exported: true,
        project_path: project_dir.to_string_lossy().to_string(),
        output_path: result
            .get("output_path")
            .or_else(|| result.get("artifact_path"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        format: format.to_string(),
        chapter_count: before.chapter_count,
        total_units: before.total_units,
        message: "Novel project exported.".to_string(),
    }))
}
