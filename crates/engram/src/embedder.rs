use crate::error::{EngramError, Result};
use crate::runtime_bridge::block_on_sync;
use crate::storage::Storage;
use benshu_inference::backend::{EmbeddingBackend, InferenceFactory};
use benshu_inference::diagnose_windows_native_small_model_error;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{error, info};

pub struct Embedder {
    backend: Arc<dyn EmbeddingBackend>,
    storage: Option<Arc<dyn Storage>>,
}

impl std::fmt::Debug for Embedder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Embedder")
            .field("backend", &self.backend.model_info())
            .finish()
    }
}

impl Embedder {
    pub async fn new(model_path: PathBuf, storage: Option<Arc<dyn Storage>>) -> Result<Self> {
        info!(
            "Initializing Unified Embedder for Engram from: {:?}",
            model_path
        );
        let backend = InferenceFactory::create_embedding_backend(&model_path)
            .await
            .map_err(|e| {
                let diagnosis = diagnose_windows_native_small_model_error(Some(&model_path), &e);
                EngramError::Inference(format!(
                    "{} [windows_native_outcome={} strategy={}] {}",
                    e, diagnosis.outcome, diagnosis.strategy, diagnosis.note
                ))
            })?;

        Ok(Self { backend, storage })
    }

    pub fn load(model_path: &std::path::Path, kv: Option<Arc<dyn Storage>>) -> Result<Self> {
        let path = model_path.to_path_buf();
        block_on_sync(async move { Self::new(path, kv).await })
    }

    pub fn memory_size(&self) -> usize {
        self.backend.estimated_memory_usage() as usize
    }

    pub fn is_gpu(&self) -> bool {
        self.backend.device_info().is_gpu()
    }

    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        if text.is_empty() {
            return Err(EngramError::Inference(
                "Cannot embed empty text".to_string(),
            ));
        }

        // Fast Cache Lookup
        if let Some(storage) = &self.storage {
            let hash = self.hash_text(text);
            if let Ok(Some(cached)) = storage.get_embedding_cache(&hash) {
                return Ok(cached);
            }
        }

        let vector = self.backend.embed(text).await.map_err(|e| {
            let diagnosis = diagnose_windows_native_small_model_error(None, &e);
            EngramError::Inference(format!(
                "{} [windows_native_outcome={} strategy={}] {}",
                e, diagnosis.outcome, diagnosis.strategy, diagnosis.note
            ))
        })?;

        // Update Cache
        if let Some(storage) = &self.storage {
            let hash = self.hash_text(text);
            if let Err(e) = storage.put_embedding_cache(&hash, &vector) {
                error!("Failed to update embedding cache for hash {}: {}", hash, e);
            }
        }

        Ok(vector)
    }

    pub async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        // Optimize: Check cache for all texts first
        // (For simplicity in this step, we delegate to backend's batch method if possible)
        let owned_texts: Vec<String> = texts.iter().map(|s| s.to_string()).collect();

        // 🧪 Advanced: Check cache for each first
        // For production, we would want to only send non-cached items to the backend

        self.backend.embed_batch(&owned_texts).await.map_err(|e| {
            let diagnosis = diagnose_windows_native_small_model_error(None, &e);
            EngramError::Inference(format!(
                "{} [windows_native_outcome={} strategy={}] {}",
                e, diagnosis.outcome, diagnosis.strategy, diagnosis.note
            ))
        })
    }

    fn hash_text(&self, text: &str) -> String {
        let mut hasher = Sha256::new();
        // Identity: Include model info in hash to avoid cross-model cache pollution
        hasher.update(self.backend.model_info().as_bytes());
        hasher.update(text.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    pub fn dimension(&self) -> usize {
        self.backend.dimension()
    }
}
