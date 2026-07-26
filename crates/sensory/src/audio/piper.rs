use crate::audio::AudioPlugin;
use crate::protocol::SensoryOutput;
use anyhow::{Context, Result};
use async_trait::async_trait;
use benshu_inference::backend::TtsBackend;
use std::sync::Arc;

/// Unified Text-to-Speech (TTS) Plugin for the Sensory Hub.
///
/// This plugin provides a standardized interface for converting text into audio bytes
/// using various backends like Piper, ChatTTS, or cloud services.
pub struct UnifiedTTS {
    backend: Arc<dyn TtsBackend>,
    name: String,
}

impl UnifiedTTS {
    /// Creates a new TTS plugin with a specific backend.
    pub fn new(name: String, backend: Arc<dyn TtsBackend>) -> Self {
        Self { name, backend }
    }
}

// Keep legacy PiperTTS for compatibility with existing registration points
pub type PiperTTS = UnifiedTTS;

#[async_trait]
impl AudioPlugin for UnifiedTTS {
    fn name(&self) -> &str {
        &self.name
    }

    async fn process(&self, _audio_data: &[u8]) -> Result<SensoryOutput> {
        anyhow::bail!(
            "TTS plugin '{}' does not support processing audio data. Use 'synthesize' instead.",
            self.name
        )
    }

    async fn synthesize(&self, text: &str) -> Result<Vec<u8>> {
        if text.trim().is_empty() {
            anyhow::bail!("Cannot synthesize empty text");
        }

        self.backend.synthesize(text).await.with_context(|| {
            format!(
                "[Backend: {}] Speech synthesis failed for text: '{}'",
                self.name, text
            )
        })
    }

    fn estimated_memory_usage(&self) -> u64 {
        self.backend.estimated_memory_usage()
    }
}
