//! Candle-based Audio Backends (Whisper STT).

use crate::backend::{AudioModelBackend, InferenceError, Result, SttBackend};
use async_trait::async_trait;
use candle_core::{Device, IndexOp, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::whisper::{self, audio, Config as WhisperConfig};
use std::path::{Path, PathBuf};
use tokenizers::Tokenizer;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

/// Candle-based Whisper STT Backend.
/// Implements high-performance local speech recognition using the Candle framework.
pub struct WhisperCandleBackend {
    inner: Mutex<WhisperInner>,
    model_id: String,
}

struct WhisperInner {
    model: Option<whisper::model::Whisper>,
    tokenizer: Tokenizer,
    mel_filters: Vec<f32>,
    config: WhisperConfig,
    language: Option<String>,
    device: Device,
    model_path: PathBuf,
}

impl WhisperCandleBackend {
    /// Creates a new Whisper backend instance.
    /// Performs proactive file checks to avoid runtime panics.
    pub fn new<P: AsRef<Path>>(
        dir: P,
        model_id: String,
    ) -> std::result::Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let dir = dir.as_ref();
        let config_path = dir.join("config.json");
        let tokenizer_path = dir.join("tokenizer.json");

        // Support common model naming conventions (safetensors preferred)
        let model_path = if dir.join("model.safetensors").exists() {
            dir.join("model.safetensors")
        } else if dir.join("model.bin").exists() {
            warn!("⚠️ Found model.bin instead of model.safetensors. Loading might be slower.");
            dir.join("model.bin")
        } else {
            return Err(format!(
                "No valid model weights found in {:?}. Expected model.safetensors.",
                dir
            )
            .into());
        };

        if !config_path.exists() {
            return Err(format!("config.json missing in {:?}", dir).into());
        }
        if !tokenizer_path.exists() {
            return Err(format!("tokenizer.json missing in {:?}", dir).into());
        }

        let config: WhisperConfig = serde_json::from_str(&std::fs::read_to_string(&config_path)?)?;
        let tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| format!("Failed to parse tokenizer: {}", e))?;

        // Safer Device Initialization
        let device = if candle_core::utils::cuda_is_available() {
            Device::new_cuda(0)
                .map_err(|e| {
                    warn!("CUDA initialized failed: {}. Falling back to CPU.", e);
                })
                .unwrap_or(Device::Cpu)
        } else if candle_core::utils::metal_is_available() {
            Device::new_metal(0)
                .map_err(|e| {
                    warn!("Metal initialized failed: {}. Falling back to CPU.", e);
                })
                .unwrap_or(Device::Cpu)
        } else {
            Device::Cpu
        };

        Ok(Self {
            model_id,
            inner: Mutex::new(WhisperInner {
                model: None,
                tokenizer,
                mel_filters: Vec::new(),
                config,
                language: None,
                device,
                model_path,
            }),
        })
    }

    /// Injects the Mel filters and validates their dimensions.
    pub async fn set_mel_filters(&self, filters: Vec<f32>) -> Result<()> {
        let mut inner = self.inner.lock().await;
        let expected_len = inner.config.num_mel_bins * (whisper::N_FFT / 2 + 1);
        if filters.len() != expected_len {
            return Err(InferenceError::Internal(format!(
                "Invalid Mel filter length: expected {}, got {}. (num_mel_bins={})",
                expected_len,
                filters.len(),
                inner.config.num_mel_bins
            )));
        }
        inner.mel_filters = filters;
        Ok(())
    }

    /// Set target language (e.g. "zh", "en")
    pub async fn set_language(&self, lang: Option<String>) {
        let mut inner = self.inner.lock().await;
        inner.language = lang;
    }
}

#[async_trait]
impl AudioModelBackend for WhisperCandleBackend {
    fn model_info(&self) -> String {
        format!("Whisper-Candle: {}", self.model_id)
    }

    fn estimated_memory_usage(&self) -> u64 {
        let id_lower = self.model_id.to_lowercase();
        // Dynamic memory estimation based on model size hints
        if id_lower.contains("tiny") {
            800 * 1024 * 1024
        } else if id_lower.contains("base") {
            1200 * 1024 * 1024
        } else if id_lower.contains("small") {
            2000 * 1024 * 1024
        } else if id_lower.contains("medium") {
            4000 * 1024 * 1024
        } else if id_lower.contains("large") {
            6000 * 1024 * 1024
        } else {
            1000 * 1024 * 1024
        } // Default 1GB
    }

    fn is_quantized(&self) -> bool {
        false // Whisper candle usually runs F16
    }

    fn device_info(&self) -> crate::backend::DeviceType {
        let inner = self.inner.try_lock();
        match inner {
            Ok(i) => (&i.device).into(),
            Err(_) => crate::backend::DeviceType::Cpu,
        }
    }
}

#[async_trait]
impl SttBackend for WhisperCandleBackend {
    async fn transcribe(&self, pcm_data: &[f32]) -> Result<String> {
        if pcm_data.is_empty() {
            return Err(InferenceError::Execution(
                "Received empty PCM audio data".into(),
                self.model_id.clone(),
            ));
        }

        let start_time = std::time::Instant::now();
        let mut inner = self.inner.lock().await;

        if inner.mel_filters.is_empty() {
            return Err(InferenceError::Internal(format!(
                "[{}] Mel filters not initialized.",
                self.model_id
            )));
        }

        // Lazy load weights protected by Mutex
        if inner.model.is_none() {
            info!(
                "⏳ [{}] Loading model: {:?}",
                self.model_id, inner.model_path
            );
            let vb = unsafe {
                VarBuilder::from_mmaped_safetensors(
                    &[inner.model_path.clone()],
                    whisper::DTYPE,
                    &inner.device,
                )
                .map_err(|e| {
                    InferenceError::LoadFailed(format!(
                        "Failed to mapping weights {}: {}",
                        self.model_id, e
                    ))
                })?
            };
            let model = whisper::model::Whisper::load(&vb, inner.config.clone()).map_err(|e| {
                InferenceError::LoadFailed(format!("Whisper architecture load failed: {}", e))
            })?;
            inner.model = Some(model);
        }

        // Clone state to release context borrows
        let config = inner.config.clone();
        let mel_filters = inner.mel_filters.clone();
        let device = inner.device.clone();
        let tokenizer = inner.tokenizer.clone();
        let language = inner.language.clone();
        let model = inner.model.as_mut().unwrap();

        // 1. PCM -> Mel Spectrogram
        let mel = audio::pcm_to_mel(&config, pcm_data, &mel_filters);
        let mel_len = mel.len();
        let mel_tensor = Tensor::from_vec(
            mel,
            (1, config.num_mel_bins, mel_len / config.num_mel_bins),
            &device,
        )
        .map_err(|e| {
            InferenceError::Execution(
                format!("Mel tensor creation failed: {}", e),
                self.model_id.clone(),
            )
        })?;

        // 2. Token Initialization with safety checks
        let sot_token = tokenizer.token_to_id(whisper::SOT_TOKEN).ok_or_else(|| {
            InferenceError::Internal("Special token <|startoftext|> not found in tokenizer".into())
        })?;
        let transcribe_token = tokenizer
            .token_to_id(whisper::TRANSCRIBE_TOKEN)
            .ok_or_else(|| {
                InferenceError::Internal(
                    "Special token <|transcribe|> not found in tokenizer".into(),
                )
            })?;
        let no_timestamps_token = tokenizer
            .token_to_id(whisper::NO_TIMESTAMPS_TOKEN)
            .ok_or_else(|| {
                InferenceError::Internal(
                    "Special token <|notimestamps|> not found in tokenizer".into(),
                )
            })?;
        let eot_token = tokenizer
            .token_to_id(whisper::EOT_TOKEN)
            .ok_or_else(|| InferenceError::Internal("End-of-text token not found".into()))?;

        let mut tokens = vec![sot_token];
        if let Some(lang) = language {
            if let Some(lang_token) = tokenizer.token_to_id(&format!("<|{}|>", lang)) {
                tokens.push(lang_token);
                debug!("STT using language: {}", lang);
            }
        }
        tokens.push(transcribe_token);
        tokens.push(no_timestamps_token);

        // 3. Encoder Inference
        let audio_features = model.encoder.forward(&mel_tensor, true).map_err(|e| {
            InferenceError::Execution(format!("Encoder pass failed: {}", e), self.model_id.clone())
        })?;

        let mut max_steps = (config.max_target_positions / 2).max(1);
        if max_steps > 448 {
            max_steps = 448;
        } // Safety hardcap

        // 4. Autoregressive Loop
        for i in 0..max_steps {
            let tokens_tensor = Tensor::new(tokens.as_slice(), &device)?
                .unsqueeze(0)
                .map_err(|e| InferenceError::Execution(e.to_string(), self.model_id.clone()))?;

            let ys = model
                .decoder
                .forward(&tokens_tensor, &audio_features, i == 0)
                .map_err(|e| {
                    InferenceError::Execution(
                        format!("Decoder step {} failed: {}", i, e),
                        self.model_id.clone(),
                    )
                })?;

            let (_, seq_len, _) = ys
                .dims3()
                .map_err(|e| InferenceError::Execution(e.to_string(), self.model_id.clone()))?;
            let logits =
                model
                    .decoder
                    .final_linear(&ys.i((..1, seq_len - 1..)).map_err(|e| {
                        InferenceError::Execution(e.to_string(), self.model_id.clone())
                    })?)
                    .map_err(|e| InferenceError::Execution(e.to_string(), self.model_id.clone()))?
                    .i(0)
                    .map_err(|e| InferenceError::Execution(e.to_string(), self.model_id.clone()))?
                    .i(0)
                    .map_err(|e| InferenceError::Execution(e.to_string(), self.model_id.clone()))?;

            let logits_v: Vec<f32> = logits
                .to_vec1()
                .map_err(|e| InferenceError::Execution(e.to_string(), self.model_id.clone()))?;
            let next_token = logits_v
                .iter()
                .enumerate()
                .max_by(|(_, u), (_, v)| u.total_cmp(v))
                .map(|(i, _)| i as u32)
                .unwrap();

            if next_token == eot_token {
                break;
            }
            tokens.push(next_token);
        }

        let decoded = tokenizer.decode(&tokens, true).map_err(|e| {
            InferenceError::Execution(
                format!("Tokenizer decoding failed: {}", e),
                self.model_id.clone(),
            )
        })?;

        debug!(
            "STT Transcription complete in {:?}. Generated tokens: {}",
            start_time.elapsed(),
            tokens.len()
        );
        Ok(decoded)
    }
}
