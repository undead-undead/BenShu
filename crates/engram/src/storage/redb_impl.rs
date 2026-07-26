//! redb implementation of the Storage trait

use crate::error::{EngramError, Result};
use crate::storage::Storage;
use bytes::Bytes;
use redb::{Database, ReadableTable, ReadableTableMetadata, TableDefinition, TableHandle};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{debug, info};

// ============================
// Table Definitions
// ============================
type StrTable = TableDefinition<'static, &'static str, &'static [u8]>;
type StrStrTable = TableDefinition<'static, &'static str, &'static str>;

const DOCUMENTS_TABLE: StrTable = TableDefinition::new("documents");
const COLLECTIONS_TABLE: StrTable = TableDefinition::new("collections");
const CONTENT_TABLE: StrTable = TableDefinition::new("content");
const SESSIONS_TABLE: StrStrTable = TableDefinition::new("sessions");
const FTS_FORWARD_TABLE: StrTable = TableDefinition::new("fts_forward");
const FTS_INVERTED_TABLE: StrTable = TableDefinition::new("fts_inverted");
const VECTORS_TABLE: StrTable = TableDefinition::new("vectors");
const METADATA_TABLE: StrStrTable = TableDefinition::new("metadata");
const EMBEDDING_CACHE_TABLE: StrTable = TableDefinition::new("embedding_cache");
const DOCID_MAP_TABLE: StrStrTable = TableDefinition::new("docid_map");
const EXPERIENCES_TABLE: StrTable = TableDefinition::new("experiences");
const ANTI_PATTERNS_TABLE: StrTable = TableDefinition::new("anti_patterns");
const HNSW_IDX_TABLE: StrStrTable = TableDefinition::new("hnsw_idx");

// Persistent Cognitive Index Tables (SPO/OPS/POS)
const KG_SPO_TABLE: TableDefinition<String, Vec<u8>> = TableDefinition::new("kg_spo");
const KG_OPS_TABLE: TableDefinition<String, Vec<u8>> = TableDefinition::new("kg_ops");
const KG_POS_TABLE: TableDefinition<String, Vec<u8>> = TableDefinition::new("kg_pos");

const SEPARATOR: &str = "\u{1f}";

/// Engram-KV storage engine using redb
pub struct EngramKV {
    db: Arc<Database>,
    path: PathBuf,
}

impl EngramKV {
    /// Create or open an Engram-KV database at the given path
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let db = Database::create(&path)?;

        // Initialize all tables
        let write_txn = db.begin_write()?;
        {
            let _ = write_txn.open_table(DOCUMENTS_TABLE)?;
            let _ = write_txn.open_table(COLLECTIONS_TABLE)?;
            let _ = write_txn.open_table(CONTENT_TABLE)?;
            let _ = write_txn.open_table(SESSIONS_TABLE)?;
            let _ = write_txn.open_table(FTS_FORWARD_TABLE)?;
            let _ = write_txn.open_table(FTS_INVERTED_TABLE)?;
            let _ = write_txn.open_table(VECTORS_TABLE)?;
            let _ = write_txn.open_table(METADATA_TABLE)?;
            let _ = write_txn.open_table(EMBEDDING_CACHE_TABLE)?;
            let _ = write_txn.open_table(DOCID_MAP_TABLE)?;
            let _ = write_txn.open_table(EXPERIENCES_TABLE)?;
            let _ = write_txn.open_table(ANTI_PATTERNS_TABLE)?;
            let _ = write_txn.open_table(HNSW_IDX_TABLE)?;
            let _ = write_txn.open_table(KG_SPO_TABLE)?;
            let _ = write_txn.open_table(KG_OPS_TABLE)?;
            let _ = write_txn.open_table(KG_POS_TABLE)?;
        }
        write_txn.commit()?;

        info!("Engram-KV opened at: {}", path.display());

        Ok(Self {
            db: Arc::new(db),
            path,
        })
    }

    // ============================
    // Helper Methods (DRY)
    // ============================

    fn put_generic(&self, table_def: StrTable, key: &str, value: &[u8]) -> Result<()> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(table_def)?;
            table.insert(key, value)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    fn get_generic(&self, table_def: StrTable, key: &str) -> Result<Option<Bytes>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(table_def)?;
        Ok(table.get(key)?.map(|v| Bytes::copy_from_slice(v.value())))
    }

    fn delete_generic(&self, table_def: StrTable, key: &str) -> Result<bool> {
        let write_txn = self.db.begin_write()?;
        let mut table = write_txn.open_table(table_def)?;
        let removed = table.remove(key)?.is_some();
        drop(table);
        write_txn.commit()?;
        Ok(removed)
    }

    fn iter_generic(&self, table_def: StrTable) -> Result<Vec<(String, Bytes)>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(table_def)?;
        let mut results = Vec::new();
        let mut iter = table.iter()?;
        while let Some(entry_res) = iter.next() {
            let (key, value) = entry_res?;
            results.push((
                key.value().to_string(),
                Bytes::copy_from_slice(value.value()),
            ));
        }
        Ok(results)
    }

    fn count_generic(&self, table_def: StrTable) -> Result<u64> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(table_def)?;
        Ok(table.len()?)
    }

    // Helpers for StrStrTable
    fn put_strstr(&self, table_def: StrStrTable, key: &str, value: &str) -> Result<()> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(table_def)?;
            table.insert(key, value)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    fn get_strstr(&self, table_def: StrStrTable, key: &str) -> Result<Option<String>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(table_def)?;
        Ok(table.get(key)?.map(|v| v.value().to_string()))
    }

    fn delete_strstr(&self, table_def: StrStrTable, key: &str) -> Result<bool> {
        let write_txn = self.db.begin_write()?;
        let mut table = write_txn.open_table(table_def)?;
        let removed = table.remove(key)?.is_some();
        drop(table);
        write_txn.commit()?;
        Ok(removed)
    }

    fn process_triple_result(
        &self,
        table_name: &str,
        key: String,
        value: Vec<u8>,
        s_filter: Option<&str>,
        p_filter: Option<&str>,
        o_filter: Option<&str>,
        results: &mut Vec<(String, String, String, Bytes)>,
    ) {
        let parts: Vec<&str> = key.split(SEPARATOR).collect();
        if parts.len() != 3 {
            return;
        }

        let (subj, pred, obj) = if table_name == KG_SPO_TABLE.name() {
            (parts[0], parts[1], parts[2])
        } else if table_name == KG_OPS_TABLE.name() {
            (parts[2], parts[1], parts[0])
        } else {
            (parts[2], parts[0], parts[1])
        };

        if let Some(s) = s_filter {
            if subj != s {
                return;
            }
        }
        if let Some(p) = p_filter {
            if pred != p {
                return;
            }
        }
        if let Some(o) = o_filter {
            if obj != o {
                return;
            }
        }

        results.push((
            subj.to_string(),
            pred.to_string(),
            obj.to_string(),
            Bytes::copy_from_slice(&value),
        ));
    }
}

impl Storage for EngramKV {
    fn path(&self) -> &Path {
        &self.path
    }

    // ============ Document Operations ============

    fn put_document(&self, key: &str, data: &[u8]) -> Result<()> {
        self.put_generic(DOCUMENTS_TABLE, key, data)
    }

    fn get_document(&self, key: &str) -> Result<Option<Bytes>> {
        self.get_generic(DOCUMENTS_TABLE, key)
    }

    fn delete_document(&self, key: &str) -> Result<bool> {
        self.delete_generic(DOCUMENTS_TABLE, key)
    }

    fn iter_documents(&self) -> Result<Vec<(String, Bytes)>> {
        self.iter_generic(DOCUMENTS_TABLE)
    }

    fn document_count(&self) -> Result<u64> {
        self.count_generic(DOCUMENTS_TABLE)
    }

    // ============ DocID to Path Mapping ============

    fn put_docid_map(&self, docid: &str, doc_key: &str) -> Result<()> {
        self.put_strstr(DOCID_MAP_TABLE, docid, doc_key)
    }

    fn get_docid_map(&self, docid: &str) -> Result<Option<String>> {
        self.get_strstr(DOCID_MAP_TABLE, docid)
    }

    fn delete_docid_map(&self, docid: &str) -> Result<bool> {
        self.delete_strstr(DOCID_MAP_TABLE, docid)
    }

    // ============ Content Blob Operations ============

    fn put_content(&self, hash: &str, data: &[u8]) -> Result<()> {
        self.put_generic(CONTENT_TABLE, hash, data)
    }

    fn get_content(&self, hash: &str) -> Result<Option<Bytes>> {
        self.get_generic(CONTENT_TABLE, hash)
    }

    fn content_count(&self) -> Result<u64> {
        self.count_generic(CONTENT_TABLE)
    }

    // ============ Collection Operations ============

    fn put_collection(&self, name: &str, data: &[u8]) -> Result<()> {
        self.put_generic(COLLECTIONS_TABLE, name, data)
    }

    fn get_collection(&self, name: &str) -> Result<Option<Bytes>> {
        self.get_generic(COLLECTIONS_TABLE, name)
    }

    fn list_collections(&self) -> Result<Vec<(String, Bytes)>> {
        self.iter_generic(COLLECTIONS_TABLE)
    }

    // ============ Session Operations ============

    fn put_session(&self, id: &str, data: &str) -> Result<()> {
        self.put_strstr(SESSIONS_TABLE, id, data)
    }

    fn get_session(&self, id: &str) -> Result<Option<String>> {
        self.get_strstr(SESSIONS_TABLE, id)
    }

    fn delete_session(&self, id: &str) -> Result<bool> {
        self.delete_strstr(SESSIONS_TABLE, id)
    }

    fn list_sessions(&self) -> Result<Vec<(String, String)>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(SESSIONS_TABLE)?;
        let mut results = Vec::new();
        let mut iter = table.iter()?;
        while let Some(entry_res) = iter.next() {
            let (key, value) = entry_res?;
            results.push((key.value().to_string(), value.value().to_string()));
        }
        Ok(results)
    }

    // ============ FTS Index Operations ============

    fn put_fts_forward(&self, doc_key: &str, data: &[u8]) -> Result<()> {
        self.put_generic(FTS_FORWARD_TABLE, doc_key, data)
    }

    fn get_fts_forward(&self, doc_key: &str) -> Result<Option<Bytes>> {
        self.get_generic(FTS_FORWARD_TABLE, doc_key)
    }

    fn delete_fts_forward(&self, doc_key: &str) -> Result<bool> {
        self.delete_generic(FTS_FORWARD_TABLE, doc_key)
    }

    fn put_fts_inverted(&self, term: &str, data: &[u8]) -> Result<()> {
        self.put_generic(FTS_INVERTED_TABLE, term, data)
    }

    fn get_fts_inverted(&self, term: &str) -> Result<Option<Bytes>> {
        self.get_generic(FTS_INVERTED_TABLE, term)
    }

    fn delete_fts_inverted(&self, term: &str) -> Result<bool> {
        self.delete_generic(FTS_INVERTED_TABLE, term)
    }

    // ============ Vector Operations ============

    fn put_vector(&self, key: &str, data: &[u8]) -> Result<()> {
        self.put_generic(VECTORS_TABLE, key, data)
    }

    fn get_vector(&self, key: &str) -> Result<Option<Bytes>> {
        self.get_generic(VECTORS_TABLE, key)
    }

    fn delete_vector(&self, key: &str) -> Result<bool> {
        self.delete_generic(VECTORS_TABLE, key)
    }

    fn iter_vectors(&self) -> Result<Vec<(String, Bytes)>> {
        self.iter_generic(VECTORS_TABLE)
    }

    // ============ Embedding Cache Operations ============

    fn put_embedding_cache(&self, hash: &str, vector: &[f32]) -> Result<()> {
        let bytes: Vec<u8> = vector.iter().flat_map(|&f| f.to_le_bytes()).collect();
        self.put_generic(EMBEDDING_CACHE_TABLE, hash, &bytes)
    }

    fn get_embedding_cache(&self, hash: &str) -> Result<Option<Vec<f32>>> {
        let bytes = match self.get_generic(EMBEDDING_CACHE_TABLE, hash)? {
            Some(b) => b,
            None => return Ok(None),
        };

        if bytes.len() % 4 != 0 {
            return Err(EngramError::Storage("Invalid embedding cache size".into()));
        }

        let mut vector = Vec::with_capacity(bytes.len() / 4);
        for i in 0..(bytes.len() / 4) {
            let mut chunk = [0u8; 4];
            chunk.copy_from_slice(&bytes[i * 4..(i + 1) * 4]);
            vector.push(f32::from_le_bytes(chunk));
        }
        Ok(Some(vector))
    }

    // ============ Cognitive Experience Operations ============

    fn put_experience(&self, key: &str, data: &[u8]) -> Result<()> {
        self.put_generic(EXPERIENCES_TABLE, key, data)
    }

    fn get_experience(&self, key: &str) -> Result<Option<Bytes>> {
        self.get_generic(EXPERIENCES_TABLE, key)
    }

    fn delete_experience(&self, key: &str) -> Result<bool> {
        self.delete_generic(EXPERIENCES_TABLE, key)
    }

    fn iter_experiences(&self) -> Result<Vec<(String, Bytes)>> {
        self.iter_generic(EXPERIENCES_TABLE)
    }

    fn experience_count(&self) -> Result<u64> {
        self.count_generic(EXPERIENCES_TABLE)
    }

    // ============ Anti-Pattern Operations ============

    fn put_anti_pattern(&self, key: &str, data: &[u8]) -> Result<()> {
        self.put_generic(ANTI_PATTERNS_TABLE, key, data)
    }

    fn get_anti_pattern(&self, key: &str) -> Result<Option<Bytes>> {
        self.get_generic(ANTI_PATTERNS_TABLE, key)
    }

    fn delete_anti_pattern(&self, key: &str) -> Result<bool> {
        self.delete_generic(ANTI_PATTERNS_TABLE, key)
    }

    fn iter_anti_patterns(&self) -> Result<Vec<(String, Bytes)>> {
        self.iter_generic(ANTI_PATTERNS_TABLE)
    }

    fn anti_pattern_count(&self) -> Result<u64> {
        self.count_generic(ANTI_PATTERNS_TABLE)
    }

    // ============ Maintenance ============

    fn compact(&self) -> Result<()> {
        debug!("Compact requested (auto-handled by redb)");
        Ok(())
    }

    fn disk_usage(&self) -> Result<Option<u64>> {
        Ok(std::fs::metadata(&self.path).ok().map(|m| m.len()))
    }

    // ============ HNSW Index Operations ============
    fn put_idx(&self, id: &str, key: &str) -> Result<()> {
        self.put_strstr(HNSW_IDX_TABLE, id, key)
    }

    fn get_idx(&self, id: &str) -> Result<Option<String>> {
        self.get_strstr(HNSW_IDX_TABLE, id)
    }

    // ============ Knowledge Graph (Triple) Operations ============

    fn put_triple(&self, s: &str, p: &str, o: &str, metadata: &[u8]) -> Result<()> {
        let spo_key = format!("{}{}{}{}{}", s, SEPARATOR, p, SEPARATOR, o);
        let ops_key = format!("{}{}{}{}{}", o, SEPARATOR, p, SEPARATOR, s);
        let pos_key = format!("{}{}{}{}{}", p, SEPARATOR, o, SEPARATOR, s);
        let meta_vec = metadata.to_vec();

        let write_txn = self.db.begin_write()?;
        {
            let mut spo = write_txn.open_table(KG_SPO_TABLE)?;
            let mut ops = write_txn.open_table(KG_OPS_TABLE)?;
            let mut pos = write_txn.open_table(KG_POS_TABLE)?;

            spo.insert(spo_key, meta_vec.clone())?;
            ops.insert(ops_key, meta_vec.clone())?;
            pos.insert(pos_key, meta_vec)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    fn put_triples_batch(&self, triples: Vec<(String, String, String, Vec<u8>)>) -> Result<()> {
        let write_txn = self.db.begin_write()?;
        {
            let mut spo = write_txn.open_table(KG_SPO_TABLE)?;
            let mut ops = write_txn.open_table(KG_OPS_TABLE)?;
            let mut pos = write_txn.open_table(KG_POS_TABLE)?;

            for (s, p, o, meta) in triples {
                let spo_key = format!("{}{}{}{}{}", s, SEPARATOR, p, SEPARATOR, o);
                let ops_key = format!("{}{}{}{}{}", o, SEPARATOR, p, SEPARATOR, s);
                let pos_key = format!("{}{}{}{}{}", p, SEPARATOR, o, SEPARATOR, s);

                spo.insert(spo_key, meta.clone())?;
                ops.insert(ops_key, meta.clone())?;
                pos.insert(pos_key, meta)?;
            }
        }
        write_txn.commit()?;
        Ok(())
    }

    fn delete_triple(&self, s: &str, p: &str, o: &str) -> Result<bool> {
        let spo_key = format!("{}{}{}{}{}", s, SEPARATOR, p, SEPARATOR, o);
        let ops_key = format!("{}{}{}{}{}", o, SEPARATOR, p, SEPARATOR, s);
        let pos_key = format!("{}{}{}{}{}", p, SEPARATOR, o, SEPARATOR, s);

        let write_txn = self.db.begin_write()?;
        let removed = {
            let mut spo = write_txn.open_table(KG_SPO_TABLE)?;
            let mut ops = write_txn.open_table(KG_OPS_TABLE)?;
            let mut pos = write_txn.open_table(KG_POS_TABLE)?;

            ops.remove(ops_key)?;
            pos.remove(pos_key)?;
            let r = spo.remove(spo_key)?.is_some();
            r
        };
        write_txn.commit()?;
        Ok(removed)
    }

    fn query_triples(
        &self,
        s: Option<&str>,
        p: Option<&str>,
        o: Option<&str>,
    ) -> Result<Vec<(String, String, String, Bytes)>> {
        let read_txn = self.db.begin_read()?;

        let (table_name, prefix) = match (s, p, o) {
            (Some(subject), Some(predicate), _) => (
                KG_SPO_TABLE.name(),
                format!("{}{}{}{}", subject, SEPARATOR, predicate, SEPARATOR),
            ),
            (Some(subject), None, _) => (KG_SPO_TABLE.name(), format!("{}{}", subject, SEPARATOR)),
            (None, Some(predicate), Some(object)) => (
                KG_OPS_TABLE.name(),
                format!("{}{}{}{}", object, SEPARATOR, predicate, SEPARATOR),
            ),
            (None, None, Some(object)) => (KG_OPS_TABLE.name(), format!("{}{}", object, SEPARATOR)),
            (None, Some(predicate), None) => {
                (KG_POS_TABLE.name(), format!("{}{}", predicate, SEPARATOR))
            }
            (None, None, None) => (KG_SPO_TABLE.name(), String::new()),
        };

        let mut results = Vec::new();
        let end_range = format!("{}\u{10ffff}", prefix);

        // All KG tables have the same signature TableDefinition<String, Vec<u8>>
        // We open the specific table based on the name from strategy selection
        if table_name == KG_SPO_TABLE.name() {
            let table = read_txn.open_table(KG_SPO_TABLE)?;
            for item in table.range(prefix.clone()..end_range)? {
                let (key_handle, value_handle) = item?;
                self.process_triple_result(
                    KG_SPO_TABLE.name(),
                    key_handle.value(),
                    value_handle.value(),
                    s,
                    p,
                    o,
                    &mut results,
                );
            }
        } else if table_name == KG_OPS_TABLE.name() {
            let table = read_txn.open_table(KG_OPS_TABLE)?;
            for item in table.range(prefix.clone()..end_range)? {
                let (key_handle, value_handle) = item?;
                self.process_triple_result(
                    KG_OPS_TABLE.name(),
                    key_handle.value(),
                    value_handle.value(),
                    s,
                    p,
                    o,
                    &mut results,
                );
            }
        } else {
            let table = read_txn.open_table(KG_POS_TABLE)?;
            for item in table.range(prefix.clone()..end_range)? {
                let (key_handle, value_handle) = item?;
                self.process_triple_result(
                    KG_POS_TABLE.name(),
                    key_handle.value(),
                    value_handle.value(),
                    s,
                    p,
                    o,
                    &mut results,
                );
            }
        };

        Ok(results)
    }
}
