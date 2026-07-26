use anyhow::Result;

/// Simple energy-based Voice Activity Detection
pub struct SimpleVAD {
    /// RMS threshold to consider as "active"
    threshold: f32,
    /// Number of samples to wait before declaring "silence"
    hangover_samples: usize,
    /// Counter for silent samples
    silent_count: usize,
    /// Current state
    is_active: bool,
}

impl SimpleVAD {
    pub fn new(threshold: f32, hangover_ms: u32, sample_rate: u32) -> Self {
        let hangover_samples = (hangover_ms as f32 * sample_rate as f32 / 1000.0) as usize;
        Self {
            threshold,
            hangover_samples,
            silent_count: 0,
            is_active: false,
        }
    }

    /// Default VAD for voice interaction
    pub fn voice_default() -> Self {
        // Threshold 0.01, 800ms hangover, 16kHz
        Self::new(0.01, 800, 16000)
    }

    /// Process a chunk of PCM audio and return if voice is active
    pub fn is_voice_active(&mut self, pcm: &[f32]) -> bool {
        if pcm.is_empty() {
            return self.is_active;
        }

        let rms = (pcm.iter().map(|&s| s * s).sum::<f32>() / pcm.len() as f32).sqrt();

        if rms > self.threshold {
            self.silent_count = 0;
            self.is_active = true;
        } else {
            self.silent_count += pcm.len();
            if self.silent_count >= self.hangover_samples {
                self.is_active = false;
            }
        }

        self.is_active
    }

    /// Find the first point of silence in a buffer to perform "tail-cut"
    pub fn find_silence_cutoff(&self, pcm: &[f32], chunk_size: usize) -> Option<usize> {
        let mut temp_vad = Self::new(
            self.threshold,
            self.hangover_samples as u32 * 1000 / 16000,
            16000,
        );
        for (i, chunk) in pcm.chunks(chunk_size).enumerate() {
            if !temp_vad.is_voice_active(chunk) && i > 0 {
                return Some(i * chunk_size);
            }
        }
        None
    }
}
