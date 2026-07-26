use crate::backend::{InferenceError, RerankBackend, Result};
use async_trait::async_trait;
use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::xlm_roberta::{Config, XLMRobertaForSequenceClassification};
use std::path::Path;
use std::sync::Arc;
use tokenizers::Tokenizer;
use tracing::{info, warn};

/// 🎯 Candle-based Local Reranker (Cross-Encoders)
pub struct CandleRerankBackend {
    model: XLMRobertaForSequenceClassification,
    tokenizer: Tokenizer,
    device: Device,
    model_id: String,
}

impl CandleRerankBackend {
    pub fn load<P: AsRef<Path>>(
        dir: P,
        model_id: String,
    ) -> std::result::Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let dir = dir.as_ref();
        let config_path = dir.join("config.json");
        let tokenizer_path = dir.join("tokenizer.json");
        let model_path = dir.join("model.safetensors");

        if !config_path.exists() || !tokenizer_path.exists() || !model_path.exists() {
            return Err(format!("Missing Rerank model files in {:?}", dir).into());
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

        let vb =
            unsafe { VarBuilder::from_mmaped_safetensors(&[model_path], DType::F32, &device)? };
        // Cross-Encoders for reranking usually have 1 output label
        let model = XLMRobertaForSequenceClassification::new(1, &config, vb)?;

        info!(
            "🎯 [Rerank] Loaded Cross-Encoder: {} (Device: {:?})",
            model_id, device
        );

        Ok(Self {
            model,
            tokenizer,
            device,
            model_id,
        })
    }
}

#[async_trait]
impl RerankBackend for CandleRerankBackend {
    fn model_info(&self) -> String {
        format!("reranker:{}", self.model_id)
    }

    fn device_info(&self) -> crate::backend::DeviceType {
        (&self.device).into()
    }

    fn estimated_memory_usage(&self) -> u64 {
        // XLMRoberta-base is ~500MB
        500 * 1024 * 1024
    }

    async fn rerank(&self, query: &str, documents: &[String]) -> Result<Vec<f32>> {
        if documents.is_empty() {
            return Ok(Vec::new());
        }

        // Preparation for batch inference
        let mut scores = Vec::with_capacity(documents.len());

        for doc in documents {
            // Standard cross-encoder input: [CLS] Query [SEP] Document [SEP]
            let tokens = self
                .tokenizer
                .encode((query, doc.as_str()), true)
                .map_err(|e| InferenceError::InvalidInput(format!("Tokenization failed: {}", e)))?;

            let token_ids = Tensor::new(tokens.get_ids(), &self.device)?.unsqueeze(0)?;
            let attention_mask =
                Tensor::new(tokens.get_attention_mask(), &self.device)?.unsqueeze(0)?;
            let token_type_ids = token_ids.zeros_like()?;
            let logits = self
                .model
                .forward(&token_ids, &attention_mask, &token_type_ids)
                .map_err(|e| InferenceError::Execution(e.to_string(), self.model_id.clone()))?;

            let logit_val = logits.to_vec2::<f32>()?[0][0];
            // Sigmoid for normalized [0, 1] score
            let score = 1.0 / (1.0 + (-logit_val).exp());
            scores.push(score);
        }

        Ok(scores)
    }
}

/// 🚫 Fallback null reranker (Returns flat scores)
pub struct NullRerankBackend;

#[async_trait]
impl RerankBackend for NullRerankBackend {
    fn model_info(&self) -> String {
        "null-reranker".to_string()
    }
    fn device_info(&self) -> crate::backend::DeviceType {
        crate::backend::DeviceType::Cpu
    }
    fn estimated_memory_usage(&self) -> u64 {
        0
    }

    async fn rerank(&self, _query: &str, documents: &[String]) -> Result<Vec<f32>> {
        warn!("⚠️ Using NullRerankBackend. Scores will be uniform.");
        Ok(vec![1.0; documents.len()])
    }
}
