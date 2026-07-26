use crate::error::Result;
use crate::hybrid_search::HybridSearchResult;
use std::fmt::Debug;
use tracing::{debug, trace, warn};

/// Reranker Trait: Precision scoring for retrieved documents
///
/// This trait allows plugging in different reranking backends (e.g., Local Candle Models,
/// Cloud APIs) to improve the precision of the initial BM25/Vector retrieval.
pub trait Reranker: Send + Sync + Debug + 'static {
    /// Re-score and re-order the given documents based on the query.
    ///
    /// # Arguments
    /// - `query`: The user's search query
    /// - `documents`: The list of documents to re-rank (consumed to optimize performance)
    fn rerank(
        &self,
        query: &str,
        documents: Vec<HybridSearchResult>,
    ) -> Result<Vec<HybridSearchResult>>;

    /// Get a human-readable name for the reranker (for logging/metrics)
    fn name(&self) -> &str {
        std::any::type_name::<Self>()
    }

    /// Check if the reranker is ready to use (e.g., model loaded, API authenticated)
    fn is_ready(&self) -> bool {
        true
    }

    /// Get estimated latency in milliseconds for reranking N documents
    fn estimated_latency_ms(&self, _num_documents: usize) -> u64 {
        0
    }
}

/// A No-Op Reranker that performs no operations.
///
/// Used as a safe fallback when no model is available, ensuring the system
/// degrades gracefully.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoOpReranker;

impl NoOpReranker {
    pub fn new() -> Self {
        trace!("Initializing NoOpReranker (fallback mode)");
        Self
    }
}

impl Reranker for NoOpReranker {
    fn rerank(
        &self,
        query: &str,
        documents: Vec<HybridSearchResult>,
    ) -> Result<Vec<HybridSearchResult>> {
        trace!(
            "NoOpReranker: Passing through {} documents for query '{}'",
            documents.len(),
            query
        );
        Ok(documents)
    }

    fn name(&self) -> &str {
        "NoOpReranker"
    }

    fn estimated_latency_ms(&self, _num_documents: usize) -> u64 {
        0
    }
}

/// A composite reranker that falls back to NoOp if the primary fails
#[derive(Debug)]
pub struct FallbackReranker<R: Reranker> {
    pub primary: R,
    pub fallback: NoOpReranker,
}

/// Helper: Create a fallback chain
pub fn fallback_reranker<R: Reranker>(primary: R) -> FallbackReranker<R> {
    FallbackReranker {
        primary,
        fallback: NoOpReranker::new(),
    }
}

impl<R: Reranker> Reranker for FallbackReranker<R> {
    fn rerank(
        &self,
        query: &str,
        documents: Vec<HybridSearchResult>,
    ) -> Result<Vec<HybridSearchResult>> {
        // We clone here to keep the original results in case primary fails
        let docs_copy = documents.clone();

        match self.primary.rerank(query, documents) {
            Ok(results) => {
                debug!(
                    "Successfully reranked {} documents with primary: {}",
                    results.len(),
                    self.primary.name()
                );
                Ok(results)
            }
            Err(e) => {
                warn!(
                    "Primary reranker '{}' failed ({}), using fallback",
                    self.primary.name(),
                    e
                );
                self.fallback.rerank(query, docs_copy)
            }
        }
    }

    fn name(&self) -> &str {
        "FallbackReranker"
    }

    fn is_ready(&self) -> bool {
        true
    }

    fn estimated_latency_ms(&self, num_documents: usize) -> u64 {
        if self.primary.is_ready() {
            self.primary.estimated_latency_ms(num_documents)
        } else {
            0
        }
    }
}

// Blanket implementation for Arc-wrapped rerankers
impl<R: Reranker + ?Sized> Reranker for std::sync::Arc<R> {
    fn rerank(
        &self,
        query: &str,
        documents: Vec<HybridSearchResult>,
    ) -> Result<Vec<HybridSearchResult>> {
        (**self).rerank(query, documents)
    }

    fn name(&self) -> &str {
        (**self).name()
    }

    fn is_ready(&self) -> bool {
        (**self).is_ready()
    }

    fn estimated_latency_ms(&self, num_documents: usize) -> u64 {
        (**self).estimated_latency_ms(num_documents)
    }
}
