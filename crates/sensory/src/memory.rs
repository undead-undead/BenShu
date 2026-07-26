use anyhow::Result;
use benshu_engram::vector_store::{VectorEntry, VectorStore};
use std::sync::Arc;
use uuid::Uuid;

/// Persistent memory for sensory features (Visual/Audio embeddings).
pub struct SensoryMemory {
    store: Arc<VectorStore>,
}

impl SensoryMemory {
    pub fn new(store: Arc<VectorStore>) -> Self {
        Self { store }
    }

    /// Remember a sensory feature vector.
    pub async fn remember(
        &self,
        features: Vec<f32>,
        _metadata: serde_json::Value,
    ) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        // VectorStore::add(collection, path, docid, chunk_seq, embedding)
        self.store
            .add("sensory", "features", &id, 0, features)
            .map_err(|e| anyhow::anyhow!("Failed to store vector: {}", e))?;

        Ok(id)
    }

    /// Recall similar sensory experiences.
    pub async fn recall(
        &self,
        features: &[f32],
        limit: usize,
    ) -> Result<Vec<benshu_engram::vector_store::VectorSearchResult>> {
        let results = self
            .store
            .search("sensory", features, limit)
            .map_err(|e| anyhow::anyhow!("Recall failed: {}", e))?;
        Ok(results)
    }
}
