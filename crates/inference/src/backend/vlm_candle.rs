//! vlm_candle.rs — Modular Vision-Language Model Backend using Candle
//! Supports LLaVA-style architectures with vision encoders and LLM decoders.

use crate::backend::clip::CLIPVisionModel;
use crate::backend::projector::VisionProjector;
use crate::backend::{
    GenerationConfig, InferenceError, ModelBackend, Result, VisionModelBackend, VisionTask,
};
use crate::engine::KvEngine;
use async_trait::async_trait;
use candle_core::{DType, Device, Tensor};
use candle_transformers::generation::LogitsProcessor;
use candle_transformers::models::llama as llama_model;
use dashmap::DashMap;
use llama_model::Cache;
use llama_model::Config as LlamaConfig;
use llama_model::Llama;
use llama_model::LlamaEosToks;
use parking_lot::RwLock as SyncRwLock;
use std::sync::Arc;
use std::time::Instant;
use tokenizers::Tokenizer;
use tokio::sync::{mpsc, RwLock as AsyncRwLock};
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Session state for VLM context persistence
pub struct VlmSession {
    pub cache: Cache,
    pub tokens: Vec<u32>,
    pub last_used: Instant,
    pub priority: i8,
}

pub struct CandleVlmBackend {
    device: Device,
    dtype: DType,
    encoder: Arc<CLIPVisionModel>,
    projector: Arc<dyn VisionProjector>,
    llm: Llama,
    llm_config: LlamaConfig,
    tokenizer: Tokenizer,
    model_id: String,
    sessions: DashMap<String, Arc<AsyncRwLock<VlmSession>>>,
    max_active_sessions: usize,
}

impl CandleVlmBackend {
    /// Generic VLM loader
    pub fn new(
        encoder: Arc<CLIPVisionModel>,
        projector: Arc<dyn VisionProjector>,
        llm: Llama,
        llm_config: LlamaConfig,
        tokenizer: Tokenizer,
        model_id: String,
        device: Device,
    ) -> Self {
        let dtype = if device.is_cpu() {
            DType::F32
        } else {
            DType::F16
        };
        Self {
            device,
            dtype,
            encoder,
            projector,
            llm,
            llm_config,
            tokenizer,
            model_id,
            sessions: DashMap::new(),
            max_active_sessions: 4,
        }
    }

    /// Load VLM from a local directory (Phase 21.7)
    /// Expects: model.safetensors (LLM), vision_encoder.safetensors (CLIP), mmproj.safetensors (Projector), config.json, tokenizer.json
    pub fn load_local(model_dir: &std::path::Path, device: Device) -> Result<Self> {
        let tokenizer_filename = model_dir.join("tokenizer.json");
        let config_filename = model_dir.join("config.json");
        let model_filename = model_dir.join("model.safetensors");
        let mmproj_filename = model_dir.join("mmproj.safetensors");
        let vision_encoder_filename = model_dir.join("vision_encoder.safetensors");

        if !tokenizer_filename.exists()
            || !config_filename.exists()
            || !model_filename.exists()
            || !mmproj_filename.exists()
        {
            return Err(InferenceError::NotFound(format!(
                "Required VLM files missing in {:?}",
                model_dir
            )));
        }

        let dtype = if device.is_cpu() {
            DType::F32
        } else {
            DType::F16
        };

        let tokenizer = Tokenizer::from_file(&tokenizer_filename)
            .map_err(|e| InferenceError::LoadFailed(format!("Tokenizer error: {}", e)))?;

        let config_str = std::fs::read_to_string(&config_filename)
            .map_err(|e| InferenceError::LoadFailed(format!("Config read error: {}", e)))?;
        let llm_config_ser: LlamaConfigSerializable = serde_json::from_str(&config_str)
            .map_err(|e| InferenceError::LoadFailed(format!("Config parse error: {}", e)))?;
        let llm_config: LlamaConfig = llm_config_ser.into();

        // 1. Load LLM
        let vb = unsafe {
            candle_nn::var_builder::VarBuilder::from_mmaped_safetensors(
                &[model_filename],
                dtype,
                &device,
            )
        }
        .map_err(|e| InferenceError::LoadFailed(format!("LLM weight load error: {}", e)))?;
        let llm = Llama::load(vb, &llm_config).map_err(|e| {
            InferenceError::LoadFailed(format!("LLM architecture load error: {}", e))
        })?;

        // 2. Load Projector
        let projector = crate::backend::projector::MLPProjector::load(
            &mmproj_filename,
            &device,
            crate::backend::projector::ProjectorType::LlavaV15,
        )
        .map_err(|e| InferenceError::LoadFailed(format!("Projector load error: {}", e)))?;

        // 3. Load Vision Encoder
        let clip_variant = crate::backend::clip::CLIPVariant::ViTL14_336; // Default for LLaVA 1.5
        let clip_vb = unsafe {
            candle_nn::var_builder::VarBuilder::from_mmaped_safetensors(
                &[vision_encoder_filename],
                dtype,
                &device,
            )
        }
        .map_err(|e| InferenceError::LoadFailed(format!("CLIP weight load error: {}", e)))?;
        let encoder = CLIPVisionModel::new(clip_vb, &clip_variant.config()).map_err(|e| {
            InferenceError::LoadFailed(format!("CLIP architecture load error: {}", e))
        })?;

        Ok(Self::new(
            Arc::new(encoder),
            Arc::new(projector),
            llm,
            llm_config,
            tokenizer,
            model_dir.to_string_lossy().to_string(),
            device,
        ))
    }

    async fn evict_lowest_priority_session(&self) {
        let mut target_key = None;
        let mut lowest_score = (i8::MIN, Instant::now()); // (Priority DESC, Time ASC)

        for entry in self.sessions.iter() {
            let session = entry.value().read().await;
            let score = (session.priority, session.last_used);

            if score.0 > lowest_score.0 || (score.0 == lowest_score.0 && score.1 < lowest_score.1) {
                lowest_score = score;
                target_key = Some(entry.key().clone());
            }
        }

        if let Some(key) = target_key {
            info!(
                "♻️ [VLM] Capacity reached. Evicting LRU/Low-priority session: {}",
                key
            );
            self.sessions.remove(&key);
        }
    }

    async fn get_or_create_session(
        &self,
        session_id: &str,
        priority: i8,
    ) -> Result<Arc<AsyncRwLock<VlmSession>>> {
        if let Some(session) = self.sessions.get(session_id) {
            let mut s = session.write().await;
            s.last_used = Instant::now();
            return Ok(session.clone());
        }

        // VRAM Arbitration
        let hw = crate::hardware::HardwareStatus::detect();
        if hw.vram_total_mb > 0 {
            let usage_pct = (hw.vram_used_mb as f64 / hw.vram_total_mb as f64) * 100.0;
            if usage_pct > 85.0 {
                info!(
                    "⚠️ High VRAM usage detected: {:.1}%, evicting lowest priority session",
                    usage_pct
                );
                self.evict_lowest_priority_session().await;
            }
        }

        // Capacity check
        if self.sessions.len() >= self.max_active_sessions {
            self.evict_lowest_priority_session().await;
        }

        let cache = Cache::new(true, self.dtype, &self.llm_config, &self.device).map_err(|e| {
            InferenceError::CacheError(format!("LLM Cache initialization failed: {}", e))
        })?;

        let session = Arc::new(AsyncRwLock::new(VlmSession {
            cache,
            tokens: Vec::new(),
            last_used: Instant::now(),
            priority,
        }));

        self.sessions
            .insert(session_id.to_string(), session.clone());
        Ok(session)
    }

    async fn vision_analyze_internal(
        &self,
        images: Option<&[image::DynamicImage]>,
        prompt: &str,
        config: GenerationConfig,
    ) -> Result<String> {
        let session_id = config
            .session_id
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let session_lock = self
            .get_or_create_session(&session_id, config.priority)
            .await?;
        let mut session = session_lock.write().await;
        session.priority = config.priority;

        // Reset if standalone
        if config.session_id.is_none() {
            session.tokens.clear();
            session.cache = Cache::new(true, self.dtype, &self.llm_config, &self.device)
                .map_err(|e| InferenceError::CacheError(format!("Cache reset failed: {}", e)))?;
        }

        // 🛡️ KV Cache Guard: Reset if state is logically inconsistent
        if session.tokens.len() > self.llm_config.max_position_embeddings {
            warn!("⚠️ [VLM] Context exceeded for {}. Resetting.", session_id);
            session.cache = Cache::new(true, self.dtype, &self.llm_config, &self.device)
                .map_err(|e| InferenceError::CacheError(e.to_string()))?;
            session.tokens.clear();
        }

        let mut input_embeds = if let Some(imgs) = images {
            let mut embeds_list = Vec::new();
            for img in imgs {
                let features = self.encoder.extract_features(&img).map_err(|e| {
                    InferenceError::Execution(
                        format!("CLIP feature extraction failed: {}", e),
                        session_id.clone(),
                    )
                })?;

                let visual_embeds = self.projector.project(&features).map_err(|e| {
                    InferenceError::Execution(
                        format!("Vision projection failed: {}", e),
                        session_id.clone(),
                    )
                })?;

                // Ensure visual embeds has batch dimension (1, seq, dim)
                let visual_embeds = if visual_embeds.dims().len() == 2 {
                    visual_embeds.unsqueeze(0)?
                } else {
                    visual_embeds
                };
                embeds_list.push(visual_embeds);
            }

            let tokens = self.tokenizer.encode(prompt, true).map_err(|e| {
                InferenceError::Execution(format!("Tokenizer error: {}", e), session_id.clone())
            })?;
            let text_embeds = self
                .llm
                .embed(&Tensor::new(tokens.get_ids(), &self.device)?.unsqueeze(0)?)?;

            embeds_list.push(text_embeds);
            Tensor::cat(&embeds_list, 1)?
        } else {
            let tokens = self.tokenizer.encode(prompt, true).map_err(|e| {
                InferenceError::Execution(format!("Tokenizer error: {}", e), session_id.clone())
            })?;
            self.llm
                .embed(&Tensor::new(tokens.get_ids(), &self.device)?.unsqueeze(0)?)?
        };

        let mut logits_processor = LogitsProcessor::new(
            config.seed as u64,
            Some(config.temperature as f64),
            Some(config.top_p as f64),
        );
        let mut output_text = String::new();
        // Dynamic EOS detection (Qwen/Llama/Intern)
        let eos_tokens = match &self.llm_config.eos_token_id {
            Some(LlamaEosToks::Single(id)) => vec![*id],
            Some(LlamaEosToks::Multiple(ids)) => ids.clone(),
            None => vec![2],
        };

        for _ in 0..config.max_new_tokens {
            let pos = session.tokens.len();
            let logits = self
                .llm
                .forward(&input_embeds, pos, &mut session.cache)
                .map_err(|e| {
                    InferenceError::Execution(
                        format!("LLM prediction failed: {}", e),
                        session_id.clone(),
                    )
                })?
                .squeeze(0)?;

            let last_logit = logits.narrow(0, logits.dim(0)? - 1, 1)?.squeeze(0)?;
            let next_token = logits_processor.sample(&last_logit)?;

            session.tokens.push(next_token);
            if eos_tokens.contains(&next_token) {
                break;
            }

            if let Some(t) = self.tokenizer.id_to_token(next_token) {
                let token_str = if t.starts_with(' ') {
                    t.replace(' ', " ")
                } else if t == "<0x20>" {
                    " ".to_string()
                } else {
                    t.to_string()
                };
                output_text.push_str(&token_str);
            }

            input_embeds = self
                .llm
                .embed(&Tensor::new(&[next_token], &self.device)?.unsqueeze(0)?)?;
        }

        Ok(output_text)
    }
}

#[async_trait]
impl ModelBackend for CandleVlmBackend {
    fn is_quantized(&self) -> bool {
        self.model_id.contains("q4_")
            || self.model_id.contains("q5_")
            || self.model_id.contains("gguf")
    }

    async fn generate(
        &self,
        _rid: &str,
        prompt: &str,
        images: Option<Vec<image::DynamicImage>>,
        config: GenerationConfig,
        _kv: Arc<SyncRwLock<KvEngine>>,
    ) -> Result<String> {
        self.vision_analyze_internal(images.as_deref(), prompt, config)
            .await
    }

    async fn stream_generate(
        &self,
        _rid: &str,
        prompt: &str,
        images: Option<Vec<image::DynamicImage>>,
        config: GenerationConfig,
        _kv: Arc<SyncRwLock<KvEngine>>,
        tx: mpsc::Sender<Result<String>>,
    ) -> Result<()> {
        let session_id = config
            .session_id
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let session_lock = self
            .get_or_create_session(&session_id, config.priority)
            .await?;
        let mut session = session_lock.write().await;
        session.priority = config.priority;

        // Reset if standalone
        if config.session_id.is_none() {
            session.tokens.clear();
            session.cache = Cache::new(true, self.dtype, &self.llm_config, &self.device)
                .map_err(|e| InferenceError::CacheError(format!("Cache reset failed: {}", e)))?;
        }

        // 🛡️ KV Cache Guard (Streaming path)
        if session.tokens.len() > self.llm_config.max_position_embeddings {
            warn!(
                "⚠️ [VLM-Stream] Context exceeded for {}. Reseting.",
                session_id
            );
            session.cache = Cache::new(true, self.dtype, &self.llm_config, &self.device)
                .map_err(|e| InferenceError::CacheError("Memory state mismatch".into()))?;
            session.tokens.clear();
        }

        let mut input_embeds = if let Some(imgs) = images {
            let mut embeds_list = Vec::new();
            for img in imgs {
                let features = self.encoder.extract_features(&img).map_err(|e| {
                    InferenceError::Execution(
                        format!("CLIP extraction failed: {}", e),
                        session_id.clone(),
                    )
                })?;

                let visual_embeds = self.projector.project(&features).map_err(|e| {
                    InferenceError::Execution(
                        format!("Vision projection failed: {}", e),
                        session_id.clone(),
                    )
                })?;

                let visual_embeds = if visual_embeds.dims().len() == 2 {
                    visual_embeds.unsqueeze(0)?
                } else {
                    visual_embeds
                };
                embeds_list.push(visual_embeds);
            }

            let tokens = self.tokenizer.encode(prompt, true).map_err(|e| {
                InferenceError::Execution(format!("Tokenizer error: {}", e), session_id.clone())
            })?;
            let text_embeds = self
                .llm
                .embed(&Tensor::new(tokens.get_ids(), &self.device)?.unsqueeze(0)?)?;

            embeds_list.push(text_embeds);
            Tensor::cat(&embeds_list, 1)?
        } else {
            let tokens = self.tokenizer.encode(prompt, true).map_err(|e| {
                InferenceError::Execution(format!("Tokenizer error: {}", e), session_id.clone())
            })?;
            self.llm
                .embed(&Tensor::new(tokens.get_ids(), &self.device)?.unsqueeze(0)?)?
        };

        let mut logits_processor = LogitsProcessor::new(
            config.seed as u64,
            Some(config.temperature as f64),
            Some(config.top_p as f64),
        );
        let eos_tokens = match &self.llm_config.eos_token_id {
            Some(LlamaEosToks::Single(id)) => vec![*id],
            Some(LlamaEosToks::Multiple(ids)) => ids.clone(),
            None => vec![2],
        };

        for _ in 0..config.max_new_tokens {
            let pos = session.tokens.len();
            let logits = self
                .llm
                .forward(&input_embeds, pos, &mut session.cache)
                .map_err(|e| {
                    let err = InferenceError::Execution(
                        format!("LLM prediction failed: {}", e),
                        session_id.clone(),
                    );
                    let _ = tx.try_send(Err(err.clone()));
                    err
                })?
                .squeeze(0)?;

            let last_logit = logits.narrow(0, logits.dim(0)? - 1, 1)?.squeeze(0)?;
            let next_token = logits_processor.sample(&last_logit)?;

            session.tokens.push(next_token);
            if eos_tokens.contains(&next_token) {
                break;
            }

            if let Some(t) = self.tokenizer.id_to_token(next_token) {
                let token_str = if t.starts_with(' ') {
                    t.replace(' ', " ")
                } else if t == "<0x20>" {
                    " ".to_string()
                } else {
                    t.to_string()
                };

                if tx.send(Ok(token_str)).await.is_err() {
                    break;
                }
            }

            input_embeds = self
                .llm
                .embed(&Tensor::new(&[next_token], &self.device)?.unsqueeze(0)?)?;
        }

        Ok(())
    }

    fn model_info(&self) -> String {
        format!("VLM-Candle: {}", self.model_id)
    }

    fn device_info(&self) -> crate::backend::DeviceType {
        (&self.device).into()
    }

    fn estimated_memory_usage(&self) -> u64 {
        // Estimate based on LLM and Vision Encoder (crude)
        let llm_vram = self.llm_config.hidden_size as u64
            * self.llm_config.num_hidden_layers as u64
            * 4
            * 1024;
        let vision_vram = 512 * 1024 * 1024; // Static ~512MB for CLIP-style encoder
        llm_vram + vision_vram
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[async_trait]
impl VisionModelBackend for CandleVlmBackend {
    async fn vision_analyze(
        &self,
        image: &image::DynamicImage,
        task: VisionTask,
        prompt: Option<&str>,
        config: Option<GenerationConfig>,
    ) -> Result<String> {
        let p = prompt.unwrap_or(match task {
            VisionTask::Describe => "Describe this image in detail.",
            VisionTask::OCR => "Extract all text from this image.",
            VisionTask::Grounding => "Identify and locate objects in this image.",
        });

        self.vision_analyze_internal(Some(&[image.clone()]), p, config.unwrap_or_default())
            .await
    }

    async fn vision_analyze_video(
        &self,
        frames: &[image::DynamicImage],
        prompt: Option<&str>,
        config: Option<GenerationConfig>,
    ) -> Result<String> {
        self.vision_analyze_internal(
            Some(frames),
            prompt.unwrap_or("Summarize this video sequence."),
            config.unwrap_or_default(),
        )
        .await
    }
}
#[derive(Debug, Clone, serde::Deserialize)]
struct LlamaConfigSerializable {
    pub n_layer: usize,
    pub n_head: usize,
    pub n_embd: usize,
    pub n_inner: Option<usize>,
    pub vocab_size: usize,
    pub n_kv_head: Option<usize>,
    pub rope_theta: Option<f32>,
    pub bos_token_id: Option<u32>,
    pub eos_token_id: Option<u32>,
    pub max_position_embeddings: Option<usize>,
}

impl From<LlamaConfigSerializable> for LlamaConfig {
    fn from(s: LlamaConfigSerializable) -> Self {
        Self {
            num_hidden_layers: s.n_layer,
            num_attention_heads: s.n_head,
            hidden_size: s.n_embd,
            intermediate_size: s.n_inner.unwrap_or(s.n_embd * 4),
            vocab_size: s.vocab_size,
            num_key_value_heads: s.n_kv_head.unwrap_or(s.n_head),
            rope_theta: s.rope_theta.unwrap_or(10000.0),
            bos_token_id: s.bos_token_id,
            eos_token_id: Some(
                s.eos_token_id
                    .map(LlamaEosToks::Single)
                    .unwrap_or(LlamaEosToks::Single(2)),
            ),
            max_position_embeddings: s.max_position_embeddings.unwrap_or(2048),
            rms_norm_eps: 1e-5,
            rope_scaling: None,
            tie_word_embeddings: false,
            use_flash_attn: false,
        }
    }
}
