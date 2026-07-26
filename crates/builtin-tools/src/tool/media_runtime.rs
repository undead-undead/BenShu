use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::process::Command;

use benshu_inference::backend::audio_preprocess as media_audio;
use benshu_inference::backend::video as media_video;
use benshu_infra::error::Error;
use benshu_infra::{Tool, ToolDefinition};
use benshu_state::{ArtifactLifecycle, ArtifactManager};

use super::{register_tool_output_artifact, ToolArtifactRegistration, ToolCleanup};

const MAX_NORMALIZED_AUDIO_BYTES: u64 = 100 * 1024 * 1024;

fn ffmpeg_binary(name: &str) -> PathBuf {
    #[cfg(target_os = "windows")]
    let name_with_ext = format!("{name}.exe");
    #[cfg(not(target_os = "windows"))]
    let name_with_ext = name.to_string();

    let local_bin = std::env::current_exe().ok().and_then(|p| {
        p.parent()
            .map(|parent| parent.join("bin").join(&name_with_ext))
    });
    if let Some(path) = local_bin {
        if path.exists() {
            return path;
        }
    }
    PathBuf::from(name)
}

fn infer_media_kind(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("mp4" | "mov" | "avi" | "mkv" | "webm") => "video",
        Some("mp3" | "wav" | "ogg" | "m4a" | "flac" | "aac") => "audio",
        Some("png" | "jpg" | "jpeg" | "webp" | "bmp" | "gif") => "image",
        _ => "unknown",
    }
}

fn auto_output_path(input: &Path, suffix: &str, extension: &str) -> PathBuf {
    let base = input
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("media");
    std::env::temp_dir().join(format!(
        "benshu-{base}-{suffix}-{}.{}",
        uuid::Uuid::new_v4(),
        extension
    ))
}

fn cleanup_for_output(user_supplied: bool) -> ToolCleanup {
    if user_supplied {
        ToolCleanup::inactive()
    } else {
        ToolCleanup::active(
            "temporary_media_output",
            "auto_generated_media_runtime_output",
            "The media preprocessing output was written to an BenShu-managed temporary path. Move it if you want to keep it permanently.",
            "Delete the generated temp file after downstream use if you do not need to keep it.",
            false,
        )
    }
}

async fn ensure_path_exists(path: &str, tool_name: &str) -> anyhow::Result<PathBuf> {
    let path_buf = PathBuf::from(path);
    if !tokio::fs::try_exists(&path_buf).await.unwrap_or(false) {
        return Err(Error::ToolArguments {
            tool_name: tool_name.to_string(),
            message: format!("File not found: {path}"),
        }
        .into());
    }
    Ok(path_buf)
}

async fn command_output(command: &Path, args: &[String]) -> anyhow::Result<std::process::Output> {
    let output = Command::new(command)
        .args(args)
        .output()
        .await
        .map_err(|err| media_command_start_error(command, err))?;
    Ok(output)
}

async fn command_status(
    command: &Path,
    args: &[String],
) -> anyhow::Result<std::process::ExitStatus> {
    let status = Command::new(command)
        .args(args)
        .status()
        .await
        .map_err(|err| media_command_start_error(command, err))?;
    Ok(status)
}

fn media_command_start_error(command: &Path, err: std::io::Error) -> anyhow::Error {
    let hint = match err.kind() {
        ErrorKind::NotFound => {
            "install ffmpeg/ffprobe or configure the bundled Windows media runtime"
        }
        ErrorKind::PermissionDenied => {
            "the media runtime binary exists but is not executable; fix file permissions or choose a valid ffmpeg/ffprobe path"
        }
        _ => "check the media runtime installation and executable path",
    };
    Error::ToolExecution {
        tool_name: "media_runtime".to_string(),
        message: format!(
            "failed to start media dependency `{}`: {err}. {hint}",
            command.display()
        ),
    }
    .into()
}

pub struct AudioArtifact {
    pub output_path: PathBuf,
    pub sample_rate: u32,
    pub channels: u8,
    pub cleanup: ToolCleanup,
}

pub struct VideoFrameArtifacts {
    pub output_dir: PathBuf,
    pub frame_paths: Vec<PathBuf>,
    pub duration_secs: f32,
    pub fps: f32,
    pub cleanup: ToolCleanup,
}

pub async fn extract_video_frame_artifacts(
    input: &Path,
    frame_count: usize,
    output_dir: Option<PathBuf>,
) -> anyhow::Result<VideoFrameArtifacts> {
    let output_dir_user_supplied = output_dir.is_some();
    let output_dir = output_dir.unwrap_or_else(|| auto_output_path(input, "frames", "dir"));
    let artifacts = media_video::extract_frame_artifacts(input, frame_count, output_dir.clone())
        .await
        .map_err(|error| Error::ToolArguments {
            tool_name: "extract_video_frames".to_string(),
            message: error.to_string(),
        })?;

    Ok(VideoFrameArtifacts {
        output_dir: artifacts.output_dir,
        frame_paths: artifacts.frame_paths,
        duration_secs: artifacts.duration_secs,
        fps: artifacts.fps,
        cleanup: cleanup_for_output(output_dir_user_supplied),
    })
}

pub async fn sample_video_frames_for_analysis(
    input: &Path,
    frame_count: usize,
) -> anyhow::Result<Vec<image::DynamicImage>> {
    media_video::sample_frames(input, frame_count)
        .await
        .map_err(anyhow::Error::from)
}

pub async fn extract_audio_track_artifact(
    input: &Path,
    output_path: Option<PathBuf>,
    sample_rate: u32,
    channels: u8,
) -> anyhow::Result<AudioArtifact> {
    let output_user_supplied = output_path.is_some();
    let output_path = output_path.unwrap_or_else(|| auto_output_path(input, "audio-track", "wav"));
    let artifact =
        media_audio::extract_audio_track_artifact(input, output_path, sample_rate, channels)
            .await
            .map_err(|error| Error::ToolArguments {
                tool_name: "extract_audio_track".to_string(),
                message: error.to_string(),
            })?;

    Ok(AudioArtifact {
        output_path: artifact.output_path,
        sample_rate: artifact.sample_rate,
        channels: artifact.channels,
        cleanup: cleanup_for_output(output_user_supplied),
    })
}

pub async fn normalize_audio_artifact(
    input: &Path,
    output_path: Option<PathBuf>,
    sample_rate: u32,
    channels: u8,
) -> anyhow::Result<AudioArtifact> {
    let output_user_supplied = output_path.is_some();
    let output_path = output_path.unwrap_or_else(|| auto_output_path(input, "normalized", "wav"));
    let artifact = media_audio::normalize_audio_artifact(input, output_path, sample_rate, channels)
        .await
        .map_err(|error| Error::ToolArguments {
            tool_name: "normalize_audio".to_string(),
            message: error.to_string(),
        })?;

    Ok(AudioArtifact {
        output_path: artifact.output_path,
        sample_rate: artifact.sample_rate,
        channels: artifact.channels,
        cleanup: cleanup_for_output(output_user_supplied),
    })
}

pub async fn normalize_audio_bytes_for_stt(
    input: &Path,
    sample_rate: u32,
    channels: u8,
) -> anyhow::Result<Vec<u8>> {
    let temp_dir = tempfile::tempdir()?;
    let artifact = normalize_audio_artifact(
        input,
        Some(temp_dir.path().join("normalized.wav")),
        sample_rate,
        channels,
    )
    .await?;
    let metadata = tokio::fs::metadata(&artifact.output_path).await?;
    if metadata.len() > MAX_NORMALIZED_AUDIO_BYTES {
        anyhow::bail!(
            "normalized audio output is larger than the 100MB safety limit: {} bytes",
            metadata.len()
        );
    }
    Ok(tokio::fs::read(artifact.output_path).await?)
}

async fn register_media_output_artifact(
    artifact_manager: Option<&ArtifactManager>,
    agent_id: &str,
    tool_name: &str,
    output_path: &Path,
    kind: &str,
    metadata: HashMap<String, String>,
) -> anyhow::Result<Option<serde_json::Value>> {
    let Some(manager) = artifact_manager else {
        return Ok(None);
    };

    let record = register_tool_output_artifact(
        manager,
        agent_id,
        tool_name,
        &output_path.to_string_lossy(),
        ArtifactLifecycle::Session,
        kind,
        metadata,
    )
    .await?;
    Ok(Some(
        ToolArtifactRegistration::from_record(&record).as_json(),
    ))
}

#[derive(Deserialize)]
struct ProbeMediaArgs {
    path: String,
}

pub struct ProbeMediaTool;

#[async_trait]
impl Tool for ProbeMediaTool {
    fn name(&self) -> String {
        "probe_media".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Probe media metadata using ffprobe. Use this before video/audio preprocessing to inspect streams, duration, codec, and dimensions.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Absolute path to the local media file." }
                },
                "required": ["path"]
            }),
            parameters_ts: None,
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some("Use this first for local video/audio files to inspect stream structure and duration before extracting frames or audio.".to_string()),
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let args: ProbeMediaArgs =
            serde_json::from_str(arguments).map_err(|e| Error::ToolArguments {
                tool_name: self.name(),
                message: format!("Invalid arguments: {e}"),
            })?;
        let input = ensure_path_exists(&args.path, &self.name()).await?;
        let ffprobe = ffmpeg_binary("ffprobe");
        let output = command_output(
            &ffprobe,
            &[
                "-v".to_string(),
                "error".to_string(),
                "-show_format".to_string(),
                "-show_streams".to_string(),
                "-print_format".to_string(),
                "json".to_string(),
                input.display().to_string(),
            ],
        )
        .await?;

        if !output.status.success() {
            return Ok(json!({
                "status": "error",
                "tool": self.name(),
                "path": input,
                "error": String::from_utf8_lossy(&output.stderr).trim(),
            })
            .to_string());
        }

        let raw: serde_json::Value = serde_json::from_slice(&output.stdout)?;
        let duration = raw
            .get("format")
            .and_then(|f| f.get("duration"))
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let bit_rate = raw
            .get("format")
            .and_then(|f| f.get("bit_rate"))
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let streams = raw
            .get("streams")
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default();
        let normalized_streams: Vec<_> = streams
            .into_iter()
            .map(|stream| {
                json!({
                    "index": stream.get("index").cloned().unwrap_or(serde_json::Value::Null),
                    "codec_type": stream.get("codec_type").cloned().unwrap_or(serde_json::Value::Null),
                    "codec_name": stream.get("codec_name").cloned().unwrap_or(serde_json::Value::Null),
                    "width": stream.get("width").cloned().unwrap_or(serde_json::Value::Null),
                    "height": stream.get("height").cloned().unwrap_or(serde_json::Value::Null),
                    "sample_rate": stream.get("sample_rate").cloned().unwrap_or(serde_json::Value::Null),
                    "channels": stream.get("channels").cloned().unwrap_or(serde_json::Value::Null),
                    "duration": stream.get("duration").cloned().unwrap_or(serde_json::Value::Null),
                })
            })
            .collect();

        Ok(serde_json::to_string_pretty(&json!({
            "status": "ok",
            "tool": self.name(),
            "path": input,
            "media_kind": infer_media_kind(&input),
            "duration_secs": duration,
            "bit_rate": bit_rate,
            "streams": normalized_streams,
            "probe_engine": "ffprobe"
        }))?)
    }
}

#[derive(Deserialize)]
struct ExtractVideoFramesArgs {
    path: String,
    #[serde(default = "default_frame_count")]
    frame_count: usize,
    #[serde(default)]
    output_dir: Option<String>,
}

fn default_frame_count() -> usize {
    4
}

pub struct ExtractVideoFramesTool {
    artifact_manager: Option<Arc<ArtifactManager>>,
    agent_id: String,
}

impl ExtractVideoFramesTool {
    pub fn new(agent_id: impl Into<String>) -> Self {
        Self {
            artifact_manager: None,
            agent_id: agent_id.into(),
        }
    }

    pub fn with_artifact_manager(mut self, manager: Arc<ArtifactManager>) -> Self {
        self.artifact_manager = Some(manager);
        self
    }
}

#[async_trait]
impl Tool for ExtractVideoFramesTool {
    fn name(&self) -> String {
        "extract_video_frames".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Extract representative frames from a local video file via ffmpeg.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Absolute path to the local video file." },
                    "frame_count": { "type": "integer", "description": "Number of representative frames to extract.", "default": 4 },
                    "output_dir": { "type": "string", "description": "Optional output directory for extracted frames." }
                },
                "required": ["path"]
            }),
            parameters_ts: None,
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some("Use this before VLM/video understanding so frame sampling is explicit and reusable.".to_string()),
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let args: ExtractVideoFramesArgs =
            serde_json::from_str(arguments).map_err(|e| Error::ToolArguments {
                tool_name: self.name(),
                message: format!("Invalid arguments: {e}"),
            })?;
        let input = ensure_path_exists(&args.path, &self.name()).await?;
        let artifacts = extract_video_frame_artifacts(
            &input,
            args.frame_count,
            args.output_dir.as_ref().map(PathBuf::from),
        )
        .await;
        let Ok(artifacts) = artifacts else {
            return Ok(serde_json::to_string_pretty(&json!({
                "status": "error",
                "tool": self.name(),
                "path": input,
                "error": "ffmpeg failed to extract frames"
            }))?);
        };
        let frames = artifacts
            .frame_paths
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>();
        let artifact_registration = register_media_output_artifact(
            self.artifact_manager.as_deref(),
            &self.agent_id,
            &self.name(),
            &artifacts.output_dir,
            "video_frame_output_dir",
            HashMap::from([
                ("frame_count".to_string(), frames.len().to_string()),
                (
                    "duration_secs".to_string(),
                    format!("{:.3}", artifacts.duration_secs),
                ),
            ]),
        )
        .await?;

        Ok(serde_json::to_string_pretty(&json!({
            "status": "ok",
            "tool": self.name(),
            "path": input,
            "frame_count_requested": args.frame_count,
            "frame_count_extracted": frames.len(),
            "frames": frames,
            "output_dir": artifacts.output_dir,
            "artifact_kind": "video_frame_output_dir",
            "duration_secs": artifacts.duration_secs,
            "fps": artifacts.fps,
            "cleanup": artifacts.cleanup.as_json(),
            "artifact_registration": artifact_registration,
        }))?)
    }
}

#[derive(Deserialize)]
struct ExtractAudioTrackArgs {
    path: String,
    #[serde(default)]
    output_path: Option<String>,
}

pub struct ExtractAudioTrackTool {
    artifact_manager: Option<Arc<ArtifactManager>>,
    agent_id: String,
}

impl ExtractAudioTrackTool {
    pub fn new(agent_id: impl Into<String>) -> Self {
        Self {
            artifact_manager: None,
            agent_id: agent_id.into(),
        }
    }

    pub fn with_artifact_manager(mut self, manager: Arc<ArtifactManager>) -> Self {
        self.artifact_manager = Some(manager);
        self
    }
}

#[async_trait]
impl Tool for ExtractAudioTrackTool {
    fn name(&self) -> String {
        "extract_audio_track".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Extract the audio track from a local video or audio container into a WAV file via ffmpeg.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Absolute path to the local media file." },
                    "output_path": { "type": "string", "description": "Optional output WAV path." }
                },
                "required": ["path"]
            }),
            parameters_ts: None,
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some("Use this when STT or audio analysis should consume a deterministic extracted track instead of a container file.".to_string()),
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let args: ExtractAudioTrackArgs =
            serde_json::from_str(arguments).map_err(|e| Error::ToolArguments {
                tool_name: self.name(),
                message: format!("Invalid arguments: {e}"),
            })?;
        let input = ensure_path_exists(&args.path, &self.name()).await?;
        let artifact = extract_audio_track_artifact(
            &input,
            args.output_path.as_ref().map(PathBuf::from),
            16_000,
            1,
        )
        .await;
        let Ok(artifact) = artifact else {
            return Ok(serde_json::to_string_pretty(&json!({
                "status": "error",
                "tool": self.name(),
                "path": input,
                "error": "ffmpeg failed to extract audio track"
            }))?);
        };

        let artifact_registration = register_media_output_artifact(
            self.artifact_manager.as_deref(),
            &self.agent_id,
            &self.name(),
            &artifact.output_path,
            "audio_track_output",
            HashMap::from([
                ("sample_rate".to_string(), artifact.sample_rate.to_string()),
                ("channels".to_string(), artifact.channels.to_string()),
            ]),
        )
        .await?;

        Ok(serde_json::to_string_pretty(&json!({
            "status": "ok",
            "tool": self.name(),
            "path": input,
            "sample_rate": artifact.sample_rate,
            "channels": artifact.channels,
            "output_path": artifact.output_path,
            "artifact_kind": "audio_track_output",
            "cleanup": artifact.cleanup.as_json(),
            "artifact_registration": artifact_registration,
        }))?)
    }
}

#[derive(Deserialize)]
struct NormalizeAudioArgs {
    path: String,
    #[serde(default)]
    output_path: Option<String>,
    #[serde(default = "default_sample_rate")]
    sample_rate: u32,
    #[serde(default = "default_audio_channels")]
    channels: u8,
}

fn default_sample_rate() -> u32 {
    16_000
}

fn default_audio_channels() -> u8 {
    1
}

pub struct NormalizeAudioTool {
    artifact_manager: Option<Arc<ArtifactManager>>,
    agent_id: String,
}

impl NormalizeAudioTool {
    pub fn new(agent_id: impl Into<String>) -> Self {
        Self {
            artifact_manager: None,
            agent_id: agent_id.into(),
        }
    }

    pub fn with_artifact_manager(mut self, manager: Arc<ArtifactManager>) -> Self {
        self.artifact_manager = Some(manager);
        self
    }
}

#[async_trait]
impl Tool for NormalizeAudioTool {
    fn name(&self) -> String {
        "normalize_audio".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Normalize a local audio file into a deterministic mono PCM WAV preprocessing output.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Absolute path to the local audio file." },
                    "output_path": { "type": "string", "description": "Optional normalized WAV output path." },
                    "sample_rate": { "type": "integer", "description": "Target sample rate.", "default": 16000 },
                    "channels": { "type": "integer", "description": "Target channel count.", "default": 1 }
                },
                "required": ["path"]
            }),
            parameters_ts: None,
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some("Use this before STT or audio inspection to standardize local audio inputs into a stable preprocessing surface.".to_string()),
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let args: NormalizeAudioArgs =
            serde_json::from_str(arguments).map_err(|e| Error::ToolArguments {
                tool_name: self.name(),
                message: format!("Invalid arguments: {e}"),
            })?;
        let input = ensure_path_exists(&args.path, &self.name()).await?;
        let artifact = normalize_audio_artifact(
            &input,
            args.output_path.as_ref().map(PathBuf::from),
            args.sample_rate,
            args.channels,
        )
        .await;
        let Ok(artifact) = artifact else {
            return Ok(serde_json::to_string_pretty(&json!({
                "status": "error",
                "tool": self.name(),
                "path": input,
                "error": "ffmpeg failed to normalize audio"
            }))?);
        };

        let artifact_registration = register_media_output_artifact(
            self.artifact_manager.as_deref(),
            &self.agent_id,
            &self.name(),
            &artifact.output_path,
            "normalized_audio_output",
            HashMap::from([
                ("sample_rate".to_string(), artifact.sample_rate.to_string()),
                ("channels".to_string(), artifact.channels.to_string()),
            ]),
        )
        .await?;

        Ok(serde_json::to_string_pretty(&json!({
            "status": "ok",
            "tool": self.name(),
            "path": input,
            "sample_rate": artifact.sample_rate,
            "channels": artifact.channels,
            "output_path": artifact.output_path,
            "artifact_kind": "normalized_audio_output",
            "cleanup": artifact.cleanup.as_json(),
            "artifact_registration": artifact_registration,
        }))?)
    }
}

#[derive(Deserialize)]
struct RenderVideoThumbnailArgs {
    path: String,
    #[serde(default)]
    output_path: Option<String>,
    #[serde(default = "default_thumbnail_second")]
    second: f32,
}

fn default_thumbnail_second() -> f32 {
    1.0
}

pub struct RenderVideoThumbnailTool {
    artifact_manager: Option<Arc<ArtifactManager>>,
    agent_id: String,
}

impl RenderVideoThumbnailTool {
    pub fn new(agent_id: impl Into<String>) -> Self {
        Self {
            artifact_manager: None,
            agent_id: agent_id.into(),
        }
    }

    pub fn with_artifact_manager(mut self, manager: Arc<ArtifactManager>) -> Self {
        self.artifact_manager = Some(manager);
        self
    }
}

#[async_trait]
impl Tool for RenderVideoThumbnailTool {
    fn name(&self) -> String {
        "render_video_thumbnail".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Render a representative thumbnail image from a local video file via ffmpeg.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Absolute path to the local video file." },
                    "output_path": { "type": "string", "description": "Optional output thumbnail path." },
                    "second": { "type": "number", "description": "Timestamp in seconds for the thumbnail frame.", "default": 1.0 }
                },
                "required": ["path"]
            }),
            parameters_ts: None,
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some("Use this when a stable video poster frame is needed for previews, OCR, or attachment inspection.".to_string()),
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let args: RenderVideoThumbnailArgs =
            serde_json::from_str(arguments).map_err(|e| Error::ToolArguments {
                tool_name: self.name(),
                message: format!("Invalid arguments: {e}"),
            })?;
        let input = ensure_path_exists(&args.path, &self.name()).await?;
        let output_path = args
            .output_path
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| auto_output_path(&input, "thumbnail", "jpg"));
        let ffmpeg = ffmpeg_binary("ffmpeg");
        let status = command_status(
            &ffmpeg,
            &[
                "-ss".to_string(),
                args.second.to_string(),
                "-i".to_string(),
                input.display().to_string(),
                "-frames:v".to_string(),
                "1".to_string(),
                "-q:v".to_string(),
                "2".to_string(),
                "-y".to_string(),
                output_path.display().to_string(),
            ],
        )
        .await?;
        if !status.success() {
            return Ok(serde_json::to_string_pretty(&json!({
                "status": "error",
                "tool": self.name(),
                "path": input,
                "error": "ffmpeg failed to render thumbnail"
            }))?);
        }

        let artifact_registration = register_media_output_artifact(
            self.artifact_manager.as_deref(),
            &self.agent_id,
            &self.name(),
            &output_path,
            "video_thumbnail_output",
            HashMap::from([("timestamp_secs".to_string(), args.second.to_string())]),
        )
        .await?;

        Ok(serde_json::to_string_pretty(&json!({
            "status": "ok",
            "tool": self.name(),
            "path": input,
            "timestamp_secs": args.second,
            "output_path": output_path,
            "artifact_kind": "video_thumbnail_output",
            "cleanup": cleanup_for_output(args.output_path.is_some()).as_json(),
            "artifact_registration": artifact_registration,
        }))?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use redb::Database;

    #[tokio::test]
    async fn probe_media_reports_missing_file() {
        let tool = ProbeMediaTool;
        let error = tool
            .call(&json!({"path": "/tmp/definitely-missing-media.mp4"}).to_string())
            .await
            .expect_err("missing file should error");
        assert!(error.to_string().contains("File not found"));
    }

    #[tokio::test]
    async fn normalize_audio_reports_missing_file() {
        let tool = NormalizeAudioTool::new("media-test");
        let error = tool
            .call(&json!({"path": "/tmp/definitely-missing-audio.wav"}).to_string())
            .await
            .expect_err("missing file should error");
        assert!(error.to_string().contains("File not found"));
    }

    #[tokio::test]
    async fn register_media_output_artifact_persists_when_registry_is_present() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db = Database::create(temp.path().join("media-artifacts.redb")).expect("db");
        let manager = ArtifactManager::new(Arc::new(db));
        let output_path = temp.path().join("normalized.wav");
        tokio::fs::write(&output_path, b"fake")
            .await
            .expect("write");

        let registration = register_media_output_artifact(
            Some(&manager),
            "media-agent",
            "normalize_audio",
            &output_path,
            "normalized_audio_output",
            HashMap::from([("sample_rate".to_string(), "16000".to_string())]),
        )
        .await
        .expect("registration")
        .expect("artifact registration");

        assert_eq!(registration["registered"], true);
        assert_eq!(registration["source_kind"], "builtin_tool_output");
    }
}
