pub mod coref;

use self::coref::CoreferenceResolver;
use async_trait::async_trait;
use benshu_infra::traits::nlu::{DialogueContext, MetabolicMode, NluEngine, NluIntent, NluResult};
use benshu_infra::traits::{HealthStatus, ResourceSensor, ThrottleLevel};
use parking_lot::RwLock;
use std::sync::Arc;
use tracing::{debug, warn};

/// Unified NLU Cluster that manages intelligent metabolic switching.
/// Automatically promotes/demotes precision based on system-wide resource pressure.
pub struct NluCluster {
    /// Local Optimal Backend (GPU FP16/32)
    optimal_backend: Option<Arc<dyn NluEngine>>,
    /// Local Cold Backend (CPU INT4 Accelerated via kernels.rs)
    cold_backend: Option<Arc<dyn NluEngine>>,
    /// Fallback to LLM-based NLU (Cloud/Local LLM)
    llm_backend: Option<Arc<dyn crate::backend::ModelBackend>>,
    /// Multi-modal sensory/resource sensor (Thread-safe wrapper)
    sensor: Arc<RwLock<dyn ResourceSensor>>,
    /// Heuristic coreference resolver
    resolver: CoreferenceResolver,
}

impl NluCluster {
    pub fn new(
        optimal_backend: Option<Arc<dyn NluEngine>>,
        cold_backend: Option<Arc<dyn NluEngine>>,
        llm_backend: Option<Arc<dyn crate::backend::ModelBackend>>,
        sensor: Arc<RwLock<dyn ResourceSensor>>,
    ) -> Self {
        Self {
            optimal_backend,
            cold_backend,
            llm_backend,
            sensor,
            resolver: CoreferenceResolver::new(),
        }
    }

    /// Determines the best metabolic mode based on real-time hardware pressure
    fn select_mode(&self) -> MetabolicMode {
        // We use write lock briefly to refresh system metrics
        let level = self.sensor.write().suggest_throttle_level(None);
        match level {
            ThrottleLevel::High => MetabolicMode::Optimal,
            ThrottleLevel::Medium => MetabolicMode::Efficient,
            ThrottleLevel::Low => MetabolicMode::Cold,
        }
    }
}

#[async_trait]
impl NluEngine for NluCluster {
    async fn analyze(&self, text: &str) -> NluResult {
        let mode = self.select_mode();
        debug!("NLU Metabolic Switch: Current mode is {:?}", mode);

        let mut res = match mode {
            MetabolicMode::Optimal | MetabolicMode::Efficient => {
                if let Some(backend) = &self.optimal_backend {
                    let mut res = backend.analyze(text).await;
                    res.mode = mode;
                    if res.intent.confidence > 0.6 {
                        return res;
                    }
                }
                self.analyze_cold_or_survival(text).await
            }
            MetabolicMode::Cold | MetabolicMode::Survival => {
                let mut res = self.analyze_cold(text).await;
                // If it's survival (cloud/heuristic), the analyze_cold will handle the fallback
                res.mode = mode;
                res
            }
        };
        res.mode = mode;
        res
    }

    async fn analyze_with_context(&self, text: &str, context: &DialogueContext) -> NluResult {
        let mut res = self.analyze(text).await;

        // Apply Coreference Resolution
        let refs = self.resolver.resolve(text, context);
        res.references = refs;

        res
    }

    fn model_info(&self) -> String {
        let mode = self.select_mode();
        match mode {
            MetabolicMode::Optimal => self
                .optimal_backend
                .as_ref()
                .map(|b| b.model_info())
                .unwrap_or_else(|| "none".into()),
            MetabolicMode::Cold => self
                .cold_backend
                .as_ref()
                .map(|b| b.model_info())
                .unwrap_or_else(|| "cpu:int4".into()),
            _ => "fallback".into(),
        }
    }

    fn status(&self) -> HealthStatus {
        let mode = self.select_mode();
        match mode {
            MetabolicMode::Optimal => HealthStatus::Healthy,
            MetabolicMode::Efficient => {
                HealthStatus::Degraded("High load, using efficient mode".into())
            }
            MetabolicMode::Cold | MetabolicMode::Survival => {
                HealthStatus::Degraded("Memory Pressure, switched to INT4/Fallback".into())
            }
        }
    }
}

impl NluCluster {
    async fn analyze_cold(&self, text: &str) -> NluResult {
        if let Some(cold) = &self.cold_backend {
            return cold.analyze(text).await;
        }
        self.analyze_survival(text).await
    }

    async fn analyze_cold_or_survival(&self, text: &str) -> NluResult {
        if let Some(cold) = &self.cold_backend {
            let res = cold.analyze(text).await;
            if res.intent.confidence > 0.5 {
                return res;
            }
        }
        self.analyze_survival(text).await
    }

    async fn analyze_survival(&self, text: &str) -> NluResult {
        if let Some(llm) = &self.llm_backend {
            if let Ok(res) = self.analyze_with_llm(text, llm.as_ref()).await {
                return res;
            }
        }
        self.heuristic_analyze(text)
    }

    async fn analyze_with_llm(
        &self,
        text: &str,
        llm: &dyn crate::backend::ModelBackend,
    ) -> anyhow::Result<NluResult> {
        let prompt = format!(
            r#"Identify intent and extract slots. Query: "{}"
Respond ONLY JSON: {{ "intent": {{ "name": "...", "confidence": 0.95 }}, "slots": [], "metadata": {{}} }}"#,
            text
        );
        let response = llm
            .generate(
                "nlu_fallback",
                &prompt,
                None,
                Default::default(),
                Arc::new(parking_lot::RwLock::new(crate::engine::KvEngine::new(
                    Default::default(),
                ))),
            )
            .await?;

        // Remove markdown block if model was too helpful
        let cleaned = response
            .trim_start_matches("```json")
            .trim_end_matches("```")
            .trim();
        let mut res: NluResult = serde_json::from_str(cleaned)?;
        res.mode = MetabolicMode::Survival;
        Ok(res)
    }

    fn heuristic_analyze(&self, text: &str) -> NluResult {
        warn!("⚠️ Survival Heuristic NLU active for: {}", text);
        NluResult {
            intent: NluIntent {
                name: "unknown".to_string(),
                confidence: 0.1,
            },
            slots: vec![],
            references: vec![],
            mode: MetabolicMode::Survival,
            metadata: serde_json::json!({}),
        }
    }
}

pub struct NullNluEngine;
#[async_trait]
impl NluEngine for NullNluEngine {
    async fn analyze(&self, _text: &str) -> NluResult {
        NluResult {
            intent: NluIntent {
                name: "unknown".into(),
                confidence: 0.0,
            },
            slots: vec![],
            references: vec![],
            mode: MetabolicMode::Survival,
            metadata: serde_json::json!({}),
        }
    }
    fn model_info(&self) -> String {
        "none".into()
    }
    fn status(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}
