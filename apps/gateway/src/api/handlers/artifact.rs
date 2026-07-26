use crate::api::state::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use benshu_state::{ArtifactCleanupPolicy, ArtifactCleanupReport, ArtifactQuery, ArtifactRecord};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, Serialize)]
pub struct ArtifactRecordDto {
    pub artifact_id: String,
    pub kind: String,
    pub uri: String,
    pub scope: String,
    pub lifecycle: String,
    pub created_at: String,
    pub updated_at: String,
    pub agent_id: String,
    pub task_id: Option<String>,
    pub run_id: Option<String>,
    pub trace_id: Option<String>,
    pub session_id: Option<String>,
    pub thread_id: Option<String>,
    pub tool_name: Option<String>,
    pub media_type: Option<String>,
    pub virtual_path: Option<String>,
    pub source_kind: String,
    pub metadata: std::collections::HashMap<String, String>,
}

impl From<ArtifactRecord> for ArtifactRecordDto {
    fn from(value: ArtifactRecord) -> Self {
        Self {
            artifact_id: value.artifact_id,
            kind: value.kind,
            uri: value.uri,
            scope: format!("{:?}", value.scope).to_lowercase(),
            lifecycle: format!("{:?}", value.lifecycle).to_lowercase(),
            created_at: value.created_at.to_rfc3339(),
            updated_at: value.updated_at.to_rfc3339(),
            agent_id: value.agent_id,
            task_id: value.task_id.map(|id| id.to_string()),
            run_id: value.run_id.map(|id| id.to_string()),
            trace_id: value.trace_id.map(|id| id.to_string()),
            session_id: value.session_id,
            thread_id: value.thread_id,
            tool_name: value.tool_name,
            media_type: value.media_type,
            virtual_path: value.virtual_path,
            source_kind: value.source_kind,
            metadata: value.metadata,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ArtifactCleanupReportDto {
    pub dry_run: bool,
    pub scanned: usize,
    pub matched: usize,
    pub deleted: usize,
    pub kept: usize,
    pub skipped_durable_without_policy: usize,
    pub deleted_artifact_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpenArtifactTargetRequest {
    pub artifact_id: Option<String>,
    pub target: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenArtifactTargetResponse {
    pub opened: bool,
    pub target: String,
    pub target_kind: String,
    pub opener: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpenTargetKind {
    Url,
    File,
}

#[derive(Debug, Clone)]
struct OpenTarget {
    target: String,
    kind: OpenTargetKind,
}

impl From<ArtifactCleanupReport> for ArtifactCleanupReportDto {
    fn from(value: ArtifactCleanupReport) -> Self {
        Self {
            dry_run: value.dry_run,
            scanned: value.scanned,
            matched: value.matched,
            deleted: value.deleted,
            kept: value.kept,
            skipped_durable_without_policy: value.skipped_durable_without_policy,
            deleted_artifact_ids: value.deleted_artifact_ids,
        }
    }
}

pub async fn list_artifacts(
    State(state): State<AppState>,
    Query(query): Query<ArtifactQuery>,
) -> Result<Json<Vec<ArtifactRecordDto>>, (StatusCode, String)> {
    let artifacts = state
        .kernel
        .state_artifact()
        .query(&query)
        .await
        .map_err(internal_error)?;
    Ok(Json(
        artifacts.into_iter().map(ArtifactRecordDto::from).collect(),
    ))
}

pub async fn get_artifact(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ArtifactRecordDto>, (StatusCode, String)> {
    let Some(artifact) = state
        .kernel
        .state_artifact()
        .load(&id)
        .await
        .map_err(internal_error)?
    else {
        return Err((
            StatusCode::NOT_FOUND,
            format!("artifact not found for id {}", id),
        ));
    };
    Ok(Json(ArtifactRecordDto::from(artifact)))
}

pub async fn cleanup_artifacts(
    State(state): State<AppState>,
    Json(policy): Json<ArtifactCleanupPolicy>,
) -> Result<Json<ArtifactCleanupReportDto>, (StatusCode, String)> {
    let report = state
        .kernel
        .state_artifact()
        .cleanup(&policy)
        .await
        .map_err(internal_error)?;
    Ok(Json(ArtifactCleanupReportDto::from(report)))
}

pub async fn open_artifact_target(
    State(state): State<AppState>,
    Json(request): Json<OpenArtifactTargetRequest>,
) -> Result<Json<OpenArtifactTargetResponse>, (StatusCode, String)> {
    let raw_target = if let Some(artifact_id) = request
        .artifact_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        let Some(artifact) = state
            .kernel
            .state_artifact()
            .load(artifact_id)
            .await
            .map_err(internal_error)?
        else {
            return Err((
                StatusCode::NOT_FOUND,
                format!("artifact not found for id {artifact_id}"),
            ));
        };
        artifact.uri
    } else {
        request.target.unwrap_or_default()
    };

    let target = classify_open_target(&raw_target)
        .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;
    let opener_target = platform_open_target(&target);
    let opener = open_with_platform_default(&opener_target)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let target_kind = match target.kind {
        OpenTargetKind::Url => "url",
        OpenTargetKind::File => "file",
    };

    Ok(Json(OpenArtifactTargetResponse {
        opened: true,
        target: target.target,
        target_kind: target_kind.to_string(),
        opener,
        message: "Opened with the operating system default application.".to_string(),
    }))
}

fn internal_error(error: anyhow::Error) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

fn classify_open_target(raw: &str) -> anyhow::Result<OpenTarget> {
    let trimmed = raw.trim().trim_matches('"').trim_matches('\'');
    if trimmed.is_empty() {
        anyhow::bail!("open target is empty");
    }

    let lowered = trimmed.to_ascii_lowercase();
    if lowered.starts_with("http://") || lowered.starts_with("https://") {
        return Ok(OpenTarget {
            target: trimmed.to_string(),
            kind: OpenTargetKind::Url,
        });
    }
    if lowered.starts_with("file://") {
        return classify_file_target(&decode_file_uri(trimmed));
    }

    classify_file_target(trimmed)
}

fn classify_file_target(raw: &str) -> anyhow::Result<OpenTarget> {
    let Some(path) = resolve_local_open_path(raw) else {
        anyhow::bail!("only http(s) URLs or existing local files can be opened");
    };
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .unwrap_or_default();
    if extension.is_empty() {
        anyhow::bail!("local open target must have a file extension");
    }
    if blocked_open_extension(&extension) {
        anyhow::bail!("refusing to open executable or script-like file type: .{extension}");
    }
    if !allowed_open_extension(&extension) {
        anyhow::bail!("file type .{extension} is not configured for one-click opening");
    }

    Ok(OpenTarget {
        target: path.to_string_lossy().to_string(),
        kind: OpenTargetKind::File,
    })
}

fn resolve_local_open_path(raw: &str) -> Option<PathBuf> {
    let candidate = PathBuf::from(raw);
    if candidate.exists() {
        return canonical_or_original(candidate);
    }
    if candidate.is_absolute() || looks_like_windows_absolute(raw) {
        return None;
    }
    let joined = std::env::current_dir().ok()?.join(candidate);
    if joined.exists() {
        return canonical_or_original(joined);
    }
    None
}

fn canonical_or_original(path: PathBuf) -> Option<PathBuf> {
    Some(std::fs::canonicalize(&path).unwrap_or(path))
}

fn looks_like_windows_absolute(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 3 && bytes[1] == b':' && (bytes[2] == b'\\' || bytes[2] == b'/')
        || value.starts_with("\\\\")
}

fn decode_file_uri(uri: &str) -> String {
    let mut rest = uri.trim_start_matches("file://").to_string();
    if rest.len() >= 3 && rest.as_bytes()[0] == b'/' && rest.as_bytes()[2] == b':' {
        rest.remove(0);
    }
    urlencoding::decode(&rest)
        .map(|decoded| decoded.into_owned())
        .unwrap_or(rest)
}

fn blocked_open_extension(ext: &str) -> bool {
    matches!(
        ext,
        "exe"
            | "bat"
            | "cmd"
            | "com"
            | "ps1"
            | "psm1"
            | "vbs"
            | "vbe"
            | "js"
            | "jse"
            | "msi"
            | "msp"
            | "scr"
            | "lnk"
            | "reg"
            | "hta"
            | "sh"
            | "bash"
            | "zsh"
            | "fish"
            | "app"
    )
}

fn allowed_open_extension(ext: &str) -> bool {
    matches!(
        ext,
        "pdf"
            | "txt"
            | "log"
            | "md"
            | "markdown"
            | "csv"
            | "json"
            | "jsonl"
            | "html"
            | "htm"
            | "xml"
            | "png"
            | "jpg"
            | "jpeg"
            | "webp"
            | "gif"
            | "bmp"
            | "svg"
            | "mp3"
            | "wav"
            | "ogg"
            | "m4a"
            | "flac"
            | "mp4"
            | "mov"
            | "avi"
            | "mkv"
            | "webm"
            | "doc"
            | "docx"
            | "xls"
            | "xlsx"
            | "ppt"
            | "pptx"
            | "rtf"
            | "odt"
            | "ods"
            | "odp"
    )
}

fn platform_open_target(target: &OpenTarget) -> String {
    if target.kind == OpenTargetKind::File && running_inside_wsl() {
        if let Ok(output) = Command::new("wslpath")
            .arg("-w")
            .arg(&target.target)
            .output()
        {
            if output.status.success() {
                let converted = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !converted.is_empty() {
                    return converted;
                }
            }
        }
    }
    target.target.clone()
}

fn running_inside_wsl() -> bool {
    if std::env::var_os("WSL_INTEROP").is_some() || std::env::var_os("WSL_DISTRO_NAME").is_some() {
        return true;
    }
    std::fs::read_to_string("/proc/version")
        .map(|version| version.to_ascii_lowercase().contains("microsoft"))
        .unwrap_or(false)
}

fn open_with_platform_default(target: &str) -> anyhow::Result<String> {
    let (program, args): (&str, Vec<&str>) = if cfg!(target_os = "windows") {
        ("explorer", vec![target])
    } else if cfg!(target_os = "macos") {
        ("open", vec![target])
    } else if running_inside_wsl() {
        ("explorer.exe", vec![target])
    } else {
        ("xdg-open", vec![target])
    };

    Command::new(program)
        .args(args)
        .spawn()
        .map_err(|error| anyhow::anyhow!("failed to launch {program} for open target: {error}"))?;
    Ok(program.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_http_pdf_as_url() {
        let target = classify_open_target("https://bitcoin.org/bitcoin.pdf").unwrap();
        assert_eq!(target.kind, OpenTargetKind::Url);
        assert_eq!(target.target, "https://bitcoin.org/bitcoin.pdf");
    }

    #[test]
    fn classifies_existing_safe_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.txt");
        std::fs::write(&path, "hello").unwrap();
        let target = classify_open_target(&path.to_string_lossy()).unwrap();
        assert_eq!(target.kind, OpenTargetKind::File);
        assert!(target.target.ends_with("report.txt"));
    }

    #[test]
    fn rejects_script_like_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run.ps1");
        std::fs::write(&path, "Write-Host test").unwrap();
        let error = classify_open_target(&path.to_string_lossy()).unwrap_err();
        assert!(error.to_string().contains("refusing to open"));
    }

    #[test]
    fn decodes_file_uri() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hello world.pdf");
        std::fs::write(&path, b"%PDF").unwrap();
        let uri = format!("file://{}", path.to_string_lossy().replace(' ', "%20"));
        let target = classify_open_target(&uri).unwrap();
        assert_eq!(target.kind, OpenTargetKind::File);
        assert!(target.target.ends_with("hello world.pdf"));
    }

    #[test]
    fn allowed_extension_list_includes_windows_native_documents() {
        for ext in ["pdf", "txt", "html", "png", "docx", "xlsx", "pptx"] {
            assert!(allowed_open_extension(ext));
        }
    }

    #[test]
    fn blocked_extension_list_rejects_executables() {
        for ext in ["exe", "bat", "cmd", "ps1", "msi", "lnk"] {
            assert!(blocked_open_extension(ext));
        }
    }

    #[test]
    fn recognizes_windows_absolute_paths() {
        assert!(looks_like_windows_absolute(r"C:\Users\alice\report.pdf"));
        assert!(looks_like_windows_absolute(r"\\server\share\report.pdf"));
        assert!(!looks_like_windows_absolute("data/report.pdf"));
    }
}
