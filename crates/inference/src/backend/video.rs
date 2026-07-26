use crate::backend::{InferenceError, Result};
use image::DynamicImage;
use std::path::{Path, PathBuf};
use tempfile::tempdir;
use tokio::process::Command;
use tracing::{error, info};

pub struct VideoFrameArtifacts {
    pub output_dir: PathBuf,
    pub frame_paths: Vec<PathBuf>,
    pub duration_secs: f32,
    pub fps: f32,
}

/// Attempts to find the ffmpeg/ffprobe binary, checking local bin/ first
fn find_binary(name: &str) -> PathBuf {
    #[cfg(target_os = "windows")]
    let name_with_ext = format!("{}.exe", name);
    #[cfg(not(target_os = "windows"))]
    let name_with_ext = name.to_string();

    let local_bin = std::env::current_exe().ok().and_then(|p| {
        p.parent()
            .map(|parent| parent.join("bin").join(&name_with_ext))
    });

    if let Some(p) = local_bin {
        if p.exists() {
            return p;
        }
    }
    PathBuf::from(name)
}

/// Gets video duration in seconds using ffprobe
async fn get_video_duration(video_path: &Path) -> Result<f32> {
    let ffprobe = find_binary("ffprobe");

    let output = Command::new(&ffprobe)
        .arg("-v")
        .arg("error")
        .arg("-show_entries")
        .arg("format=duration")
        .arg("-of")
        .arg("default=noprint_wrappers=1:nokey=1")
        .arg(video_path)
        .output()
        .await
        .map_err(|e| {
            InferenceError::Execution(
                format!("ffprobe failed: {}", e),
                "video_processor".to_string(),
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(InferenceError::Execution(
            format!("ffprobe could not read video duration: {}", stderr),
            "video_processor".to_string(),
        ));
    }

    let duration_str = String::from_utf8_lossy(&output.stdout);
    duration_str.trim().parse::<f32>().map_err(|e| {
        InferenceError::Execution(
            format!("Invalid duration output from ffprobe: {}", e),
            "video_processor".to_string(),
        )
    })
}

/// Extracts representative video frame files into a directory.
pub async fn extract_frame_artifacts(
    video_path: &Path,
    num_frames: usize,
    output_dir: PathBuf,
) -> Result<VideoFrameArtifacts> {
    let duration = get_video_duration(video_path).await.unwrap_or(5.0); // Fallback to 5s if probe fails
    let fps = (num_frames as f32 / duration.max(0.1)).max(0.1);
    tokio::fs::create_dir_all(&output_dir)
        .await
        .map_err(|e| InferenceError::Internal(format!("Output dir failed: {}", e)))?;
    let output_pattern = output_dir.join("frame_%03d.jpg");

    info!(
        "🎬 Video Processor: Extracting {} frames from {} (Duration: {:.2}s, FPS: {:.2})",
        num_frames,
        video_path.display(),
        duration,
        fps
    );

    let ffmpeg = find_binary("ffmpeg");
    let status = Command::new(ffmpeg)
        .arg("-i")
        .arg(video_path)
        .arg("-vf")
        .arg(format!("fps={}", fps))
        .arg("-frames:v")
        .arg(num_frames.to_string())
        .arg("-y") // Overwrite output
        .arg(&output_pattern)
        .status()
        .await;

    match status {
        Ok(s) if s.success() => {
            let mut frame_paths = Vec::new();
            for i in 1..=num_frames {
                let frame_path = output_dir.join(format!("frame_{:03}.jpg", i));
                if frame_path.exists() {
                    frame_paths.push(frame_path);
                }
            }
            Ok(VideoFrameArtifacts {
                output_dir,
                frame_paths,
                duration_secs: duration,
                fps,
            })
        }
        _ => Err(InferenceError::Execution(
            "Failed to extract frames via ffmpeg. Ensure ffmpeg is installed and in PATH/bin/."
                .to_string(),
            "video_processor".to_string(),
        )),
    }
}

/// Loads previously extracted frame files as images.
pub fn load_extracted_frames(frame_paths: &[PathBuf]) -> Vec<DynamicImage> {
    let mut frames = Vec::new();
    for (index, frame_path) in frame_paths.iter().enumerate() {
        match image::open(frame_path) {
            Ok(img) => frames.push(img),
            Err(e) => error!(
                "Failed to open frame {} ({}): {}",
                index + 1,
                frame_path.display(),
                e
            ),
        }
    }
    frames
}

/// Samples representative frames from a video into in-memory images.
pub async fn sample_frames(video_path: &Path, num_frames: usize) -> Result<Vec<DynamicImage>> {
    let tmp = tempdir().map_err(|e| InferenceError::Internal(format!("Temp dir failed: {}", e)))?;
    let artifacts =
        extract_frame_artifacts(video_path, num_frames, tmp.path().to_path_buf()).await?;
    Ok(load_extracted_frames(&artifacts.frame_paths))
}
