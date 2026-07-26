use crate::backend::{InferenceError, Result};
use std::path::{Path, PathBuf};
use tempfile::tempdir;
use tokio::process::Command;

pub struct ExtractedAudioTrackArtifact {
    pub output_path: PathBuf,
    pub sample_rate: u32,
    pub channels: u8,
}

pub struct NormalizedAudioArtifact {
    pub output_path: PathBuf,
    pub sample_rate: u32,
    pub channels: u8,
}

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

fn extension_for_media_type(media_type: &str) -> &'static str {
    match media_type.to_ascii_lowercase().as_str() {
        "audio/wav" | "audio/x-wav" | "audio/wave" => "wav",
        "audio/mpeg" | "audio/mp3" => "mp3",
        "audio/ogg" | "audio/opus" => "ogg",
        "audio/flac" => "flac",
        "audio/aac" => "aac",
        "audio/mp4" | "audio/m4a" | "audio/x-m4a" => "m4a",
        "audio/webm" => "webm",
        _ => "bin",
    }
}

async fn run_audio_ffmpeg(args: &[String], route: &str, failure_message: &str) -> Result<()> {
    let ffmpeg = find_binary("ffmpeg");
    let status = Command::new(&ffmpeg)
        .args(args)
        .status()
        .await
        .map_err(|e| {
            InferenceError::Execution(format!("ffmpeg failed: {}", e), route.to_string())
        })?;

    if !status.success() {
        return Err(InferenceError::Execution(
            failure_message.to_string(),
            route.to_string(),
        ));
    }

    Ok(())
}

pub async fn extract_audio_track_artifact(
    input: &Path,
    output_path: PathBuf,
    sample_rate: u32,
    channels: u8,
) -> Result<ExtractedAudioTrackArtifact> {
    run_audio_ffmpeg(
        &[
            "-i".to_string(),
            input.display().to_string(),
            "-vn".to_string(),
            "-acodec".to_string(),
            "pcm_s16le".to_string(),
            "-ar".to_string(),
            sample_rate.to_string(),
            "-ac".to_string(),
            channels.to_string(),
            "-y".to_string(),
            output_path.display().to_string(),
        ],
        "extract_audio_track",
        "ffmpeg failed to extract audio track",
    )
    .await?;

    Ok(ExtractedAudioTrackArtifact {
        output_path,
        sample_rate,
        channels,
    })
}

pub async fn normalize_audio_artifact(
    input: &Path,
    output_path: PathBuf,
    sample_rate: u32,
    channels: u8,
) -> Result<NormalizedAudioArtifact> {
    run_audio_ffmpeg(
        &[
            "-i".to_string(),
            input.display().to_string(),
            "-ac".to_string(),
            channels.to_string(),
            "-ar".to_string(),
            sample_rate.to_string(),
            "-c:a".to_string(),
            "pcm_s16le".to_string(),
            "-y".to_string(),
            output_path.display().to_string(),
        ],
        "normalize_audio",
        "ffmpeg failed to normalize audio",
    )
    .await?;

    Ok(NormalizedAudioArtifact {
        output_path,
        sample_rate,
        channels,
    })
}

pub async fn normalize_audio_bytes_to_pcm_f32(
    audio_bytes: &[u8],
    media_type: &str,
    sample_rate: u32,
    channels: u8,
) -> Result<Vec<f32>> {
    let temp_dir =
        tempdir().map_err(|e| InferenceError::Internal(format!("Temp dir failed: {}", e)))?;
    let input_path = temp_dir
        .path()
        .join(format!("input.{}", extension_for_media_type(media_type)));
    let output_path = temp_dir.path().join("normalized.wav");

    tokio::fs::write(&input_path, audio_bytes)
        .await
        .map_err(|e| {
            InferenceError::Execution(
                format!("Failed to write temp audio input: {}", e),
                "audio_preprocess".to_string(),
            )
        })?;

    normalize_audio_artifact(
        Path::new(&input_path),
        output_path.clone(),
        sample_rate,
        channels,
    )
    .await?;

    let wav_bytes = tokio::fs::read(&output_path).await.map_err(|e| {
        InferenceError::Execution(
            format!("Failed to read normalized audio: {}", e),
            "audio_preprocess".to_string(),
        )
    })?;

    let pcm_start = if wav_bytes.len() > 44 { 44 } else { 0 };
    let pcm = wav_bytes[pcm_start..]
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0)
        .collect::<Vec<_>>();
    Ok(pcm)
}

#[cfg(test)]
mod tests {
    use super::extension_for_media_type;

    #[test]
    fn extension_for_media_type_maps_common_audio_types() {
        assert_eq!(extension_for_media_type("audio/wav"), "wav");
        assert_eq!(extension_for_media_type("audio/mpeg"), "mp3");
        assert_eq!(extension_for_media_type("audio/ogg"), "ogg");
        assert_eq!(extension_for_media_type("audio/flac"), "flac");
        assert_eq!(extension_for_media_type("audio/mp4"), "m4a");
        assert_eq!(extension_for_media_type("application/octet-stream"), "bin");
    }
}
