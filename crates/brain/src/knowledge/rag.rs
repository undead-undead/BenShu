//! RAG (Retrieval-Augmented Generation) Interfaces
//!
//! This module defines the standard interface for vector stores.
//! Implementations are handled in standalone crates.

use async_trait::async_trait;
use std::collections::HashMap;

use crate::error::Result;

pub use benshu_memory_core::Document;

/// Interface for vector stores
#[async_trait]
pub trait VectorStore: Send + Sync {
    /// Store a text with metadata
    async fn store(&self, content: &str, metadata: HashMap<String, String>) -> Result<String>;

    /// Search for similar documents
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Document>>;

    /// Delete a document by ID
    async fn delete(&self, id: &str) -> Result<()>;

    /// Phase 14.3: Migrate vectors older than days to a lower quantization level
    async fn age_vectors(&self, older_than_days: i64) -> Result<usize>;

    /// List all collection names in the vector store
    async fn list_collections(&self) -> Result<Vec<String>>;
}

/// Interface for embeddings providers
#[async_trait]
pub trait Embeddings: Send + Sync {
    /// Generate embedding vector for text
    async fn embed(&self, text: &str) -> Result<Vec<f32>>;
}
