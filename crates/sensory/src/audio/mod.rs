pub mod cloud;
pub mod piper;
pub mod streaming;
pub mod streaming_buffer;
pub mod unified;
pub mod vad;
pub mod whisper;

pub use cloud::{CloudSTT, CloudTTS};
pub use piper::PiperTTS;
pub use unified::UnifiedAudioPlugin;
pub use whisper::WhisperSTT;

use crate::protocol::SensoryOutput;
use anyhow::Result;
use async_trait::async_trait;

/// Unified Audio Perception Plugin
/// Can be STT (Whisper/SenseVoice) or TTS (Piper/ChatTTS)
#[async_trait]
pub trait AudioPlugin: Send + Sync {
    /// Unique identifier for the plugin
    fn name(&self) -> &str;

    /// Process raw audio (usually returns Text for STT)
    async fn process(&self, audio_data: &[u8]) -> Result<SensoryOutput>;

    /// Process text (returns PCM bytes for TTS)
    async fn synthesize(&self, _text: &str) -> Result<Vec<u8>> {
        anyhow::bail!("Synthesize not supported by {}", self.name())
    }

    /// Resource management
    async fn load(&self) -> Result<()> {
        Ok(())
    }
    fn unload(&self) {}
    fn is_loaded(&self) -> bool {
        true
    }
    fn estimated_memory_usage(&self) -> u64;
}

pub enum AudioTask {
    Transcribe,
    Translate,
    VAD,
    Speak,
}

/// The standard sample rate required by most STT backends (Whisper, SenseVoice, etc.)
pub const TARGET_SAMPLE_RATE: u32 = 16000;

/// Helper: WAV Decoding
/// Decodes raw WAV bytes into f32 PCM data and extracts the sample rate.
/// Supports 16-bit Int and 32-bit Float formats.
pub fn pcm_decode_wav(bytes: &[u8]) -> Result<(Vec<f32>, u32)> {
    use hound::{SampleFormat, WavReader};
    use std::io::Cursor;

    if bytes.len() < 44 {
        // Minimum WAV header size
        anyhow::bail!("Invalid WAV data: Buffer too short");
    }

    let mut reader = WavReader::new(Cursor::new(bytes))
        .map_err(|e| anyhow::anyhow!("Failed to open WAV reader: {}", e))?;

    let spec = reader.spec();
    let channels = spec.channels as usize;
    let sample_rate = spec.sample_rate;

    let mut samples: Vec<f32> = match (spec.sample_format, spec.bits_per_sample) {
        (SampleFormat::Int, 16) => reader
            .samples::<i16>()
            .map(|s| s.unwrap_or(0) as f32 / 32768.0)
            .collect(),
        (SampleFormat::Float, 32) => reader.samples::<f32>().map(|s| s.unwrap_or(0.0)).collect(),
        (fmt, bits) => {
            anyhow::bail!(
                "Unsupported WAV format: {:?} {}-bit. Currently only 16-bit PCM and 32-bit Float are supported. Please convert your audio to 16/32-bit format.", 
                fmt, bits
            );
        }
    };

    // Convert to mono if necessary (averaging channels)
    if channels > 1 {
        let mut mono = Vec::with_capacity(samples.len() / channels + 1);
        for chunk in samples.chunks(channels) {
            let avg: f32 = chunk.iter().sum::<f32>() / chunk.len() as f32;
            mono.push(avg);
        }
        samples = mono;
    }

    Ok((samples, sample_rate))
}

/// Helper: Resampling
/// Resamples audio from `from_rate` to `TARGET_SAMPLE_RATE` (16kHz).
/// Uses Sinc interpolation for high fidelity at 16kHz.
pub fn resample_to_16k(input: &[f32], from_rate: u32) -> Result<Vec<f32>> {
    use rubato::{Resampler, SincFixedIn};

    if from_rate == TARGET_SAMPLE_RATE {
        return Ok(input.to_vec());
    }

    let f_ratio = TARGET_SAMPLE_RATE as f64 / from_rate as f64;

    // Valid ratio check to avoid resampler initialization panics
    if f_ratio < 0.01 || f_ratio > 100.0 {
        anyhow::bail!("Unsupported resampling ratio: {} (from {}Hz to 16kHz). Please provide audio closer to 16kHz.", f_ratio, from_rate);
    }

    // sinc_len=128 provides a healthy balance between speed (40% faster than 256) and quality for STT
    let params = rubato::SincInterpolationParameters {
        sinc_len: 128,
        f_cutoff: 0.95,
        interpolation: rubato::SincInterpolationType::Linear,
        oversampling_factor: 128,
        window: rubato::WindowFunction::BlackmanHarris2,
    };

    // Initialize resampler with exact capacity to minimize reallocations
    let mut resampler = SincFixedIn::<f32>::new(f_ratio, 2.0, params, input.len(), 1)
        .map_err(|e| anyhow::anyhow!("Failed to initialize resampler: {}", e))?;

    let input_buffer = vec![input.to_vec()];
    let resampled = resampler
        .process(&input_buffer, None)
        .map_err(|e| anyhow::anyhow!("Resampling execution failed: {}", e))?;

    if resampled.is_empty() || resampled[0].is_empty() {
        anyhow::bail!("Resampler returned empty output");
    }

    Ok(resampled[0].clone())
}
