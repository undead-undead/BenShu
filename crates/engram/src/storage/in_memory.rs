use crate::error::Result;
use crate::storage::Storage;
use bytes::Bytes;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A simple thread-safe in-memory storage for testing
pub struct InMemoryStorage {
    docs: RwLock<HashMap<String, Bytes>>,
    collections: RwLock<HashMap<String, Bytes>>,
    fts_forward: RwLock<HashMap<String, Bytes>>,
    fts_inverted: RwLock<HashMap<String, Bytes>>,
    vectors: RwLock<HashMap<String, Bytes>>,
    id_map: RwLock<HashMap<String, String>>,
    triples: RwLock<Vec<(String, String, String, Bytes)>>,
    path: PathBuf,
}

impl InMemoryStorage {
    pub fn new() -> Self {
        Self {
            docs: RwLock::new(HashMap::new()),
            collections: RwLock::new(HashMap::new()),
            fts_forward: RwLock::new(HashMap::new()),
            fts_inverted: RwLock::new(HashMap::new()),
            vectors: RwLock::new(HashMap::new()),
            id_map: RwLock::new(HashMap::new()),
            triples: RwLock::new(Vec::new()),
            path: PathBuf::from(":memory:"),
        }
    }
}

impl Storage for InMemoryStorage {
    fn path(&self) -> &Path {
        &self.path
    }

    fn put_document(&self, key: &str, data: &[u8]) -> Result<()> {
        self.docs
            .write()
            .insert(key.to_string(), Bytes::copy_from_slice(data));
        Ok(())
    }

    fn get_document(&self, key: &str) -> Result<Option<Bytes>> {
        Ok(self.docs.read().get(key).cloned())
    }

    fn delete_document(&self, key: &str) -> Result<bool> {
        Ok(self.docs.write().remove(key).is_some())
    }

    fn iter_documents(&self) -> Result<Vec<(String, Bytes)>> {
        Ok(self
            .docs
            .read()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect())
    }

    fn document_count(&self) -> Result<u64> {
        Ok(self.docs.read().len() as u64)
    }

    fn put_docid_map(&self, _docid: &str, _doc_key: &str) -> Result<()> {
        Ok(())
    }
    fn get_docid_map(&self, _docid: &str) -> Result<Option<String>> {
        Ok(None)
    }
    fn delete_docid_map(&self, _docid: &str) -> Result<bool> {
        Ok(false)
    }

    fn put_content(&self, _hash: &str, _data: &[u8]) -> Result<()> {
        Ok(())
    }
    fn get_content(&self, _hash: &str) -> Result<Option<Bytes>> {
        Ok(None)
    }
    fn content_count(&self) -> Result<u64> {
        Ok(0)
    }

    fn put_collection(&self, name: &str, data: &[u8]) -> Result<()> {
        self.collections
            .write()
            .insert(name.to_string(), Bytes::copy_from_slice(data));
        Ok(())
    }
    fn get_collection(&self, name: &str) -> Result<Option<Bytes>> {
        Ok(self.collections.read().get(name).cloned())
    }
    fn list_collections(&self) -> Result<Vec<(String, Bytes)>> {
        Ok(self
            .collections
            .read()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect())
    }

    fn put_session(&self, _id: &str, _data: &str) -> Result<()> {
        Ok(())
    }
    fn get_session(&self, _id: &str) -> Result<Option<String>> {
        Ok(None)
    }
    fn delete_session(&self, _id: &str) -> Result<bool> {
        Ok(false)
    }
    fn list_sessions(&self) -> Result<Vec<(String, String)>> {
        Ok(vec![])
    }

    fn put_fts_forward(&self, doc_key: &str, data: &[u8]) -> Result<()> {
        self.fts_forward
            .write()
            .insert(doc_key.to_string(), Bytes::copy_from_slice(data));
        Ok(())
    }
    fn get_fts_forward(&self, doc_key: &str) -> Result<Option<Bytes>> {
        Ok(self.fts_forward.read().get(doc_key).cloned())
    }
    fn delete_fts_forward(&self, doc_key: &str) -> Result<bool> {
        Ok(self.fts_forward.write().remove(doc_key).is_some())
    }

    fn put_fts_inverted(&self, term: &str, data: &[u8]) -> Result<()> {
        self.fts_inverted
            .write()
            .insert(term.to_string(), Bytes::copy_from_slice(data));
        Ok(())
    }
    fn get_fts_inverted(&self, term: &str) -> Result<Option<Bytes>> {
        Ok(self.fts_inverted.read().get(term).cloned())
    }
    fn delete_fts_inverted(&self, term: &str) -> Result<bool> {
        Ok(self.fts_inverted.write().remove(term).is_some())
    }

    fn put_vector(&self, key: &str, data: &[u8]) -> Result<()> {
        self.vectors
            .write()
            .insert(key.to_string(), Bytes::copy_from_slice(data));
        Ok(())
    }

    fn get_vector(&self, key: &str) -> Result<Option<Bytes>> {
        Ok(self.vectors.read().get(key).cloned())
    }

    fn delete_vector(&self, key: &str) -> Result<bool> {
        Ok(self.vectors.write().remove(key).is_some())
    }

    fn iter_vectors(&self) -> Result<Vec<(String, Bytes)>> {
        Ok(self
            .vectors
            .read()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect())
    }

    fn put_embedding_cache(&self, _hash: &str, _vector: &[f32]) -> Result<()> {
        Ok(())
    }
    fn get_embedding_cache(&self, _hash: &str) -> Result<Option<Vec<f32>>> {
        Ok(None)
    }

    fn put_experience(&self, _key: &str, _data: &[u8]) -> Result<()> {
        Ok(())
    }
    fn get_experience(&self, _key: &str) -> Result<Option<Bytes>> {
        Ok(None)
    }
    fn delete_experience(&self, _key: &str) -> Result<bool> {
        Ok(false)
    }
    fn iter_experiences(&self) -> Result<Vec<(String, Bytes)>> {
        Ok(vec![])
    }
    fn experience_count(&self) -> Result<u64> {
        Ok(0)
    }

    fn put_anti_pattern(&self, _key: &str, _data: &[u8]) -> Result<()> {
        Ok(())
    }
    fn get_anti_pattern(&self, _key: &str) -> Result<Option<Bytes>> {
        Ok(None)
    }
    fn delete_anti_pattern(&self, _key: &str) -> Result<bool> {
        Ok(false)
    }
    fn iter_anti_patterns(&self) -> Result<Vec<(String, Bytes)>> {
        Ok(vec![])
    }
    fn anti_pattern_count(&self) -> Result<u64> {
        Ok(0)
    }

    fn put_idx(&self, id: &str, key: &str) -> Result<()> {
        self.id_map.write().insert(id.to_string(), key.to_string());
        Ok(())
    }

    fn get_idx(&self, id: &str) -> Result<Option<String>> {
        Ok(self.id_map.read().get(id).cloned())
    }

    // ============ Knowledge Graph (Triple) Operations ============
    fn put_triple(&self, s: &str, p: &str, o: &str, metadata: &[u8]) -> Result<()> {
        let mut triples = self.triples.write();
        // Remove if exists
        triples.retain(|(ts, tp, to, _)| ts != s || tp != p || to != o);
        triples.push((
            s.to_string(),
            p.to_string(),
            o.to_string(),
            Bytes::copy_from_slice(metadata),
        ));
        Ok(())
    }

    fn delete_triple(&self, s: &str, p: &str, o: &str) -> Result<bool> {
        let mut triples = self.triples.write();
        let len_before = triples.len();
        triples.retain(|(ts, tp, to, _)| ts != s || tp != p || to != o);
        Ok(triples.len() < len_before)
    }

    fn put_triples_batch(&self, batch: Vec<(String, String, String, Vec<u8>)>) -> Result<()> {
        let mut triples = self.triples.write();
        for (s, p, o, metadata) in batch {
            triples.retain(|(ts, tp, to, _)| ts != &s || tp != &p || to != &o);
            triples.push((s, p, o, Bytes::from(metadata)));
        }
        Ok(())
    }

    fn query_triples(
        &self,
        s: Option<&str>,
        p: Option<&str>,
        o: Option<&str>,
    ) -> Result<Vec<(String, String, String, Bytes)>> {
        let triples = self.triples.read();
        Ok(triples
            .iter()
            .filter(|(ts, tp, to, _)| {
                let s_match = s.map_or(true, |sv| ts == sv);
                let p_match = p.map_or(true, |pv| tp == pv);
                let o_match = o.map_or(true, |ov| to == ov);
                s_match && p_match && o_match
            })
            .cloned()
            .collect())
    }

    fn compact(&self) -> Result<()> {
        Ok(())
    }
    fn disk_usage(&self) -> Result<Option<u64>> {
        Ok(None)
    }
}
