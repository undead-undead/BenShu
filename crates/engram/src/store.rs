//! Core storage engine for Engram
//!
//! Handles ACID-compliant storage for Documents, Collections, and Cognitive Assets.
//! Optimized for high-concurrency and fast metadata lookups.

use crate::content_hash::{get_docid, hash_content};
use crate::error::{EngramError, Result};
use crate::fts::FtsEngine;
use crate::metadata_contract::{
    document_event_audit_metadata, document_summary_metadata, pending_review_metadata,
    retention_archive_metadata, retention_prune_metadata, session_event_audit_metadata,
    session_prune_audit_metadata, AuditMetadataView, DocumentMetadataView, FactMetadataView,
    SessionAuditMetadataView, AUDIT_KIND_DOCUMENT_AUTO_ARCHIVED, AUDIT_KIND_DOCUMENT_PRUNED,
    AUDIT_KIND_SESSION_ARCHIVED, AUDIT_KIND_SESSION_PRUNED, AUDIT_KIND_SESSION_RECOVERED,
    COLLECTION_DOCUMENT_AUDIT, COLLECTION_SESSION_AUDIT, DOCUMENT_LIFECYCLE_ACTIVE,
    DOCUMENT_LIFECYCLE_ARCHIVED, DOCUMENT_LIFECYCLE_PRUNED, FACT_LIFECYCLE_ARCHIVED,
    FACT_LIFECYCLE_PRUNED, META_DOCUMENT_ARCHIVED_AT_MS, META_DOCUMENT_COLLECTION,
    META_DOCUMENT_ID, META_DOCUMENT_LIFECYCLE_STATE, META_DOCUMENT_PATH,
    META_DOCUMENT_RETENTION_POLICY_VERSION, META_DOCUMENT_UPDATED_AT_MS, META_FACT_LIFECYCLE_STATE,
    META_FACT_STATUS, META_SESSION_ARCHIVED_AT_MS, META_SESSION_ARCHIVE_REASON,
    META_SESSION_BACKGROUND_LIFECYCLE_STATE, META_SESSION_BACKGROUND_PRESENT,
    META_SESSION_BACKGROUND_REVISION, META_SESSION_ID, META_SESSION_LAST_RECOVERED_AT_MS,
    META_SESSION_LIFECYCLE_STATE, META_SESSION_RECOVERED_FROM, META_SESSION_RETENTION_UNTIL_MS,
    META_SESSION_UPDATED_AT_MS, RETENTION_REASON_ARCHIVED, RETENTION_REASON_UNVERIFIED,
    SESSION_LIFECYCLE_ACTIVE, SESSION_LIFECYCLE_ARCHIVED,
};
use crate::storage::redb_impl::EngramKV;
use crate::storage::Storage;
use benshu_protocol_core::AgentSession;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tracing::{info, trace, warn};

const RETENTION_POLICY_KEY: &str = "meta:engram:retention_policy";
const RETENTION_LAST_RUN_KEY: &str = "meta:engram:retention_last_run";

fn default_retention_policy_version() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetentionPruningPolicy {
    #[serde(default = "default_retention_policy_version")]
    pub contract_version: u32,
    pub session_prune_after_days: i64,
    pub require_archived_for_session_prune: bool,
    pub unverified_document_archive_after_days: i64,
    pub archived_document_prune_after_days: i64,
}

impl Default for RetentionPruningPolicy {
    fn default() -> Self {
        Self {
            contract_version: default_retention_policy_version(),
            session_prune_after_days: 30,
            require_archived_for_session_prune: true,
            unverified_document_archive_after_days: 14,
            archived_document_prune_after_days: 90,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct RetentionRunReport {
    pub run_at_ms: i64,
    pub contract_version: u32,
    pub pruned_sessions: u64,
    pub auto_archived_documents: u64,
    pub pruned_documents: u64,
}

/// High-performance Document structure for RAG
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Document {
    /// O(1) Unique ID (short hash)
    pub docid: String,
    pub collection: String,
    /// Virtual path within the collection
    pub path: String,
    pub title: String,
    /// Optional summary created by the cognitive distiller
    pub summary: Option<String>,
    /// Tiered context for hierarchical retrieval
    pub abstract_content: Option<String>,
    pub overview_content: Option<String>,
    /// Content fingerprint for deduplication
    pub content_hash: String,
    /// Timestamps stored as Unix milliseconds for performance
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    /// Phase 12-B: Quarantine status
    pub unverified: bool,
    /// Phase 14: Causal Efficiency (0.0 - 1.0)
    pub utility_score: f32,
    /// Flexible extensions (tags, relations, etc.)
    pub metadata: HashMap<String, String>,
}

impl Document {
    pub fn is_structural(&self) -> bool {
        self.abstract_content.is_some() || self.overview_content.is_some()
    }

    pub fn age_seconds(&self) -> u64 {
        let now = Utc::now().timestamp_millis();
        (now.saturating_sub(self.created_at_ms) / 1000) as u64
    }
}

/// A proven golden path for task execution
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Experience {
    pub id: String,
    pub task_query: String,
    pub tool_calls: Vec<serde_json::Value>,
    pub success_score: f32,
    pub utility_score: f32,
    pub created_at_ms: i64,
    pub last_used_at_ms: i64,
    pub metadata: HashMap<String, String>,
}

/// A recorded failure pattern to be avoided
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AntiPattern {
    pub id: String,
    pub error_fingerprint: String,
    pub root_cause: String,
    pub correction: String,
    pub utility_score: f32,
    pub created_at_ms: i64,
    pub metadata: HashMap<String, String>,
}

/// Search match including snippet and score
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub document: Document,
    pub score: f64,
    pub snippet: Option<String>,
}

/// Logical grouping of documents
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Collection {
    pub name: String,
    pub description: Option<String>,
    pub document_count: u64,
    pub created_at_ms: i64,
}

/// Runtime statistics for the dashboard
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StoreStats {
    pub total_documents: u64,
    pub total_collections: usize,
    pub total_unverified: u64,
    pub total_experiences: u64,
    pub total_anti_patterns: u64,
    pub disk_usage_bytes: u64,
}

/// Storage configuration
#[derive(Debug, Clone)]
pub struct EngramStoreConfig {
    pub max_content_size: usize,
    pub enable_fts: bool,
    pub auto_vacuum_on_start: bool,
}

impl Default for EngramStoreConfig {
    fn default() -> Self {
        Self {
            max_content_size: 20 * 1024 * 1024, // 20MB
            enable_fts: true,
            auto_vacuum_on_start: false,
        }
    }
}

/// Core storage coordinator
pub struct EngramStore {
    kv: Arc<dyn Storage>,
    config: EngramStoreConfig,
    fts: Arc<FtsEngine>,
    retention_policy: RwLock<RetentionPruningPolicy>,
    last_retention_report: RwLock<Option<RetentionRunReport>>,
    path: PathBuf,
}

impl EngramStore {
    fn session_background_prune_protected(
        session: &AgentSession,
        now: chrono::DateTime<chrono::Utc>,
    ) -> bool {
        let Some(background) = session.background_envelope.as_ref() else {
            return false;
        };

        let lifecycle_state = background
            .metadata
            .get("background_session_lifecycle_state")
            .map(String::as_str)
            .unwrap_or(SESSION_LIFECYCLE_ACTIVE);
        if !matches!(lifecycle_state, SESSION_LIFECYCLE_ARCHIVED) {
            return true;
        }

        background
            .metadata
            .get("background_session_retention_until_ms")
            .and_then(|value| value.parse::<i64>().ok())
            .map(|deadline| deadline > now.timestamp_millis())
            .unwrap_or(false)
    }

    fn session_lifecycle_state(session: &AgentSession) -> &'static str {
        if session.is_archived() {
            SESSION_LIFECYCLE_ARCHIVED
        } else {
            SESSION_LIFECYCLE_ACTIVE
        }
    }

    fn document_lifecycle_state(document: &Document) -> String {
        document
            .metadata
            .get(META_DOCUMENT_LIFECYCLE_STATE)
            .cloned()
            .unwrap_or_else(|| DOCUMENT_LIFECYCLE_ACTIVE.to_string())
    }

    fn insert_session_audit_context(
        metadata: &mut HashMap<String, String>,
        session: &AgentSession,
    ) {
        metadata.insert(META_SESSION_ID.to_string(), session.id.clone());
        metadata.insert(
            META_SESSION_LIFECYCLE_STATE.to_string(),
            Self::session_lifecycle_state(session).to_string(),
        );
        metadata.insert(
            META_SESSION_UPDATED_AT_MS.to_string(),
            session.updated_at.timestamp_millis().to_string(),
        );
        if let Some(archived_at) = session.lifecycle.archived_at {
            metadata.insert(
                META_SESSION_ARCHIVED_AT_MS.to_string(),
                archived_at.timestamp_millis().to_string(),
            );
        }
        if let Some(retention_until) = session.lifecycle.retention_until {
            metadata.insert(
                META_SESSION_RETENTION_UNTIL_MS.to_string(),
                retention_until.timestamp_millis().to_string(),
            );
        }
        if let Some(archive_reason) = session.lifecycle.archive_reason.clone() {
            metadata.insert(META_SESSION_ARCHIVE_REASON.to_string(), archive_reason);
        }
        if let Some(recovered_from) = session.lifecycle.recovered_from.clone() {
            metadata.insert(META_SESSION_RECOVERED_FROM.to_string(), recovered_from);
        }
        if let Some(last_recovered_at) = session.lifecycle.last_recovered_at {
            metadata.insert(
                META_SESSION_LAST_RECOVERED_AT_MS.to_string(),
                last_recovered_at.timestamp_millis().to_string(),
            );
        }
        metadata.insert(
            META_SESSION_BACKGROUND_PRESENT.to_string(),
            session.background_envelope.is_some().to_string(),
        );
        if let Some(background) = session.background_envelope.as_ref() {
            metadata.insert(
                META_SESSION_BACKGROUND_REVISION.to_string(),
                background.revision.revision.to_string(),
            );
            if let Some(lifecycle_state) = background
                .metadata
                .get("background_session_lifecycle_state")
            {
                metadata.insert(
                    META_SESSION_BACKGROUND_LIFECYCLE_STATE.to_string(),
                    lifecycle_state.clone(),
                );
            }
        }
    }

    fn insert_document_audit_context(metadata: &mut HashMap<String, String>, document: &Document) {
        metadata.insert(META_DOCUMENT_ID.to_string(), document.docid.clone());
        metadata.insert(
            META_DOCUMENT_COLLECTION.to_string(),
            document.collection.clone(),
        );
        metadata.insert(META_DOCUMENT_PATH.to_string(), document.path.clone());
        metadata.insert(
            META_DOCUMENT_LIFECYCLE_STATE.to_string(),
            Self::document_lifecycle_state(document),
        );
        metadata.insert(
            META_DOCUMENT_UPDATED_AT_MS.to_string(),
            document.updated_at_ms.to_string(),
        );
    }

    fn load_retention_policy(kv: &Arc<dyn Storage>) -> Result<RetentionPruningPolicy> {
        if let Some(raw) = kv.get_collection(RETENTION_POLICY_KEY)? {
            let policy = bincode::deserialize::<RetentionPruningPolicy>(&raw)
                .map_err(|e| EngramError::Serialization(e.to_string()))?;
            return Ok(policy);
        }
        Ok(RetentionPruningPolicy::default())
    }

    fn load_retention_report(kv: &Arc<dyn Storage>) -> Result<Option<RetentionRunReport>> {
        let Some(raw) = kv.get_collection(RETENTION_LAST_RUN_KEY)? else {
            return Ok(None);
        };
        let report = bincode::deserialize::<RetentionRunReport>(&raw)
            .map_err(|e| EngramError::Serialization(e.to_string()))?;
        Ok(Some(report))
    }

    fn persist_retention_policy(&self) -> Result<()> {
        let policy = self
            .retention_policy
            .read()
            .expect("retention policy poisoned");
        let data =
            bincode::serialize(&*policy).map_err(|e| EngramError::Serialization(e.to_string()))?;
        self.kv.put_collection(RETENTION_POLICY_KEY, &data)
    }

    fn persist_retention_report(&self, report: &RetentionRunReport) -> Result<()> {
        let data =
            bincode::serialize(report).map_err(|e| EngramError::Serialization(e.to_string()))?;
        self.kv.put_collection(RETENTION_LAST_RUN_KEY, &data)
    }

    fn document_is_archived(doc: &Document) -> bool {
        doc.metadata
            .get(META_DOCUMENT_LIFECYCLE_STATE)
            .map(|value| {
                matches!(
                    value.as_str(),
                    DOCUMENT_LIFECYCLE_ARCHIVED | DOCUMENT_LIFECYCLE_PRUNED
                )
            })
            .unwrap_or(false)
            || doc
                .metadata
                .get(META_FACT_LIFECYCLE_STATE)
                .map(|value| {
                    matches!(
                        value.as_str(),
                        FACT_LIFECYCLE_ARCHIVED | FACT_LIFECYCLE_PRUNED
                    )
                })
                .unwrap_or(false)
            || doc
                .metadata
                .get(META_FACT_STATUS)
                .map(|value| value.contains("archived"))
                .unwrap_or(false)
    }

    fn record_session_prune_audit(
        &self,
        session: &AgentSession,
        reason: &str,
        pruned_at_ms: i64,
    ) -> Result<()> {
        let mut metadata = session_prune_audit_metadata(reason, pruned_at_ms);
        Self::insert_session_audit_context(&mut metadata, session);

        let path = format!("prune/{}/{}", session.id, pruned_at_ms);
        let body = format!(
            "Session {} pruned from engram durable storage. reason={} archived={} updated_at_ms={}",
            session.id,
            reason,
            session.is_archived(),
            session.updated_at.timestamp_millis()
        );
        self.store_document(
            COLLECTION_SESSION_AUDIT,
            &path,
            &format!("Session prune audit: {}", session.id),
            &body,
            false,
            metadata,
        )?;
        Ok(())
    }

    pub fn record_session_audit(
        &self,
        session: &AgentSession,
        audit_kind: &str,
        audit_reason: Option<&str>,
        event_at_ms: i64,
        mut metadata: HashMap<String, String>,
    ) -> Result<()> {
        metadata.extend(session_event_audit_metadata(
            audit_kind,
            audit_reason,
            event_at_ms,
        ));
        Self::insert_session_audit_context(&mut metadata, session);

        let path = format!("{}/{}/{}", audit_kind, session.id, event_at_ms);
        let body = format!(
            "Session {} lifecycle event recorded by engram. kind={} archived={} updated_at_ms={}",
            session.id,
            audit_kind,
            session.is_archived(),
            session.updated_at.timestamp_millis()
        );
        self.store_document(
            COLLECTION_SESSION_AUDIT,
            &path,
            &format!("Session {} audit: {}", audit_kind, session.id),
            &body,
            false,
            metadata,
        )?;
        Ok(())
    }

    fn record_document_audit(
        &self,
        document: &Document,
        audit_kind: &str,
        audit_reason: &str,
        event_at_ms: i64,
        mut metadata: HashMap<String, String>,
    ) -> Result<()> {
        metadata.extend(document_event_audit_metadata(
            audit_kind,
            audit_reason,
            event_at_ms,
        ));
        Self::insert_document_audit_context(&mut metadata, document);

        let path = format!("{}/{}/{}", audit_kind, document.docid, event_at_ms);
        let body = format!(
            "Document {} lifecycle event recorded by engram. kind={} collection={} path={}",
            document.docid, audit_kind, document.collection, document.path
        );
        self.store_document(
            COLLECTION_DOCUMENT_AUDIT,
            &path,
            &format!("Document {} audit: {}", audit_kind, document.docid),
            &body,
            false,
            metadata,
        )?;
        Ok(())
    }

    fn session_audit_counts(&self) -> Result<(u64, u64, u64, u64)> {
        let all = self.kv.iter_documents()?;
        let mut archive_count = 0u64;
        let mut recovery_count = 0u64;
        let mut background_archive_count = 0u64;
        let mut background_recovery_count = 0u64;
        for (_, data) in all {
            if let Ok(doc) = bincode::deserialize::<Document>(&data) {
                if doc.collection != COLLECTION_SESSION_AUDIT {
                    continue;
                }
                let audit = SessionAuditMetadataView::new(&doc.metadata);
                match audit.audit_kind() {
                    Some(AUDIT_KIND_SESSION_ARCHIVED) => {
                        archive_count += 1;
                        if audit.background_present() == Some("true") {
                            background_archive_count += 1;
                        }
                    }
                    Some(AUDIT_KIND_SESSION_RECOVERED) => {
                        recovery_count += 1;
                        if audit.background_present() == Some("true") {
                            background_recovery_count += 1;
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok((
            archive_count,
            recovery_count,
            background_archive_count,
            background_recovery_count,
        ))
    }

    pub fn latest_session_audit_metadata(
        &self,
        session_id: &str,
    ) -> Result<Option<HashMap<String, String>>> {
        let all = self.kv.iter_documents()?;
        let mut latest: Option<(i64, HashMap<String, String>)> = None;

        for (_, data) in all {
            let Ok(doc) = bincode::deserialize::<Document>(&data) else {
                continue;
            };
            if doc.collection != COLLECTION_SESSION_AUDIT {
                continue;
            }

            let audit = SessionAuditMetadataView::new(&doc.metadata);
            if audit.session_id() != Some(session_id) {
                continue;
            }

            let event_at_ms = audit.event_at_ms().unwrap_or(doc.updated_at_ms);
            match latest.as_ref() {
                Some((current, _)) if *current >= event_at_ms => {}
                _ => latest = Some((event_at_ms, doc.metadata)),
            }
        }

        Ok(latest.map(|(_, metadata)| metadata))
    }

    fn prune_counts_by_reason(&self) -> Result<BTreeMap<String, u64>> {
        let all = self.kv.iter_documents()?;
        let mut counts = BTreeMap::new();
        for (_, data) in all {
            if let Ok(doc) = bincode::deserialize::<Document>(&data) {
                if doc.collection == COLLECTION_SESSION_AUDIT
                    && AuditMetadataView::new(&doc.metadata).audit_kind()
                        == Some(AUDIT_KIND_SESSION_PRUNED)
                {
                    let reason = SessionAuditMetadataView::new(&doc.metadata)
                        .prune_reason()
                        .unwrap_or("unknown")
                        .to_string();
                    *counts.entry(reason).or_insert(0) += 1;
                }

                if let Some(reason) = FactMetadataView::new(
                    &doc.metadata,
                    doc.unverified,
                    doc.created_at_ms,
                    doc.updated_at_ms,
                )
                .prune_reason()
                .map(str::to_string)
                {
                    *counts.entry(reason).or_insert(0) += 1;
                }

                if doc.collection == COLLECTION_DOCUMENT_AUDIT
                    && AuditMetadataView::new(&doc.metadata).audit_kind()
                        == Some(AUDIT_KIND_DOCUMENT_PRUNED)
                {
                    let reason = AuditMetadataView::new(&doc.metadata)
                        .reason()
                        .unwrap_or("unknown")
                        .to_string();
                    *counts.entry(reason).or_insert(0) += 1;
                }
            }
        }
        Ok(counts)
    }

    pub fn fetch_all_docs_legacy(&self) -> Result<Vec<Document>> {
        let all = self.kv.iter_documents()?;
        let mut results = Vec::new();
        for (_, data) in all {
            if let Ok(doc) = bincode::deserialize::<Document>(&data) {
                results.push(doc);
            }
        }
        Ok(results)
    }

    pub fn new(db_path: impl Into<PathBuf>) -> Result<Self> {
        let path = db_path.into();
        let kv: Arc<dyn Storage> = Arc::new(EngramKV::open(&path)?);
        let fts = Arc::new(FtsEngine::new(kv.clone()));
        let retention_policy = Self::load_retention_policy(&kv)?;
        let last_retention_report = Self::load_retention_report(&kv)?;

        let store = Self {
            kv,
            config: EngramStoreConfig::default(),
            fts,
            retention_policy: RwLock::new(retention_policy),
            last_retention_report: RwLock::new(last_retention_report),
            path,
        };
        store.persist_retention_policy()?;
        Ok(store)
    }

    /// Optimized: Store a new document or update existing one
    pub fn store_document(
        &self,
        collection_name: &str,
        path: &str,
        title: &str,
        body: &str,
        unverified: bool,
        metadata: HashMap<String, String>,
    ) -> Result<Document> {
        if body.len() > self.config.max_content_size {
            return Err(EngramError::ContentTooLarge {
                size: body.len(),
                max: self.config.max_content_size,
            });
        }

        let now = Utc::now().timestamp_millis();
        let content_hash = hash_content(body);
        let docid = get_docid(&content_hash);

        // 1. Persist Raw Content (CAS - Content Addressable Storage)
        self.kv.put_content(&content_hash, body.as_bytes())?;

        // 2. Build Document Metadata
        let mut metadata = metadata;
        metadata
            .entry(META_DOCUMENT_LIFECYCLE_STATE.to_string())
            .or_insert_with(|| DOCUMENT_LIFECYCLE_ACTIVE.to_string());
        let mut doc = Document {
            docid: docid.clone(),
            collection: collection_name.to_string(),
            path: path.to_string(),
            title: title.to_string(),
            summary: None,
            abstract_content: None,
            overview_content: None,
            content_hash,
            created_at_ms: now,
            updated_at_ms: now,
            unverified,
            utility_score: 0.0,
            metadata,
        };

        // If updating, preserve creation time
        let doc_key = format!("{}:{}", collection_name, path);
        if let Ok(Some(existing)) = self.get_by_doc_key(&doc_key) {
            doc.created_at_ms = existing.created_at_ms;
            doc.summary = existing.summary;
            doc.abstract_content = existing.abstract_content;
            doc.overview_content = existing.overview_content;
            doc.utility_score = existing.utility_score;
        }

        // 3. Persist Metadata & Indices
        let data =
            bincode::serialize(&doc).map_err(|e| EngramError::Serialization(e.to_string()))?;
        self.kv.put_document(&doc_key, &data)?;
        self.kv.put_docid_map(&docid, &doc_key)?;

        // 4. Update FTS Index
        if self.config.enable_fts {
            self.fts.index_document(&doc_key, body)?;
        }

        trace!("Stored document: {} (docid: {})", doc_key, docid);
        Ok(doc)
    }

    pub fn get_by_path(&self, collection: &str, path: &str) -> Result<Option<Document>> {
        let doc_key = format!("{}:{}", collection, path);
        self.get_by_doc_key(&doc_key)
    }

    pub fn list_documents_in_collection(&self, collection: &str) -> Result<Vec<Document>> {
        let all = self.kv.iter_documents()?;
        let mut results = Vec::new();
        for (_, data) in all {
            if let Ok(doc) = bincode::deserialize::<Document>(&data) {
                if doc.collection == collection {
                    results.push(doc);
                }
            }
        }
        Ok(results)
    }

    pub fn list_documents(&self) -> Result<Vec<Document>> {
        let all = self.kv.iter_documents()?;
        let mut results = Vec::new();
        for (_, data) in all {
            if let Ok(doc) = bincode::deserialize::<Document>(&data) {
                results.push(doc);
            }
        }
        Ok(results)
    }

    pub fn get_by_docid(&self, docid: &str) -> Result<Option<Document>> {
        match self.kv.get_docid_map(docid)? {
            Some(key) => self.get_by_doc_key(&key),
            None => Ok(None),
        }
    }

    fn get_by_doc_key(&self, key: &str) -> Result<Option<Document>> {
        match self.kv.get_document(key)? {
            Some(data) => {
                let doc: Document = bincode::deserialize(&data)
                    .map_err(|e| EngramError::Serialization(e.to_string()))?;
                Ok(Some(doc))
            }
            None => Ok(None),
        }
    }

    /// Retrieve raw text content for a document
    pub fn get_content(&self, doc: &Document) -> Result<Option<String>> {
        match self.kv.get_content(&doc.content_hash)? {
            Some(bytes) => {
                let s = String::from_utf8(bytes.to_vec())
                    .map_err(|e| EngramError::Internal(format!("UTF-8 error: {}", e)))?;
                Ok(Some(s))
            }
            None => Ok(None),
        }
    }

    /// Search results with automatic verification filtering
    pub fn search_fts(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let fts_results = self.fts.search(query, 1000, limit * 2)?;
        let mut results = Vec::with_capacity(fts_results.len());

        for res in fts_results {
            if let Some(doc) = self.get_by_doc_key(&res.doc_key)? {
                // Production skip unverified
                if doc.unverified || Self::document_is_archived(&doc) {
                    continue;
                }

                results.push(SearchResult {
                    document: doc,
                    score: res.score,
                    snippet: None,
                });

                if results.len() >= limit {
                    break;
                }
            }
        }
        Ok(results)
    }

    pub fn search_fts_in_collection(
        &self,
        query: &str,
        collection: &str,
        limit: usize,
    ) -> Result<Vec<SearchResult>> {
        let fts_results = self.fts.search(query, 1000, limit * 4)?;
        let mut results = Vec::new();

        for res in fts_results {
            if let Some(doc) = self.get_by_doc_key(&res.doc_key)? {
                if doc.collection == collection
                    && !doc.unverified
                    && !Self::document_is_archived(&doc)
                {
                    results.push(SearchResult {
                        document: doc,
                        score: res.score,
                        snippet: None,
                    });
                }
            }
            if results.len() >= limit {
                break;
            }
        }
        Ok(results)
    }

    pub fn search_fts_with_path(
        &self,
        query: &str,
        collection: &str,
        path: &str,
        limit: usize,
    ) -> Result<Vec<SearchResult>> {
        let fts_results = self.fts.search(query, 1000, limit * 10)?;
        let mut results = Vec::new();

        for res in fts_results {
            if let Some(doc) = self.get_by_doc_key(&res.doc_key)? {
                if doc.collection == collection
                    && doc.path.starts_with(path)
                    && !doc.unverified
                    && !Self::document_is_archived(&doc)
                {
                    results.push(SearchResult {
                        document: doc,
                        score: res.score,
                        snippet: None,
                    });
                }
            }
            if results.len() >= limit {
                break;
            }
        }
        Ok(results)
    }

    pub fn create_collection(&self, name: &str) -> Result<()> {
        self.kv.put_collection(name, &[])
    }

    /// Optimized: Count unverified by leveraging specialized index if available,
    /// otherwise defaulting to a safe iterator.
    pub fn count_unverified(&self) -> Result<u64> {
        // In this architecture, redb iteration is reasonably fast but we should ideally
        // have a dedicated 'unverified' table in redb_impl.
        let mut count = 0;
        for (_k, v) in self.kv.iter_documents()? {
            if let Ok(doc) = bincode::deserialize::<Document>(&v) {
                if doc.unverified {
                    count += 1;
                }
            }
        }
        Ok(count)
    }

    /// Maintenance: Reclaim disk space
    pub fn vacuum(&self) -> Result<()> {
        info!("Starting database compaction (Vacuum)...");
        self.kv.compact()?;
        Ok(())
    }

    pub fn delete_document(&self, collection: &str, path: &str) -> Result<()> {
        let doc_key = format!("{}:{}", collection, path);
        if let Some(doc) = self.get_by_path(collection, path)? {
            self.kv.delete_docid_map(&doc.docid)?;
        }
        self.kv.delete_document(&doc_key)?;
        let _ = self.fts.delete_document(&doc_key);
        Ok(())
    }

    /// Verification: Mark a document as verified (Phase 12)
    pub fn mark_verified(&self, collection: &str, path: &str) -> Result<()> {
        let mut doc = self
            .get_by_path(collection, path)?
            .ok_or_else(|| EngramError::NotFound(format!("{}:{}", collection, path)))?;

        doc.unverified = false;
        doc.updated_at_ms = Utc::now().timestamp_millis();

        let data = bincode::serialize(&doc)?;
        let key = format!("{}:{}", collection, path);
        self.kv.put_document(&key, &data)?;
        Ok(())
    }

    pub fn mark_pending_review(
        &self,
        collection: &str,
        path: &str,
        summary: Option<&str>,
    ) -> Result<()> {
        let mut doc = self
            .get_by_path(collection, path)?
            .ok_or_else(|| EngramError::NotFound(format!("{}:{}", collection, path)))?;

        doc.unverified = true;
        doc.updated_at_ms = Utc::now().timestamp_millis();
        for (key, value) in pending_review_metadata(doc.updated_at_ms, summary) {
            doc.metadata.insert(key, value);
        }

        let data = bincode::serialize(&doc)?;
        let key = format!("{}:{}", collection, path);
        self.kv.put_document(&key, &data)?;
        Ok(())
    }

    pub fn update_summary(&self, collection: &str, path: &str, summary: String) -> Result<()> {
        let mut doc = self
            .get_by_path(collection, path)?
            .ok_or_else(|| EngramError::NotFound(format!("{}:{}", collection, path)))?;

        doc.summary = Some(summary);
        doc.updated_at_ms = Utc::now().timestamp_millis();
        for (key, value) in document_summary_metadata(doc.updated_at_ms) {
            doc.metadata.insert(key, value);
        }

        let data = bincode::serialize(&doc)?;
        let key = format!("{}:{}", collection, path);
        self.kv.put_document(&key, &data)?;
        Ok(())
    }

    pub fn merge_metadata(
        &self,
        collection: &str,
        path: &str,
        metadata: HashMap<String, String>,
    ) -> Result<()> {
        let mut doc = self
            .get_by_path(collection, path)?
            .ok_or_else(|| EngramError::NotFound(format!("{}:{}", collection, path)))?;

        for (key, value) in metadata {
            doc.metadata.insert(key, value);
        }
        doc.updated_at_ms = Utc::now().timestamp_millis();

        let data = bincode::serialize(&doc)?;
        let key = format!("{}:{}", collection, path);
        self.kv.put_document(&key, &data)?;
        Ok(())
    }

    pub fn archive_document(
        &self,
        collection: &str,
        path: &str,
        mut metadata: HashMap<String, String>,
    ) -> Result<()> {
        metadata
            .entry(META_DOCUMENT_LIFECYCLE_STATE.to_string())
            .or_insert_with(|| DOCUMENT_LIFECYCLE_ARCHIVED.to_string());
        metadata
            .entry(META_DOCUMENT_ARCHIVED_AT_MS.to_string())
            .or_insert_with(|| Utc::now().timestamp_millis().to_string());
        self.merge_metadata(collection, path, metadata)
    }

    pub fn retention_policy(&self) -> RetentionPruningPolicy {
        self.retention_policy
            .read()
            .expect("retention policy poisoned")
            .clone()
    }

    pub fn set_retention_policy(&self, policy: RetentionPruningPolicy) -> Result<()> {
        *self
            .retention_policy
            .write()
            .expect("retention policy poisoned") = policy;
        self.persist_retention_policy()
    }

    pub fn last_retention_report(&self) -> Option<RetentionRunReport> {
        self.last_retention_report
            .read()
            .expect("retention report poisoned")
            .clone()
    }

    /// Phase 14: Update utility score for a document
    pub fn update_utility(&self, docid: &str, increment: f32) -> Result<()> {
        let key = self
            .kv
            .get_docid_map(docid)?
            .ok_or_else(|| EngramError::NotFound(docid.to_string()))?;

        let mut document = self
            .get_by_doc_key(&key)?
            .ok_or_else(|| EngramError::NotFound(key.clone()))?;

        document.utility_score = (document.utility_score + increment).clamp(0.0, 1.0);
        document.updated_at_ms = Utc::now().timestamp_millis();

        let data = bincode::serialize(&document)?;
        self.kv.put_document(&key, &data)?;
        Ok(())
    }

    fn prune_sessions_with_policy(
        &self,
        now: chrono::DateTime<chrono::Utc>,
        policy: &RetentionPruningPolicy,
    ) -> Result<u64> {
        let threshold =
            now.timestamp_millis() - (policy.session_prune_after_days * 24 * 60 * 60 * 1000);
        let mut count = 0u64;
        let sessions = self.kv.list_sessions().unwrap_or_default();

        for (id, data) in sessions {
            let Ok(session) = serde_json::from_str::<AgentSession>(&data) else {
                warn!(
                    "Skipping stale-session pruning for '{}' because the stored payload could not be parsed",
                    id
                );
                continue;
            };

            let stale_by_age = session.updated_at.timestamp_millis() <= threshold;
            let stale_by_retention = session.retention_expired_at(now);
            let archived_requirement_met =
                !policy.require_archived_for_session_prune || session.is_archived();
            let background_prune_protected =
                Self::session_background_prune_protected(&session, now);
            if archived_requirement_met
                && !background_prune_protected
                && (stale_by_retention || stale_by_age)
            {
                let pruned_at_ms = now.timestamp_millis();
                let reason = if stale_by_retention {
                    "retention_expired"
                } else {
                    "archived_older_than_threshold"
                };
                self.record_session_prune_audit(&session, reason, pruned_at_ms)?;
                if self.kv.delete_session(&id)? {
                    count += 1;
                }
            } else {
                trace!(
                    "Retaining session '{}' during stale-session pruning; archived={} stale_by_age={} stale_by_retention={} archived_requirement_met={} background_prune_protected={}",
                    id,
                    session.is_archived(),
                    stale_by_age,
                    stale_by_retention,
                    archived_requirement_met,
                    background_prune_protected
                );
            }
        }

        Ok(count)
    }

    fn apply_document_retention_policy(
        &self,
        now: chrono::DateTime<chrono::Utc>,
        policy: &RetentionPruningPolicy,
    ) -> Result<(u64, u64)> {
        let mut auto_archived_documents = 0u64;
        let mut pruned_documents = 0u64;
        let archive_threshold = now.timestamp_millis()
            - (policy.unverified_document_archive_after_days * 24 * 60 * 60 * 1000);
        let prune_threshold = now.timestamp_millis()
            - (policy.archived_document_prune_after_days * 24 * 60 * 60 * 1000);

        for (_, data) in self.kv.iter_documents()? {
            let Ok(document) = bincode::deserialize::<Document>(&data) else {
                continue;
            };
            if matches!(
                document.collection.as_str(),
                COLLECTION_SESSION_AUDIT | COLLECTION_DOCUMENT_AUDIT
            ) {
                continue;
            }

            let key = format!("{}:{}", document.collection, document.path);
            let lifecycle_state = document
                .metadata
                .get(META_DOCUMENT_LIFECYCLE_STATE)
                .map(String::as_str)
                .unwrap_or(DOCUMENT_LIFECYCLE_ACTIVE);
            let is_pruned = matches!(lifecycle_state, DOCUMENT_LIFECYCLE_PRUNED)
                || matches!(
                    document
                        .metadata
                        .get(META_FACT_LIFECYCLE_STATE)
                        .map(String::as_str),
                    Some(FACT_LIFECYCLE_PRUNED)
                );
            if is_pruned {
                continue;
            }

            if document.unverified
                && !Self::document_is_archived(&document)
                && document.updated_at_ms <= archive_threshold
            {
                let event_at_ms = now.timestamp_millis();
                let metadata = retention_archive_metadata(
                    policy.contract_version,
                    RETENTION_REASON_UNVERIFIED,
                );
                self.archive_document(&document.collection, &document.path, metadata)?;
                let updated = self
                    .get_by_doc_key(&key)?
                    .ok_or_else(|| EngramError::NotFound(key.clone()))?;
                let mut audit_metadata = HashMap::new();
                audit_metadata.insert(
                    META_DOCUMENT_RETENTION_POLICY_VERSION.to_string(),
                    policy.contract_version.to_string(),
                );
                self.record_document_audit(
                    &updated,
                    AUDIT_KIND_DOCUMENT_AUTO_ARCHIVED,
                    RETENTION_REASON_UNVERIFIED,
                    event_at_ms,
                    audit_metadata,
                )?;
                auto_archived_documents += 1;
                continue;
            }

            if Self::document_is_archived(&document) {
                let document_view = DocumentMetadataView::new(
                    &document.metadata,
                    document.summary.is_some(),
                    document.is_structural(),
                );
                let archived_at_ms = document_view.archived_at_ms(document.updated_at_ms);
                if archived_at_ms <= prune_threshold {
                    let event_at_ms = now.timestamp_millis();
                    let metadata = retention_prune_metadata(
                        policy.contract_version,
                        event_at_ms,
                        document_view.has_fact_contract_metadata(),
                        RETENTION_REASON_ARCHIVED,
                    );
                    self.merge_metadata(&document.collection, &document.path, metadata)?;
                    let _ = self.fts.delete_document(&key);
                    let updated = self
                        .get_by_doc_key(&key)?
                        .ok_or_else(|| EngramError::NotFound(key.clone()))?;
                    let mut audit_metadata = HashMap::new();
                    audit_metadata.insert(
                        META_DOCUMENT_RETENTION_POLICY_VERSION.to_string(),
                        policy.contract_version.to_string(),
                    );
                    self.record_document_audit(
                        &updated,
                        AUDIT_KIND_DOCUMENT_PRUNED,
                        RETENTION_REASON_ARCHIVED,
                        event_at_ms,
                        audit_metadata,
                    )?;
                    pruned_documents += 1;
                }
            }
        }

        Ok((auto_archived_documents, pruned_documents))
    }

    pub fn apply_retention_policy(&self) -> Result<RetentionRunReport> {
        let policy = self.retention_policy();
        let now = Utc::now();
        let pruned_sessions = self.prune_sessions_with_policy(now, &policy)?;
        let (auto_archived_documents, pruned_documents) =
            self.apply_document_retention_policy(now, &policy)?;

        let report = RetentionRunReport {
            run_at_ms: now.timestamp_millis(),
            contract_version: policy.contract_version,
            pruned_sessions,
            auto_archived_documents,
            pruned_documents,
        };
        *self
            .last_retention_report
            .write()
            .expect("retention report poisoned") = Some(report.clone());
        self.persist_retention_report(&report)?;
        Ok(report)
    }

    /// Maintenance: Delete sessions older than X days
    pub fn delete_stale_sessions(&self, days: i64) -> Result<usize> {
        let mut policy = self.retention_policy();
        if days > 0 {
            policy.session_prune_after_days = days;
        }
        self.prune_sessions_with_policy(Utc::now(), &policy)
            .map(|count| count as usize)
    }

    pub fn kv(&self) -> Arc<dyn Storage> {
        Arc::clone(&self.kv)
    }

    pub fn kv_arc(&self) -> Arc<dyn Storage> {
        Arc::clone(&self.kv)
    }

    pub fn store_session(&self, id: &str, data: &str) -> Result<()> {
        self.kv.put_session(id, data)
    }

    pub fn get_session(&self, id: &str) -> Result<Option<String>> {
        self.kv.get_session(id)
    }

    pub fn delete_session(&self, id: &str) -> Result<()> {
        self.kv.delete_session(id)?;
        Ok(())
    }

    pub fn list_sessions(&self) -> Result<Vec<(String, String)>> {
        self.kv.list_sessions()
    }

    pub fn stats(&self) -> Result<StoreStats> {
        Ok(StoreStats {
            total_documents: self.kv.document_count()?,
            total_collections: self.kv.list_collections()?.len(),
            total_unverified: self.count_unverified()?,
            total_experiences: self.kv.experience_count()?,
            total_anti_patterns: self.kv.anti_pattern_count()?,
            disk_usage_bytes: self.kv.disk_usage()?.unwrap_or(0),
        })
    }

    pub fn lifecycle_metrics(&self) -> Result<(u64, u64, u64, u64, BTreeMap<String, u64>)> {
        let (archive_count, recovery_count, background_archive_count, background_recovery_count) =
            self.session_audit_counts()?;
        let prune_counts = self.prune_counts_by_reason()?;
        Ok((
            archive_count,
            recovery_count,
            background_archive_count,
            background_recovery_count,
            prune_counts,
        ))
    }

    pub fn list_unverified(&self, limit: usize) -> Result<Vec<Document>> {
        let all = self.kv.iter_documents()?;
        let mut results = Vec::new();
        for (_, data) in all {
            if let Ok(doc) = bincode::deserialize::<Document>(&data) {
                if doc.unverified && !Self::document_is_archived(&doc) {
                    results.push(doc);
                }
            }
            if results.len() >= limit {
                break;
            }
        }
        Ok(results)
    }

    pub fn get_experience(&self, id: &str) -> Result<Option<Experience>> {
        match self.kv.get_experience(id)? {
            Some(data) => {
                let exp: Experience = bincode::deserialize(&data)
                    .map_err(|e| EngramError::Serialization(e.to_string()))?;
                Ok(Some(exp))
            }
            None => Ok(None),
        }
    }

    pub fn get_anti_pattern(&self, id: &str) -> Result<Option<AntiPattern>> {
        match self.kv.get_anti_pattern(id)? {
            Some(data) => {
                let ap: AntiPattern = bincode::deserialize(&data)
                    .map_err(|e| EngramError::Serialization(e.to_string()))?;
                Ok(Some(ap))
            }
            None => Ok(None),
        }
    }

    pub fn store_experience(&self, exp: Experience) -> Result<()> {
        let data =
            bincode::serialize(&exp).map_err(|e| EngramError::Serialization(e.to_string()))?;
        self.kv.put_experience(&exp.id, &data)
    }

    pub fn store_anti_pattern(&self, ap: AntiPattern) -> Result<()> {
        let data =
            bincode::serialize(&ap).map_err(|e| EngramError::Serialization(e.to_string()))?;
        self.kv.put_anti_pattern(&ap.id, &data)
    }

    pub fn delete_experience(&self, id: &str) -> Result<()> {
        self.kv.delete_experience(id)?;
        Ok(())
    }

    pub fn delete_anti_pattern(&self, id: &str) -> Result<()> {
        self.kv.delete_anti_pattern(id)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{EngramStore, RetentionPruningPolicy};
    use crate::metadata_contract::{
        AuditMetadataView, DocumentMetadataView, SessionAuditMetadataView,
        AUDIT_KIND_DOCUMENT_AUTO_ARCHIVED, AUDIT_KIND_DOCUMENT_PRUNED, AUDIT_KIND_SESSION_ARCHIVED,
        AUDIT_KIND_SESSION_PRUNED, AUDIT_KIND_SESSION_RECOVERED, COLLECTION_DOCUMENT_AUDIT,
        COLLECTION_SESSION_AUDIT, DOCUMENT_LIFECYCLE_ARCHIVED, DOCUMENT_LIFECYCLE_PRUNED,
        META_DOCUMENT_ARCHIVED_AT_MS, META_DOCUMENT_UPDATED_AT_MS, META_SESSION_PRUNE_REASON,
        RETENTION_REASON_ARCHIVED, RETENTION_REASON_UNVERIFIED,
    };
    use benshu_memory_core::{BackgroundEnvelope, BackgroundRevision};
    use benshu_protocol_core::AgentSession;
    use chrono::{Duration, Utc};
    use std::collections::HashMap;
    use tempfile::tempdir;

    #[test]
    fn delete_stale_sessions_only_prunes_expired_archived_sessions_and_records_audit() {
        let temp = tempdir().expect("tempdir");
        let store =
            EngramStore::new(temp.path().join("engram-store-session-prune.redb")).expect("store");

        let active = AgentSession::new("active-session".to_string());
        store
            .store_session(
                &active.id,
                &serde_json::to_string(&active).expect("active json"),
            )
            .expect("store active");

        let mut archived_expired = AgentSession::new("archived-expired".to_string());
        archived_expired.archive(
            Some("expire me".to_string()),
            Some(Utc::now() - Duration::days(2)),
        );
        store
            .store_session(
                &archived_expired.id,
                &serde_json::to_string(&archived_expired).expect("archived json"),
            )
            .expect("store archived expired");

        let mut archived_retained = AgentSession::new("archived-retained".to_string());
        archived_retained.archive(
            Some("keep me".to_string()),
            Some(Utc::now() + Duration::days(2)),
        );
        store
            .store_session(
                &archived_retained.id,
                &serde_json::to_string(&archived_retained).expect("retained json"),
            )
            .expect("store archived retained");

        let deleted = store.delete_stale_sessions(30).expect("delete stale");
        assert_eq!(deleted, 1);
        assert!(store
            .get_session("active-session")
            .expect("get active")
            .is_some());
        assert!(store
            .get_session("archived-expired")
            .expect("get archived expired")
            .is_none());
        assert!(store
            .get_session("archived-retained")
            .expect("get archived retained")
            .is_some());

        let docs = store.fetch_all_docs_legacy().expect("fetch docs");
        let audit = docs
            .into_iter()
            .find(|doc| {
                let audit = SessionAuditMetadataView::new(&doc.metadata);
                doc.collection == COLLECTION_SESSION_AUDIT
                    && audit.session_id() == Some("archived-expired")
            })
            .expect("session audit");
        let session_audit = SessionAuditMetadataView::new(&audit.metadata);
        assert_eq!(session_audit.audit_kind(), Some(AUDIT_KIND_SESSION_PRUNED));
        assert_eq!(
            audit
                .metadata
                .get(META_SESSION_PRUNE_REASON)
                .map(String::as_str),
            Some("retention_expired")
        );
    }

    #[test]
    fn delete_stale_sessions_keeps_active_background_even_when_age_threshold_hits() {
        let temp = tempdir().expect("tempdir");
        let store = EngramStore::new(
            temp.path()
                .join("engram-store-session-prune-background-protected.redb"),
        )
        .expect("store");

        let mut protected = AgentSession::new("background-protected".to_string());
        protected.updated_at = Utc::now() - Duration::days(45);
        protected.background_envelope = Some(BackgroundEnvelope {
            revision: BackgroundRevision {
                revision: 3,
                previous_revision: Some(2),
                updated_at: Utc::now(),
                update_reason: Some("protected-active-background".to_string()),
            },
            metadata: [(
                "background_session_lifecycle_state".to_string(),
                "active".to_string(),
            )]
            .into_iter()
            .collect(),
            ..BackgroundEnvelope::default()
        });
        store
            .store_session(
                &protected.id,
                &serde_json::to_string(&protected).expect("protected json"),
            )
            .expect("store protected");

        let deleted = store
            .delete_stale_sessions(30)
            .expect("delete stale background protected");
        assert_eq!(deleted, 0);
        assert!(store
            .get_session("background-protected")
            .expect("get protected")
            .is_some());
    }

    #[test]
    fn delete_stale_sessions_keeps_archived_background_until_background_retention_expires() {
        let temp = tempdir().expect("tempdir");
        let store = EngramStore::new(
            temp.path()
                .join("engram-store-session-prune-background-retention.redb"),
        )
        .expect("store");

        let mut retained = AgentSession::new("background-retained".to_string());
        retained.archive(
            Some("keep archived background".to_string()),
            Some(Utc::now() - Duration::days(2)),
        );
        retained.updated_at = Utc::now() - Duration::days(60);
        retained.background_envelope = Some(BackgroundEnvelope {
            revision: BackgroundRevision {
                revision: 5,
                previous_revision: Some(4),
                updated_at: Utc::now(),
                update_reason: Some("protected-archived-background".to_string()),
            },
            metadata: HashMap::from([
                (
                    "background_session_lifecycle_state".to_string(),
                    "archived".to_string(),
                ),
                (
                    "background_session_retention_until_ms".to_string(),
                    (Utc::now() + Duration::days(7))
                        .timestamp_millis()
                        .to_string(),
                ),
            ]),
            ..BackgroundEnvelope::default()
        });
        store
            .store_session(
                &retained.id,
                &serde_json::to_string(&retained).expect("retained json"),
            )
            .expect("store retained");

        let deleted = store
            .delete_stale_sessions(30)
            .expect("delete stale retained");
        assert_eq!(deleted, 0);
        assert!(store
            .get_session("background-retained")
            .expect("get retained")
            .is_some());
    }

    #[test]
    fn latest_session_audit_metadata_prefers_latest_event() {
        let temp = tempdir().expect("tempdir");
        let store =
            EngramStore::new(temp.path().join("engram-store-latest-audit.redb")).expect("store");

        let mut session = AgentSession::new("session-audit-latest".to_string());
        session.archive(Some("archive".to_string()), None);

        store
            .record_session_audit(
                &session,
                AUDIT_KIND_SESSION_ARCHIVED,
                Some("archive"),
                100,
                Default::default(),
            )
            .expect("record archive");
        session.mark_recovered("engram");
        store
            .record_session_audit(
                &session,
                AUDIT_KIND_SESSION_RECOVERED,
                Some("engram"),
                200,
                Default::default(),
            )
            .expect("record recovery");

        let latest = store
            .latest_session_audit_metadata("session-audit-latest")
            .expect("latest")
            .expect("metadata");
        let latest_view = SessionAuditMetadataView::new(&latest);

        assert_eq!(latest_view.audit_kind(), Some(AUDIT_KIND_SESSION_RECOVERED));
        assert_eq!(latest_view.event_at_ms(), Some(200));
    }

    #[test]
    fn retention_policy_persists_across_store_reopen() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("engram-store-retention.redb");
        let store = EngramStore::new(&path).expect("store");
        let policy = RetentionPruningPolicy {
            contract_version: 1,
            session_prune_after_days: 7,
            require_archived_for_session_prune: true,
            unverified_document_archive_after_days: 3,
            archived_document_prune_after_days: 21,
        };
        store
            .set_retention_policy(policy.clone())
            .expect("persist retention policy");
        drop(store);

        let reopened = EngramStore::new(&path).expect("reopen store");
        assert_eq!(reopened.retention_policy(), policy);
    }

    #[test]
    fn apply_retention_policy_archives_unverified_and_prunes_archived_documents() {
        let temp = tempdir().expect("tempdir");
        let store =
            EngramStore::new(temp.path().join("engram-store-retention-docs.redb")).expect("store");

        store
            .set_retention_policy(RetentionPruningPolicy {
                contract_version: 1,
                session_prune_after_days: 30,
                require_archived_for_session_prune: true,
                unverified_document_archive_after_days: 1,
                archived_document_prune_after_days: 1,
            })
            .expect("policy");

        store
            .store_document(
                "docs",
                "unverified.md",
                "Unverified",
                "needs review",
                true,
                Default::default(),
            )
            .expect("store unverified");
        store
            .merge_metadata(
                "docs",
                "unverified.md",
                [(
                    META_DOCUMENT_UPDATED_AT_MS.to_string(),
                    (Utc::now() - Duration::days(3))
                        .timestamp_millis()
                        .to_string(),
                )]
                .into_iter()
                .collect(),
            )
            .ok();
        {
            let mut doc = store
                .get_by_path("docs", "unverified.md")
                .expect("get unverified")
                .expect("doc");
            doc.updated_at_ms = (Utc::now() - Duration::days(3)).timestamp_millis();
            let data = bincode::serialize(&doc).expect("serialize");
            store
                .kv()
                .put_document("docs:unverified.md", &data)
                .expect("persist updated");
        }

        store
            .store_document(
                "docs",
                "archived.md",
                "Archived",
                "already archived",
                false,
                Default::default(),
            )
            .expect("store archived");
        store
            .archive_document(
                "docs",
                "archived.md",
                [(
                    META_DOCUMENT_ARCHIVED_AT_MS.to_string(),
                    (Utc::now() - Duration::days(4))
                        .timestamp_millis()
                        .to_string(),
                )]
                .into_iter()
                .collect(),
            )
            .expect("archive document");

        let report = store.apply_retention_policy().expect("apply policy");
        assert_eq!(report.auto_archived_documents, 1);
        assert_eq!(report.pruned_documents, 1);

        let unverified = store
            .get_by_path("docs", "unverified.md")
            .expect("get unverified")
            .expect("doc");
        let unverified_document =
            DocumentMetadataView::new(&unverified.metadata, unverified.summary.is_some(), false);
        assert_eq!(
            unverified_document.lifecycle_state(),
            DOCUMENT_LIFECYCLE_ARCHIVED
        );
        assert_eq!(
            unverified_document.archive_reason(),
            Some(RETENTION_REASON_UNVERIFIED)
        );

        let archived = store
            .get_by_path("docs", "archived.md")
            .expect("get archived")
            .expect("doc");
        let archived_document =
            DocumentMetadataView::new(&archived.metadata, archived.summary.is_some(), false);
        assert_eq!(
            archived_document.lifecycle_state(),
            DOCUMENT_LIFECYCLE_PRUNED
        );
        assert_eq!(
            archived_document.prune_reason(),
            Some(RETENTION_REASON_ARCHIVED)
        );

        let docs = store.fetch_all_docs_legacy().expect("fetch docs");
        assert!(docs.iter().any(|doc| {
            doc.collection == COLLECTION_DOCUMENT_AUDIT
                && AuditMetadataView::new(&doc.metadata).audit_kind()
                    == Some(AUDIT_KIND_DOCUMENT_AUTO_ARCHIVED)
        }));
        assert!(docs.iter().any(|doc| {
            doc.collection == COLLECTION_DOCUMENT_AUDIT
                && AuditMetadataView::new(&doc.metadata).audit_kind()
                    == Some(AUDIT_KIND_DOCUMENT_PRUNED)
        }));
    }
}
