//! RAG (Retrieval-Augmented Generation) interfaces owned by `benshu-knowledge`.
//!
//! This crate owns the knowledge-facing document and vector-store contracts.
//! `brain` should call into these contracts instead of being the source of
//! truth for knowledge DTOs.

use async_trait::async_trait;
use benshu_infra::error::Result;
use std::collections::HashMap;

/// A document retrieved from the vector store.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Document {
    /// Unique identifier.
    pub id: String,
    /// The title or mnemonic for the document.
    pub title: String,
    /// The full text content.
    pub content: String,
    /// A shorter summary of the content.
    pub summary: Option<String>,
    /// The collection it belongs to.
    pub collection: Option<String>,
    /// The virtual path/source.
    pub path: Option<String>,
    /// Metadata associated with the document.
    #[serde(default)]
    pub metadata: HashMap<String, String>,
    /// Similarity score.
    #[serde(default)]
    pub score: f32,
}

#[async_trait]
pub trait VectorStore: Send + Sync {
    /// Store a text with metadata.
    async fn store(&self, content: &str, metadata: HashMap<String, String>) -> Result<String>;

    /// Search for similar documents.
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Document>>;

    /// Delete a document by ID.
    async fn delete(&self, id: &str) -> Result<()>;

    /// Migrate vectors older than days to a lower quantization level.
    async fn age_vectors(&self, older_than_days: i64) -> Result<usize>;

    /// List all collection names in the vector store.
    async fn list_collections(&self) -> Result<Vec<String>>;
}

#[async_trait]
pub trait Embeddings: Send + Sync {
    /// Generate embedding vector for text.
    async fn embed(&self, text: &str) -> Result<Vec<f32>>;
}
