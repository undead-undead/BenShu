use crate::audio::{pcm_decode_wav, resample_to_16k, AudioPlugin, TARGET_SAMPLE_RATE};
use crate::protocol::SensoryOutput;
use anyhow::{Context, Result};
use async_trait::async_trait;
use benshu_inference::backend::{SttBackend, TtsBackend};
use std::sync::Arc;

/// Unified Audio Plugin that can wrap ANY Inference-Factory backend.
/// Optimized for safety, observability, and protocol compliance.
pub struct UnifiedAudioPlugin {
    name: String,
    stt: Option<Arc<dyn SttBackend>>,
    tts: Option<Arc<dyn TtsBackend>>,
}

impl UnifiedAudioPlugin {
    pub fn for_stt(name: String, backend: Arc<dyn SttBackend>) -> Self {
        Self {
            name,
            stt: Some(backend),
            tts: None,
        }
    }

    pub fn for_tts(name: String, backend: Arc<dyn TtsBackend>) -> Self {
        Self {
            name,
            stt: None,
            tts: Some(backend),
        }
    }
}

#[async_trait]
impl AudioPlugin for UnifiedAudioPlugin {
    fn name(&self) -> &str {
        &self.name
    }

    async fn process(&self, audio_data: &[u8]) -> Result<SensoryOutput> {
        let stt = self
            .stt
            .as_ref()
            .context("Plugin is not configured for STT")?;

        if audio_data.is_empty() {
            anyhow::bail!("[Backend: {}] Cannot process empty audio data", self.name);
        }

        let (pcm_data, original_rate) = pcm_decode_wav(audio_data)
            .with_context(|| format!("[Backend: {}] WAV decoding failed", self.name))?;

        let pcm_16k = if original_rate != TARGET_SAMPLE_RATE {
            resample_to_16k(&pcm_data, original_rate).with_context(|| {
                format!(
                    "[Backend: {}] Audio resampling from {}Hz to 16kHz failed",
                    self.name, original_rate
                )
            })?
        } else {
            pcm_data
        };

        let text = stt
            .transcribe(&pcm_16k)
            .await
            .map_err(|e| anyhow::anyhow!("[Backend: {}] Transcription failed: {}", self.name, e))?;

        Ok(SensoryOutput::Text(text))
    }

    async fn synthesize(&self, text: &str) -> Result<Vec<u8>> {
        let tts = self
            .tts
            .as_ref()
            .context("Plugin is not configured for TTS")?;

        if text.is_empty() {
            anyhow::bail!("[Backend: {}] Cannot synthesize empty text", self.name);
        }

        let pcm_bytes = tts
            .synthesize(text)
            .await
            .map_err(|e| anyhow::anyhow!("[Backend: {}] Synthesis failed: {}", self.name, e))?;

        // 🎼 Check if the backend already returned a standard audio file (e.g., Cloud WAV/MP3)
        // We look for "RIFF....WAVE" or other headers.
        if pcm_bytes.len() >= 12 && &pcm_bytes[0..4] == b"RIFF" && &pcm_bytes[8..12] == b"WAVE" {
            return Ok(pcm_bytes);
        }

        // 🎹 Convert raw i16 PCM bytes (Piper standard) to f32 normalized for WAV encoding
        let pcm_f32: Vec<f32> = pcm_bytes
            .chunks_exact(2)
            .map(|c| {
                let s = i16::from_le_bytes([c[0], c[1]]);
                s as f32 / 32768.0
            })
            .collect();

        let wav_data = encode_pcm_to_wav(&pcm_f32, TARGET_SAMPLE_RATE)
            .context("Failed to encode PCM to WAV container")?;

        Ok(wav_data)
    }

    fn estimated_memory_usage(&self) -> u64 {
        let stt_usage = self
            .stt
            .as_ref()
            .map(|s| s.estimated_memory_usage())
            .unwrap_or(0);
        let tts_usage = self
            .tts
            .as_ref()
            .map(|t| t.estimated_memory_usage())
            .unwrap_or(0);

        if stt_usage > 0 && tts_usage > 0 {
            tracing::warn!("[Backend: {}] Plugin has both STT and TTS configured, memory arbitration may be imprecise.", self.name);
        }

        stt_usage + tts_usage
    }
}

/// Helper: Encodes f32 PCM data into a WAV container (16kHz, Mono, 32-bit Float)
fn encode_pcm_to_wav(pcm_data: &[f32], sample_rate: u32) -> Result<Vec<u8>> {
    use hound::{SampleFormat, WavSpec, WavWriter};
    use std::io::Cursor;

    let spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };

    let mut buffer = Vec::new();
    {
        let mut writer = WavWriter::new(Cursor::new(&mut buffer), spec)
            .map_err(|e| anyhow::anyhow!("Failed to create WAV writer: {}", e))?;

        for &sample in pcm_data {
            writer
                .write_sample(sample)
                .map_err(|e| anyhow::anyhow!("Failed to write sample to WAV: {}", e))?;
        }
        writer
            .finalize()
            .map_err(|e| anyhow::anyhow!("Failed to finalize WAV: {}", e))?;
    }

    Ok(buffer)
}
