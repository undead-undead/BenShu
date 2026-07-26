//! External/Binary-based Audio Backends (Piper TTS).

use crate::backend::{AudioModelBackend, InferenceError, Result, TtsBackend};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::time::timeout;
use tracing::{debug, info, warn};

/// Piper-based local TTS Backend.
/// Uses the pre-compiled `piper` binary to synthesize speech at high speed.
pub struct PiperBackend {
    model_path: PathBuf,
    piper_bin: PathBuf,
    model_id: String,
}

impl PiperBackend {
    /// Creates a new Piper backend.
    /// Requires the directory containing the `piper` binary and the `.onnx` model.
    pub fn new<P: AsRef<Path>>(dir: P, model_id: String) -> Result<Self> {
        let dir = dir.as_ref();
        let model_path = dir.join("model.onnx");
        let mut piper_bin = dir.join("piper");

        if !piper_bin.exists() && cfg!(windows) {
            piper_bin = dir.join("piper.exe");
        }

        if !piper_bin.exists() {
            return Err(InferenceError::NotFound(format!(
                "[{}] Piper binary not found at {:?}. Please ensure it is installed correctly.",
                model_id, piper_bin
            )));
        }
        if !model_path.exists() {
            return Err(InferenceError::NotFound(format!(
                "[{}] Piper model not found at {:?}. Expected an .onnx file.",
                model_id, model_path
            )));
        }

        Ok(Self {
            model_path,
            piper_bin,
            model_id,
        })
    }
}

#[async_trait]
impl AudioModelBackend for PiperBackend {
    fn model_info(&self) -> String {
        format!("Piper-TTS: {}", self.model_id)
    }

    fn estimated_memory_usage(&self) -> u64 {
        std::fs::metadata(&self.model_path)
            .map(|m| m.len())
            .unwrap_or(128 * 1024 * 1024)
    }
}

#[async_trait]
impl TtsBackend for PiperBackend {
    async fn synthesize(&self, text: &str) -> Result<Vec<u8>> {
        if text.trim().is_empty() || text.chars().all(|c| c.is_whitespace()) {
            return Err(InferenceError::Execution(
                "Synthesis text is empty".into(),
                self.model_id.clone(),
            ));
        }

        let start_time = std::time::Instant::now();
        info!(
            "🎙️ [{}] Starting Piper synthesis. Text length: {} chars",
            self.model_id,
            text.len()
        );

        let mut cmd = Command::new(&self.piper_bin);
        // Ensure child is killed if the future is dropped (e.g. timeout)
        cmd.kill_on_drop(true);

        let mut child = cmd
            .arg("--model")
            .arg(&self.model_path)
            .arg("--output_raw")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                InferenceError::Execution(
                    format!("[{}] Failed to spawn Piper: {}", self.model_id, e),
                    self.model_id.clone(),
                )
            })?;

        if let Some(mut stdin) = child.stdin.take() {
            if let Err(e) = stdin.write_all(text.as_bytes()).await {
                return Err(InferenceError::Execution(
                    format!("[{}] Failed to write to Piper stdin: {}", self.model_id, e),
                    self.model_id.clone(),
                ));
            }
            let _ = stdin.flush().await;
            drop(stdin); // Signal EOF
        }

        // Wait with Timeout. If timeout occurs, 'child' is dropped inside the future,
        // and because 'kill_on_drop(true)' was set, the process is killed.
        let wait_result = timeout(Duration::from_secs(15), child.wait_with_output()).await;

        let output = match wait_result {
            Ok(Ok(o)) => o,
            Ok(Err(e)) => {
                return Err(InferenceError::Execution(
                    format!("[{}] Piper execution failed: {}", self.model_id, e),
                    self.model_id.clone(),
                ))
            }
            Err(_) => {
                return Err(InferenceError::Timeout(
                    format!("[{}] Piper synthesis timed out after 15s", self.model_id),
                    self.model_id.clone(),
                ));
            }
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(InferenceError::Execution(
                format!(
                    "[{}] Piper error (status: {}): {}",
                    self.model_id, output.status, stderr
                ),
                self.model_id.clone(),
            ));
        }

        if output.stdout.is_empty() {
            return Err(InferenceError::Execution(
                format!("[{}] Piper returned empty audio", self.model_id),
                self.model_id.clone(),
            ));
        }

        debug!(
            "✅ [{}] Synthesis complete. Audio: {} bytes, Duration: {:?}",
            self.model_id,
            output.stdout.len(),
            start_time.elapsed()
        );

        Ok(output.stdout)
    }
}
