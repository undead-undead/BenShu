use crate::error::{EngramError, Result};
use crate::hybrid_search::HybridSearchResult;
use crate::reranker::Reranker;
use crate::runtime_bridge::block_on_sync;
use crate::storage::Storage;
use benshu_inference::backend::{InferenceFactory, RerankBackend};
use benshu_inference::diagnose_windows_native_small_model_error;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, warn};

pub struct LocalCandleReranker {
    backend: Arc<dyn RerankBackend>,
    storage: Option<Arc<dyn Storage>>,
}

impl std::fmt::Debug for LocalCandleReranker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalCandleReranker")
            .field("backend", &self.backend.model_info())
            .finish()
    }
}
impl LocalCandleReranker {
    pub async fn new(model_path: PathBuf, storage: Option<Arc<dyn Storage>>) -> Result<Self> {
        info!(
            "Initializing Unified Reranker for Engram from: {:?}",
            model_path
        );
        let backend = InferenceFactory::create_rerank_backend(&model_path)
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

    pub fn load(model_path: &std::path::Path, storage: Option<Arc<dyn Storage>>) -> Result<Self> {
        let path = model_path.to_path_buf();
        block_on_sync(async move { Self::new(path, storage).await })
    }

    pub fn memory_size(&self) -> usize {
        self.backend.estimated_memory_usage() as usize
    }

    pub fn is_gpu(&self) -> bool {
        self.backend.device_info().is_gpu()
    }
}

impl Reranker for LocalCandleReranker {
    fn rerank(
        &self,
        query: &str,
        documents: Vec<HybridSearchResult>,
    ) -> Result<Vec<HybridSearchResult>> {
        if documents.is_empty() {
            return Ok(documents);
        }

        // We use block_on or specialized async handling if the trait is sync.
        // The Engram Reranker trait is sync, so we wrap the async backend call.
        // Fetch full content for reranking if storage is available, otherwise fallback to title
        let doc_texts: Vec<String> = documents
            .iter()
            .map(|d| {
                if let Some(storage) = &self.storage {
                    storage
                        .get_content(&d.document.content_hash)
                        .ok()
                        .flatten()
                        .and_then(|b| String::from_utf8(b.to_vec()).ok())
                        .unwrap_or_else(|| d.document.title.clone())
                } else {
                    d.document.title.clone()
                }
            })
            .collect();

        // Safety: In a production RAG pipeline, we'd prefer an async Reranker trait.
        // For now, we utilize the tokio runtime if available or a simple block_on.
        let scores = block_on_sync(async { self.backend.rerank(query, &doc_texts).await })
            .map_err(|e| {
                let diagnosis = diagnose_windows_native_small_model_error(None, &e);
                EngramError::Inference(format!(
                    "{} [windows_native_outcome={} strategy={}] {}",
                    e, diagnosis.outcome, diagnosis.strategy, diagnosis.note
                ))
            })?;

        let mut results = documents;
        for (doc, score) in results.iter_mut().zip(scores) {
            doc.rrf_score = score as f64;
        }

        results.sort_by(|a, b| b.rrf_score.partial_cmp(&a.rrf_score).unwrap());
        Ok(results)
    }
}
