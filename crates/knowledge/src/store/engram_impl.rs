use crate::rag::{Document, VectorStore};
use async_trait::async_trait;
use benshu_engram::{HybridSearchEngine, HybridSearchResult, QuantLevel};
use benshu_infra::error::{Error, Result};
use std::collections::HashMap;
use std::sync::Arc;

/// A bridge between benshu-knowledge and benshu-engram.
///
/// Implements the VectorStore trait using Engram's HybridSearchEngine.
pub struct EngramStore {
    engine: Arc<HybridSearchEngine>,
    collection: String,
}

impl EngramStore {
    pub fn new(engine: Arc<HybridSearchEngine>, collection: impl Into<String>) -> Self {
        Self {
            engine,
            collection: collection.into(),
        }
    }
}

#[async_trait]
impl VectorStore for EngramStore {
    async fn store(&self, content: &str, metadata: HashMap<String, String>) -> Result<String> {
        let title = metadata
            .get("title")
            .cloned()
            .unwrap_or_else(|| content.chars().take(50).collect());

        let path = metadata
            .get("path")
            .cloned()
            .unwrap_or_else(|| format!("unnamed/{}", uuid::Uuid::new_v4()));

        self.engine
            .index_at_level(
                &self.collection,
                &path,
                &title,
                content,
                QuantLevel::Warm, // Default level
                false,            // unverified
                metadata,
            )
            .map_err(|e| Error::Internal(e.to_string()))?;

        Ok(path)
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Document>> {
        let results = self
            .engine
            .search(query, limit)
            .map_err(|e| Error::Internal(e.to_string()))?;

        let mut docs = Vec::new();
        let store = self.engine.engram_store();

        for res in results {
            let content = store
                .get_content(&res.document)
                .unwrap_or(None)
                .unwrap_or_else(|| "[Content missing in CAS]".to_string());

            docs.push(map_to_document(res, content));
        }

        Ok(docs)
    }

    async fn delete(&self, id: &str) -> Result<()> {
        let store = self.engine.engram_store();
        store
            .delete_document(&self.collection, id)
            .map_err(|e| Error::Internal(e.to_string()))?;
        Ok(())
    }

    async fn age_vectors(&self, older_than_days: i64) -> Result<usize> {
        self.engine
            .perform_distillation(older_than_days as u32, 0)
            .map_err(|e| Error::Internal(e.to_string()))?;
        // For now, we don't return exact count from distillation, returning 1 to indicate attempt
        Ok(1)
    }

    async fn list_collections(&self) -> Result<Vec<String>> {
        let store = self.engine.engram_store();
        let colls = store
            .kv()
            .list_collections()
            .map_err(|e| Error::Internal(e.to_string()))?;
        Ok(colls.into_iter().map(|(name, _)| name).collect())
    }
}

fn map_to_document(res: HybridSearchResult, content: String) -> Document {
    Document {
        id: res.document.docid.clone(),
        title: res.document.title.clone(),
        content,
        summary: res.document.summary.clone(),
        collection: Some(res.document.collection.clone()),
        path: Some(res.document.path.clone()),
        metadata: res.document.metadata.clone(),
        score: res.rrf_score as f32,
    }
}
