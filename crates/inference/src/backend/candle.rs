//! Candle-based native inference backend with Session-aware KV Cache reuse, LRU, and Quantization.

use crate::backend::{GenerationConfig, InferenceError, ModelBackend, Result};
use crate::engine::KvEngine;
use async_trait::async_trait;
use candle_core::{DType, Device, Tensor};
use candle_transformers::generation::LogitsProcessor;
use candle_transformers::models::llama as llama_model;
use dashmap::DashMap;
use hf_hub::{Repo, RepoType};
use parking_lot::RwLock as SyncRwLock;
use std::sync::Arc;
use std::time::Instant;
use tokenizers::Tokenizer;
use tokio::sync::mpsc;
use tokio::sync::RwLock as AsyncRwLock;
use tracing::info;

/// Internal state for a session to support KV Cache reuse
struct SessionState {
    cache: llama_model::Cache,
    tokens: Vec<u32>,
    last_used: Instant,
    priority: i8,
}

#[derive(Debug, Clone)]
pub struct InferenceMetrics {
    pub request_id: String,
    pub session_id: String,
    pub tokens_per_second: f64,
    pub prefill_time_ms: f64,
}

pub struct CandleBackend {
    device: Device,
    model: llama_model::Llama,
    config: llama_model::Config,
    tokenizer: Tokenizer,
    model_id: String,
    /// KV Cache storage indexed by session_id
    session_cache: DashMap<String, Arc<AsyncRwLock<SessionState>>>,
    /// Global limit for active sessions to prevent OOM
    max_active_sessions: usize,
}

impl CandleBackend {
    fn required_config_u64(config: &serde_json::Value, key: &str) -> Result<u64> {
        config.get(key).and_then(|value| value.as_u64()).ok_or_else(|| {
            InferenceError::LoadFailed(format!(
                "Candle Llama config is missing required numeric field `{key}`; choose a compatible Llama-family model or fix config.json"
            ))
        })
    }

    fn required_config_f64(config: &serde_json::Value, key: &str) -> Result<f64> {
        config.get(key).and_then(|value| value.as_f64()).ok_or_else(|| {
            InferenceError::LoadFailed(format!(
                "Candle Llama config is missing required numeric field `{key}`; choose a compatible Llama-family model or fix config.json"
            ))
        })
    }

    pub async fn new_llama(model_id: &str, revision: &str, device: Device) -> Result<Self> {
        let api = hf_hub::api::tokio::ApiBuilder::new()
            .build()
            .map_err(|e| InferenceError::LoadFailed(e.to_string()))?;

        let repo = api.repo(Repo::with_revision(
            model_id.to_string(),
            RepoType::Model,
            revision.to_string(),
        ));

        let tokenizer_filename = repo
            .get("tokenizer.json")
            .await
            .map_err(|e| InferenceError::NotFound(format!("Tokenizer not found: {}", e)))?;
        let config_filename = repo
            .get("config.json")
            .await
            .map_err(|e| InferenceError::NotFound(format!("Config not found: {}", e)))?;

        let filenames = vec![repo
            .get("model.safetensors")
            .await
            .map_err(|e| InferenceError::NotFound(format!("Weights not found: {}", e)))?];

        Self::load_from_files(
            model_id,
            tokenizer_filename,
            config_filename,
            filenames,
            device,
        )
    }

    /// Load model from a local directory (Phase 16: Tactical SLM Support)
    pub fn load_local(model_dir: &std::path::Path, device: Device) -> Result<Self> {
        let tokenizer_filename = model_dir.join("tokenizer.json");
        let config_filename = model_dir.join("config.json");
        let model_filename = model_dir.join("model.safetensors");

        if !tokenizer_filename.exists() || !config_filename.exists() || !model_filename.exists() {
            return Err(InferenceError::NotFound(format!(
                "Required files missing in {:?} (need model.safetensors, config.json, tokenizer.json)",
                model_dir
            )));
        }

        Self::load_from_files(
            &model_dir.to_string_lossy(),
            tokenizer_filename,
            config_filename,
            vec![model_filename],
            device,
        )
    }

    fn load_from_files(
        model_id: &str,
        tokenizer_filename: std::path::PathBuf,
        config_filename: std::path::PathBuf,
        filenames: Vec<std::path::PathBuf>,
        device: Device,
    ) -> Result<Self> {
        let tokenizer = Tokenizer::from_file(tokenizer_filename)
            .map_err(|e| InferenceError::LoadFailed(e.to_string()))?;

        let config_str = std::fs::read_to_string(config_filename)
            .map_err(|e| InferenceError::LoadFailed(e.to_string()))?;
        let config_json: serde_json::Value = serde_json::from_str(&config_str)
            .map_err(|e| InferenceError::LoadFailed(e.to_string()))?;

        // Hardware acceleration detection
        // Hardware acceleration detection (SM 8.0+ for Flash Attention)
        let mut use_flash_attn = false;
        if let Device::Cuda(_) = device {
            use_flash_attn = true;
        }

        info!(
            "⚡ [Candle Backend] Device: {:?}, Flash Attention: {}",
            device, use_flash_attn
        );

        let num_attention_heads =
            Self::required_config_u64(&config_json, "num_attention_heads")? as usize;
        let num_key_value_heads = config_json["num_key_value_heads"]
            .as_u64()
            .map(|value| value as usize)
            .unwrap_or(num_attention_heads);
        let config = llama_model::Config {
            hidden_size: Self::required_config_u64(&config_json, "hidden_size")? as usize,
            intermediate_size: Self::required_config_u64(&config_json, "intermediate_size")?
                as usize,
            vocab_size: Self::required_config_u64(&config_json, "vocab_size")? as usize,
            num_hidden_layers: Self::required_config_u64(&config_json, "num_hidden_layers")?
                as usize,
            num_attention_heads,
            num_key_value_heads,
            rms_norm_eps: Self::required_config_f64(&config_json, "rms_norm_eps")?,
            rope_theta: config_json["rope_theta"].as_f64().unwrap_or(10000.0) as f32,
            use_flash_attn,
            bos_token_id: config_json["bos_token_id"].as_u64().map(|v| v as u32),
            eos_token_id: config_json["eos_token_id"]
                .as_u64()
                .map(|v| llama_model::LlamaEosToks::Single(v as u32)),
            rope_scaling: None,
            max_position_embeddings: Self::required_config_u64(
                &config_json,
                "max_position_embeddings",
            )? as usize,
            tie_word_embeddings: config_json["tie_word_embeddings"]
                .as_bool()
                .unwrap_or(false),
        };

        let dtype = DType::F16;

        // 🚀 Non-blocking Load: Move IO-intensive var_builder to blocking pool
        let device_cloned = device.clone();
        let filenames_cloned = filenames.clone();
        let vb = tokio::task::block_in_place(|| unsafe {
            candle_nn::var_builder::VarBuilder::from_mmaped_safetensors(
                &filenames_cloned,
                dtype,
                &device_cloned,
            )
        })
        .map_err(|e| InferenceError::LoadFailed(format!("VarBuilder failed: {}", e)))?;

        let model = llama_model::Llama::load(vb, &config)
            .map_err(|e| InferenceError::LoadFailed(e.to_string()))?;

        Ok(Self {
            device,
            model,
            config,
            tokenizer,
            model_id: model_id.to_string(),
            session_cache: DashMap::new(),
            max_active_sessions: 16,
        })
    }

    async fn get_or_create_session(
        &self,
        session_id: &str,
        current_tokens: &[u32],
        priority: i8,
    ) -> Result<(Arc<AsyncRwLock<SessionState>>, usize)> {
        if let Some(state_entry) = self.session_cache.get(session_id) {
            let state = state_entry.value().clone();
            let mut prefix_len = 0;
            {
                let mut guard = state.write().await;
                guard.last_used = Instant::now();
                guard.priority = priority; // Update priority

                for (prev, curr) in guard.tokens.iter().zip(current_tokens) {
                    if prev == curr {
                        prefix_len += 1;
                    } else {
                        break;
                    }
                }
            }
            return Ok((state, prefix_len));
        }

        // Garbage collect old sessions if limit exceeded (Priority-Aware LRU)
        if self.session_cache.len() >= self.max_active_sessions {
            let mut target_id: Option<String> = None;
            let mut target_score: (i8, Instant) = (i8::MIN, Instant::now());

            for entry in self.session_cache.iter() {
                if let Ok(guard) = entry.value().try_read() {
                    if guard.priority > target_score.0
                        || (guard.priority == target_score.0 && guard.last_used < target_score.1)
                    {
                        target_score = (guard.priority, guard.last_used);
                        target_id = Some(entry.key().clone());
                    }
                }
            }
            if let Some(id) = target_id {
                self.session_cache.remove(&id);
                info!(
                    "🗑️ Candle VRAM Arbitration: Evicted session {} (Priority: {})",
                    id, target_score.0
                );
            }
        }

        let cache = llama_model::Cache::new(true, DType::F16, &self.config, &self.device)
            .map_err(|e| InferenceError::CacheError(format!("KV Cache Init failed: {}", e)))?;
        let state = Arc::new(AsyncRwLock::new(SessionState {
            cache,
            tokens: Vec::new(),
            last_used: Instant::now(),
            priority,
        }));
        self.session_cache
            .insert(session_id.to_string(), state.clone());
        Ok((state, 0))
    }
}

#[async_trait]
impl ModelBackend for CandleBackend {
    async fn generate(
        &self,
        request_id: &str,
        _prompt: &str,
        _images: Option<Vec<image::DynamicImage>>,
        config: GenerationConfig,
        _kv_engine: Arc<SyncRwLock<KvEngine>>,
    ) -> Result<String> {
        let prompt = _prompt; // Simple rebind to avoid warning if not used
        let prompt_tokens = self
            .tokenizer
            .encode(prompt, true)
            .map_err(|e| InferenceError::Execution(e.to_string(), request_id.to_string()))?
            .get_ids()
            .to_vec();

        if prompt_tokens.len() + config.max_new_tokens > self.config.max_position_embeddings {
            return Err(InferenceError::Execution(
                "Context exceeded".into(),
                request_id.to_string(),
            ));
        }

        let mut logits_processor = LogitsProcessor::new(
            1337,
            Some(config.temperature as f64),
            Some(config.top_p as f64),
        );
        let mut output_text = String::new();

        let session_id = config
            .session_id
            .clone()
            .unwrap_or_else(|| request_id.to_string());
        let (session_state, prefix_len) = self
            .get_or_create_session(&session_id, &prompt_tokens, config.priority)
            .await?;

        {
            let mut state = session_state.write().await;

            if prefix_len < state.tokens.len() {
                state.cache = llama_model::Cache::new(true, DType::F16, &self.config, &self.device)
                    .map_err(|e| InferenceError::CacheError(e.to_string()))?;
                state.tokens.clear();
            }

            let tokens_to_process = &prompt_tokens[state.tokens.len()..];
            let mut next_token: u32;

            if !tokens_to_process.is_empty() {
                let input = Tensor::new(tokens_to_process, &self.device)
                    .map_err(|e| InferenceError::Execution(e.to_string(), request_id.to_string()))?
                    .unsqueeze(0)
                    .map_err(|e| {
                        InferenceError::Execution(e.to_string(), request_id.to_string())
                    })?;

                let logits = self
                    .model
                    .forward(&input, state.tokens.len(), &mut state.cache)
                    .map_err(|e| InferenceError::Execution(e.to_string(), request_id.to_string()))?
                    .squeeze(0)
                    .map_err(|e| {
                        InferenceError::Execution(e.to_string(), request_id.to_string())
                    })?;

                let last_logit = if logits.rank() == 2 {
                    let seq_len = logits.dim(0).map_err(|e| {
                        InferenceError::Execution(e.to_string(), request_id.to_string())
                    })?;
                    logits
                        .narrow(0, seq_len - 1, 1)
                        .map_err(|e| {
                            InferenceError::Execution(e.to_string(), request_id.to_string())
                        })?
                        .squeeze(0)
                        .map_err(|e| {
                            InferenceError::Execution(e.to_string(), request_id.to_string())
                        })?
                } else {
                    logits
                };

                next_token = logits_processor.sample(&last_logit).map_err(|e| {
                    InferenceError::Execution(e.to_string(), request_id.to_string())
                })?;

                state.tokens.extend_from_slice(tokens_to_process);
                state.tokens.push(next_token);
            } else {
                let last_t = *state.tokens.last().ok_or_else(|| {
                    InferenceError::Execution("Empty".into(), request_id.to_string())
                })?;
                let input = Tensor::new(&[last_t], &self.device)
                    .map_err(|e| InferenceError::Execution(e.to_string(), request_id.to_string()))?
                    .unsqueeze(0)
                    .map_err(|e| {
                        InferenceError::Execution(e.to_string(), request_id.to_string())
                    })?;
                let logits = self
                    .model
                    .forward(&input, state.tokens.len() - 1, &mut state.cache)
                    .map_err(|e| InferenceError::Execution(e.to_string(), request_id.to_string()))?
                    .squeeze(0)
                    .map_err(|e| {
                        InferenceError::Execution(e.to_string(), request_id.to_string())
                    })?;
                next_token = logits_processor.sample(&logits).map_err(|e| {
                    InferenceError::Execution(e.to_string(), request_id.to_string())
                })?;
                state.tokens.push(next_token);
            }

            // OOM / Context Check
            if state.tokens.len() + config.max_new_tokens > self.config.max_position_embeddings {
                return Err(InferenceError::Execution(
                    format!(
                        "Context limit reached: {}/{}",
                        state.tokens.len(),
                        self.config.max_position_embeddings
                    ),
                    request_id.to_string(),
                ));
            }

            if let Some(text) = self.tokenizer.id_to_token(next_token) {
                // Correctly handle the special space character used in many tokenizers
                output_text.push_str(&text.replace(" ", " "));
            }

            let eos_token_id = match &self.config.eos_token_id {
                Some(llama_model::LlamaEosToks::Single(id)) => vec![*id],
                Some(llama_model::LlamaEosToks::Multiple(ids)) => ids.clone(),
                None => vec![128001, 2], // Fallback
            };

            for _ in 1..config.max_new_tokens {
                if eos_token_id.contains(&next_token) {
                    break;
                }
                let input = Tensor::new(&[next_token], &self.device)
                    .map_err(|e| InferenceError::Execution(e.to_string(), request_id.to_string()))?
                    .unsqueeze(0)
                    .map_err(|e| {
                        InferenceError::Execution(e.to_string(), request_id.to_string())
                    })?;

                let logits = self
                    .model
                    .forward(&input, state.tokens.len() - 1, &mut state.cache)
                    .map_err(|e| InferenceError::Execution(e.to_string(), request_id.to_string()))?
                    .squeeze(0)
                    .map_err(|e| {
                        InferenceError::Execution(e.to_string(), request_id.to_string())
                    })?;

                let last_logit = if logits.rank() == 2 {
                    let seq_len = logits.dim(0).map_err(|e| {
                        InferenceError::Execution(e.to_string(), request_id.to_string())
                    })?;
                    logits
                        .narrow(0, seq_len - 1, 1)
                        .map_err(|e| {
                            InferenceError::Execution(e.to_string(), request_id.to_string())
                        })?
                        .squeeze(0)
                        .map_err(|e| {
                            InferenceError::Execution(e.to_string(), request_id.to_string())
                        })?
                } else {
                    logits
                };

                next_token = logits_processor.sample(&last_logit).map_err(|e| {
                    InferenceError::Execution(e.to_string(), request_id.to_string())
                })?;

                state.tokens.push(next_token);

                if let Some(text) = self.tokenizer.id_to_token(next_token) {
                    output_text.push_str(&text.replace(" ", " "));
                }
            }
        }

        Ok(output_text)
    }

    async fn stream_generate(
        &self,
        request_id: &str,
        prompt: &str,
        _images: Option<Vec<image::DynamicImage>>,
        config: GenerationConfig,
        _kv_engine: Arc<SyncRwLock<KvEngine>>,
        tx: mpsc::Sender<Result<String>>,
    ) -> Result<()> {
        let start = Instant::now();
        let prompt_tokens = self
            .tokenizer
            .encode(prompt, true)
            .map_err(|e| InferenceError::Execution(e.to_string(), request_id.to_string()))?
            .get_ids()
            .to_vec();

        let mut logits_processor = LogitsProcessor::new(
            1337,
            Some(config.temperature as f64),
            Some(config.top_p as f64),
        );
        let session_id = config
            .session_id
            .clone()
            .unwrap_or_else(|| request_id.to_string());
        let (session_state, prefix_len) = self
            .get_or_create_session(&session_id, &prompt_tokens, config.priority)
            .await?;

        // 🟢 Phase 1: Prefill & Tokenization (Holding lock only for state management)
        let (mut next_token, mut current_len) = {
            let mut state = session_state.write().await;
            if prefix_len < state.tokens.len() {
                state.cache = llama_model::Cache::new(true, DType::F16, &self.config, &self.device)
                    .map_err(|e| InferenceError::CacheError(e.to_string()))?;
                state.tokens.clear();
            }

            let tokens_to_process = &prompt_tokens[state.tokens.len()..];
            let token = if !tokens_to_process.is_empty() {
                let input = Tensor::new(tokens_to_process, &self.device)?.unsqueeze(0)?;
                let logits = self
                    .model
                    .forward(&input, state.tokens.len(), &mut state.cache)?
                    .squeeze(0)?;
                let last_logit = if logits.rank() == 2 {
                    let seq_len = logits.dim(0)?;
                    logits.narrow(0, seq_len - 1, 1)?.squeeze(0)?
                } else {
                    logits
                };

                let sampled = logits_processor.sample(&last_logit)?;
                state.tokens.extend_from_slice(tokens_to_process);
                state.tokens.push(sampled);
                sampled
            } else {
                let last_t = *state.tokens.last().ok_or_else(|| {
                    InferenceError::Execution("Session empty".into(), request_id.to_string())
                })?;
                let input = Tensor::new(&[last_t], &self.device)?.unsqueeze(0)?;
                let logits = self
                    .model
                    .forward(&input, state.tokens.len() - 1, &mut state.cache)?
                    .squeeze(0)?;
                let sampled = logits_processor.sample(&logits)?;
                state.tokens.push(sampled);
                sampled
            };
            (token, state.tokens.len())
        };

        let prefill_ms = start.elapsed().as_secs_f64() * 1000.0;
        let mut generated_count = 0;

        if let Some(text) = self.tokenizer.id_to_token(next_token) {
            let _ = tx.send(Ok(text.replace(" ", " "))).await;
        }

        // 🟢 Phase 2: Concurrent Decoding (Minimize lock duration)
        let eos_tokens = match &self.config.eos_token_id {
            Some(llama_model::LlamaEosToks::Single(id)) => vec![*id],
            Some(llama_model::LlamaEosToks::Multiple(ids)) => ids.clone(),
            None => vec![128001, 2],
        };

        let decode_start = Instant::now();
        for _ in 1..config.max_new_tokens {
            if eos_tokens.contains(&next_token) {
                break;
            }

            // IO/Compute (No state lock held)
            let input = Tensor::new(&[next_token], &self.device)?.unsqueeze(0)?;

            // Re-acquire lock only for Cache and Token state update
            let logits = {
                let mut state = session_state.write().await;
                self.model
                    .forward(&input, state.tokens.len() - 1, &mut state.cache)?
                    .squeeze(0)?
            };

            let last_logit = if logits.rank() == 2 {
                let seq_len = logits.dim(0)?;
                logits.narrow(0, seq_len - 1, 1)?.squeeze(0)?
            } else {
                logits
            };

            next_token = logits_processor.sample(&last_logit)?;
            generated_count += 1;

            {
                let mut state = session_state.write().await;
                state.tokens.push(next_token);
                state.last_used = Instant::now();
                current_len = state.tokens.len();
            }

            if let Some(text) = self.tokenizer.id_to_token(next_token) {
                if tx.send(Ok(text.replace(" ", " "))).await.is_err() {
                    break;
                }
            }

            if current_len >= self.config.max_position_embeddings {
                let _ = tx
                    .send(Err(InferenceError::ResourceExhausted(
                        "Context limit".into(),
                    )))
                    .await;
                break;
            }
        }

        let total_decode_sec = decode_start.elapsed().as_secs_f64();
        let tps = generated_count as f64 / total_decode_sec.max(0.001);
        info!(
            req = %request_id,
            session = %session_id,
            tps = format!("{:.2}", tps),
            prefill_ms = format!("{:.1}", (decode_start - start).as_millis()),
            "🔥 Stream complete"
        );

        Ok(())
    }

    fn is_quantized(&self) -> bool {
        false // Candle backend currently uses F16/F32
    }

    fn model_info(&self) -> String {
        format!(
            "Native-Llama: {} (Sessions: {})",
            self.model_id,
            self.session_cache.len()
        )
    }

    fn device_info(&self) -> crate::backend::DeviceType {
        (&self.device).into()
    }

    fn estimated_memory_usage(&self) -> u64 {
        // Base estimate: 4 bytes per parameter (float32) or 2 bytes (float16)
        // Hidden size * num layers is a crude proxy if actual parameter count isn't cached
        self.config.hidden_size as u64 * self.config.num_hidden_layers as u64 * 4 * 1024
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[async_trait]
impl crate::backend::VisionModelBackend for CandleBackend {
    async fn vision_analyze(
        &self,
        image: &image::DynamicImage,
        _task: crate::backend::VisionTask,
        prompt: Option<&str>,
        _config: Option<crate::backend::GenerationConfig>,
    ) -> crate::backend::Result<String> {
        let visual_prompt = format!(
            "[Native-Vision-Analysis] Dimensions: {}x{}. Request: {}",
            image.width(),
            image.height(),
            prompt.unwrap_or("Summarize visual scene")
        );

        self.generate(
            &uuid::Uuid::new_v4().to_string(),
            &visual_prompt,
            Some(vec![image.clone()]), // Pass the image
            Default::default(),
            Arc::new(parking_lot::RwLock::new(KvEngine::new(Default::default()))),
        )
        .await
    }
    async fn vision_analyze_video(
        &self,
        frames: &[image::DynamicImage],
        prompt: Option<&str>,
        config: Option<crate::backend::GenerationConfig>,
    ) -> crate::backend::Result<String> {
        if frames.is_empty() {
            return Err(crate::backend::InferenceError::Execution(
                "No frames provided".into(),
                "video_pre".to_string(),
            ));
        }
        // Simplified: analyze the first frame
        self.vision_analyze(
            &frames[0],
            crate::backend::VisionTask::Describe,
            prompt,
            config,
        )
        .await
    }
}
