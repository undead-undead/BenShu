//! ocr.rs — Universal OCR base for BenShu
//! Implements Phase 21: Modular OCR Infrastructure (Pluggable Backends)

use crate::backend::{InferenceError, OcrBackend, Result, VisionModelBackend, VisionTask};
use async_trait::async_trait;
use dashmap::DashMap;
use std::sync::Arc;
use tracing::{info, warn};

/// OCR Engine categorization for metabolic scaling
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum OcrEngineType {
    /// VLM-based (Semantic OCR, high accuracy, high resource)
    Vlm,
    /// Dedicated Neural (e.g. PaddleOCR, low resource, fast)
    Neural,
    /// External API (Cloud/SaaS, no local resource)
    External,
}

/// The universal OCR dispatcher
pub struct OcrManager {
    backends: DashMap<String, (OcrEngineType, Arc<dyn OcrBackend>)>,
    default_backend: Option<String>,
}

impl OcrManager {
    pub fn new() -> Self {
        Self {
            backends: DashMap::new(),
            default_backend: None,
        }
    }

    /// Register a new OCR engine
    pub fn register(
        &mut self,
        name: &str,
        engine_type: OcrEngineType,
        backend: Arc<dyn OcrBackend>,
    ) {
        info!(
            "📝 [OCR Master] Registering engine: {} ({:?})",
            name, engine_type
        );
        self.backends
            .insert(name.to_string(), (engine_type, backend));
        if self.default_backend.is_none() {
            self.default_backend = Some(name.to_string());
        }
    }

    /// Get a specific backend by name
    pub fn get(&self, name: &str) -> Option<Arc<dyn OcrBackend>> {
        self.backends.get(name).map(|b| b.value().1.clone())
    }

    /// Get the default backend
    pub fn default(&self) -> Result<Arc<dyn OcrBackend>> {
        let name = self
            .default_backend
            .as_ref()
            .ok_or_else(|| InferenceError::NotFound("No OCR backends registered".into()))?;
        self.get(name)
            .ok_or_else(|| InferenceError::NotFound(format!("Default OCR {} not found", name)))
    }

    /// Set default backend
    pub fn set_default(&mut self, name: &str) -> Result<()> {
        if self.backends.contains_key(name) {
            self.default_backend = Some(name.to_string());
            Ok(())
        } else {
            Err(InferenceError::NotFound(format!(
                "OCR backend {} not found",
                name
            )))
        }
    }
}

/// Specialized factory for OCR backends (Phase 21.2)
pub struct OcrFactory;

impl OcrFactory {
    /// Create an OCR backend from a path.
    pub async fn create(path: &std::path::Path) -> Result<Arc<dyn OcrBackend>> {
        // Fallback: Try to load as a VLM and wrap it
        info!(
            "🔍 [OCR Factory] Attempting to load VLM-based OCR from {:?}",
            path
        );

        // This is a simplified loader. In a real system, we'd check if the path
        // contains specialized OCR weights first.
        let vlm_backend = crate::backend::InferenceFactory::create_backend(path, None).await?;

        // Wrap as OcrBackend if it supports vision (dynamic check via downstream logic)
        Ok(Arc::new(VlmOcrWrapper::new(vlm_backend)))
    }
}

/// 🔗 L0 Adapter: Transforms any Vision-Language Model into a standard OCR Backend
pub struct VlmOcrWrapper {
    vlm: Arc<dyn crate::backend::ModelBackend>,
}

impl VlmOcrWrapper {
    pub fn new(vlm: Arc<dyn crate::backend::ModelBackend>) -> Self {
        Self { vlm }
    }
}

#[async_trait]
impl OcrBackend for VlmOcrWrapper {
    fn model_info(&self) -> String {
        format!("VLM-OCR-Adapter({})", self.vlm.model_info())
    }

    async fn recognize(&self, image: &image::DynamicImage) -> Result<String> {
        // Try to downcast or use generic generation if it's a VLM
        // Since we don't have a clean downcast here, we use the ModelBackend directly if possible
        // or assume the underlying implementation handles VisionTask::OCR.

        // For production, we'd use a dedicated VisionModelBackend trait check.
        // For now, we use a default placeholder or specific VLM prompt.
        let config = crate::backend::GenerationConfig::default();
        self.vlm
            .generate(
                "ocr_req",
                "Extract all text from this image exactly.",
                Some(vec![image.clone()]),
                config,
                Arc::new(parking_lot::RwLock::new(crate::engine::KvEngine::new(
                    Default::default(),
                ))),
            )
            .await
    }

    fn device_info(&self) -> crate::backend::DeviceType {
        self.vlm.device_info()
    }

    fn estimated_memory_usage(&self) -> u64 {
        self.vlm.estimated_memory_usage()
    }
}
