//! Runtime lifecycle contracts shared by local model backends.
//!
//! These types describe the product-facing lifecycle.  Concrete backends still
//! decide how to load weights, talk to a bridge, or call Windows-native APIs.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ModelRuntimeKind {
    LargeLanguageModel,
    MultimodalLanguageModel,
    Embedding,
    Rerank,
    Ocr,
    SpeechToText,
    TextToSpeech,
    ImageGeneration,
    ImageEdit,
    AudioUnderstanding,
    RealtimeVad,
    DuplexVoice,
    Classifier,
    Router,
    SafetyChecker,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ModelRuntimeState {
    Configured,
    Loading,
    Loaded,
    Unloaded,
    Reloading,
    Failed,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelRuntimeBinding {
    pub id: String,
    pub role: String,
    pub kind: ModelRuntimeKind,
    pub runtime_family: String,
    pub model_path: Option<String>,
    pub companion_path: Option<String>,
    pub provider_base_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelRuntimeStatus {
    pub binding: ModelRuntimeBinding,
    pub state: ModelRuntimeState,
    pub readiness: String,
    pub diagnostics: Vec<String>,
    pub estimated_vram_mb: Option<u64>,
    pub estimated_ram_mb: Option<u64>,
    pub loaded_vram_mb: Option<u64>,
    pub loaded_ram_mb: Option<u64>,
}

impl ModelRuntimeStatus {
    pub fn configured(binding: ModelRuntimeBinding) -> Self {
        Self {
            binding,
            state: ModelRuntimeState::Configured,
            readiness: "configured".to_string(),
            diagnostics: Vec::new(),
            estimated_vram_mb: None,
            estimated_ram_mb: None,
            loaded_vram_mb: None,
            loaded_ram_mb: None,
        }
    }

    pub fn failed(binding: ModelRuntimeBinding, reason: impl Into<String>) -> Self {
        Self {
            binding,
            state: ModelRuntimeState::Failed,
            readiness: "failed".to_string(),
            diagnostics: vec![reason.into()],
            estimated_vram_mb: None,
            estimated_ram_mb: None,
            loaded_vram_mb: None,
            loaded_ram_mb: None,
        }
    }
}

#[async_trait]
pub trait ModelRuntimeManager: Send + Sync {
    async fn load(&self, binding: &ModelRuntimeBinding) -> anyhow::Result<ModelRuntimeStatus>;
    async fn unload(&self, binding_id: &str) -> anyhow::Result<ModelRuntimeStatus>;
    async fn reload(&self, binding: &ModelRuntimeBinding) -> anyhow::Result<ModelRuntimeStatus> {
        let _ = self.unload(&binding.id).await;
        self.load(binding).await
    }
    async fn status(&self, binding_id: &str) -> anyhow::Result<ModelRuntimeStatus>;
    async fn diagnose(&self, binding: &ModelRuntimeBinding) -> anyhow::Result<ModelRuntimeStatus>;
    async fn warmup(&self, binding_id: &str) -> anyhow::Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_status_keeps_user_readable_reason() {
        let binding = ModelRuntimeBinding {
            id: "main".to_string(),
            role: "benshu".to_string(),
            kind: ModelRuntimeKind::LargeLanguageModel,
            runtime_family: "llama_cpp".to_string(),
            model_path: None,
            companion_path: None,
            provider_base_url: Some("http://127.0.0.1:8012/v1".to_string()),
        };

        let status = ModelRuntimeStatus::failed(binding, "model file missing");
        assert_eq!(status.state, ModelRuntimeState::Failed);
        assert_eq!(status.diagnostics, vec!["model file missing"]);
    }
}
