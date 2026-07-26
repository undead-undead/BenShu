//! Pure Rust BM25 full-text search engine
//!
//! Provides inverted index and BM25 scoring without SQLite FTS5.
//! Index data is persisted in Engram-KV tables.

use crate::error::Result;
use crate::storage::Storage;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use jieba_rs::Jieba;
use lru::LruCache;
use once_cell::sync::Lazy;
use std::num::NonZeroUsize;

static JIEBA: Lazy<Jieba> = Lazy::new(Jieba::new);

/// List of common stopwords to reduce index noise and size
static STOPWORDS: Lazy<std::collections::HashSet<&'static str>> = Lazy::new(|| {
    let mut s = std::collections::HashSet::new();
    // English
    s.extend([
        "the", "a", "an", "and", "or", "but", "in", "on", "at", "to", "for", "with", "is", "are",
        "was", "were", "of",
    ]);
    // CJK
    s.extend([
        "的", "了", "是", "在", "我", "有", "和", "就", "不", "人", "都", "一", "一个", "上", "也",
        "很", "到", "说", "要", "去", "你", "会", "着", "没有", "看", "好", "自己", "这",
    ]);
    s
});

/// Global FTS statistics for dynamic BM25 scoring
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FtsStats {
    pub total_docs: u64,
    pub total_len: u64,
    pub avg_dl: f64,
}

impl FtsStats {
    pub fn update(&mut self, new_doc_len: u32) {
        self.total_docs += 1;
        self.total_len += new_doc_len as u64;
        if self.total_docs > 0 {
            self.avg_dl = self.total_len as f64 / self.total_docs as f64;
        }
    }

    pub fn remove(&mut self, old_doc_len: u32) {
        if self.total_docs > 0 {
            self.total_docs -= 1;
            self.total_len = self.total_len.saturating_sub(old_doc_len as u64);
            if self.total_docs > 0 {
                self.avg_dl = self.total_len as f64 / self.total_docs as f64;
            } else {
                self.avg_dl = 0.0;
            }
        }
    }
}

/// Term frequency entry for a document
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TermFrequency {
    pub doc_key: String,
    pub term: String,
    pub count: u32,
    pub doc_length: u32,
}

/// Posting list entry (term -> list of documents containing it)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostingList {
    pub term: String,
    pub entries: Vec<PostingEntry>,
}

const POSTING_STATE_CONTRACT_VERSION: u32 = 1;
const POSTING_SEGMENT_COMPACT_THRESHOLD: usize = 8;
const POSTING_SEGMENT_PREFIX: &str = "__fts_segment__";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PostingListState {
    contract_version: u32,
    term: String,
    segments: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PostingSegment {
    term: String,
    added_entries: Vec<PostingEntry>,
    removed_doc_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostingEntry {
    pub doc_key: String,
    pub term_frequency: u32,
    pub doc_length: u32,
}

/// BM25 scoring parameters
pub struct Bm25Config {
    pub k1: f64,
    pub b: f64,
}

impl Default for Bm25Config {
    fn default() -> Self {
        Self { k1: 1.2, b: 0.75 }
    }
}

/// BM25 search result
#[derive(Debug, Clone)]
pub struct FtsResult {
    pub doc_key: String,
    pub score: f64,
}

/// Full-text search engine using BM25 on Engram-KV
pub struct FtsEngine {
    kv: Arc<dyn Storage>,
    config: Bm25Config,
    /// Concurrency lock to prevent data corruption during simultaneous writes
    lock: parking_lot::RwLock<()>,
    /// In-memory cache for frequently accessed posting lists
    posting_cache: parking_lot::Mutex<LruCache<String, PostingList>>,
    segment_seq: AtomicU64,
}

impl FtsEngine {
    pub fn new(kv: Arc<dyn Storage>) -> Self {
        Self {
            kv,
            config: Bm25Config::default(),
            lock: parking_lot::RwLock::new(()),
            posting_cache: parking_lot::Mutex::new(LruCache::new(NonZeroUsize::new(1000).unwrap())),
            segment_seq: AtomicU64::new(1),
        }
    }

    /// Tokenize text into terms. Supports CJK characters via Jieba segmentation.
    pub fn tokenize(text: &str) -> Vec<String> {
        let text_lower = text.to_lowercase();
        let raw_tokens = JIEBA.cut(&text_lower, false);

        raw_tokens
            .into_iter()
            .filter_map(|s| {
                let trimmed = s.trim();
                if trimmed.is_empty() || STOPWORDS.contains(trimmed) {
                    return None;
                }

                let first_char = trimmed.chars().next().unwrap_or(' ');
                let is_cjk = ('\u{4e00}'..='\u{9fff}').contains(&first_char);
                let is_alphanum = first_char.is_alphanumeric();

                // Keep CJK characters or alphanum with length >= 2
                if (is_cjk && is_alphanum) || trimmed.len() >= 2 {
                    Some(trimmed.to_string())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Index a document's text
    pub fn index_document(&self, doc_key: &str, text: &str) -> Result<()> {
        let _write_lock = self.lock.write();

        // 1. Tokenize and calculate stats
        let terms = Self::tokenize(text);
        let doc_length = terms.len() as u32;
        let previous_tf_map = self.get_forward_tf_map(doc_key)?;

        let mut tf_map: HashMap<String, u32> = HashMap::new();
        for term in &terms {
            *tf_map.entry(term.clone()).or_insert(0) += 1;
        }

        // 2. Update Stats
        let mut stats = self.get_stats()?;
        if let Some(previous) = &previous_tf_map {
            stats.remove(previous.values().copied().sum());
        }
        stats.update(doc_length);
        self.put_stats(&stats)?;

        // 3. Store Forward Index
        let forward_data = bincode::serialize(&tf_map)
            .map_err(|e| crate::error::EngramError::Serialization(e.to_string()))?;
        self.kv.put_fts_forward(doc_key, &forward_data)?;

        // 4. Update Inverted Index
        let mut affected_terms: HashSet<String> = tf_map.keys().cloned().collect();
        if let Some(previous) = &previous_tf_map {
            affected_terms.extend(previous.keys().cloned());
        }
        for term in affected_terms {
            let removed_doc_keys = previous_tf_map
                .as_ref()
                .and_then(|previous| previous.get(&term))
                .map(|_| vec![doc_key.to_string()])
                .unwrap_or_default();
            let added_entries = tf_map
                .get(&term)
                .map(|count| {
                    vec![PostingEntry {
                        doc_key: doc_key.to_string(),
                        term_frequency: *count,
                        doc_length,
                    }]
                })
                .unwrap_or_default();
            self.apply_term_delta(&term, added_entries, removed_doc_keys)?;
        }

        Ok(())
    }

    /// Batch version of indexing for massive performance gains
    pub fn index_batch(&self, docs: &[(String, String)]) -> Result<()> {
        let _write_lock = self.lock.write();
        let mut stats = self.get_stats()?;
        let mut batch_entries: HashMap<String, Vec<PostingEntry>> = HashMap::new();
        let mut affected_docs_by_term: HashMap<String, HashSet<String>> = HashMap::new();

        for (doc_key, text) in docs {
            let terms = Self::tokenize(text);
            let doc_length = terms.len() as u32;
            if let Some(previous) = self.get_forward_tf_map(doc_key)? {
                stats.remove(previous.values().copied().sum());
                for term in previous.keys() {
                    affected_docs_by_term
                        .entry(term.clone())
                        .or_default()
                        .insert(doc_key.clone());
                }
            }
            stats.update(doc_length);

            let mut tf_map: HashMap<String, u32> = HashMap::new();
            for term in &terms {
                *tf_map.entry(term.clone()).or_insert(0) += 1;
            }

            let forward_data = bincode::serialize(&tf_map)
                .map_err(|e| crate::error::EngramError::Serialization(e.to_string()))?;
            self.kv.put_fts_forward(doc_key, &forward_data)?;

            for (term, count) in &tf_map {
                batch_entries
                    .entry(term.clone())
                    .or_default()
                    .push(PostingEntry {
                        doc_key: doc_key.to_string(),
                        term_frequency: *count,
                        doc_length,
                    });
                affected_docs_by_term
                    .entry(term.clone())
                    .or_default()
                    .insert(doc_key.clone());
            }
        }

        for (term, affected_docs) in affected_docs_by_term {
            let added_entries = batch_entries.remove(&term).unwrap_or_default();
            let removed_doc_keys = affected_docs.into_iter().collect::<Vec<_>>();
            self.apply_term_delta(&term, added_entries, removed_doc_keys)?;
        }
        self.put_stats(&stats)?;
        Ok(())
    }

    fn get_stats(&self) -> Result<FtsStats> {
        // We use a reserved key in collections or metadata table
        if let Some(data) = self.kv.get_collection("__fts_stats__")? {
            return Ok(bincode::deserialize(&data)
                .map_err(|e| crate::error::EngramError::Serialization(e.to_string()))?);
        }
        Ok(FtsStats::default())
    }

    fn put_stats(&self, stats: &FtsStats) -> Result<()> {
        let data = bincode::serialize(stats)
            .map_err(|e| crate::error::EngramError::Serialization(e.to_string()))?;
        self.kv.put_collection("__fts_stats__", &data)?;
        Ok(())
    }

    fn next_segment_key(&self, term: &str) -> String {
        let seq = self.segment_seq.fetch_add(1, Ordering::Relaxed);
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or_default();
        format!("{POSTING_SEGMENT_PREFIX}:{term}:{now_ms}:{seq}")
    }

    fn posting_state_from_legacy(
        &self,
        term: &str,
        legacy: PostingList,
    ) -> Result<PostingListState> {
        let segment_key = self.next_segment_key(term);
        let segment = PostingSegment {
            term: term.to_string(),
            added_entries: legacy.entries,
            removed_doc_keys: Vec::new(),
        };
        let segment_data = bincode::serialize(&segment)
            .map_err(|e| crate::error::EngramError::Serialization(e.to_string()))?;
        self.kv.put_fts_inverted(&segment_key, &segment_data)?;

        let state = PostingListState {
            contract_version: POSTING_STATE_CONTRACT_VERSION,
            term: term.to_string(),
            segments: vec![segment_key],
        };
        self.persist_posting_state(&state)?;
        Ok(state)
    }

    fn persist_posting_state(&self, state: &PostingListState) -> Result<()> {
        let data = bincode::serialize(state)
            .map_err(|e| crate::error::EngramError::Serialization(e.to_string()))?;
        self.kv.put_fts_inverted(&state.term, &data)?;
        Ok(())
    }

    fn materialize_posting_state(&self, state: &PostingListState) -> Result<PostingList> {
        let mut entries = HashMap::<String, PostingEntry>::new();
        for segment_key in &state.segments {
            let Some(data) = self.kv.get_fts_inverted(segment_key)? else {
                continue;
            };
            let segment: PostingSegment = bincode::deserialize(&data)
                .map_err(|e| crate::error::EngramError::Serialization(e.to_string()))?;

            for doc_key in &segment.removed_doc_keys {
                entries.remove(doc_key);
            }
            for entry in segment.added_entries {
                entries.insert(entry.doc_key.clone(), entry);
            }
        }

        let mut resolved_entries = entries.into_values().collect::<Vec<_>>();
        resolved_entries.sort_by(|left, right| left.doc_key.cmp(&right.doc_key));
        Ok(PostingList {
            term: state.term.clone(),
            entries: resolved_entries,
        })
    }

    fn get_or_migrate_posting_state(&self, term: &str) -> Result<Option<PostingListState>> {
        let Some(data) = self.kv.get_fts_inverted(term)? else {
            return Ok(None);
        };

        if let Ok(state) = bincode::deserialize::<PostingListState>(&data) {
            return Ok(Some(state));
        }

        if let Ok(legacy) = bincode::deserialize::<PostingList>(&data) {
            return self.posting_state_from_legacy(term, legacy).map(Some);
        }

        Err(crate::error::EngramError::Serialization(format!(
            "Failed to deserialize posting state for term {term}"
        )))
    }

    /// Internal posting list retrieval with cache
    fn get_posting_list_internal(&self, term: &str) -> Result<PostingList> {
        {
            let mut cache = self.posting_cache.lock();
            if let Some(list) = cache.get(term) {
                return Ok(list.clone());
            }
        }

        match self.kv.get_fts_inverted(term)? {
            Some(data) => {
                let list = if let Ok(state) = bincode::deserialize::<PostingListState>(&data) {
                    self.materialize_posting_state(&state)?
                } else {
                    bincode::deserialize(&data)
                        .map_err(|e| crate::error::EngramError::Serialization(e.to_string()))?
                };
                self.posting_cache
                    .lock()
                    .put(term.to_string(), list.clone());
                Ok(list)
            }
            None => Ok(PostingList {
                term: term.to_string(),
                entries: Vec::new(),
            }),
        }
    }

    fn apply_term_delta(
        &self,
        term: &str,
        added_entries: Vec<PostingEntry>,
        removed_doc_keys: Vec<String>,
    ) -> Result<()> {
        let mut state = self
            .get_or_migrate_posting_state(term)?
            .unwrap_or(PostingListState {
                contract_version: POSTING_STATE_CONTRACT_VERSION,
                term: term.to_string(),
                segments: Vec::new(),
            });

        if !added_entries.is_empty() || !removed_doc_keys.is_empty() {
            let segment_key = self.next_segment_key(term);
            let segment = PostingSegment {
                term: term.to_string(),
                added_entries,
                removed_doc_keys,
            };
            let segment_data = bincode::serialize(&segment)
                .map_err(|e| crate::error::EngramError::Serialization(e.to_string()))?;
            self.kv.put_fts_inverted(&segment_key, &segment_data)?;
            state.segments.push(segment_key);
        }

        let mut resolved = self.materialize_posting_state(&state)?;
        if state.segments.len() >= POSTING_SEGMENT_COMPACT_THRESHOLD {
            let old_segments = std::mem::take(&mut state.segments);
            let compact_segment_key = self.next_segment_key(term);
            let compact_segment = PostingSegment {
                term: term.to_string(),
                added_entries: resolved.entries.clone(),
                removed_doc_keys: Vec::new(),
            };
            let compact_data = bincode::serialize(&compact_segment)
                .map_err(|e| crate::error::EngramError::Serialization(e.to_string()))?;
            self.kv
                .put_fts_inverted(&compact_segment_key, &compact_data)?;
            for segment_key in old_segments {
                self.kv.delete_fts_inverted(&segment_key)?;
            }
            state.segments = vec![compact_segment_key];
            resolved = self.materialize_posting_state(&state)?;
        }

        let mut cache = self.posting_cache.lock();
        cache.pop(term);
        if resolved.entries.is_empty() {
            for segment_key in state.segments {
                self.kv.delete_fts_inverted(&segment_key)?;
            }
            self.kv.delete_fts_inverted(term)?;
            return Ok(());
        }

        self.persist_posting_state(&state)?;
        cache.put(term.to_string(), resolved);
        Ok(())
    }

    /// Search using BM25 scoring with dynamic stats and concurrency protection
    pub fn search(&self, query: &str, _total_docs: u64, limit: usize) -> Result<Vec<FtsResult>> {
        let _read_lock = self.lock.read();
        let query_terms = Self::tokenize(query);
        let mut doc_scores: HashMap<String, f64> = HashMap::new();

        // 1. Get dynamic stats instead of hardcoded 100.0 or shared params
        let stats = self.get_stats()?;
        let total_docs = stats.total_docs;
        let avg_dl = if stats.avg_dl > 0.0 {
            stats.avg_dl
        } else {
            100.0
        };

        if total_docs == 0 {
            return Ok(Vec::new());
        }

        for term in &query_terms {
            let posting_list = self.get_posting_list_internal(term)?;
            let df = posting_list.entries.len() as f64;
            if df == 0.0 {
                continue;
            }

            // BM25 IDF variant
            let idf = ((total_docs as f64 - df + 0.5) / (df + 0.5) + 1.0).ln();

            // Early Pruning Optimization:
            // If posting list is massive, only process top entries to protect system responsiveness.
            // Using a threshold of 5000 entries for personal-scale long-term performance.
            let entries_to_process = if posting_list.entries.len() > 5000 {
                &posting_list.entries[0..5000]
            } else {
                &posting_list.entries[..]
            };

            for entry in entries_to_process {
                let tf = entry.term_frequency as f64;
                let dl = entry.doc_length as f64;
                let numerator = tf * (self.config.k1 + 1.0);
                let denominator =
                    tf + self.config.k1 * (1.0 - self.config.b + self.config.b * dl / avg_dl);
                let score = idf * numerator / denominator;
                *doc_scores.entry(entry.doc_key.clone()).or_insert(0.0) += score;
            }
        }

        let mut results: Vec<FtsResult> = doc_scores
            .into_iter()
            .map(|(doc_key, score)| FtsResult { doc_key, score })
            .collect();

        // High-Performance Truncation:
        // For massive datasets, use select_nth_unstable_by_key instead of full sort
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        results.truncate(limit);
        Ok(results)
    }

    /// Delete a document from the FTS index
    pub fn delete_document(&self, doc_key: &str) -> Result<()> {
        let _write_lock = self.lock.write();
        let mut stats = self.get_stats()?;

        // 1. Get forward index to find terms and original doc length
        if let Some(forward_data) = self.kv.get_fts_forward(doc_key)? {
            let tf_map: HashMap<String, u32> = bincode::deserialize(&forward_data)
                .map_err(|e| crate::error::EngramError::Serialization(e.to_string()))?;

            let mut doc_len = 0u32;
            let terms = tf_map.keys().cloned().collect::<Vec<_>>();

            // 2. Remove entry from each term's posting list
            for term in terms {
                let posting_list = self.get_posting_list_internal(&term)?;
                // Find and capture doc length for stats before removing
                if doc_len == 0 {
                    if let Some(entry) = posting_list.entries.iter().find(|e| e.doc_key == doc_key)
                    {
                        doc_len = entry.doc_length;
                    }
                }
                self.apply_term_delta(&term, Vec::new(), vec![doc_key.to_string()])?;
            }

            // 3. Update stats if found
            if doc_len > 0 {
                stats.remove(doc_len);
                self.put_stats(&stats)?;
            }
        }

        // 4. Delete forward index
        self.kv.delete_fts_forward(doc_key)?;
        Ok(())
    }

    fn get_forward_tf_map(&self, doc_key: &str) -> Result<Option<HashMap<String, u32>>> {
        self.kv
            .get_fts_forward(doc_key)?
            .map(|forward_data| {
                bincode::deserialize(&forward_data)
                    .map_err(|e| crate::error::EngramError::Serialization(e.to_string()))
            })
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::InMemoryStorage;
    use std::sync::Arc;

    #[test]
    fn test_fts_tokenize_cjk() {
        let text = "BENSHU是一个强大的AI代理框架";
        let tokens = FtsEngine::tokenize(text);

        assert!(tokens.contains(&"benshu".to_string()));
        assert!(tokens.contains(&"强大".to_string()));
        assert!(tokens.contains(&"代理".to_string()));
        assert!(tokens.contains(&"框架".to_string()));
    }

    #[test]
    fn test_fts_tokenize_english() {
        let text = "quick brown fox jumps";
        let tokens = FtsEngine::tokenize(text);
        assert_eq!(tokens.len(), 4);
        assert!(tokens.contains(&"fox".to_string()));
    }

    #[test]
    fn test_fts_batch_replaces_existing_postings_without_duplicates() {
        let engine = FtsEngine::new(Arc::new(InMemoryStorage::new()));

        engine
            .index_batch(&[
                ("doc-1".to_string(), "rust systems language".to_string()),
                ("doc-2".to_string(), "rust async runtime".to_string()),
            ])
            .expect("initial batch index should succeed");

        let initial = engine
            .search("rust", 0, 10)
            .expect("initial rust query should succeed");
        assert_eq!(initial.len(), 2);

        engine
            .index_batch(&[("doc-1".to_string(), "python data tooling".to_string())])
            .expect("replacement batch index should succeed");

        let rust_results = engine
            .search("rust", 0, 10)
            .expect("rust query after replacement should succeed");
        assert_eq!(rust_results.len(), 1);
        assert_eq!(rust_results[0].doc_key, "doc-2");

        let python_results = engine
            .search("python", 0, 10)
            .expect("python query after replacement should succeed");
        assert_eq!(python_results.len(), 1);
        assert_eq!(python_results[0].doc_key, "doc-1");
    }

    #[test]
    fn test_fts_segmented_postings_append_friendly_layout() {
        let storage = Arc::new(InMemoryStorage::new());
        let engine = FtsEngine::new(storage.clone());

        engine
            .index_document("doc-1", "rust systems language")
            .expect("first document should index");
        engine
            .index_document("doc-2", "rust async runtime")
            .expect("second document should index");

        let state: PostingListState = bincode::deserialize(
            &storage
                .get_fts_inverted("rust")
                .expect("state fetch should work")
                .expect("term state should exist"),
        )
        .expect("term state should deserialize");
        assert_eq!(state.contract_version, POSTING_STATE_CONTRACT_VERSION);
        assert_eq!(state.segments.len(), 2);

        let results = engine
            .search("rust", 0, 10)
            .expect("segmented rust query should succeed");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_fts_segment_compaction_limits_segment_growth() {
        let storage = Arc::new(InMemoryStorage::new());
        let engine = FtsEngine::new(storage.clone());

        for idx in 0..10 {
            engine
                .index_document(&format!("doc-{idx}"), "rust retrieval memory")
                .expect("document should index");
        }

        let state: PostingListState = bincode::deserialize(
            &storage
                .get_fts_inverted("rust")
                .expect("state fetch should work")
                .expect("term state should exist"),
        )
        .expect("term state should deserialize");
        assert!(state.segments.len() < POSTING_SEGMENT_COMPACT_THRESHOLD);

        let results = engine
            .search("rust", 0, 20)
            .expect("compacted rust query should succeed");
        assert_eq!(results.len(), 10);
    }
}
