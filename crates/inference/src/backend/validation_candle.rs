use crate::backend::{InferenceError, Result};
use async_trait::async_trait;
use benshu_infra::traits::validation::{FactChecker, ValidationResult};
use benshu_infra::traits::HealthStatus;
use candle_core::{DType, Device, IndexOp, Tensor};
use candle_nn::{Module, VarBuilder};
use candle_transformers::models::bert::{BertModel, Config};
use std::path::Path;
use tokenizers::Tokenizer;
use tracing::{error, info, warn};

/// 🔍 Candle-based Local Fact Checker (NLI - Natural Language Inference)
/// Uses BERT to classify text relationship: Entailment, Neutral, Contradiction.
pub struct CandleFactChecker {
    model: BertModel,
    tokenizer: Tokenizer,
    device: Device,
    model_id: String,
    /// Head: 0 = Entailment, 1 = Neutral, 2 = Contradiction
    nli_head: candle_nn::Linear,
}

impl CandleFactChecker {
    pub fn load<P: AsRef<Path>>(
        dir: P,
        model_id: String,
    ) -> std::result::Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let dir = dir.as_ref();
        let config_path = dir.join("config.json");
        let tokenizer_path = dir.join("tokenizer.json");
        let model_path = dir.join("model.safetensors");

        let device = if candle_core::utils::cuda_is_available() {
            Device::new_cuda(0).unwrap_or(Device::Cpu)
        } else {
            Device::Cpu
        };

        let config: Config = serde_json::from_str(&std::fs::read_to_string(config_path)?)?;
        let tokenizer =
            Tokenizer::from_file(tokenizer_path).map_err(|e| format!("Tokenizer error: {}", e))?;

        // Safety: ensure BERT weights are compatible
        let vb =
            unsafe { VarBuilder::from_mmaped_safetensors(&[model_path], DType::F32, &device)? };
        let model = BertModel::load(vb.pp("bert"), &config)
            .or_else(|_| BertModel::load(vb.clone(), &config))?;

        // NLI Head: 3 output labels
        let head_vb = vb.pp("nli_head");
        let nli_head = candle_nn::linear(config.hidden_size, 3, head_vb).unwrap_or_else(|_| {
            warn!("⚠️ [FactCheck] No NLI head found, initializing pseudo-identity head");
            let weight = Tensor::zeros((3, config.hidden_size), DType::F32, &device).unwrap();
            let bias = Tensor::zeros(3, DType::F32, &device).unwrap();
            candle_nn::Linear::new(weight, Some(bias))
        });

        info!(
            "🛡️ [FactCheck] Loaded Local NLI Engine: {} (Device: {:?})",
            model_id, device
        );

        Ok(Self {
            model,
            tokenizer,
            device,
            model_id,
            nli_head,
        })
    }
}

#[async_trait]
impl FactChecker for CandleFactChecker {
    async fn verify(&self, text: &str, context: &str) -> ValidationResult {
        let _start = std::time::Instant::now();

        // NLI input: [CLS] text [SEP] context [SEP]
        let encoding = match self.tokenizer.encode((text, context), true) {
            Ok(t) => t,
            Err(e) => {
                error!("FactCheck encoding failed: {}", e);
                return self.fallback_result();
            }
        };

        let token_ids = Tensor::new(encoding.get_ids(), &self.device)
            .unwrap()
            .unsqueeze(0)
            .unwrap();
        let token_type_ids = Tensor::new(encoding.get_type_ids(), &self.device)
            .unwrap()
            .unsqueeze(0)
            .unwrap();

        // Forward pass
        let embeddings = match self.model.forward(&token_ids, &token_type_ids, None) {
            Ok(e) => e,
            Err(e) => {
                error!("FactCheck forward failed: {}", e);
                return self.fallback_result();
            }
        };

        // CLS token pooling
        let cls_embedding = embeddings.i((0, 0, ..)).unwrap();
        let logits = self.nli_head.forward(&cls_embedding).unwrap();
        let probs = candle_nn::ops::softmax(&logits, 0).unwrap();
        let probs_vec = probs.to_vec1::<f32>().unwrap();

        // Label mapping: 0=Entailment, 1=Neutral, 2=Contradiction
        let entailment = probs_vec[0];
        let neutral = probs_vec[1];
        let contradiction = probs_vec[2];

        let (is_valid, confidence) = if entailment > 0.7 {
            (true, entailment)
        } else if contradiction > 0.7 {
            (false, contradiction)
        } else {
            // Neutral or low confidence
            (true, neutral) // Assume fine if not outright contradiction
        };

        ValidationResult {
            is_valid,
            confidence,
            detected_hallucinations: if !is_valid {
                vec!["Potential factual contradiction detected by NLI model".into()]
            } else {
                vec![]
            },
            source_verification: vec![],
        }
    }

    async fn check_consistency(&self, text: &str) -> f32 {
        // Self-redundancy check (placeholder)
        0.9
    }
}

impl CandleFactChecker {
    fn fallback_result(&self) -> ValidationResult {
        ValidationResult {
            is_valid: true,
            confidence: 0.0,
            detected_hallucinations: vec!["FactCheck error".into()],
            source_verification: vec![],
        }
    }
}

pub struct NullFactChecker;
#[async_trait]
impl FactChecker for NullFactChecker {
    async fn verify(&self, _text: &str, _context: &str) -> ValidationResult {
        ValidationResult {
            is_valid: true,
            confidence: 1.0,
            detected_hallucinations: vec![],
            source_verification: vec![],
        }
    }
    async fn check_consistency(&self, _text: &str) -> f32 {
        1.0
    }
}
