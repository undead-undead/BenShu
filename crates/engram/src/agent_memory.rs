use crate::hybrid_search::HybridSearchEngine;
use crate::metadata_contract::{
    runtime_metadata_value_from_stats, DocumentMetadataView, FactMetadataView,
    FactReviewMetadataView, AUDIT_KIND_SESSION_ARCHIVED, AUDIT_KIND_SESSION_RECOVERED,
    DOCUMENT_CONTEXT_ROLE_DURABLE_DOCUMENT, DOCUMENT_CONTEXT_ROLE_TRANSIENT_CONTEXT,
    DOCUMENT_DURABLE_AUTHORITY_ENGRAM, DOCUMENT_INGEST_SOURCE_BRAIN_MULTIMODAL_WRITEBACK,
    DOCUMENT_INGEST_SOURCE_BRAIN_STORE_KNOWLEDGE, DOCUMENT_LIFECYCLE_ACTIVE,
    DOCUMENT_LIFECYCLE_MULTIMODAL_RECORDED, DOCUMENT_LIFECYCLE_SUMMARIZED,
    DOCUMENT_PERSISTENCE_SCOPE_DURABLE, DOCUMENT_PERSISTENCE_SCOPE_TRANSIENT,
    DOCUMENT_POLICY_OWNER_BRAIN, DOCUMENT_SUMMARY_STATE_READY, FACT_DURABLE_AUTHORITY_ENGRAM,
    FACT_HOT_AUTHORITY_BRAIN_HOT_MEMORY, FACT_LIFECYCLE_ACTIVE, FACT_LIFECYCLE_ARCHIVED,
    FACT_LIFECYCLE_PRUNED, FACT_LIFECYCLE_VERIFIED, FACT_PERSISTENCE_SCOPE_DURABLE,
    FACT_POLICY_OWNER_BRAIN, FACT_PRUNE_REASON_MANUAL, FACT_REVIEW_OUTCOME_PENDING_REVIEW,
    FACT_REVIEW_OUTCOME_PRUNED, FACT_REVIEW_OUTCOME_VERIFIED, META_DOCUMENT_ARCHIVED,
    META_DOCUMENT_CONTEXT_ROLE, META_DOCUMENT_CONTRACT_VERSION, META_DOCUMENT_DURABLE_AUTHORITY,
    META_DOCUMENT_HAS_SUMMARY, META_DOCUMENT_INGEST_SOURCE, META_DOCUMENT_IS_STRUCTURAL,
    META_DOCUMENT_LIFECYCLE_STATE, META_DOCUMENT_PERSISTENCE_SCOPE, META_DOCUMENT_POLICY_OWNER,
    META_DOCUMENT_SUMMARY_STATE, META_FACT_ARCHIVED_AT_MS, META_FACT_CONFIDENCE,
    META_FACT_CONTRACT_VERSION, META_FACT_CREATED_AT, META_FACT_DURABLE_AUTHORITY,
    META_FACT_HOT_AUTHORITY, META_FACT_LIFECYCLE_STATE, META_FACT_PERSISTENCE_SCOPE,
    META_FACT_POLICY_OWNER, META_FACT_PROTECTION, META_FACT_PRUNED_AT_MS, META_FACT_PRUNE_REASON,
    META_FACT_RELATIONS, META_FACT_REVIEW_REASON, META_FACT_REVIEW_RESOLUTION_BASIS,
    META_FACT_REVIEW_RESOLUTION_OUTCOME, META_FACT_REVIEW_RESOLUTION_REASON,
    META_FACT_REVIEW_RESOLVED_AT_MS, META_FACT_REVIEW_RESOLVED_BY, META_FACT_REVIEW_SOURCE,
    META_FACT_REVIEW_SUMMARY, META_FACT_SEMANTIC_HASH, META_FACT_SOURCE, META_FACT_STATUS,
    META_FACT_UPDATED_AT, META_FACT_VERIFIED, META_MULTIMODAL_ARTIFACT_LOCATOR,
    META_MULTIMODAL_CONTRACT_VERSION, META_MULTIMODAL_HAS_DERIVED_FACT, META_MULTIMODAL_KIND,
    META_MULTIMODAL_MODALITY, META_MULTIMODAL_MODEL, META_MULTIMODAL_PROMPT, META_MULTIMODAL_ROUTE,
    META_MULTIMODAL_SOURCE_PATH, META_MULTIMODAL_SOURCE_URL, META_SESSION_AUDIT_SOURCE,
    META_SESSION_BACKGROUND_LIFECYCLE_STATE, META_SESSION_BACKGROUND_REVISION,
    META_SESSION_CONTRACT_VERSION, META_SUMMARY_SOURCE, META_SUMMARY_UPDATED_AT_MS,
    MULTIMODAL_CONTRACT_VERSION, SESSION_AUDIT_SOURCE_ENGRAM_STORE_SESSION,
    SUMMARY_SOURCE_BRAIN_MEMORY_MANAGER,
};
use async_trait::async_trait;
#[cfg(feature = "vector")]
use benshu_inference::QuantLevel;
use benshu_infra::traits::memory::{EventLevel, MemoryEmitter, MemoryEvent};
use benshu_infra::SecurityHandler;
use benshu_memory_api::Memory as MemoryApi;
use benshu_memory_core::{
    traverse_related_facts_with_report, BackgroundRevision, Document, Fact, FactProtection,
    FactReviewPayload, FactReviewResolution, FactReviewResolutionOutcome, FactStatus,
    MultimodalMemoryKind, MultimodalMemoryRecord,
};
use benshu_protocol_core::{AgentSession, Message, SessionStatus};
use chrono::Utc;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

const FACT_CONTRACT_VERSION: &str = "1";
const DOCUMENT_CONTRACT_VERSION: &str = "1";
const SESSION_CONTRACT_VERSION: &str = "1";

/// Adapter to use HybridSearchEngine as an BenShu Memory backend.
/// Now fully aligned with high-performance digital timestamping (Unix ms).
pub struct EngramMemory {
    engine: Arc<HybridSearchEngine>,
    security: Arc<RwLock<Option<Arc<dyn SecurityHandler>>>>,
    emitter: Arc<RwLock<Option<Arc<dyn MemoryEmitter>>>>,
    runtime_metadata: Arc<RwLock<HashMap<String, String>>>,
}

impl EngramMemory {
    pub fn new(engine: Arc<HybridSearchEngine>) -> Self {
        Self {
            engine,
            security: Arc::new(RwLock::new(None)),
            emitter: Arc::new(RwLock::new(None)),
            runtime_metadata: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Optimized: Pass through to engine config
    pub fn with_config(engine: Arc<HybridSearchEngine>) -> Self {
        Self::new(engine)
    }

    fn encrypt_if_sensitive(&self, collection: &str, content: &str) -> String {
        let sensitive = [
            "identity",
            "core",
            "secret",
            "private",
            "credential",
            "private_chat",
        ];
        if sensitive.contains(&collection.to_lowercase().as_str()) {
            if let Some(sec) = self.security.read().as_ref() {
                match sec.encrypt_fact(content) {
                    Ok(enc) => enc,
                    Err(e) => {
                        tracing::error!("Vault encryption failed: {}", e);
                        content.to_string()
                    }
                }
            } else {
                tracing::warn!(
                    "Vault not linked - storing sensitive collection '{}' in plaintext!",
                    collection
                );
                content.to_string()
            }
        } else {
            content.to_string()
        }
    }

    fn decrypt_if_needed(&self, content: &str) -> String {
        // Only attempt decryption if the content has the secure prefix
        if content.starts_with("enc:") {
            if let Some(sec) = self.security.read().as_ref() {
                match sec.decrypt_fact(content) {
                    Ok(dec) => dec,
                    Err(_) => "[Encrypted - Decryption Failed]".to_string(),
                }
            } else {
                "[Encrypted - Vault Locked]".to_string()
            }
        } else {
            content.to_string()
        }
    }

    fn fact_path(fact_id: &str) -> String {
        format!("fact/{}", fact_id)
    }

    fn backfill_background_from_session_audit(
        session: &mut AgentSession,
        audit_metadata: &HashMap<String, String>,
    ) {
        let Some(background) = session.background_envelope.as_mut() else {
            return;
        };

        if let Some(lifecycle_state) = audit_metadata.get(META_SESSION_BACKGROUND_LIFECYCLE_STATE) {
            background
                .metadata
                .entry("background_session_lifecycle_state".to_string())
                .or_insert_with(|| lifecycle_state.clone());
        }

        if let Some(revision) = audit_metadata
            .get(META_SESSION_BACKGROUND_REVISION)
            .and_then(|value| value.parse::<u64>().ok())
        {
            if background.revision.revision < revision {
                background.revision = BackgroundRevision {
                    revision,
                    ..background.revision.clone()
                };
            }
        }
    }

    fn extract_fact_id(path: &str, fallback: &str) -> String {
        path.strip_prefix("fact/").unwrap_or(fallback).to_string()
    }

    fn fact_metadata(fact: &Fact) -> std::collections::HashMap<String, String> {
        let mut metadata = std::collections::HashMap::new();
        if let Some(source) = &fact.source {
            metadata.insert(META_FACT_SOURCE.to_string(), source.clone());
        }
        metadata.insert(
            META_FACT_CONTRACT_VERSION.to_string(),
            FACT_CONTRACT_VERSION.to_string(),
        );
        metadata.insert(
            META_FACT_POLICY_OWNER.to_string(),
            FACT_POLICY_OWNER_BRAIN.to_string(),
        );
        metadata.insert(
            META_FACT_HOT_AUTHORITY.to_string(),
            FACT_HOT_AUTHORITY_BRAIN_HOT_MEMORY.to_string(),
        );
        metadata.insert(
            META_FACT_DURABLE_AUTHORITY.to_string(),
            FACT_DURABLE_AUTHORITY_ENGRAM.to_string(),
        );
        metadata.insert(
            META_FACT_PERSISTENCE_SCOPE.to_string(),
            FACT_PERSISTENCE_SCOPE_DURABLE.to_string(),
        );
        metadata.insert(
            META_FACT_LIFECYCLE_STATE.to_string(),
            if matches!(fact.status, FactStatus::Archived) {
                FACT_LIFECYCLE_ARCHIVED.to_string()
            } else {
                FACT_LIFECYCLE_ACTIVE.to_string()
            },
        );
        metadata.insert(
            META_FACT_CONFIDENCE.to_string(),
            fact.confidence.to_string(),
        );
        metadata.insert(
            META_FACT_STATUS.to_string(),
            serde_json::to_string(&fact.status).unwrap_or_else(|_| "\"pending\"".to_string()),
        );
        metadata.insert(
            META_FACT_PROTECTION.to_string(),
            serde_json::to_string(&fact.protection).unwrap_or_else(|_| "\"normal\"".to_string()),
        );
        metadata.insert(META_FACT_VERIFIED.to_string(), fact.verified.to_string());
        metadata.insert(
            META_FACT_SEMANTIC_HASH.to_string(),
            fact.semantic_hash.clone().unwrap_or_default(),
        );
        metadata.insert(
            META_FACT_CREATED_AT.to_string(),
            fact.created_at.timestamp_millis().to_string(),
        );
        metadata.insert(
            META_FACT_UPDATED_AT.to_string(),
            fact.updated_at.timestamp_millis().to_string(),
        );
        metadata.insert(
            META_FACT_RELATIONS.to_string(),
            serde_json::to_string(&fact.relations).unwrap_or_else(|_| "[]".to_string()),
        );
        metadata
    }

    fn base_document_metadata(
        persistence_scope: &'static str,
        context_role: &'static str,
        lifecycle_state: &'static str,
        ingest_source: &str,
    ) -> HashMap<String, String> {
        let mut metadata = HashMap::new();
        metadata.insert(
            META_DOCUMENT_CONTRACT_VERSION.to_string(),
            DOCUMENT_CONTRACT_VERSION.to_string(),
        );
        metadata.insert(
            META_DOCUMENT_POLICY_OWNER.to_string(),
            DOCUMENT_POLICY_OWNER_BRAIN.to_string(),
        );
        metadata.insert(
            META_DOCUMENT_DURABLE_AUTHORITY.to_string(),
            DOCUMENT_DURABLE_AUTHORITY_ENGRAM.to_string(),
        );
        metadata.insert(
            META_DOCUMENT_PERSISTENCE_SCOPE.to_string(),
            persistence_scope.to_string(),
        );
        metadata.insert(
            META_DOCUMENT_CONTEXT_ROLE.to_string(),
            context_role.to_string(),
        );
        metadata.insert(
            META_DOCUMENT_LIFECYCLE_STATE.to_string(),
            lifecycle_state.to_string(),
        );
        metadata.insert(
            META_DOCUMENT_INGEST_SOURCE.to_string(),
            ingest_source.to_string(),
        );
        metadata
    }

    fn durable_document_metadata(ingest_source: &str) -> HashMap<String, String> {
        Self::base_document_metadata(
            DOCUMENT_PERSISTENCE_SCOPE_DURABLE,
            DOCUMENT_CONTEXT_ROLE_DURABLE_DOCUMENT,
            DOCUMENT_LIFECYCLE_ACTIVE,
            ingest_source,
        )
    }

    fn normalize_document_metadata(doc: &crate::store::Document) -> HashMap<String, String> {
        let mut metadata = doc.metadata.clone();
        let has_summary =
            doc.summary.is_some() || metadata.contains_key(META_SUMMARY_UPDATED_AT_MS);
        let (
            contract_version,
            policy_owner,
            durable_authority,
            persistence_scope,
            context_role,
            lifecycle_state,
            archived,
            has_summary,
            is_structural,
        ) = {
            let view = DocumentMetadataView::new(&metadata, has_summary, doc.is_structural());
            (
                view.contract_version(),
                view.policy_owner(),
                view.durable_authority(),
                view.persistence_scope(),
                view.context_role(),
                view.lifecycle_state(),
                view.archived(),
                view.has_summary(),
                view.is_structural(),
            )
        };
        metadata
            .entry(META_DOCUMENT_CONTRACT_VERSION.to_string())
            .or_insert(contract_version);
        metadata
            .entry(META_DOCUMENT_POLICY_OWNER.to_string())
            .or_insert(policy_owner);
        metadata
            .entry(META_DOCUMENT_DURABLE_AUTHORITY.to_string())
            .or_insert(durable_authority);
        metadata
            .entry(META_DOCUMENT_PERSISTENCE_SCOPE.to_string())
            .or_insert(persistence_scope);
        metadata
            .entry(META_DOCUMENT_CONTEXT_ROLE.to_string())
            .or_insert(context_role);
        metadata
            .entry(META_DOCUMENT_LIFECYCLE_STATE.to_string())
            .or_insert(lifecycle_state);
        metadata.insert(META_DOCUMENT_ARCHIVED.to_string(), archived.to_string());
        metadata.insert(
            META_DOCUMENT_HAS_SUMMARY.to_string(),
            has_summary.to_string(),
        );
        metadata.insert(
            META_DOCUMENT_IS_STRUCTURAL.to_string(),
            is_structural.to_string(),
        );
        metadata
    }

    fn parse_fact(doc: &crate::store::Document, content: String) -> Fact {
        let view = FactMetadataView::new(
            &doc.metadata,
            doc.unverified,
            doc.created_at_ms,
            doc.updated_at_ms,
        );
        let created_at_ms = view.created_at_ms();
        let updated_at_ms = view.updated_at_ms();
        let status = view
            .status_json()
            .and_then(|value| serde_json::from_str::<FactStatus>(value).ok())
            .unwrap_or_else(|| {
                if view.unverified {
                    FactStatus::Pending
                } else {
                    FactStatus::Verified
                }
            });
        let relations = view
            .relations_json()
            .and_then(|value| serde_json::from_str(value).ok())
            .unwrap_or_default();
        let protection = view
            .protection_json()
            .and_then(|value| serde_json::from_str::<FactProtection>(value).ok())
            .unwrap_or_default();
        let semantic_hash = view.semantic_hash();
        let confidence = view.confidence();
        let verified = if matches!(status, FactStatus::Verified) {
            true
        } else {
            !doc.unverified
        };

        Fact {
            id: Self::extract_fact_id(&doc.path, &doc.docid),
            category: doc.collection.clone(),
            content,
            importance: doc.utility_score,
            created_at: chrono::DateTime::from_timestamp_millis(created_at_ms)
                .unwrap_or_else(Utc::now),
            updated_at: chrono::DateTime::from_timestamp_millis(updated_at_ms)
                .unwrap_or_else(Utc::now),
            verified,
            source: view.source(),
            confidence,
            relations,
            semantic_hash,
            status: if verified && !matches!(status, FactStatus::Archived) {
                FactStatus::Verified
            } else {
                status
            },
            protection,
        }
    }

    fn parse_fact_review_payload(doc: &crate::store::Document) -> Option<FactReviewPayload> {
        let view = FactReviewMetadataView::new(&doc.metadata);
        let review_reason = view.review_reason();
        let challenger_summary = view.challenger_summary();
        let challenger_source = view.challenger_source();
        let review_requested_at = view
            .review_requested_at_ms()
            .and_then(chrono::DateTime::from_timestamp_millis);
        let resolution = view
            .resolution_outcome()
            .and_then(|outcome| match outcome {
                FACT_REVIEW_OUTCOME_VERIFIED => Some(FactReviewResolutionOutcome::Verified),
                FACT_REVIEW_OUTCOME_PRUNED => Some(FactReviewResolutionOutcome::Pruned),
                FACT_REVIEW_OUTCOME_PENDING_REVIEW => {
                    Some(FactReviewResolutionOutcome::PendingReview)
                }
                _ => None,
            })
            .map(|outcome| FactReviewResolution {
                outcome,
                resolution_reason: view.resolution_reason(),
                resolution_basis: view.resolution_basis(),
                resolved_by: view.resolved_by(),
                resolved_at: view
                    .resolved_at_ms()
                    .and_then(chrono::DateTime::from_timestamp_millis)
                    .unwrap_or_else(Utc::now),
            });

        if review_reason.is_none()
            && challenger_summary.is_none()
            && challenger_source.is_none()
            && review_requested_at.is_none()
            && resolution.is_none()
        {
            None
        } else {
            Some(FactReviewPayload {
                review_reason,
                challenger_summary,
                challenger_source,
                review_requested_at,
                resolution,
            })
        }
    }

    fn review_resolution_metadata(
        resolution: &FactReviewResolution,
    ) -> std::collections::HashMap<String, String> {
        let mut metadata = std::collections::HashMap::new();
        metadata.insert(
            META_FACT_REVIEW_RESOLUTION_OUTCOME.to_string(),
            match resolution.outcome {
                FactReviewResolutionOutcome::Verified => FACT_REVIEW_OUTCOME_VERIFIED.to_string(),
                FactReviewResolutionOutcome::Pruned => FACT_REVIEW_OUTCOME_PRUNED.to_string(),
                FactReviewResolutionOutcome::PendingReview => {
                    FACT_REVIEW_OUTCOME_PENDING_REVIEW.to_string()
                }
            },
        );
        metadata.insert(
            META_FACT_REVIEW_RESOLVED_AT_MS.to_string(),
            resolution.resolved_at.timestamp_millis().to_string(),
        );
        if let Some(reason) = &resolution.resolution_reason {
            metadata.insert(
                META_FACT_REVIEW_RESOLUTION_REASON.to_string(),
                reason.clone(),
            );
        }
        if let Some(basis) = &resolution.resolution_basis {
            metadata.insert(META_FACT_REVIEW_RESOLUTION_BASIS.to_string(), basis.clone());
        }
        if let Some(actor) = &resolution.resolved_by {
            metadata.insert(META_FACT_REVIEW_RESOLVED_BY.to_string(), actor.clone());
        }
        metadata
    }

    fn multimodal_metadata(record: &MultimodalMemoryRecord) -> HashMap<String, String> {
        let mut metadata = Self::base_document_metadata(
            if record.transient {
                DOCUMENT_PERSISTENCE_SCOPE_TRANSIENT
            } else {
                DOCUMENT_PERSISTENCE_SCOPE_DURABLE
            },
            if record.transient {
                DOCUMENT_CONTEXT_ROLE_TRANSIENT_CONTEXT
            } else {
                DOCUMENT_CONTEXT_ROLE_DURABLE_DOCUMENT
            },
            DOCUMENT_LIFECYCLE_MULTIMODAL_RECORDED,
            DOCUMENT_INGEST_SOURCE_BRAIN_MULTIMODAL_WRITEBACK,
        );
        metadata.extend(record.metadata.clone());
        metadata.insert(
            META_MULTIMODAL_CONTRACT_VERSION.to_string(),
            MULTIMODAL_CONTRACT_VERSION.to_string(),
        );
        metadata.insert(
            META_MULTIMODAL_KIND.to_string(),
            serde_json::to_string(&record.kind).unwrap_or_else(|_| "\"understanding\"".to_string()),
        );
        metadata.insert(
            META_MULTIMODAL_MODALITY.to_string(),
            record.modality.clone(),
        );
        metadata.insert(
            META_MULTIMODAL_HAS_DERIVED_FACT.to_string(),
            record.derived_fact.is_some().to_string(),
        );
        if let Some(source_path) = &record.source_path {
            metadata.insert(META_MULTIMODAL_SOURCE_PATH.to_string(), source_path.clone());
        }
        if let Some(source_url) = &record.source_url {
            metadata.insert(META_MULTIMODAL_SOURCE_URL.to_string(), source_url.clone());
        }
        if let Some(route) = &record.route {
            metadata.insert(META_MULTIMODAL_ROUTE.to_string(), route.clone());
        }
        if let Some(model) = &record.model {
            metadata.insert(META_MULTIMODAL_MODEL.to_string(), model.clone());
        }
        if let Some(prompt) = &record.prompt {
            metadata.insert(META_MULTIMODAL_PROMPT.to_string(), prompt.clone());
        }
        if let Some(locator) = &record.artifact_locator {
            metadata.insert(
                META_MULTIMODAL_ARTIFACT_LOCATOR.to_string(),
                locator.clone(),
            );
        }
        metadata
    }

    fn emit(&self, event: MemoryEvent, level: EventLevel) {
        if let Some(emitter) = self.emitter.read().as_ref() {
            emitter.emit(event, level);
        }
    }

    fn resolve_document_id(
        &self,
        collection: &str,
        reference: &str,
    ) -> benshu_infra::error::Result<String> {
        if !collection.is_empty() {
            let mut candidates = vec![reference.to_string()];
            if collection == "facts" && !reference.starts_with("fact/") {
                candidates.push(Self::fact_path(reference));
            }

            for candidate in candidates {
                if let Some(doc) = self
                    .engine
                    .get_by_path(collection, &candidate)
                    .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?
                {
                    return Ok(doc.docid);
                }
            }
        }

        Ok(reference.to_string())
    }

    fn load_existing_session(
        &self,
        session_id: &str,
    ) -> benshu_infra::error::Result<Option<AgentSession>> {
        let raw = self
            .engine
            .engram_store()
            .get_session(session_id)
            .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?;
        let Some(raw) = raw else {
            return Ok(None);
        };
        let session = serde_json::from_str::<AgentSession>(&raw)
            .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?;
        Ok(Some(session))
    }

    fn record_session_lifecycle_audit(
        &self,
        session: &AgentSession,
        audit_kind: &str,
        audit_reason: Option<&str>,
        extra: std::collections::HashMap<String, String>,
    ) -> benshu_infra::error::Result<()> {
        self.engine
            .engram_store()
            .record_session_audit(
                session,
                audit_kind,
                audit_reason,
                Utc::now().timestamp_millis(),
                extra,
            )
            .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))
    }

    fn runtime_metadata_value(&self, key: &str) -> Option<String> {
        if let Some(value) = self.runtime_metadata.read().get(key).cloned() {
            return Some(value);
        }
        runtime_metadata_value_from_stats(
            key,
            &self.engine.stats(),
            FACT_CONTRACT_VERSION,
            DOCUMENT_CONTRACT_VERSION,
            SESSION_CONTRACT_VERSION,
        )
    }

    #[cfg(test)]
    fn runtime_metadata_snapshot(&self) -> HashMap<String, String> {
        let mut metadata = crate::metadata_contract::runtime_metadata_snapshot_from_stats(
            &self.engine.stats(),
            FACT_CONTRACT_VERSION,
            DOCUMENT_CONTRACT_VERSION,
            SESSION_CONTRACT_VERSION,
        );
        for (key, value) in self.runtime_metadata.read().iter() {
            metadata.insert(key.clone(), value.clone());
        }
        metadata
    }

    fn set_runtime_metadata(&self, key: &str, value: String) {
        self.runtime_metadata.write().insert(key.to_string(), value);
    }
}

#[async_trait]
impl MemoryApi for EngramMemory {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn store(
        &self,
        _user_id: &str,
        _agent_id: Option<&str>,
        _message: Message,
    ) -> benshu_infra::error::Result<()> {
        // Engram focuses on long-term knowledge; short-term chat history managed by brain STM
        Ok(())
    }

    async fn store_batch(
        &self,
        _user_id: &str,
        _agent_id: Option<&str>,
        _messages: Vec<Message>,
    ) -> benshu_infra::error::Result<()> {
        Ok(())
    }

    async fn retrieve(
        &self,
        _user_id: &str,
        _agent_id: Option<&str>,
        _limit: usize,
    ) -> Vec<Message> {
        Vec::new()
    }

    async fn retrieve_full_history(
        &self,
        _user_id: &str,
        _agent_id: Option<&str>,
    ) -> benshu_infra::error::Result<Vec<Message>> {
        Ok(Vec::new())
    }

    async fn clear(
        &self,
        _user_id: &str,
        _agent_id: Option<&str>,
    ) -> benshu_infra::error::Result<()> {
        Ok(())
    }

    async fn undo(
        &self,
        _user_id: &str,
        _agent_id: Option<&str>,
    ) -> benshu_infra::error::Result<Option<Message>> {
        Ok(None)
    }

    async fn store_knowledge(
        &self,
        _user_id: &str,
        _agent_id: Option<&str>,
        title: &str,
        content: &str,
        collection: &str,
        unverified: bool,
    ) -> benshu_infra::error::Result<()> {
        let path = format!("manual/{}", uuid::Uuid::new_v4());
        let final_content = self.encrypt_if_sensitive(collection, content);
        let metadata =
            Self::durable_document_metadata(DOCUMENT_INGEST_SOURCE_BRAIN_STORE_KNOWLEDGE);

        #[cfg(feature = "vector")]
        {
            let level = match collection.to_lowercase().as_str() {
                "experience" | "anti_pattern" => QuantLevel::Cold,
                "agent" | "core" | "identity" => QuantLevel::Full,
                _ => QuantLevel::Warm,
            };

            self.engine
                .index_at_level(
                    collection,
                    &path,
                    title,
                    &final_content,
                    level,
                    unverified,
                    metadata,
                )
                .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?;
        }
        #[cfg(not(feature = "vector"))]
        {
            self.engine
                .engram_store()
                .store_document(
                    collection,
                    &path,
                    title,
                    &final_content,
                    unverified,
                    metadata,
                )
                .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?;
        }

        self.emit(
            MemoryEvent::FactCreated {
                id: path,
                category: collection.to_string(),
                status: if unverified {
                    "pending".to_string()
                } else {
                    "verified".to_string()
                },
            },
            EventLevel::Info,
        );

        Ok(())
    }

    async fn store_multimodal_memory(
        &self,
        _user_id: &str,
        _agent_id: Option<&str>,
        record: MultimodalMemoryRecord,
    ) -> benshu_infra::error::Result<Document> {
        let path = format!("multimodal/{}/{}", record.kind_slug(), uuid::Uuid::new_v4());
        let content = self.encrypt_if_sensitive(&record.collection, &record.content);
        let metadata = Self::multimodal_metadata(&record);
        let multimodal_kind = record.kind_slug().to_string();
        let multimodal_modality = record.modality.clone();
        let multimodal_transient = record.transient;

        #[cfg(feature = "vector")]
        {
            let level = match &record.kind {
                MultimodalMemoryKind::GenerationProvenance => QuantLevel::Warm,
                MultimodalMemoryKind::Understanding => QuantLevel::Warm,
            };

            self.engine
                .index_at_level(
                    &record.collection,
                    &path,
                    &record.title,
                    &content,
                    level,
                    false,
                    metadata.clone(),
                )
                .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?;
        }
        #[cfg(not(feature = "vector"))]
        {
            self.engine
                .engram_store()
                .store_document(
                    &record.collection,
                    &path,
                    &record.title,
                    &content,
                    false,
                    metadata.clone(),
                )
                .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?;
        }

        self.engine
            .update_summary(&record.collection, &path, &record.summary)
            .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?;

        self.emit(
            MemoryEvent::MultimodalMemoryStored {
                collection: record.collection.clone(),
                path: path.clone(),
                kind: multimodal_kind,
                modality: multimodal_modality,
                transient: multimodal_transient,
            },
            EventLevel::Info,
        );

        Ok(Document {
            id: path.clone(),
            title: record.title,
            content: self.decrypt_if_needed(&content),
            summary: Some(record.summary),
            collection: Some(record.collection),
            path: Some(path),
            metadata,
            score: 1.0,
        })
    }

    async fn store_fact(
        &self,
        _user_id: &str,
        _agent_id: Option<&str>,
        fact: Fact,
    ) -> benshu_infra::error::Result<()> {
        let path = Self::fact_path(&fact.id);
        let metadata = Self::fact_metadata(&fact);

        // We store it as a document in the 'facts' collection
        let stored = self
            .engine
            .engram_store()
            .store_document(
                "facts",
                &path,
                &format!("Fact: {}", fact.category),
                &fact.content,
                fact.status != FactStatus::Verified,
                metadata,
            )
            .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?;
        let _ = self
            .engine
            .engram_store()
            .update_utility(&stored.docid, fact.importance.clamp(0.0, 1.0));

        self.emit(
            MemoryEvent::FactCreated {
                id: fact.id.clone(),
                category: fact.category.clone(),
                status: if fact.status == FactStatus::Verified {
                    "verified".to_string()
                } else {
                    "pending".to_string()
                },
            },
            EventLevel::Info,
        );

        Ok(())
    }

    async fn retrieve_facts(
        &self,
        _user_id: &str,
        _agent_id: Option<&str>,
    ) -> benshu_infra::error::Result<Vec<Fact>> {
        let docs = self
            .engine
            .list_documents_in_collection("facts")
            .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?;

        let mut facts = Vec::new();
        for doc in docs {
            if doc.collection != "facts" {
                continue;
            }
            let content = self
                .engine
                .engram_store()
                .get_content(&doc)
                .unwrap_or_default()
                .map(|c| self.decrypt_if_needed(&c))
                .unwrap_or_else(|| self.decrypt_if_needed(&doc.title));
            let fact = Self::parse_fact(&doc, content);
            if matches!(fact.status, FactStatus::Archived) {
                continue;
            }
            facts.push(fact);
        }
        Ok(facts)
    }

    async fn delete_fact(
        &self,
        _user_id: &str,
        _agent_id: Option<&str>,
        fact_id: &str,
    ) -> benshu_infra::error::Result<()> {
        self.engine
            .engram_store()
            .delete_document("facts", &Self::fact_path(fact_id))
            .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?;
        Ok(())
    }

    async fn find_related_facts(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        fact_id: &str,
        depth: usize,
    ) -> benshu_infra::error::Result<Vec<Fact>> {
        let facts = MemoryApi::retrieve_facts(self, user_id, agent_id).await?;
        let facts_by_id = facts
            .into_iter()
            .map(|fact| (fact.id.clone(), fact))
            .collect::<std::collections::HashMap<_, _>>();
        let traversal = traverse_related_facts_with_report(&facts_by_id, fact_id, depth);
        for (key, value) in traversal.report.metadata_entries(fact_id) {
            self.set_runtime_metadata(&key, value);
        }
        Ok(traversal.facts)
    }

    async fn list_unverified(
        &self,
        _agent_id: Option<&str>,
        limit: usize,
    ) -> benshu_infra::error::Result<Vec<Fact>> {
        let docs = self
            .engine
            .engram_store()
            .list_unverified(limit)
            .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?;

        let mut facts = Vec::new();
        for doc in docs {
            if doc.collection != "facts" {
                continue;
            }
            let content = self
                .engine
                .engram_store()
                .get_content(&doc)
                .unwrap_or_default()
                .map(|c| self.decrypt_if_needed(&c))
                .unwrap_or_else(|| self.decrypt_if_needed(&doc.title));
            facts.push(Self::parse_fact(&doc, content));
        }
        Ok(facts)
    }

    async fn search(
        &self,
        _user_id: &str,
        _agent_id: Option<&str>,
        query: &str,
        limit: usize,
    ) -> benshu_infra::error::Result<Vec<Document>> {
        let results = self
            .engine
            .search(query, limit)
            .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?;

        let mut brain_docs = Vec::new();
        for res in results {
            let eng_doc = res.document;
            let content = self
                .engine
                .engram_store()
                .get_content(&eng_doc)
                .unwrap_or_default()
                .unwrap_or_default();

            brain_docs.push(Document {
                id: eng_doc.docid,
                title: self.decrypt_if_needed(&eng_doc.title),
                content,
                summary: eng_doc.summary,
                collection: Some(eng_doc.collection),
                path: Some(eng_doc.path),
                metadata: eng_doc.metadata,
                score: res.rrf_score as f32,
            });
        }
        Ok(brain_docs)
    }

    async fn store_session(&self, session: AgentSession) -> benshu_infra::error::Result<()> {
        let previous = self.load_existing_session(&session.id)?;
        let data = serde_json::to_string(&session)
            .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?;
        self.engine
            .engram_store()
            .store_session(&session.id, &data)
            .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?;

        let archived_transition = previous
            .as_ref()
            .map(|prior| !prior.is_archived() && session.is_archived())
            .unwrap_or_else(|| session.is_archived());
        if archived_transition {
            let mut metadata = std::collections::HashMap::new();
            metadata.insert(
                META_SESSION_CONTRACT_VERSION.to_string(),
                SESSION_CONTRACT_VERSION.to_string(),
            );
            metadata.insert(
                META_SESSION_AUDIT_SOURCE.to_string(),
                SESSION_AUDIT_SOURCE_ENGRAM_STORE_SESSION.to_string(),
            );
            self.record_session_lifecycle_audit(
                &session,
                AUDIT_KIND_SESSION_ARCHIVED,
                session.lifecycle.archive_reason.as_deref(),
                metadata,
            )?;
        }

        let recovered_transition = match previous.as_ref() {
            Some(prior) => {
                session.lifecycle.last_recovered_at.is_some()
                    && session.lifecycle.last_recovered_at != prior.lifecycle.last_recovered_at
            }
            None => session.lifecycle.last_recovered_at.is_some(),
        };
        if recovered_transition {
            let mut metadata = std::collections::HashMap::new();
            metadata.insert(
                META_SESSION_CONTRACT_VERSION.to_string(),
                SESSION_CONTRACT_VERSION.to_string(),
            );
            metadata.insert(
                META_SESSION_AUDIT_SOURCE.to_string(),
                SESSION_AUDIT_SOURCE_ENGRAM_STORE_SESSION.to_string(),
            );
            self.record_session_lifecycle_audit(
                &session,
                AUDIT_KIND_SESSION_RECOVERED,
                session.lifecycle.recovered_from.as_deref(),
                metadata,
            )?;
        }
        self.emit(
            MemoryEvent::SessionStored {
                session_id: session.id.clone(),
                status: match &session.status {
                    SessionStatus::Thinking => "thinking".to_string(),
                    SessionStatus::PendingTools => "pending_tools".to_string(),
                    SessionStatus::AwaitingClarification { .. } => {
                        "awaiting_clarification".to_string()
                    }
                    SessionStatus::AwaitingApproval { .. } => "awaiting_approval".to_string(),
                    SessionStatus::Executing => "executing".to_string(),
                    SessionStatus::Completed => "completed".to_string(),
                    SessionStatus::Failed(_) => "failed".to_string(),
                },
                archived: session.is_archived(),
            },
            EventLevel::Info,
        );
        Ok(())
    }

    async fn retrieve_session(
        &self,
        session_id: &str,
    ) -> benshu_infra::error::Result<Option<AgentSession>> {
        let data = self
            .engine
            .engram_store()
            .get_session(session_id)
            .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?;

        if let Some(s) = data {
            let mut session: AgentSession = serde_json::from_str(&s)
                .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?;
            if let Some(audit_metadata) = self
                .engine
                .engram_store()
                .latest_session_audit_metadata(session_id)
                .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?
            {
                Self::backfill_background_from_session_audit(&mut session, &audit_metadata);
            }
            Ok(Some(session))
        } else {
            Ok(None)
        }
    }

    async fn delete_session(&self, session_id: &str) -> benshu_infra::error::Result<()> {
        self.engine
            .delete_session(session_id)
            .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?;
        self.emit(
            MemoryEvent::SessionDeleted {
                session_id: session_id.to_string(),
                reason: "explicit_delete".to_string(),
            },
            EventLevel::Info,
        );
        Ok(())
    }

    async fn maintenance(&self) -> benshu_infra::error::Result<()> {
        self.engine
            .engram_store()
            .vacuum()
            .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))
    }

    async fn age_vectors(
        &self,
        _collection: &str,
        older_than_days: usize,
    ) -> benshu_infra::error::Result<()> {
        #[cfg(feature = "vector")]
        {
            self.engine
                .perform_distillation(older_than_days as u32, 0)
                .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?;
            Ok(())
        }
        #[cfg(not(feature = "vector"))]
        {
            let _ = older_than_days;
            Ok(())
        }
    }

    async fn promote_vectors(
        &self,
        collection: &str,
        level: benshu_inference::QuantLevel,
    ) -> benshu_infra::error::Result<()> {
        if collection.is_empty() {
            return Ok(());
        }

        let promoted = self
            .engine
            .promote_collection(
                collection,
                level,
                SUMMARY_SOURCE_BRAIN_MEMORY_MANAGER,
                crate::hybrid_search::PromotionMode::PolicyDriven,
                "brain",
            )
            .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?;
        tracing::debug!(
            "Engram: Promoted collection '{}' toward {:?} across {} documents",
            collection,
            level,
            promoted
        );
        Ok(())
    }

    async fn update_fact_importance(
        &self,
        _user_id: &str,
        _agent_id: Option<&str>,
        fact_id: &str,
        importance: f32,
    ) -> benshu_infra::error::Result<()> {
        let docid = self.resolve_document_id("facts", fact_id)?;
        let _ = self
            .engine
            .engram_store()
            .update_utility(&docid, importance * 0.1);
        Ok(())
    }

    async fn set_fact_protection(
        &self,
        _user_id: &str,
        _agent_id: Option<&str>,
        fact_id: &str,
        protection: FactProtection,
    ) -> benshu_infra::error::Result<()> {
        let mut metadata = std::collections::HashMap::new();
        metadata.insert(
            META_FACT_PROTECTION.to_string(),
            serde_json::to_string(&protection).unwrap_or_else(|_| "\"normal\"".to_string()),
        );
        metadata.insert(
            META_FACT_UPDATED_AT.to_string(),
            Utc::now().timestamp_millis().to_string(),
        );
        self.engine
            .merge_document_metadata("facts", &Self::fact_path(fact_id), metadata)
            .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))
    }

    async fn search_cognitive_guidance(
        &self,
        query: &str,
        limit: usize,
    ) -> benshu_infra::error::Result<(Vec<serde_json::Value>, Vec<serde_json::Value>)> {
        #[cfg(feature = "vector")]
        {
            let emb = self
                .engine
                .embed(query)
                .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?;
            let exps = self
                .engine
                .search_experiences(query, &emb, limit)
                .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?;
            let aps = self
                .engine
                .search_anti_patterns(query, &emb, limit)
                .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?;

            let exp_values = exps
                .into_iter()
                .filter_map(|e| serde_json::to_value(e).ok())
                .collect();
            let ap_values = aps
                .into_iter()
                .filter_map(|e| serde_json::to_value(e).ok())
                .collect();

            Ok((exp_values, ap_values))
        }
        #[cfg(not(feature = "vector"))]
        {
            let _ = (query, limit);
            Ok((Vec::new(), Vec::new()))
        }
    }

    async fn search_experiences(
        &self,
        query: &str,
        limit: usize,
    ) -> benshu_infra::error::Result<Vec<serde_json::Value>> {
        let (exps, _) = MemoryApi::search_cognitive_guidance(self, query, limit).await?;
        Ok(exps)
    }

    async fn search_anti_patterns(
        &self,
        query: &str,
        limit: usize,
    ) -> benshu_infra::error::Result<Vec<serde_json::Value>> {
        let (_, aps) = MemoryApi::search_cognitive_guidance(self, query, limit).await?;
        Ok(aps)
    }

    async fn store_experience(
        &self,
        mut experience: serde_json::Value,
    ) -> benshu_infra::error::Result<()> {
        if let Some(obj) = experience.as_object_mut() {
            obj.insert(
                "created_at_ms".to_string(),
                serde_json::json!(Utc::now().timestamp_millis()),
            );
        }

        let exp: crate::store::Experience = serde_json::from_value(experience).map_err(|e| {
            benshu_infra::error::Error::Internal(format!("Invalid Experience structure: {}", e))
        })?;

        #[cfg(feature = "vector")]
        {
            let emb = self
                .engine
                .embed(&exp.task_query)
                .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?;
            self.engine
                .index_experience(exp, emb)
                .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?;
        }
        #[cfg(not(feature = "vector"))]
        {
            self.engine
                .engram_store()
                .store_experience(exp)
                .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?;
        }
        Ok(())
    }

    async fn store_anti_pattern(
        &self,
        mut anti_pattern: serde_json::Value,
    ) -> benshu_infra::error::Result<()> {
        if let Some(obj) = anti_pattern.as_object_mut() {
            obj.insert(
                "created_at_ms".to_string(),
                serde_json::json!(Utc::now().timestamp_millis()),
            );
        }

        let ap: crate::store::AntiPattern = serde_json::from_value(anti_pattern).map_err(|e| {
            benshu_infra::error::Error::Internal(format!("Invalid AntiPattern structure: {}", e))
        })?;

        #[cfg(feature = "vector")]
        {
            let emb = self
                .engine
                .embed(&ap.error_fingerprint)
                .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?;
            self.engine
                .index_anti_pattern(ap, emb)
                .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?;
        }
        #[cfg(not(feature = "vector"))]
        {
            self.engine
                .engram_store()
                .store_anti_pattern(ap)
                .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?;
        }
        Ok(())
    }

    async fn delete_experience(&self, id: &str) -> benshu_infra::error::Result<()> {
        self.engine
            .engram_store()
            .delete_experience(id)
            .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))
    }

    async fn delete_anti_pattern(&self, id: &str) -> benshu_infra::error::Result<()> {
        self.engine
            .engram_store()
            .delete_anti_pattern(id)
            .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))
    }

    async fn increment_experience_utility(
        &self,
        id: &str,
        increment: f64,
    ) -> benshu_infra::error::Result<()> {
        self.engine
            .engram_store()
            .update_utility(id, increment as f32)
            .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))
    }

    async fn increment_anti_pattern_utility(
        &self,
        id: &str,
        increment: f64,
    ) -> benshu_infra::error::Result<()> {
        self.engine
            .engram_store()
            .update_utility(id, increment as f32)
            .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))
    }

    async fn get_experience(
        &self,
        id: &str,
    ) -> benshu_infra::error::Result<Option<serde_json::Value>> {
        let exp = self
            .engine
            .engram_store()
            .get_experience(id)
            .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?;
        Ok(exp.and_then(|e| serde_json::to_value(e).ok()))
    }

    async fn get_anti_pattern(
        &self,
        id: &str,
    ) -> benshu_infra::error::Result<Option<serde_json::Value>> {
        let ap = self
            .engine
            .engram_store()
            .get_anti_pattern(id)
            .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?;
        Ok(ap.and_then(|e| serde_json::to_value(e).ok()))
    }

    async fn update_utility(
        &self,
        collection: &str,
        fact_id: &str,
        increment: f32,
    ) -> benshu_infra::error::Result<()> {
        let docid = self.resolve_document_id(collection, fact_id)?;
        self.engine
            .engram_store()
            .update_utility(&docid, increment)
            .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))
    }

    async fn get_metadata(&self, key: &str) -> benshu_infra::error::Result<Option<String>> {
        Ok(self.runtime_metadata_value(key))
    }

    fn set_emitter(&self, emitter: Arc<dyn MemoryEmitter>) {
        *self.emitter.write() = Some(emitter);
    }

    fn record_interaction(&self) {}
    fn last_interaction_elapsed(&self) -> std::time::Duration {
        std::time::Duration::from_secs(0)
    }

    fn set_security(&self, security: Arc<dyn SecurityHandler>) {
        *self.security.write() = Some(security);
    }

    fn security(&self) -> Option<Arc<dyn SecurityHandler>> {
        self.security.read().clone()
    }

    async fn mark_verified(&self, fact_id: &str) -> benshu_infra::error::Result<()> {
        self.engine
            .mark_verified("facts", &Self::fact_path(fact_id))
            .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))
    }

    async fn mark_pending_review(
        &self,
        fact_id: &str,
        summary: Option<&str>,
    ) -> benshu_infra::error::Result<()> {
        self.engine
            .mark_pending_review("facts", &Self::fact_path(fact_id), summary)
            .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?;

        let mut metadata = std::collections::HashMap::new();
        metadata.insert(
            META_FACT_REVIEW_REASON.to_string(),
            "auditor_needs_review".to_string(),
        );
        metadata.insert(
            META_FACT_REVIEW_SOURCE.to_string(),
            "memory_auditor".to_string(),
        );
        if let Some(summary) = summary {
            metadata.insert(META_FACT_REVIEW_SUMMARY.to_string(), summary.to_string());
        }
        self.engine
            .merge_document_metadata("facts", &Self::fact_path(fact_id), metadata)
            .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?;
        self.emit(
            MemoryEvent::FactReviewRequested {
                id: fact_id.to_string(),
                source: "memory_auditor".to_string(),
            },
            EventLevel::Info,
        );
        Ok(())
    }

    async fn get_fact_review_payload(
        &self,
        fact_id: &str,
    ) -> benshu_infra::error::Result<Option<FactReviewPayload>> {
        let doc = self
            .engine
            .get_by_path("facts", &Self::fact_path(fact_id))
            .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?;
        Ok(doc.as_ref().and_then(Self::parse_fact_review_payload))
    }

    async fn resolve_pending_review(
        &self,
        fact_id: &str,
        resolution: FactReviewResolution,
    ) -> benshu_infra::error::Result<()> {
        let fact_path = Self::fact_path(fact_id);
        match resolution.outcome {
            FactReviewResolutionOutcome::Verified => {
                self.engine
                    .mark_verified("facts", &fact_path)
                    .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?;
                let mut metadata = Self::review_resolution_metadata(&resolution);
                metadata.insert(
                    META_FACT_STATUS.to_string(),
                    serde_json::to_string(&FactStatus::Verified)
                        .unwrap_or_else(|_| "\"verified\"".to_string()),
                );
                metadata.insert(
                    META_FACT_LIFECYCLE_STATE.to_string(),
                    FACT_LIFECYCLE_VERIFIED.to_string(),
                );
                metadata.insert(
                    META_DOCUMENT_LIFECYCLE_STATE.to_string(),
                    FACT_LIFECYCLE_VERIFIED.to_string(),
                );
                self.engine
                    .merge_document_metadata("facts", &fact_path, metadata)
                    .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?;
            }
            FactReviewResolutionOutcome::Pruned => {
                let mut metadata = Self::review_resolution_metadata(&resolution);
                metadata.insert(
                    META_FACT_STATUS.to_string(),
                    serde_json::to_string(&FactStatus::Archived)
                        .unwrap_or_else(|_| "\"archived\"".to_string()),
                );
                metadata.insert(
                    META_FACT_LIFECYCLE_STATE.to_string(),
                    FACT_LIFECYCLE_PRUNED.to_string(),
                );
                metadata.insert(
                    META_DOCUMENT_LIFECYCLE_STATE.to_string(),
                    FACT_LIFECYCLE_PRUNED.to_string(),
                );
                metadata.insert(META_DOCUMENT_ARCHIVED.to_string(), "true".to_string());
                self.engine
                    .engram_store()
                    .archive_document("facts", &fact_path, metadata)
                    .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?;
            }
            FactReviewResolutionOutcome::PendingReview => {
                MemoryApi::mark_pending_review(
                    self,
                    fact_id,
                    resolution.resolution_reason.as_deref(),
                )
                .await?;
                let metadata = Self::review_resolution_metadata(&resolution);
                self.engine
                    .merge_document_metadata("facts", &fact_path, metadata)
                    .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?;
            }
        }
        self.emit(
            MemoryEvent::FactReviewResolved {
                id: fact_id.to_string(),
                outcome: match resolution.outcome {
                    FactReviewResolutionOutcome::Verified => "verified".to_string(),
                    FactReviewResolutionOutcome::Pruned => "pruned".to_string(),
                    FactReviewResolutionOutcome::PendingReview => "pending_review".to_string(),
                },
                resolved_by: resolution
                    .resolved_by
                    .clone()
                    .unwrap_or_else(|| "engram_memory".to_string()),
            },
            EventLevel::Info,
        );
        Ok(())
    }

    async fn mark_pruned(&self, fact_id: &str) -> benshu_infra::error::Result<()> {
        let fact_path = Self::fact_path(fact_id);
        let now = Utc::now().timestamp_millis();
        let mut metadata = std::collections::HashMap::new();
        metadata.insert(
            META_FACT_STATUS.to_string(),
            serde_json::to_string(&FactStatus::Archived)
                .unwrap_or_else(|_| "\"archived\"".to_string()),
        );
        metadata.insert(
            META_FACT_LIFECYCLE_STATE.to_string(),
            FACT_LIFECYCLE_PRUNED.to_string(),
        );
        metadata.insert(META_FACT_ARCHIVED_AT_MS.to_string(), now.to_string());
        metadata.insert(META_FACT_PRUNED_AT_MS.to_string(), now.to_string());
        metadata.insert(
            META_FACT_PRUNE_REASON.to_string(),
            FACT_PRUNE_REASON_MANUAL.to_string(),
        );
        metadata.insert(
            META_DOCUMENT_LIFECYCLE_STATE.to_string(),
            crate::metadata_contract::DOCUMENT_LIFECYCLE_PRUNED.to_string(),
        );
        metadata.insert(META_DOCUMENT_ARCHIVED.to_string(), "true".to_string());
        self.engine
            .engram_store()
            .archive_document("facts", &fact_path, metadata)
            .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?;
        self.emit(
            MemoryEvent::MemoryPruned {
                entries: 1,
                reason: "fact_marked_pruned".to_string(),
            },
            EventLevel::Info,
        );
        Ok(())
    }

    async fn list_sessions(&self) -> benshu_infra::error::Result<Vec<AgentSession>> {
        let sessions = self
            .engine
            .list_sessions()
            .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?;
        let mut parsed = Vec::with_capacity(sessions.len());
        for (_, raw) in sessions {
            let session: AgentSession = serde_json::from_str(&raw)
                .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?;
            parsed.push(session);
        }
        Ok(parsed)
    }

    async fn fetch_document(
        &self,
        collection: &str,
        path: &str,
    ) -> benshu_infra::error::Result<Option<Document>> {
        let Some(doc) = self
            .engine
            .get_by_path(collection, path)
            .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?
        else {
            return Ok(None);
        };

        let content = self
            .engine
            .engram_store()
            .get_content(&doc)
            .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?
            .unwrap_or_default();

        let metadata = Self::normalize_document_metadata(&doc);

        Ok(Some(Document {
            id: doc.docid.clone(),
            title: self.decrypt_if_needed(&doc.title),
            content: self.decrypt_if_needed(&content),
            summary: doc.summary.clone(),
            collection: Some(doc.collection.clone()),
            path: Some(doc.path.clone()),
            metadata,
            score: 1.0,
        }))
    }

    async fn update_summary(
        &self,
        collection: &str,
        path: &str,
        summary: &str,
    ) -> benshu_infra::error::Result<()> {
        self.engine
            .update_summary(collection, path, summary)
            .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?;

        let mut metadata = std::collections::HashMap::new();
        metadata.insert(
            META_SUMMARY_UPDATED_AT_MS.to_string(),
            Utc::now().timestamp_millis().to_string(),
        );
        metadata.insert(
            META_SUMMARY_SOURCE.to_string(),
            SUMMARY_SOURCE_BRAIN_MEMORY_MANAGER.to_string(),
        );
        metadata.insert(
            META_DOCUMENT_SUMMARY_STATE.to_string(),
            DOCUMENT_SUMMARY_STATE_READY.to_string(),
        );
        metadata.insert(
            META_DOCUMENT_LIFECYCLE_STATE.to_string(),
            DOCUMENT_LIFECYCLE_SUMMARIZED.to_string(),
        );
        metadata.insert(
            META_DOCUMENT_CONTRACT_VERSION.to_string(),
            DOCUMENT_CONTRACT_VERSION.to_string(),
        );
        self.engine
            .merge_document_metadata(collection, path, metadata)
            .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))?;
        self.emit(
            MemoryEvent::DocumentSummaryUpdated {
                collection: collection.to_string(),
                path: path.to_string(),
                state: "ready".to_string(),
            },
            EventLevel::Info,
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hybrid_search::HybridSearchConfig;
    use crate::metadata_contract::{
        DocumentMetadataView, FactMetadataView, MultimodalMetadataView, PromotionMetadataView,
        RuntimeMetadataView, SessionAuditMetadataView, AUDIT_KIND_SESSION_ARCHIVED,
        AUDIT_KIND_SESSION_RECOVERED, COLLECTION_SESSION_AUDIT, DOCUMENT_LIFECYCLE_ARCHIVED,
        DOCUMENT_LIFECYCLE_PENDING_REVIEW, DOCUMENT_LIFECYCLE_PRUNED, DOCUMENT_SUMMARY_STATE_READY,
        ENGRAM_CONTRACT_ROLE_DURABLE_LONG_TERM_AUTHORITY_UNDER_BRAIN_POLICY,
        FACT_PRUNE_REASON_MANUAL, META_DOCUMENT_ARCHIVE_REASON, PROMOTION_CONTRACT_VERSION,
        TIER_DURABLE_AUTHORITY_ENGRAM,
    };
    use benshu_infra::traits::memory::{MemoryEmitter, MemoryEvent};
    use benshu_memory_core::{BackgroundEnvelope, BackgroundEvidenceRef, BackgroundRevision};
    use parking_lot::Mutex;
    use std::sync::Arc as StdArc;
    use tempfile::tempdir;

    #[derive(Default)]
    struct RecordingEmitter {
        events: Mutex<Vec<MemoryEvent>>,
    }

    impl RecordingEmitter {
        fn snapshot(&self) -> Vec<MemoryEvent> {
            self.events.lock().clone()
        }
    }

    impl MemoryEmitter for RecordingEmitter {
        fn emit(&self, event: MemoryEvent, _level: EventLevel) {
            self.events.lock().push(event);
        }
    }

    #[tokio::test]
    async fn fact_metadata_round_trips_through_engram() {
        let temp = tempdir().expect("tempdir");
        let mut config = HybridSearchConfig::default();
        config.db_path = temp.path().join("engram-memory.db");
        config.use_vector = false;
        config.use_reranker = false;

        let engine = Arc::new(HybridSearchEngine::new(config, None).expect("engine"));
        let memory = EngramMemory::new(engine);

        let created_at = Utc::now() - chrono::Duration::minutes(10);
        let updated_at = Utc::now();
        let fact = Fact {
            id: "fact-round-trip".to_string(),
            category: "facts".to_string(),
            content: "Prime ownership should remain with BenShu.".to_string(),
            importance: 0.8,
            created_at,
            updated_at,
            verified: false,
            source: Some("session-123".to_string()),
            confidence: 0.76,
            relations: vec![benshu_memory_core::Relation {
                predicate: "supports".to_string(),
                target_id: "prime-agent".to_string(),
                strength: 0.9,
            }],
            semantic_hash: Some("semantic-hash-1".to_string()),
            status: FactStatus::PendingReview,
            protection: FactProtection::Protected,
        };

        memory
            .store_fact("user", None, fact.clone())
            .await
            .expect("store fact");

        let stored = memory
            .list_unverified(None, 10)
            .await
            .expect("list unverified")
            .into_iter()
            .find(|item| item.id == fact.id)
            .expect("fact exists");

        assert_eq!(stored.source.as_deref(), Some("session-123"));
        assert!((stored.confidence - 0.76).abs() < f32::EPSILON);
        assert!(matches!(stored.status, FactStatus::PendingReview));
        assert_eq!(stored.relations.len(), 1);
        assert_eq!(stored.semantic_hash.as_deref(), Some("semantic-hash-1"));
        assert!((stored.importance - 0.8).abs() < 0.001);
        assert!(matches!(stored.protection, FactProtection::Protected));
    }

    #[tokio::test]
    async fn mark_pending_review_updates_fact_status_and_metadata() {
        let temp = tempdir().expect("tempdir");
        let mut config = HybridSearchConfig::default();
        config.db_path = temp.path().join("engram-pending-review.db");
        config.use_vector = false;
        config.use_reranker = false;

        let engine = Arc::new(HybridSearchEngine::new(config, None).expect("engine"));
        let memory = EngramMemory::new(engine.clone());

        let fact = Fact {
            id: "fact-pending-review".to_string(),
            category: "facts".to_string(),
            content: "This fact should be flagged for challenger review.".to_string(),
            importance: 0.5,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            verified: true,
            source: Some("session-review".to_string()),
            confidence: 0.4,
            relations: Vec::new(),
            semantic_hash: Some("fact-pending-review".to_string()),
            status: FactStatus::Verified,
            protection: FactProtection::Normal,
        };

        memory
            .store_fact("user", None, fact)
            .await
            .expect("store fact");
        memory
            .mark_pending_review("fact-pending-review", Some("challenger detected mismatch"))
            .await
            .expect("mark pending review");

        let stored = memory
            .list_unverified(None, 10)
            .await
            .expect("list unverified")
            .into_iter()
            .find(|item| item.id == "fact-pending-review")
            .expect("fact exists");
        assert!(matches!(stored.status, FactStatus::PendingReview));
        assert!(!stored.verified);

        let doc = engine
            .get_by_path("facts", "fact/fact-pending-review")
            .expect("doc lookup")
            .expect("document should exist");
        assert_eq!(
            doc.metadata
                .get(META_FACT_REVIEW_SUMMARY)
                .map(String::as_str),
            Some("challenger detected mismatch")
        );
        assert_eq!(
            DocumentMetadataView::new(&doc.metadata, false, false)
                .lifecycle_state()
                .as_str(),
            DOCUMENT_LIFECYCLE_PENDING_REVIEW
        );
        assert!(doc.unverified);
    }

    #[tokio::test]
    async fn resolve_pending_review_records_resolution_metadata() {
        let temp = tempdir().expect("tempdir");
        let mut config = HybridSearchConfig::default();
        config.db_path = temp.path().join("engram-review-resolution.db");
        config.use_vector = false;
        config.use_reranker = false;

        let engine = Arc::new(HybridSearchEngine::new(config, None).expect("engine"));
        let memory = EngramMemory::new(engine.clone());

        let fact = Fact {
            id: "fact-review-resolution".to_string(),
            category: "facts".to_string(),
            content: "This fact needs challenger resolution.".to_string(),
            importance: 0.5,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            verified: false,
            source: Some("session-review".to_string()),
            confidence: 0.5,
            relations: Vec::new(),
            semantic_hash: Some("fact-review-resolution".to_string()),
            status: FactStatus::Pending,
            protection: FactProtection::Normal,
        };

        memory
            .store_fact("user", None, fact)
            .await
            .expect("store fact");
        memory
            .mark_pending_review(
                "fact-review-resolution",
                Some("challenger detected mismatch"),
            )
            .await
            .expect("mark pending review");

        let resolution = FactReviewResolution {
            outcome: FactReviewResolutionOutcome::Verified,
            resolution_reason: Some("challenger accepted revised summary".to_string()),
            resolution_basis: Some("challenger_re_summary".to_string()),
            resolved_by: Some("sleep_consolidator_challenger".to_string()),
            resolved_at: Utc::now(),
        };
        memory
            .resolve_pending_review("fact-review-resolution", resolution)
            .await
            .expect("resolve pending review");

        let payload = memory
            .get_fact_review_payload("fact-review-resolution")
            .await
            .expect("payload lookup")
            .expect("payload exists");
        let resolved = payload.resolution.expect("resolution exists");
        assert!(matches!(
            resolved.outcome,
            FactReviewResolutionOutcome::Verified
        ));
        assert_eq!(
            resolved.resolution_basis.as_deref(),
            Some("challenger_re_summary")
        );

        let stored = memory
            .retrieve_facts("user", None)
            .await
            .expect("retrieve facts")
            .into_iter()
            .find(|item| item.id == "fact-review-resolution")
            .expect("fact exists");
        assert!(matches!(stored.status, FactStatus::Verified));
        assert!(stored.verified);

        let doc = engine
            .get_by_path("facts", "fact/fact-review-resolution")
            .expect("doc lookup")
            .expect("document exists");
        assert_eq!(
            doc.metadata
                .get(META_FACT_REVIEW_RESOLUTION_OUTCOME)
                .map(String::as_str),
            Some(FACT_REVIEW_OUTCOME_VERIFIED)
        );
    }

    #[tokio::test]
    async fn update_summary_records_document_contract_metadata() {
        let temp = tempdir().expect("tempdir");
        let mut config = HybridSearchConfig::default();
        config.db_path = temp.path().join("engram-summary.db");
        config.use_vector = false;
        config.use_reranker = false;

        let engine = Arc::new(HybridSearchEngine::new(config, None).expect("engine"));
        engine
            .engram_store()
            .store_document(
                "docs",
                "architecture.md",
                "Architecture",
                "Prime agent notes.",
                false,
                Default::default(),
            )
            .expect("store document");
        let memory = EngramMemory::new(engine.clone());

        memory
            .update_summary("docs", "architecture.md", "Prime agent summary")
            .await
            .expect("update summary");

        let doc = memory
            .fetch_document("docs", "architecture.md")
            .await
            .expect("fetch document")
            .expect("document exists");
        assert_eq!(doc.summary.as_deref(), Some("Prime agent summary"));
        let document = DocumentMetadataView::new(&doc.metadata, true, false);
        assert_eq!(document.contract_version(), DOCUMENT_CONTRACT_VERSION);
        assert_eq!(document.summary_state(), Some(DOCUMENT_SUMMARY_STATE_READY));
        assert_eq!(
            document.summary_source(),
            Some(SUMMARY_SOURCE_BRAIN_MEMORY_MANAGER)
        );
        assert!(document.has_summary());
    }

    #[tokio::test]
    async fn session_lifecycle_round_trips_through_engram() {
        let temp = tempdir().expect("tempdir");
        let mut config = HybridSearchConfig::default();
        config.db_path = temp.path().join("engram-session.db");
        config.use_vector = false;
        config.use_reranker = false;

        let engine = Arc::new(HybridSearchEngine::new(config, None).expect("engine"));
        let memory = EngramMemory::new(engine);

        let mut session = AgentSession::new("session-round-trip".to_string());
        session.status = SessionStatus::Completed;
        let retention_until = Utc::now() + chrono::Duration::days(7);
        session.archive(
            Some("completed for archive".to_string()),
            Some(retention_until),
        );

        memory
            .store_session(session.clone())
            .await
            .expect("store session");

        let stored = memory
            .retrieve_session("session-round-trip")
            .await
            .expect("retrieve session")
            .expect("session exists");

        assert!(stored.is_archived());
        assert_eq!(
            stored.lifecycle.archive_reason.as_deref(),
            Some("completed for archive")
        );
        assert_eq!(stored.lifecycle.retention_until, Some(retention_until));
        assert!(matches!(stored.status, SessionStatus::Completed));
    }

    #[tokio::test]
    async fn background_envelope_metadata_round_trips_through_engram() {
        let temp = tempdir().expect("tempdir");
        let mut config = HybridSearchConfig::default();
        config.db_path = temp.path().join("engram-background-metadata-roundtrip.db");
        config.use_vector = false;
        config.use_reranker = false;

        let engine = Arc::new(HybridSearchEngine::new(config, None).expect("engine"));
        let memory = EngramMemory::new(engine);

        let mut session = AgentSession::new("session-background-metadata-roundtrip".to_string());
        session.background_envelope = Some(BackgroundEnvelope {
            revision: BackgroundRevision {
                revision: 7,
                previous_revision: Some(6),
                updated_at: Utc::now(),
                update_reason: Some("metadata-roundtrip".to_string()),
            },
            metadata: HashMap::from([
                (
                    "background_session_lifecycle_state".to_string(),
                    "active".to_string(),
                ),
                (
                    "background_contract_complete".to_string(),
                    "true".to_string(),
                ),
                (
                    "background_quality_source".to_string(),
                    "rule_based".to_string(),
                ),
            ]),
            ..Default::default()
        });

        memory.store_session(session).await.expect("store session");

        let retrieved = memory
            .retrieve_session("session-background-metadata-roundtrip")
            .await
            .expect("retrieve session")
            .expect("session exists");
        let background = retrieved
            .background_envelope
            .as_ref()
            .expect("background exists");

        assert_eq!(background.revision.revision, 7);
        assert_eq!(
            background
                .metadata
                .get("background_session_lifecycle_state")
                .map(String::as_str),
            Some("active")
        );
        assert_eq!(
            background
                .metadata
                .get("background_contract_complete")
                .map(String::as_str),
            Some("true")
        );
        assert_eq!(
            background
                .metadata
                .get("background_quality_source")
                .map(String::as_str),
            Some("rule_based")
        );
    }

    #[tokio::test]
    async fn background_evidence_refs_round_trip_through_engram() {
        let temp = tempdir().expect("tempdir");
        let mut config = HybridSearchConfig::default();
        config.db_path = temp.path().join("engram-background-evidence-roundtrip.db");
        config.use_vector = false;
        config.use_reranker = false;

        let engine = Arc::new(HybridSearchEngine::new(config, None).expect("engine"));
        let memory = EngramMemory::new(engine);

        let mut session = AgentSession::new("session-background-evidence-roundtrip".to_string());
        session.background_envelope = Some(BackgroundEnvelope {
            revision: BackgroundRevision {
                revision: 3,
                previous_revision: Some(2),
                updated_at: Utc::now(),
                update_reason: Some("evidence-roundtrip".to_string()),
            },
            source_refs: vec![
                BackgroundEvidenceRef {
                    source_kind: "message".to_string(),
                    source_id: "msg-1".to_string(),
                    confidence: Some(0.82),
                    occurred_at: Some(Utc::now() - chrono::Duration::minutes(5)),
                    metadata: HashMap::from([("role".to_string(), "user".to_string())]),
                },
                BackgroundEvidenceRef {
                    source_kind: "fact".to_string(),
                    source_id: "fact-relationship".to_string(),
                    confidence: Some(0.91),
                    occurred_at: Some(Utc::now() - chrono::Duration::minutes(2)),
                    metadata: HashMap::from([(
                        "review_state".to_string(),
                        "pending_review".to_string(),
                    )]),
                },
            ],
            ..Default::default()
        });

        memory.store_session(session).await.expect("store session");

        let retrieved = memory
            .retrieve_session("session-background-evidence-roundtrip")
            .await
            .expect("retrieve session")
            .expect("session exists");
        let background = retrieved
            .background_envelope
            .as_ref()
            .expect("background exists");

        assert_eq!(background.source_refs.len(), 2);
        assert_eq!(background.source_refs[0].source_kind, "message");
        assert_eq!(background.source_refs[0].source_id, "msg-1");
        assert_eq!(background.source_refs[0].confidence, Some(0.82));
        assert_eq!(
            background.source_refs[0]
                .metadata
                .get("role")
                .map(String::as_str),
            Some("user")
        );
        assert_eq!(background.source_refs[1].source_kind, "fact");
        assert_eq!(background.source_refs[1].source_id, "fact-relationship");
        assert_eq!(
            background.source_refs[1]
                .metadata
                .get("review_state")
                .map(String::as_str),
            Some("pending_review")
        );
    }

    #[tokio::test]
    async fn engram_memory_emits_session_and_document_events() {
        let temp = tempdir().expect("tempdir");
        let mut config = HybridSearchConfig::default();
        config.db_path = temp.path().join("engram-emitter.db");
        config.use_vector = false;
        config.use_reranker = false;

        let engine = Arc::new(HybridSearchEngine::new(config, None).expect("engine"));
        engine
            .engram_store()
            .store_document(
                "docs",
                "emit.md",
                "Emit",
                "Emit summary state.",
                false,
                Default::default(),
            )
            .expect("store document");
        let memory = EngramMemory::new(engine);
        let emitter = StdArc::new(RecordingEmitter::default());
        memory.set_emitter(emitter.clone());

        let mut session = AgentSession::new("session-emit".to_string());
        session.status = SessionStatus::Completed;
        memory.store_session(session).await.expect("store session");
        memory
            .update_summary("docs", "emit.md", "ready summary")
            .await
            .expect("update summary");
        memory
            .delete_session("session-emit")
            .await
            .expect("delete session");

        let events = emitter.snapshot();
        assert!(events.iter().any(|event| matches!(
            event,
            MemoryEvent::SessionStored { session_id, status, archived }
            if session_id == "session-emit" && status == "completed" && !archived
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            MemoryEvent::DocumentSummaryUpdated { collection, path, state }
            if collection == "docs" && path == "emit.md" && state == "ready"
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            MemoryEvent::SessionDeleted { session_id, reason }
            if session_id == "session-emit" && reason == "explicit_delete"
        )));
    }

    #[tokio::test]
    async fn promote_vectors_records_tier_signal_metadata() {
        let temp = tempdir().expect("tempdir");
        let mut config = HybridSearchConfig::default();
        config.db_path = temp.path().join("engram-promotion.db");
        config.use_vector = false;
        config.use_reranker = false;

        let engine = Arc::new(HybridSearchEngine::new(config, None).expect("engine"));
        engine
            .engram_store()
            .store_document(
                "docs",
                "promotion.md",
                "Promotion",
                "Tier promotion should leave an audit trail.",
                false,
                Default::default(),
            )
            .expect("store document");
        let memory = EngramMemory::new(engine.clone());

        memory
            .promote_vectors("docs", QuantLevel::Warm)
            .await
            .expect("promote vectors");

        let doc = engine
            .get_by_path("docs", "promotion.md")
            .expect("get by path")
            .expect("document exists");
        let promotion = PromotionMetadataView::new(&doc.metadata);
        assert_eq!(promotion.target(), Some("warm"));
        assert_eq!(
            promotion.source(),
            Some(SUMMARY_SOURCE_BRAIN_MEMORY_MANAGER)
        );
        assert_eq!(
            promotion.contract_version(),
            Some(PROMOTION_CONTRACT_VERSION)
        );
        assert_eq!(promotion.mode(), Some("policy_driven"));
        assert_eq!(promotion.policy_owner(), Some("brain"));
        assert_eq!(
            promotion.durable_authority(),
            Some(TIER_DURABLE_AUTHORITY_ENGRAM)
        );
        assert!(promotion.promoted_at_ms().is_some());
        assert!(doc.utility_score > 0.0);

        let stats = engine.stats();
        assert_eq!(stats.promotion_operation_count, 1);
        assert_eq!(stats.promotion_document_count, 1);
        assert_eq!(stats.promotion_last_source, "brain_memory_manager");
        assert_eq!(stats.promotion_last_target, "warm");
        assert_eq!(stats.promotion_last_mode, "policy_driven");
        assert_eq!(stats.promotion_last_policy_owner, "brain");
        assert!(stats
            .promotion_counts_by_source_target_json
            .contains("brain_memory_manager->warm"));
    }

    #[tokio::test]
    async fn runtime_metadata_exposes_vector_governance_fields() {
        let temp = tempdir().expect("tempdir");
        let mut config = HybridSearchConfig::default();
        config.db_path = temp.path().join("engram-runtime-meta.db");
        config.use_vector = false;
        config.use_reranker = false;

        let engine = Arc::new(HybridSearchEngine::new(config, None).expect("engine"));
        let memory = EngramMemory::new(engine);
        let metadata = memory.runtime_metadata_snapshot();
        let runtime = RuntimeMetadataView::new(&metadata);

        assert_eq!(
            runtime.contract_role(),
            Some(ENGRAM_CONTRACT_ROLE_DURABLE_LONG_TERM_AUTHORITY_UNDER_BRAIN_POLICY)
        );
        assert_eq!(runtime.fact_version(), Some(FACT_CONTRACT_VERSION));
        assert_eq!(runtime.document_version(), Some(DOCUMENT_CONTRACT_VERSION));
        assert_eq!(runtime.session_version(), Some(SESSION_CONTRACT_VERSION));
        assert_eq!(
            runtime.promotion_version(),
            Some(PROMOTION_CONTRACT_VERSION)
        );
        assert_eq!(runtime.vector_execution_profile(), Some("ann_rescore"));
        assert_eq!(runtime.vector_snapshot_load_count(), Some("0"));
        assert_eq!(runtime.vector_exact_scan_fallback_rate(), Some("0"));
        assert_eq!(runtime.vector_quantized_decode_fallback_count(), Some("0"));
        assert_eq!(runtime.vector_quantized_decode_fallback_rate(), Some("0"));
        assert_eq!(runtime.vector_tombstone_count(), Some("0"));
        assert_eq!(runtime.vector_tombstone_ratio(), Some("0"));
        assert_eq!(
            runtime.vector_search_latency_by_metric_and_collection(),
            Some("{}")
        );
        assert_eq!(runtime.search_total_documents(), Some("0"));
        assert_eq!(runtime.session_archive_count(), Some("0"));
        assert_eq!(runtime.session_recovery_count(), Some("0"));
        assert_eq!(runtime.session_background_archive_count(), Some("0"));
        assert_eq!(runtime.session_background_recovery_count(), Some("0"));
        assert_eq!(runtime.prune_counts_by_reason(), Some("{}"));
        let retention_policy = runtime.retention_policy().expect("retention policy");
        assert!(retention_policy.contains("session_prune_after_days"));
        let retention_last_run = runtime.retention_last_run().expect("retention last run");
        assert!(retention_last_run.contains("\"run_at_ms\":0"));
        assert_eq!(runtime.promotion_operation_count(), Some("0"));
    }

    #[tokio::test]
    async fn runtime_metadata_exposes_promotion_contract_and_counts() {
        let temp = tempdir().expect("tempdir");
        let mut config = HybridSearchConfig::default();
        config.db_path = temp.path().join("engram-promotion-meta.db");
        config.use_vector = false;
        config.use_reranker = false;

        let engine = Arc::new(HybridSearchEngine::new(config, None).expect("engine"));
        engine
            .engram_store()
            .store_document(
                "docs",
                "promotion-telemetry.md",
                "Promotion Telemetry",
                "Promotion telemetry should surface to brain.",
                false,
                Default::default(),
            )
            .expect("store document");
        let memory = EngramMemory::new(engine);
        memory
            .promote_vectors("docs", QuantLevel::Warm)
            .await
            .expect("promote vectors");
        let metadata = memory.runtime_metadata_snapshot();
        let runtime = RuntimeMetadataView::new(&metadata);

        assert_eq!(runtime.promotion_operation_count(), Some("1"));
        assert_eq!(runtime.promotion_document_count(), Some("1"));
        assert_eq!(
            runtime.promotion_last_source(),
            Some("brain_memory_manager")
        );
        assert_eq!(runtime.promotion_last_target(), Some("warm"));
        assert_eq!(runtime.promotion_last_mode(), Some("policy_driven"));
        assert_eq!(runtime.promotion_last_policy_owner(), Some("brain"));
        assert!(runtime
            .promotion_counts_by_source_target()
            .expect("json")
            .contains("brain_memory_manager->warm"));
    }

    #[tokio::test]
    async fn store_session_records_archive_and_recovery_audit_counts() {
        let temp = tempdir().expect("tempdir");
        let mut config = HybridSearchConfig::default();
        config.db_path = temp.path().join("engram-session-audit-counts.db");
        config.use_vector = false;
        config.use_reranker = false;

        let engine = Arc::new(HybridSearchEngine::new(config, None).expect("engine"));
        let memory = EngramMemory::new(engine.clone());

        let session_id = "session-audit-counts";
        let mut archived = AgentSession::new(session_id.to_string());
        archived.background_envelope = Some(BackgroundEnvelope {
            revision: BackgroundRevision {
                revision: 2,
                previous_revision: Some(1),
                updated_at: Utc::now(),
                update_reason: Some("archive-ready".to_string()),
            },
            metadata: std::collections::HashMap::from([(
                "background_session_lifecycle_state".to_string(),
                "archived".to_string(),
            )]),
            ..Default::default()
        });
        archived.archive(
            Some("policy archive".to_string()),
            Some(Utc::now() + chrono::Duration::days(1)),
        );
        memory
            .store_session(archived.clone())
            .await
            .expect("store archived session");

        let mut recovered = archived.clone();
        recovered.mark_recovered("engram");
        if let Some(background) = recovered.background_envelope.as_mut() {
            background.metadata.insert(
                "background_session_lifecycle_state".to_string(),
                "recovered".to_string(),
            );
        }
        memory
            .store_session(recovered)
            .await
            .expect("store recovered session");
        let metadata = memory.runtime_metadata_snapshot();
        let runtime = RuntimeMetadataView::new(&metadata);

        assert_eq!(runtime.session_archive_count(), Some("1"));
        assert_eq!(runtime.session_recovery_count(), Some("1"));
        assert_eq!(runtime.session_background_archive_count(), Some("1"));
        assert_eq!(runtime.session_background_recovery_count(), Some("1"));

        let audits = engine
            .engram_store()
            .fetch_all_docs_legacy()
            .expect("fetch docs");
        assert!(audits.iter().any(|doc| {
            let audit = SessionAuditMetadataView::new(&doc.metadata);
            doc.collection == COLLECTION_SESSION_AUDIT
                && audit.audit_kind() == Some(AUDIT_KIND_SESSION_ARCHIVED)
                && audit.session_id() == Some(session_id)
                && audit.background_present() == Some("true")
                && audit.background_lifecycle_state() == Some("archived")
                && audit.background_revision() == Some("2")
        }));
        assert!(audits.iter().any(|doc| {
            let audit = SessionAuditMetadataView::new(&doc.metadata);
            doc.collection == COLLECTION_SESSION_AUDIT
                && audit.audit_kind() == Some(AUDIT_KIND_SESSION_RECOVERED)
                && audit.session_id() == Some(session_id)
                && audit.background_present() == Some("true")
                && audit.background_lifecycle_state() == Some("recovered")
                && audit.background_revision() == Some("2")
        }));
    }

    #[tokio::test]
    async fn retrieve_session_backfills_background_metadata_from_latest_audit() {
        let temp = tempdir().expect("tempdir");
        let mut config = HybridSearchConfig::default();
        config.db_path = temp.path().join("engram-session-background-backfill.db");
        config.use_vector = false;
        config.use_reranker = false;

        let engine = Arc::new(HybridSearchEngine::new(config, None).expect("engine"));
        let memory = EngramMemory::new(engine.clone());

        let session_id = "session-background-backfill";
        let mut stored = AgentSession::new(session_id.to_string());
        stored.background_envelope = Some(BackgroundEnvelope {
            revision: BackgroundRevision {
                revision: 0,
                previous_revision: None,
                updated_at: Utc::now(),
                update_reason: Some("seed".to_string()),
            },
            ..Default::default()
        });
        memory
            .store_session(stored)
            .await
            .expect("store base session");

        let mut audited = AgentSession::new(session_id.to_string());
        audited.archive(Some("audit archive".to_string()), None);
        audited.background_envelope = Some(BackgroundEnvelope {
            revision: BackgroundRevision {
                revision: 4,
                previous_revision: Some(3),
                updated_at: Utc::now(),
                update_reason: Some("audit revision".to_string()),
            },
            metadata: HashMap::from([(
                "background_session_lifecycle_state".to_string(),
                "archived".to_string(),
            )]),
            ..Default::default()
        });
        engine
            .engram_store()
            .record_session_audit(
                &audited,
                AUDIT_KIND_SESSION_ARCHIVED,
                Some("audit archive"),
                Utc::now().timestamp_millis(),
                Default::default(),
            )
            .expect("record audit");

        let retrieved = memory
            .retrieve_session(session_id)
            .await
            .expect("retrieve session")
            .expect("session exists");
        let background = retrieved
            .background_envelope
            .as_ref()
            .expect("background exists");

        assert_eq!(background.revision.revision, 4);
        assert_eq!(
            background
                .metadata
                .get("background_session_lifecycle_state")
                .map(String::as_str),
            Some("archived")
        );
    }

    #[tokio::test]
    async fn fetch_document_surfaces_transient_context_role() {
        let temp = tempdir().expect("tempdir");
        let mut config = HybridSearchConfig::default();
        config.db_path = temp.path().join("engram-transient-doc.db");
        config.use_vector = false;
        config.use_reranker = false;

        let engine = Arc::new(HybridSearchEngine::new(config, None).expect("engine"));
        engine
            .engram_store()
            .store_document(
                "docs",
                "transient.md",
                "Transient",
                "Temporary attachment context",
                false,
                {
                    let mut metadata = std::collections::HashMap::new();
                    metadata.insert(
                        META_DOCUMENT_PERSISTENCE_SCOPE.to_string(),
                        DOCUMENT_PERSISTENCE_SCOPE_TRANSIENT.to_string(),
                    );
                    metadata.insert(
                        META_DOCUMENT_LIFECYCLE_STATE.to_string(),
                        DOCUMENT_LIFECYCLE_ACTIVE.to_string(),
                    );
                    metadata
                },
            )
            .expect("store transient document");
        let memory = EngramMemory::new(engine);

        let doc = memory
            .fetch_document("docs", "transient.md")
            .await
            .expect("fetch")
            .expect("document");
        let document = DocumentMetadataView::new(&doc.metadata, false, false);
        assert_eq!(
            document.context_role().as_str(),
            DOCUMENT_CONTEXT_ROLE_TRANSIENT_CONTEXT
        );
    }

    #[tokio::test]
    async fn store_knowledge_records_durable_document_contract_metadata() {
        let temp = tempdir().expect("tempdir");
        let mut config = HybridSearchConfig::default();
        config.db_path = temp.path().join("engram-store-knowledge.db");
        config.use_vector = false;
        config.use_reranker = false;

        let engine = Arc::new(HybridSearchEngine::new(config, None).expect("engine"));
        let memory = EngramMemory::new(engine.clone());

        memory
            .store_knowledge(
                "user",
                None,
                "Prime Notes",
                "Durable notes should remain under brain policy with engram as authority.",
                "docs",
                false,
            )
            .await
            .expect("store knowledge");

        let docs = engine
            .engram_store()
            .list_documents_in_collection("docs")
            .expect("list docs");
        let stored = docs
            .iter()
            .find(|doc| doc.title == "Prime Notes")
            .expect("stored doc");
        let document = DocumentMetadataView::new(&stored.metadata, false, false);
        assert_eq!(document.contract_version(), DOCUMENT_CONTRACT_VERSION);
        assert_eq!(document.policy_owner(), DOCUMENT_POLICY_OWNER_BRAIN);
        assert_eq!(
            document.durable_authority(),
            DOCUMENT_DURABLE_AUTHORITY_ENGRAM
        );
        assert_eq!(
            document.persistence_scope(),
            DOCUMENT_PERSISTENCE_SCOPE_DURABLE
        );
        assert_eq!(
            document.context_role(),
            DOCUMENT_CONTEXT_ROLE_DURABLE_DOCUMENT
        );
    }

    #[tokio::test]
    async fn store_multimodal_memory_records_contract_metadata() {
        let temp = tempdir().expect("tempdir");
        let mut config = HybridSearchConfig::default();
        config.db_path = temp.path().join("engram-multimodal-memory.db");
        config.use_vector = false;
        config.use_reranker = false;

        let engine = Arc::new(HybridSearchEngine::new(config, None).expect("engine"));
        let memory = EngramMemory::new(engine.clone());

        let stored = memory
            .store_multimodal_memory(
                "user",
                None,
                MultimodalMemoryRecord {
                    kind: MultimodalMemoryKind::GenerationProvenance,
                    modality: "image".to_string(),
                    title: "Generated Banner".to_string(),
                    summary: "Banner image generated from launch prompt.".to_string(),
                    content:
                        "Generated banner image with BenShu launch typography and warm lighting."
                            .to_string(),
                    collection: "multimodal".to_string(),
                    source_path: None,
                    source_url: Some("https://example.com/generated/banner.png".to_string()),
                    route: Some("image_generation_tool".to_string()),
                    model: Some("local-image-model".to_string()),
                    prompt: Some("Create a launch banner".to_string()),
                    artifact_locator: Some("artifact://generated/banner.png".to_string()),
                    transient: false,
                    derived_fact: None,
                    metadata: HashMap::new(),
                },
            )
            .await
            .expect("multimodal store");

        let doc = memory
            .fetch_document(
                stored.collection.as_deref().unwrap_or("multimodal"),
                stored.path.as_deref().unwrap_or("unknown"),
            )
            .await
            .expect("fetch")
            .expect("stored document");

        let multimodal = MultimodalMetadataView::new(&doc.metadata);
        assert_eq!(
            multimodal.contract_version(),
            Some(MULTIMODAL_CONTRACT_VERSION)
        );
        assert_eq!(multimodal.modality(), Some("image"));
        assert_eq!(multimodal.route(), Some("image_generation_tool"));
        assert_eq!(
            multimodal.ingest_source(),
            Some(DOCUMENT_INGEST_SOURCE_BRAIN_MULTIMODAL_WRITEBACK)
        );
        assert_eq!(
            doc.summary.as_deref(),
            Some("Banner image generated from launch prompt.")
        );
    }

    #[tokio::test]
    async fn store_fact_records_durable_fact_contract_metadata() {
        let temp = tempdir().expect("tempdir");
        let mut config = HybridSearchConfig::default();
        config.db_path = temp.path().join("engram-fact-contract.db");
        config.use_vector = false;
        config.use_reranker = false;

        let engine = Arc::new(HybridSearchEngine::new(config, None).expect("engine"));
        let memory = EngramMemory::new(engine.clone());

        let fact = Fact {
            id: "fact-contract".to_string(),
            category: "identity".to_string(),
            content: "Prime identity contract".to_string(),
            importance: 0.6,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            verified: true,
            source: Some("session-contract".to_string()),
            confidence: 0.9,
            relations: Vec::new(),
            semantic_hash: Some("contract-hash".to_string()),
            status: FactStatus::Verified,
            protection: FactProtection::Normal,
        };

        memory
            .store_fact("user", None, fact)
            .await
            .expect("store fact");

        let doc = engine
            .get_by_path("facts", "fact/fact-contract")
            .expect("get by path")
            .expect("fact doc");
        let fact = FactMetadataView::new(
            &doc.metadata,
            doc.unverified,
            doc.created_at_ms,
            doc.updated_at_ms,
        );
        assert_eq!(fact.contract_version(), FACT_CONTRACT_VERSION);
        assert_eq!(fact.policy_owner(), FACT_POLICY_OWNER_BRAIN);
        assert_eq!(fact.hot_authority(), FACT_HOT_AUTHORITY_BRAIN_HOT_MEMORY);
        assert_eq!(fact.durable_authority(), FACT_DURABLE_AUTHORITY_ENGRAM);
        assert_eq!(fact.persistence_scope(), FACT_PERSISTENCE_SCOPE_DURABLE);
    }

    #[tokio::test]
    async fn mark_pruned_archives_fact_and_emits_prune_audit_event() {
        let temp = tempdir().expect("tempdir");
        let mut config = HybridSearchConfig::default();
        config.db_path = temp.path().join("engram-prune-fact.db");
        config.use_vector = false;
        config.use_reranker = false;

        let engine = Arc::new(HybridSearchEngine::new(config, None).expect("engine"));
        let memory = EngramMemory::new(engine.clone());
        let emitter = StdArc::new(RecordingEmitter::default());
        memory.set_emitter(emitter.clone());

        let fact = Fact {
            id: "fact-pruned".to_string(),
            category: "work".to_string(),
            content: "Outdated operational fact".to_string(),
            importance: 0.2,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            verified: false,
            source: Some("session-prune".to_string()),
            confidence: 0.4,
            relations: Vec::new(),
            semantic_hash: Some("pruned-hash".to_string()),
            status: FactStatus::Pending,
            protection: FactProtection::Normal,
        };

        memory
            .store_fact("user", None, fact)
            .await
            .expect("store fact");
        memory
            .mark_pruned("fact-pruned")
            .await
            .expect("mark pruned");

        let stored = engine
            .get_by_path("facts", "fact/fact-pruned")
            .expect("get by path")
            .expect("fact doc");
        let fact = FactMetadataView::new(
            &stored.metadata,
            stored.unverified,
            stored.created_at_ms,
            stored.updated_at_ms,
        );
        let document = DocumentMetadataView::new(&stored.metadata, stored.summary.is_some(), false);
        assert_eq!(fact.lifecycle_state(), FACT_LIFECYCLE_PRUNED);
        assert_eq!(document.lifecycle_state(), DOCUMENT_LIFECYCLE_PRUNED);
        assert_eq!(fact.prune_reason(), Some(FACT_PRUNE_REASON_MANUAL));
        let stored_content = engine
            .engram_store()
            .get_content(&stored)
            .expect("content")
            .expect("stored content");
        let archived = EngramMemory::parse_fact(&stored, stored_content);
        assert!(matches!(archived.status, FactStatus::Archived));

        let facts = memory
            .retrieve_facts("user", None)
            .await
            .expect("retrieve facts");
        assert!(!facts.into_iter().any(|item| item.id == "fact-pruned"));
        let metadata = memory.runtime_metadata_snapshot();
        let runtime = RuntimeMetadataView::new(&metadata);
        assert!(runtime
            .prune_counts_by_reason()
            .expect("json")
            .contains("manual_prune"));

        let events = emitter.snapshot();
        assert!(events.iter().any(|event| matches!(
            event,
            MemoryEvent::MemoryPruned { entries, reason }
            if *entries == 1 && reason == "fact_marked_pruned"
        )));
    }

    #[tokio::test]
    async fn archived_documents_are_filtered_from_fts_results() {
        let temp = tempdir().expect("tempdir");
        let mut config = HybridSearchConfig::default();
        config.db_path = temp.path().join("engram-archive-filter.db");
        config.use_vector = false;
        config.use_reranker = false;

        let engine = Arc::new(HybridSearchEngine::new(config, None).expect("engine"));
        engine
            .engram_store()
            .store_document(
                "docs",
                "active.md",
                "Active",
                "Prime active retrieval contract",
                false,
                {
                    let mut metadata = std::collections::HashMap::new();
                    metadata.insert(
                        META_DOCUMENT_LIFECYCLE_STATE.to_string(),
                        DOCUMENT_LIFECYCLE_ACTIVE.to_string(),
                    );
                    metadata
                },
            )
            .expect("store active");
        engine
            .engram_store()
            .store_document(
                "docs",
                "archived.md",
                "Archived",
                "Prime archived retrieval contract",
                false,
                {
                    let mut metadata = std::collections::HashMap::new();
                    metadata.insert(
                        META_DOCUMENT_LIFECYCLE_STATE.to_string(),
                        DOCUMENT_LIFECYCLE_ACTIVE.to_string(),
                    );
                    metadata
                },
            )
            .expect("store archived");
        engine
            .engram_store()
            .archive_document("docs", "archived.md", {
                let mut metadata = std::collections::HashMap::new();
                metadata.insert(
                    META_DOCUMENT_LIFECYCLE_STATE.to_string(),
                    DOCUMENT_LIFECYCLE_ARCHIVED.to_string(),
                );
                metadata.insert(
                    META_DOCUMENT_ARCHIVE_REASON.to_string(),
                    "superseded".to_string(),
                );
                metadata
            })
            .expect("archive document");

        let results = engine
            .engram_store()
            .search_fts_in_collection("retrieval contract", "docs", 10)
            .expect("search");

        assert!(results
            .iter()
            .any(|result| result.document.path == "active.md"));
        assert!(!results
            .iter()
            .any(|result| result.document.path == "archived.md"));
    }

    #[tokio::test]
    async fn engram_memory_round_trips_session_provenance() {
        let temp = tempdir().expect("tempdir");
        let mut config = HybridSearchConfig::default();
        config.db_path = temp.path().join("engram-brain-recovery.db");
        config.use_vector = false;
        config.use_reranker = false;

        let engine = Arc::new(HybridSearchEngine::new(config, None).expect("engine"));
        let memory = EngramMemory::new(engine);

        let mut session = AgentSession::new("engram-recovery".to_string());
        session.status = SessionStatus::Completed;
        session.background_envelope = Some(BackgroundEnvelope {
            revision: BackgroundRevision {
                revision: 2,
                previous_revision: Some(1),
                updated_at: Utc::now(),
                update_reason: Some("engram recovery seed".to_string()),
            },
            metadata: HashMap::from([(
                "background_session_lifecycle_state".to_string(),
                "archived".to_string(),
            )]),
            ..Default::default()
        });
        session.archive(
            Some("persist for recovery".to_string()),
            Some(Utc::now() + chrono::Duration::days(3)),
        );
        memory
            .store_session(session)
            .await
            .expect("store engram session");

        let recovered = memory
            .retrieve_session("engram-recovery")
            .await
            .expect("retrieve")
            .expect("session");

        assert!(recovered.is_archived());
        assert_eq!(
            recovered.lifecycle.archive_reason.as_deref(),
            Some("persist for recovery")
        );
        let recovered_background = recovered
            .background_envelope
            .as_ref()
            .expect("background should be recovered");
        assert_eq!(recovered_background.revision.revision, 2);
        assert_eq!(
            recovered_background
                .metadata
                .get("background_session_lifecycle_state")
                .map(String::as_str),
            Some("archived")
        );
    }
}
