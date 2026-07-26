use crate::audio::{pcm_decode_wav, resample_to_16k, AudioPlugin, TARGET_SAMPLE_RATE};
use crate::protocol::SensoryOutput;
use anyhow::{Context, Result};
use async_trait::async_trait;
use benshu_inference::backend::SttBackend;
use std::sync::Arc;

/// Unified Speech-to-Text (STT) Plugin for the Sensory Hub.
///
/// This plugin acts as a bridge between the raw audio input and various inference backends.
/// It handles audio decoding, resampling to 16kHz, and mono-conversion before pushing
/// data to the specialized `SttBackend` implementations.
pub struct UnifiedSTT {
    backend: Arc<dyn SttBackend>,
    name: String,
}

impl UnifiedSTT {
    /// Creates a new STT plugin with a specific backend (e.g., Whisper, SenseVoice).
    pub fn new(name: String, backend: Arc<dyn SttBackend>) -> Self {
        Self { name, backend }
    }

    /// Helper to initialize Whisper filters if using WhisperCandleBackend.
    ///
    /// Note: This is a specialized initialization step for Candle-based Whisper models
    /// that require external Mel filters.
    pub async fn with_whisper_filters(self) -> Self {
        let filters_bytes = include_bytes!("../../assets/audio/melfilters.bytes");
        let mut filters = vec![0f32; filters_bytes.len() / 4];
        <byteorder::LittleEndian as byteorder::ByteOrder>::read_f32_into(
            filters_bytes,
            &mut filters,
        );

        // This is a placeholder for model-specific initialization logic
        self
    }
}

// Keep legacy WhisperSTT for compatibility with existing registration points
pub type WhisperSTT = UnifiedSTT;

#[async_trait]
impl AudioPlugin for UnifiedSTT {
    fn name(&self) -> &str {
        &self.name
    }

    async fn load(&self) -> Result<()> {
        // Backends implement their own lazy-loading or pre-loading strategy
        Ok(())
    }

    async fn process(&self, audio_data: &[u8]) -> Result<SensoryOutput> {
        if audio_data.is_empty() {
            anyhow::bail!("Cannot process empty audio data");
        }

        let (pcm_data, original_rate) =
            pcm_decode_wav(audio_data).context("WAV decoding failed")?;

        let pcm_16k = if original_rate != TARGET_SAMPLE_RATE {
            resample_to_16k(&pcm_data, original_rate).context("Audio resampling to 16kHz failed")?
        } else {
            pcm_data
        };

        let text =
            self.backend.transcribe(&pcm_16k).await.map_err(|e| {
                anyhow::anyhow!("[Backend: {}] Transcription failed: {}", self.name, e)
            })?;

        Ok(SensoryOutput::Text(text))
    }

    fn estimated_memory_usage(&self) -> u64 {
        self.backend.estimated_memory_usage()
    }
}
