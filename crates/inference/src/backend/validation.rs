use crate::backend::{InferenceError, ModelBackend, Result};
use async_trait::async_trait;
use benshu_infra::traits::validation::{FactChecker, ValidationResult};
use benshu_infra::traits::{MetabolicMode, ResourceSensor, ThrottleLevel};
use parking_lot::RwLock;
use std::sync::Arc;

/// 🧠 Fact Check Cluster (Anti-Hallucination Guard)
/// Hierarchical verification: Local NLI Model -> LLM Verification -> Heuristic
pub struct FactCheckCluster {
    local_backend: Option<Arc<dyn FactChecker>>,
    llm_backend: Option<Arc<dyn ModelBackend>>,
    sensor: Arc<RwLock<dyn ResourceSensor>>,
}

impl FactCheckCluster {
    pub fn new(
        local_backend: Option<Arc<dyn FactChecker>>,
        llm_backend: Option<Arc<dyn ModelBackend>>,
        sensor: Arc<RwLock<dyn ResourceSensor>>,
    ) -> Self {
        Self {
            local_backend,
            llm_backend,
            sensor,
        }
    }

    fn select_mode(&self) -> MetabolicMode {
        match self.sensor.write().suggest_throttle_level(None) {
            ThrottleLevel::High => MetabolicMode::Optimal,
            ThrottleLevel::Medium => MetabolicMode::Efficient,
            ThrottleLevel::Low => MetabolicMode::Cold,
        }
    }
}

#[async_trait]
impl FactChecker for FactCheckCluster {
    async fn verify(&self, text: &str, context: &str) -> ValidationResult {
        let mode = self.select_mode();

        // 1. Try Local Model (if not in Survival mode)
        if mode != MetabolicMode::Survival {
            if let Some(local) = &self.local_backend {
                let res = local.verify(text, context).await;
                if res.confidence > 0.8 {
                    return res;
                }
            }
        }

        // 2. Fallback to LLM Verification
        if let Some(llm) = &self.llm_backend {
            if let Ok(res) = self.verify_with_llm(text, context, llm.as_ref()).await {
                return res;
            }
        }

        // 3. Last Resort: Heuristic
        self.heuristic_verify(text)
    }

    async fn check_consistency(&self, text: &str) -> f32 {
        if let Some(local) = &self.local_backend {
            return local.check_consistency(text).await;
        }
        0.5 // Unknown
    }
}

impl FactCheckCluster {
    async fn verify_with_llm(
        &self,
        text: &str,
        context: &str,
        llm: &dyn ModelBackend,
    ) -> Result<ValidationResult> {
        let prompt = format!(
            "Verify the following text against context.\nContext: {}\nText: {}\nRespond ONLY JSON: {{ \"is_valid\": true/false, \"confidence\": 0.9, \"detected_hallucinations\": [] }}",
            context, text
        );
        // Using ephemeral KV engine for internal verification
        let kv = Arc::new(RwLock::new(
            crate::engine::KvEngine::new(Default::default()),
        ));
        let response = llm
            .generate("fact_check", &prompt, None, Default::default(), kv)
            .await
            .map_err(|e| InferenceError::BackendError(e.to_string()))?;

        let res: ValidationResult = serde_json::from_str(&response)
            .map_err(|e| InferenceError::FormatError(e.to_string()))?;
        Ok(res)
    }

    fn heuristic_verify(&self, _text: &str) -> ValidationResult {
        ValidationResult {
            is_valid: true, // Optimistic default
            confidence: 0.1,
            detected_hallucinations: vec!["Low confidence: heuristic fallback".into()],
            source_verification: vec![],
        }
    }
}
