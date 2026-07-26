use crate::backend::{EmbeddingBackend, InferenceError, Result};
use async_trait::async_trait;
use candle_core::{Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config};
use std::path::Path;
use std::sync::Arc;
use tokenizers::Tokenizer;
use tracing::{info, warn};

/// 🕯️ Candle-based Local Embeddings (BERT Architecture)
pub struct BertEmbeddingBackend {
    model: BertModel,
    tokenizer: Tokenizer,
    device: Device,
    dimension: usize,
    model_id: String,
}

impl BertEmbeddingBackend {
    pub fn load<P: AsRef<Path>>(
        dir: P,
        model_id: String,
    ) -> std::result::Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let dir = dir.as_ref();
        let config_path = dir.join("config.json");
        let tokenizer_path = dir.join("tokenizer.json");
        let model_path = dir.join("model.safetensors");

        if !config_path.exists() || !tokenizer_path.exists() || !model_path.exists() {
            return Err(format!("Missing BERT model files in {:?}", dir).into());
        }

        let device = if candle_core::utils::cuda_is_available() {
            Device::new_cuda(0).unwrap_or(Device::Cpu)
        } else if candle_core::utils::metal_is_available() {
            Device::new_metal(0).unwrap_or(Device::Cpu)
        } else {
            Device::Cpu
        };

        let config: Config = serde_json::from_str(&std::fs::read_to_string(config_path)?)?;
        let tokenizer =
            Tokenizer::from_file(tokenizer_path).map_err(|e| format!("Tokenizer error: {}", e))?;

        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(&[model_path], candle_core::DType::F32, &device)?
        };
        let model = BertModel::load(vb, &config)?;

        info!(
            "🧬 [Embedding] Loaded BERT model: {} (Dim: {}, Device: {:?})",
            model_id, config.hidden_size, device
        );

        Ok(Self {
            model,
            tokenizer,
            device,
            dimension: config.hidden_size,
            model_id,
        })
    }
}

#[async_trait]
impl EmbeddingBackend for BertEmbeddingBackend {
    fn model_info(&self) -> String {
        format!("bert:{}", self.model_id)
    }

    fn dimension(&self) -> usize {
        self.dimension
    }

    fn device_info(&self) -> crate::backend::DeviceType {
        (&self.device).into()
    }

    fn estimated_memory_usage(&self) -> u64 {
        // BERT base is ~400MB
        400 * 1024 * 1024
    }

    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let tokens = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| InferenceError::InvalidInput(format!("Tokenization failed: {}", e)))?;

        let token_ids = Tensor::new(tokens.get_ids(), &self.device)?.unsqueeze(0)?;
        let token_type_ids = token_ids.zeros_like()?;

        // Mean pooling logic
        let embeddings = self
            .model
            .forward(&token_ids, &token_type_ids, None)
            .map_err(|e| InferenceError::Execution(e.to_string(), self.model_id.clone()))?;

        let (_n_batch, n_tokens, _hidden_size) = embeddings
            .dims3()
            .map_err(|e| InferenceError::Internal(e.to_string()))?;
        let mean_pooled = (embeddings.sum(1)? / (n_tokens as f64))?;

        let vec = mean_pooled
            .to_vec2::<f32>()?
            .into_iter()
            .next()
            .ok_or_else(|| InferenceError::Internal("Empty embedding result".into()))?;

        Ok(vec)
    }
}

/// 🚫 Fallback null embedder when no model is available
pub struct NullEmbeddingBackend;

#[async_trait]
impl EmbeddingBackend for NullEmbeddingBackend {
    fn model_info(&self) -> String {
        "null-embedder".to_string()
    }
    fn dimension(&self) -> usize {
        384
    }
    fn device_info(&self) -> crate::backend::DeviceType {
        crate::backend::DeviceType::Cpu
    }
    fn estimated_memory_usage(&self) -> u64 {
        0
    }

    async fn embed(&self, _text: &str) -> Result<Vec<f32>> {
        warn!("⚠️ Using NullEmbeddingBackend. Vectors will be zeros.");
        Ok(vec![0.0; 384]) // Default to MiniLM dimension
    }
}
