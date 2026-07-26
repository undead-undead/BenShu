use crate::error::{EngramError, Result};
use bytes::Bytes;
use std::path::Path;

pub mod in_memory;
pub mod redb_impl;

pub use in_memory::InMemoryStorage;

/// Storage Trait: The abstract backend for Engram
///
/// This trait decouples Engram from hardcoded redb bindings, allowing for:
/// - In-memory test stores
/// - Sled/RocksDB alternative backends
/// - Remote/Cloud KV proxies
pub trait Storage: Send + Sync {
    /// Database path (if applicable)
    fn path(&self) -> &Path;

    // ============ Document Operations ============
    fn put_document(&self, key: &str, data: &[u8]) -> Result<()>;
    fn get_document(&self, key: &str) -> Result<Option<Bytes>>;
    fn delete_document(&self, key: &str) -> Result<bool>;
    fn iter_documents(&self) -> Result<Vec<(String, Bytes)>>;
    fn document_count(&self) -> Result<u64>;

    // ============ DocID to Path Mapping ============
    fn put_docid_map(&self, docid: &str, doc_key: &str) -> Result<()>;
    fn get_docid_map(&self, docid: &str) -> Result<Option<String>>;
    fn delete_docid_map(&self, docid: &str) -> Result<bool>;

    // ============ Content Blob Operations ============
    fn put_content(&self, hash: &str, data: &[u8]) -> Result<()>;
    fn get_content(&self, hash: &str) -> Result<Option<Bytes>>;
    fn content_count(&self) -> Result<u64>;

    // ============ Collection Operations ============
    fn put_collection(&self, name: &str, data: &[u8]) -> Result<()>;
    fn get_collection(&self, name: &str) -> Result<Option<Bytes>>;
    fn list_collections(&self) -> Result<Vec<(String, Bytes)>>;

    // ============ Session Operations ============
    fn put_session(&self, id: &str, data: &str) -> Result<()>;
    fn get_session(&self, id: &str) -> Result<Option<String>>;
    fn delete_session(&self, id: &str) -> Result<bool>;
    fn list_sessions(&self) -> Result<Vec<(String, String)>>;

    // ============ FTS Index Operations ============
    fn put_fts_forward(&self, doc_key: &str, data: &[u8]) -> Result<()>;
    fn get_fts_forward(&self, doc_key: &str) -> Result<Option<Bytes>>;
    fn delete_fts_forward(&self, doc_key: &str) -> Result<bool>;

    fn put_fts_inverted(&self, term: &str, data: &[u8]) -> Result<()>;
    fn get_fts_inverted(&self, term: &str) -> Result<Option<Bytes>>;
    fn delete_fts_inverted(&self, term: &str) -> Result<bool>;

    // ============ Vector Operations ============
    fn put_vector(&self, key: &str, data: &[u8]) -> Result<()>;
    fn get_vector(&self, key: &str) -> Result<Option<Bytes>>;
    fn delete_vector(&self, key: &str) -> Result<bool>;
    fn iter_vectors(&self) -> Result<Vec<(String, Bytes)>>;

    // ============ Embedding Cache Operations ============
    fn put_embedding_cache(&self, hash: &str, vector: &[f32]) -> Result<()>;
    fn get_embedding_cache(&self, hash: &str) -> Result<Option<Vec<f32>>>;

    /// Optimized: Retrieve vector data as f32
    fn get_vector_f32(&self, key: &str) -> Result<Option<Vec<f32>>> {
        let bytes = self.get_vector(key)?;
        match bytes {
            Some(b) => {
                if b.len() % 4 != 0 {
                    return Err(EngramError::Storage(format!(
                        "Invalid vector size ({} bytes)",
                        b.len()
                    )));
                }
                let f32_count = b.len() / 4;
                let mut f32s = Vec::with_capacity(f32_count);
                for i in 0..f32_count {
                    let mut chunk = [0u8; 4];
                    chunk.copy_from_slice(&b[i * 4..(i + 1) * 4]);
                    f32s.push(f32::from_le_bytes(chunk));
                }
                Ok(Some(f32s))
            }
            None => Ok(None),
        }
    }

    // ============ Cognitive Experience Operations ============
    fn put_experience(&self, key: &str, data: &[u8]) -> Result<()>;
    fn get_experience(&self, key: &str) -> Result<Option<Bytes>>;
    fn delete_experience(&self, key: &str) -> Result<bool>;
    fn iter_experiences(&self) -> Result<Vec<(String, Bytes)>>;
    fn experience_count(&self) -> Result<u64>;

    // ============ Anti-Pattern Operations ============
    fn put_anti_pattern(&self, key: &str, data: &[u8]) -> Result<()>;
    fn get_anti_pattern(&self, key: &str) -> Result<Option<Bytes>>;
    fn delete_anti_pattern(&self, key: &str) -> Result<bool>;
    fn iter_anti_patterns(&self) -> Result<Vec<(String, Bytes)>>;
    fn anti_pattern_count(&self) -> Result<u64>;

    // ============ HNSW Index Operations ============
    fn put_idx(&self, id: &str, key: &str) -> Result<()>;
    fn get_idx(&self, id: &str) -> Result<Option<String>>;

    // ============ Knowledge Graph (Triple) Operations ============
    /// Put a triple into the cognitive SPO, OPS, and POS indexes
    fn put_triple(&self, s: &str, p: &str, o: &str, metadata: &[u8]) -> Result<()>;
    /// Atomic batch update for multiple triples
    fn put_triples_batch(&self, triples: Vec<(String, String, String, Vec<u8>)>) -> Result<()>;
    /// Delete a triple from all indexes
    fn delete_triple(&self, s: &str, p: &str, o: &str) -> Result<bool>;
    /// Query triples with optional filters. Returns Vec<(Subject, Predicate, Object, Metadata)>
    fn query_triples(
        &self,
        s: Option<&str>,
        p: Option<&str>,
        o: Option<&str>,
    ) -> Result<Vec<(String, String, String, Bytes)>>;

    // ============ Maintenance ============
    fn compact(&self) -> Result<()>;
    fn disk_usage(&self) -> Result<Option<u64>>;
}
