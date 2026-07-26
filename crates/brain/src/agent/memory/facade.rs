use crate::agent::memory::{
    Fact, FactProtection, FactReviewPayload, FactReviewResolution, FactReviewResolutionOutcome,
    Memory, MemoryCapabilities, MultimodalMemoryRecord, SharedMemoryAdapter, ShortTermMemory,
    RELATION_QUERY_DEFAULT_MAX_DEPTH, RELATION_QUERY_DEFAULT_MAX_RETURNED_EDGES,
    RELATION_QUERY_DEFAULT_MAX_VISITED_NODES, RELATION_QUERY_HARD_CAP_DEPTH,
};
use crate::agent::message::Message;
use crate::agent::session::AgentSession;
use crate::knowledge::rag::Document;
use async_trait::async_trait;
use benshu_inference::QuantLevel;
use benshu_infra::traits::memory::EventLevel;
use benshu_memory_api::Memory as SharedMemory;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundSessionPersistenceStatus {
    Persisted,
    DeferredMissingSession,
}

#[async_trait]
impl Memory for ShortTermMemory {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn capabilities(&self) -> MemoryCapabilities {
        MemoryCapabilities::episodic_only()
    }
    async fn store(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        message: Message,
    ) -> crate::error::Result<()> {
        let key = self.key(user_id, agent_id);

        if !self.store.contains_key(&key) {
            self.enforce_user_capacity();
        }

        // L1 Storage
        {
            let mut entry = self.store.entry(key.clone()).or_default();
            if entry.len() >= self.max_messages {
                entry.pop_front();
            }
            entry.push_back(message.clone());
        }

        // Observability
        if let Some(emitter_opt) = self.emitter.read().as_ref() {
            emitter_opt.emit(
                benshu_infra::traits::memory::MemoryEvent::L1Stored {
                    session_id: key.clone(),
                    role: format!("{:?}", message.role),
                },
                EventLevel::Info,
            );
        }

        self.last_access
            .insert(key.clone(), std::time::Instant::now());

        // L2 Persistence
        #[cfg(feature = "persistence")]
        {
            if self.db.is_some() {
                self.update_l2_history(key, vec![message], false).await?;
            } else {
                // Emit PersistenceFailure if we are meant to have persistence but don't
                if let Some(emitter_opt) = self.emitter.read().as_ref() {
                    emitter_opt.emit(
                        benshu_infra::traits::memory::MemoryEvent::PersistenceFailure {
                            path: self.path.to_string_lossy().to_string(),
                            error: "Redb initialization failed or database dropped".into(),
                            is_fatal: false, // Falling back to L1 RAM
                        },
                        benshu_infra::traits::memory::EventLevel::Warn,
                    );
                }
            }
        }

        self.record_interaction();
        Ok(())
    }

    async fn store_batch(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        messages: Vec<Message>,
    ) -> crate::error::Result<()> {
        let key = self.key(user_id, agent_id);

        if !self.store.contains_key(&key) {
            self.enforce_user_capacity();
        }

        // L1 Storage
        {
            let mut entry = self.store.entry(key.clone()).or_default();
            for msg in &messages {
                if entry.len() >= self.max_messages {
                    entry.pop_front();
                }
                entry.push_back(msg.clone());
            }
        }

        // Observability
        if let Some(emitter_opt) = self.emitter.read().as_ref() {
            emitter_opt.emit(
                benshu_infra::traits::memory::MemoryEvent::L1Stored {
                    session_id: key.clone(),
                    role: "Batch".to_string(),
                },
                EventLevel::Info,
            );
        }

        self.last_access
            .insert(key.clone(), std::time::Instant::now());

        // L2 Persistence
        #[cfg(feature = "persistence")]
        {
            if self.db.is_some() {
                self.update_l2_history(key, messages, false).await?;
            } else if let Some(emitter_opt) = self.emitter.read().as_ref() {
                emitter_opt.emit(
                    benshu_infra::traits::memory::MemoryEvent::PersistenceFailure {
                        path: self.path.to_string_lossy().to_string(),
                        error: "Redb initialization failed or database dropped".into(),
                        is_fatal: false,
                    },
                    benshu_infra::traits::memory::EventLevel::Warn,
                );
            }
        }

        self.record_interaction();
        Ok(())
    }

    async fn retrieve(&self, user_id: &str, agent_id: Option<&str>, limit: usize) -> Vec<Message> {
        let key = self.key(user_id, agent_id);
        self.last_access
            .insert(key.clone(), std::time::Instant::now());

        self.store
            .get(&key)
            .map(|v| v.iter().rev().take(limit).cloned().rev().collect())
            .unwrap_or_default()
    }

    async fn retrieve_full_history(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
    ) -> crate::error::Result<Vec<Message>> {
        self.retrieve_full_history_inner(user_id, agent_id).await
    }

    async fn clear(&self, user_id: &str, agent_id: Option<&str>) -> crate::error::Result<()> {
        self.clear_inner(user_id, agent_id).await
    }

    async fn undo(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
    ) -> crate::error::Result<Option<Message>> {
        self.undo_inner(user_id, agent_id).await
    }

    async fn store_session(
        &self,
        session: crate::agent::session::AgentSession,
    ) -> crate::error::Result<()> {
        self.store_session_inner(session).await
    }

    async fn retrieve_session(
        &self,
        session_id: &str,
    ) -> crate::error::Result<Option<crate::agent::session::AgentSession>> {
        self.retrieve_session_inner(session_id).await
    }

    async fn delete_session(&self, session_id: &str) -> crate::error::Result<()> {
        self.delete_session_inner(session_id).await
    }

    async fn store_fact(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        fact: Fact,
    ) -> crate::error::Result<()> {
        self.store_fact_inner(user_id, agent_id, fact).await
    }

    async fn retrieve_facts(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
    ) -> crate::error::Result<Vec<Fact>> {
        self.retrieve_facts_inner(user_id, agent_id).await
    }

    async fn delete_fact(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        fact_id: &str,
    ) -> crate::error::Result<()> {
        #[cfg(feature = "persistence")]
        if let Some(db) = &self.db {
            let base_key = self.key(user_id, agent_id);
            let fact_key = format!("{}:{}", base_key, fact_id);
            let db = db.clone();
            tokio::task::spawn_blocking(move || {
                let write_txn = db.begin_write().map_err(|e| {
                    crate::error::Error::Internal(format!("redb write error: {}", e))
                })?;
                {
                    let mut table = write_txn
                        .open_table(crate::agent::memory::episodic::STM_FACTS_TABLE)
                        .map_err(|e| {
                            crate::error::Error::Internal(format!("redb table error: {}", e))
                        })?;
                    table.remove(fact_key.as_str()).map_err(|e| {
                        crate::error::Error::Internal(format!("redb remove fact error: {}", e))
                    })?;
                }
                write_txn.commit().map_err(|e| {
                    crate::error::Error::Internal(format!("redb commit fact error: {}", e))
                })?;
                Ok::<(), crate::error::Error>(())
            })
            .await
            .map_err(|e| crate::error::Error::Internal(format!("Delete fact panicked: {}", e)))??;
        }
        Ok(())
    }

    async fn find_related_facts(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        fact_id: &str,
        depth: usize,
    ) -> crate::error::Result<Vec<Fact>> {
        self.find_related_facts_inner(user_id, agent_id, fact_id, depth)
            .await
    }

    async fn maintenance(&self) -> crate::error::Result<()> {
        self.maintenance_inner().await
    }

    fn record_interaction(&self) {
        self.record_interaction_inner();
    }

    fn last_interaction_elapsed(&self) -> std::time::Duration {
        self.last_interaction_elapsed_inner()
    }

    fn set_emitter(&self, emitter: Arc<dyn benshu_infra::traits::memory::MemoryEmitter>) {
        *self.emitter.write() = Some(emitter);
    }

    fn emit_event(
        &self,
        event: benshu_infra::traits::memory::MemoryEvent,
        level: benshu_infra::traits::memory::EventLevel,
    ) {
        if let Some(emitter_opt) = self.emitter.read().as_ref() {
            emitter_opt.emit(event, level);
        }
    }

    fn set_security(&self, security: Arc<dyn benshu_infra::traits::security::SecurityHandler>) {
        *self.security.write() = Some(security);
    }

    fn security(&self) -> Option<Arc<dyn benshu_infra::traits::security::SecurityHandler>> {
        self.security.read().clone()
    }

    async fn search(
        &self,
        _user_id: &str,
        _agent_id: Option<&str>,
        _query: &str,
        _limit: usize,
    ) -> crate::error::Result<Vec<Document>> {
        Ok(vec![]) // ShortTermMemory does not implement search
    }

    async fn store_knowledge(
        &self,
        _user_id: &str,
        _agent_id: Option<&str>,
        _title: &str,
        _content: &str,
        _category: &str,
        _is_unverified: bool,
    ) -> crate::error::Result<()> {
        Ok(()) // ShortTermMemory does not store knowledge
    }

    async fn list_unverified(
        &self,
        _agent_id: Option<&str>,
        _limit: usize,
    ) -> crate::error::Result<Vec<Fact>> {
        Ok(vec![]) // ShortTermMemory does not manage unverified knowledge
    }

    async fn mark_verified(&self, _fact_id: &str) -> crate::error::Result<()> {
        self.update_fact_status_inner(_fact_id, crate::agent::memory::FactStatus::Verified)
            .await
    }

    async fn mark_pending_review(
        &self,
        fact_id: &str,
        summary: Option<&str>,
    ) -> crate::error::Result<()> {
        self.update_fact_status_inner(fact_id, crate::agent::memory::FactStatus::PendingReview)
            .await?;
        let existing = self.get_fact_review_payload_inner(fact_id).await?;
        let payload = FactReviewPayload {
            review_reason: Some("auditor_needs_review".to_string()),
            challenger_summary: summary.map(str::to_string).or_else(|| {
                existing
                    .as_ref()
                    .and_then(|value| value.challenger_summary.clone())
            }),
            challenger_source: Some("memory_auditor".to_string()),
            review_requested_at: Some(chrono::Utc::now()),
            resolution: None,
        };
        self.store_fact_review_payload_inner(fact_id, &payload)
            .await?;
        self.emit_event(
            benshu_infra::traits::memory::MemoryEvent::FactReviewRequested {
                id: fact_id.to_string(),
                source: "memory_auditor".to_string(),
            },
            benshu_infra::traits::memory::EventLevel::Info,
        );
        Ok(())
    }

    async fn request_fact_review(
        &self,
        fact_id: &str,
        payload: FactReviewPayload,
    ) -> crate::error::Result<()> {
        self.update_fact_status_inner(fact_id, crate::agent::memory::FactStatus::PendingReview)
            .await?;
        let payload = FactReviewPayload {
            review_reason: payload
                .review_reason
                .or_else(|| Some("auditor_needs_review".to_string())),
            challenger_summary: payload.challenger_summary,
            challenger_source: payload
                .challenger_source
                .or_else(|| Some("memory_auditor".to_string())),
            review_requested_at: payload
                .review_requested_at
                .or_else(|| Some(chrono::Utc::now())),
            resolution: payload.resolution,
        };
        self.store_fact_review_payload_inner(fact_id, &payload)
            .await?;
        self.emit_event(
            benshu_infra::traits::memory::MemoryEvent::FactReviewRequested {
                id: fact_id.to_string(),
                source: payload
                    .challenger_source
                    .clone()
                    .unwrap_or_else(|| "memory_auditor".to_string()),
            },
            benshu_infra::traits::memory::EventLevel::Info,
        );
        Ok(())
    }

    async fn get_fact_review_payload(
        &self,
        fact_id: &str,
    ) -> crate::error::Result<Option<FactReviewPayload>> {
        self.get_fact_review_payload_inner(fact_id).await
    }

    async fn resolve_pending_review(
        &self,
        fact_id: &str,
        resolution: FactReviewResolution,
    ) -> crate::error::Result<()> {
        let mut payload = self
            .get_fact_review_payload_inner(fact_id)
            .await?
            .unwrap_or_default();
        payload.resolution = Some(resolution.clone());
        self.store_fact_review_payload_inner(fact_id, &payload)
            .await?;

        match resolution.outcome {
            FactReviewResolutionOutcome::Verified => {
                self.update_fact_status_inner(fact_id, crate::agent::memory::FactStatus::Verified)
                    .await?
            }
            FactReviewResolutionOutcome::Pruned => {
                self.update_fact_status_inner(fact_id, crate::agent::memory::FactStatus::Archived)
                    .await?
            }
            FactReviewResolutionOutcome::PendingReview => {
                self.update_fact_status_inner(
                    fact_id,
                    crate::agent::memory::FactStatus::PendingReview,
                )
                .await?
            }
        }
        self.emit_event(
            benshu_infra::traits::memory::MemoryEvent::FactReviewResolved {
                id: fact_id.to_string(),
                outcome: match resolution.outcome {
                    FactReviewResolutionOutcome::Verified => "verified".to_string(),
                    FactReviewResolutionOutcome::Pruned => "pruned".to_string(),
                    FactReviewResolutionOutcome::PendingReview => "pending_review".to_string(),
                },
                resolved_by: resolution
                    .resolved_by
                    .clone()
                    .unwrap_or_else(|| "short_term_memory".to_string()),
            },
            benshu_infra::traits::memory::EventLevel::Info,
        );
        Ok(())
    }

    async fn mark_pruned(&self, fact_id: &str) -> crate::error::Result<()> {
        self.update_fact_status_inner(fact_id, crate::agent::memory::FactStatus::Archived)
            .await
    }

    async fn update_utility(
        &self,
        _collection: &str,
        _fact_id: &str,
        _increment: f32,
    ) -> crate::error::Result<()> {
        Ok(()) // ShortTermMemory does not manage utility
    }

    async fn age_vectors(
        &self,
        _collection: &str,
        _older_than_days: usize,
    ) -> crate::error::Result<()> {
        Ok(()) // ShortTermMemory does not manage vectors
    }

    async fn promote_vectors(
        &self,
        _collection: &str,
        _level: QuantLevel,
    ) -> crate::error::Result<()> {
        Ok(())
    }

    async fn update_fact_importance(
        &self,
        _user_id: &str,
        _agent_id: Option<&str>,
        fact_id: &str,
        importance: f32,
    ) -> crate::error::Result<()> {
        self.update_fact_importance_by_id_inner(fact_id, importance)
            .await
    }

    async fn set_fact_protection(
        &self,
        _user_id: &str,
        _agent_id: Option<&str>,
        fact_id: &str,
        protection: FactProtection,
    ) -> crate::error::Result<()> {
        self.update_fact_protection_by_id_inner(fact_id, protection)
            .await
    }

    async fn search_experiences(
        &self,
        _query: &str,
        _limit: usize,
    ) -> crate::error::Result<Vec<serde_json::Value>> {
        Ok(vec![]) // ShortTermMemory does not store experiences
    }

    async fn search_anti_patterns(
        &self,
        _query: &str,
        _limit: usize,
    ) -> crate::error::Result<Vec<serde_json::Value>> {
        Ok(vec![]) // ShortTermMemory does not store anti-patterns
    }

    async fn search_cognitive_guidance(
        &self,
        _query: &str,
        _limit: usize,
    ) -> crate::error::Result<(Vec<serde_json::Value>, Vec<serde_json::Value>)> {
        Ok((vec![], vec![])) // ShortTermMemory does not store cognitive guidance
    }

    async fn store_experience(&self, _experience: serde_json::Value) -> crate::error::Result<()> {
        Ok(()) // ShortTermMemory does not store experiences
    }

    async fn store_anti_pattern(
        &self,
        _anti_pattern: serde_json::Value,
    ) -> crate::error::Result<()> {
        Ok(()) // ShortTermMemory does not store anti-patterns
    }

    async fn delete_experience(&self, _id: &str) -> crate::error::Result<()> {
        Ok(())
    }
    async fn delete_anti_pattern(&self, _id: &str) -> crate::error::Result<()> {
        Ok(())
    }

    async fn increment_experience_utility(
        &self,
        _id: &str,
        _increment: f64,
    ) -> crate::error::Result<()> {
        Ok(()) // ShortTermMemory does not manage experience utility
    }

    async fn increment_anti_pattern_utility(
        &self,
        _id: &str,
        _increment: f64,
    ) -> crate::error::Result<()> {
        Ok(()) // ShortTermMemory does not manage anti-pattern utility
    }

    async fn get_experience(&self, _id: &str) -> crate::error::Result<Option<serde_json::Value>> {
        Ok(None) // ShortTermMemory does not store experiences
    }

    async fn list_sessions(
        &self,
    ) -> crate::error::Result<Vec<crate::agent::session::AgentSession>> {
        self.list_sessions_inner().await
    }

    async fn prune_messages(&self, older_than: std::time::Duration) -> crate::error::Result<usize> {
        self.prune_messages_inner(older_than).await
    }

    async fn mark_cancelled(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        reason: &str,
    ) -> crate::error::Result<()> {
        self.store(
            user_id,
            agent_id,
            Message::assistant(crate::agent::message::Content::cancelled(reason)),
        )
        .await?;
        self.mark_cancelled_inner(user_id, agent_id, reason).await
    }

    async fn get_metadata(&self, key: &str) -> crate::error::Result<Option<String>> {
        self.get_metadata_inner(key).await
    }

    async fn set_metadata(&self, key: &str, value: &str) -> crate::error::Result<()> {
        self.set_metadata_inner(key, value).await
    }
}

pub struct MemoryManager {
    hot: Arc<dyn Memory>,
    engram: Arc<dyn Memory>,
}

fn memory_error_to_infra(error: crate::error::Error) -> benshu_infra::error::Error {
    benshu_infra::error::Error::Internal(error.to_string())
}

impl MemoryManager {
    pub fn new(hot: Arc<dyn Memory>, engram: Arc<dyn Memory>) -> Self {
        Self { hot, engram }
    }

    pub fn new_with_shared_engram(hot: Arc<dyn Memory>, engram: Arc<dyn SharedMemory>) -> Self {
        Self {
            hot,
            engram: Arc::new(SharedMemoryAdapter::new(engram)),
        }
    }

    pub fn new_shared(hot: Arc<dyn SharedMemory>, engram: Arc<dyn SharedMemory>) -> Self {
        Self {
            hot: Arc::new(SharedMemoryAdapter::new(hot)),
            engram: Arc::new(SharedMemoryAdapter::new(engram)),
        }
    }

    fn annotate_background_session_lifecycle(
        session: &mut AgentSession,
        lifecycle_state: &str,
        reason: Option<&str>,
        recovered_from: Option<&str>,
    ) {
        let Some(background) = session.background_envelope.as_mut() else {
            return;
        };

        background.metadata.insert(
            "background_session_lifecycle_state".to_string(),
            lifecycle_state.to_string(),
        );
        background.metadata.insert(
            "background_session_updated_at_ms".to_string(),
            session.updated_at.timestamp_millis().to_string(),
        );
        if let Some(archived_at) = session.lifecycle.archived_at {
            background.metadata.insert(
                "background_session_archived_at_ms".to_string(),
                archived_at.timestamp_millis().to_string(),
            );
        }
        if let Some(retention_until) = session.lifecycle.retention_until {
            background.metadata.insert(
                "background_session_retention_until_ms".to_string(),
                retention_until.timestamp_millis().to_string(),
            );
        }
        if let Some(value) = reason.filter(|value| !value.trim().is_empty()) {
            background.metadata.insert(
                "background_session_archive_reason".to_string(),
                value.to_string(),
            );
        }
        if let Some(value) = recovered_from.filter(|value| !value.trim().is_empty()) {
            background.metadata.insert(
                "background_session_recovered_from".to_string(),
                value.to_string(),
            );
        }
        if let Some(last_recovered_at) = session.lifecycle.last_recovered_at {
            background.metadata.insert(
                "background_session_last_recovered_at_ms".to_string(),
                last_recovered_at.timestamp_millis().to_string(),
            );
        }
    }

    fn emit_consistency_warning(
        &self,
        operation: &str,
        subject: &str,
        error: &crate::error::Error,
    ) {
        let path = format!("memory_manager::{operation}::{subject}");
        self.hot.emit_event(
            benshu_infra::traits::memory::MemoryEvent::PersistenceFailure {
                path,
                error: error.to_string(),
                is_fatal: false,
            },
            EventLevel::Warn,
        );
    }

    pub async fn persist_background_envelope(
        &self,
        session_id: &str,
        background_envelope: crate::agent::memory::BackgroundEnvelope,
        operation: &str,
    ) -> crate::error::Result<BackgroundSessionPersistenceStatus> {
        let hot_session = self.hot.retrieve_session(session_id).await?;
        let engram_session = self.engram.retrieve_session(session_id).await?;
        let Some((mut session, _)) = Self::choose_session_authority(hot_session, engram_session)
        else {
            return Ok(BackgroundSessionPersistenceStatus::DeferredMissingSession);
        };

        session.background_envelope = Some(background_envelope);
        session.updated_at = chrono::Utc::now();
        self.store_session_consistently(session, operation).await?;
        Ok(BackgroundSessionPersistenceStatus::Persisted)
    }

    pub async fn promote_background_relationship_fact(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        fact: Fact,
        operation: &str,
    ) -> crate::error::Result<()> {
        self.store_fact_consistently(user_id, agent_id, fact, operation)
            .await
    }

    pub async fn request_background_relationship_review(
        &self,
        fact_id: &str,
        summary: Option<&str>,
        session_id: Option<&str>,
    ) -> crate::error::Result<()> {
        let payload = FactReviewPayload {
            review_reason: Some("background_relationship_candidate".to_string()),
            challenger_summary: summary.map(str::to_string),
            challenger_source: Some(
                session_id
                    .map(|value| format!("background_compression:{value}"))
                    .unwrap_or_else(|| "background_compression".to_string()),
            ),
            review_requested_at: Some(chrono::Utc::now()),
            resolution: None,
        };
        <Self as Memory>::request_fact_review(self, fact_id, payload).await
    }

    async fn restore_session_layer(
        layer: &Arc<dyn Memory>,
        session_id: &str,
        previous: Option<AgentSession>,
    ) -> crate::error::Result<()> {
        match previous {
            Some(session) => layer.store_session(session).await,
            None => layer.delete_session(session_id).await,
        }
    }

    async fn store_session_consistently(
        &self,
        session: AgentSession,
        operation: &str,
    ) -> crate::error::Result<()> {
        let session_id = session.id.clone();
        let hot_previous = self.hot.retrieve_session(&session_id).await?;

        self.hot.store_session(session.clone()).await?;
        if let Err(err) = self.engram.store_session(session).await {
            let rollback_result =
                Self::restore_session_layer(&self.hot, &session_id, hot_previous).await;
            let combined = match rollback_result {
                Ok(()) => crate::error::Error::MemoryConsistency(format!(
                    "{operation} failed while writing durable session {session_id}: {err}; hot layer was rolled back"
                )),
                Err(rollback_err) => crate::error::Error::MemoryConsistency(format!(
                    "{operation} failed while writing durable session {session_id}: {err}; hot rollback also failed: {rollback_err}"
                )),
            };
            self.emit_consistency_warning(operation, &session_id, &combined);
            return Err(combined);
        }

        Ok(())
    }

    async fn delete_session_consistently(
        &self,
        session_id: &str,
        operation: &str,
    ) -> crate::error::Result<()> {
        let hot_previous = self.hot.retrieve_session(session_id).await?;

        self.hot.delete_session(session_id).await?;
        if let Err(err) = self.engram.delete_session(session_id).await {
            let rollback_result =
                Self::restore_session_layer(&self.hot, session_id, hot_previous).await;
            let combined = match rollback_result {
                Ok(()) => crate::error::Error::MemoryConsistency(format!(
                    "{operation} failed while deleting durable session {session_id}: {err}; hot layer was rolled back"
                )),
                Err(rollback_err) => crate::error::Error::MemoryConsistency(format!(
                    "{operation} failed while deleting durable session {session_id}: {err}; hot rollback also failed: {rollback_err}"
                )),
            };
            self.emit_consistency_warning(operation, session_id, &combined);
            return Err(combined);
        }

        Ok(())
    }

    async fn snapshot_fact(
        layer: &Arc<dyn Memory>,
        user_id: &str,
        agent_id: Option<&str>,
        fact_id: &str,
    ) -> crate::error::Result<Option<Fact>> {
        Ok(layer
            .retrieve_facts(user_id, agent_id)
            .await?
            .into_iter()
            .find(|fact| fact.id == fact_id))
    }

    async fn restore_fact_layer(
        layer: &Arc<dyn Memory>,
        user_id: &str,
        agent_id: Option<&str>,
        fact_id: &str,
        previous: Option<Fact>,
    ) -> crate::error::Result<()> {
        layer.delete_fact(user_id, agent_id, fact_id).await?;
        if let Some(fact) = previous {
            layer.store_fact(user_id, agent_id, fact).await?;
        }
        Ok(())
    }

    async fn store_fact_consistently(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        fact: Fact,
        operation: &str,
    ) -> crate::error::Result<()> {
        let fact_id = fact.id.clone();
        let hot_previous = Self::snapshot_fact(&self.hot, user_id, agent_id, &fact_id).await?;

        self.hot.store_fact(user_id, agent_id, fact.clone()).await?;
        if let Err(err) = self.engram.store_fact(user_id, agent_id, fact).await {
            let rollback_result =
                Self::restore_fact_layer(&self.hot, user_id, agent_id, &fact_id, hot_previous)
                    .await;
            let combined = match rollback_result {
                Ok(()) => crate::error::Error::MemoryConsistency(format!(
                    "{operation} failed while writing durable fact {fact_id}: {err}; hot layer was rolled back"
                )),
                Err(rollback_err) => crate::error::Error::MemoryConsistency(format!(
                    "{operation} failed while writing durable fact {fact_id}: {err}; hot rollback also failed: {rollback_err}"
                )),
            };
            self.emit_consistency_warning(operation, &fact_id, &combined);
            return Err(combined);
        }

        Ok(())
    }

    async fn delete_fact_consistently(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        fact_id: &str,
        operation: &str,
    ) -> crate::error::Result<()> {
        let hot_previous = Self::snapshot_fact(&self.hot, user_id, agent_id, fact_id).await?;

        self.hot.delete_fact(user_id, agent_id, fact_id).await?;
        if let Err(err) = self.engram.delete_fact(user_id, agent_id, fact_id).await {
            let rollback_result =
                Self::restore_fact_layer(&self.hot, user_id, agent_id, fact_id, hot_previous).await;
            let combined = match rollback_result {
                Ok(()) => crate::error::Error::MemoryConsistency(format!(
                    "{operation} failed while deleting durable fact {fact_id}: {err}; hot layer was rolled back"
                )),
                Err(rollback_err) => crate::error::Error::MemoryConsistency(format!(
                    "{operation} failed while deleting durable fact {fact_id}: {err}; hot rollback also failed: {rollback_err}"
                )),
            };
            self.emit_consistency_warning(operation, fact_id, &combined);
            return Err(combined);
        }

        Ok(())
    }

    async fn update_fact_importance_consistently(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        fact_id: &str,
        importance: f32,
        operation: &str,
    ) -> crate::error::Result<()> {
        let hot_previous = Self::snapshot_fact(&self.hot, user_id, agent_id, fact_id).await?;

        self.hot
            .update_fact_importance(user_id, agent_id, fact_id, importance)
            .await?;
        if let Err(err) = self
            .engram
            .update_fact_importance(user_id, agent_id, fact_id, importance)
            .await
        {
            let rollback_result =
                Self::restore_fact_layer(&self.hot, user_id, agent_id, fact_id, hot_previous).await;
            let combined = match rollback_result {
                Ok(()) => crate::error::Error::MemoryConsistency(format!(
                    "{operation} failed while updating durable fact importance {fact_id}: {err}; hot layer was rolled back"
                )),
                Err(rollback_err) => crate::error::Error::MemoryConsistency(format!(
                    "{operation} failed while updating durable fact importance {fact_id}: {err}; hot rollback also failed: {rollback_err}"
                )),
            };
            self.emit_consistency_warning(operation, fact_id, &combined);
            return Err(combined);
        }

        Ok(())
    }

    async fn update_fact_protection_consistently(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        fact_id: &str,
        protection: FactProtection,
        operation: &str,
    ) -> crate::error::Result<()> {
        let hot_previous = Self::snapshot_fact(&self.hot, user_id, agent_id, fact_id).await?;

        self.hot
            .set_fact_protection(user_id, agent_id, fact_id, protection.clone())
            .await?;
        if let Err(err) = self
            .engram
            .set_fact_protection(user_id, agent_id, fact_id, protection)
            .await
        {
            let rollback_result =
                Self::restore_fact_layer(&self.hot, user_id, agent_id, fact_id, hot_previous).await;
            let combined = match rollback_result {
                Ok(()) => crate::error::Error::MemoryConsistency(format!(
                    "{operation} failed while updating durable fact protection {fact_id}: {err}; hot layer was rolled back"
                )),
                Err(rollback_err) => crate::error::Error::MemoryConsistency(format!(
                    "{operation} failed while updating durable fact protection {fact_id}: {err}; hot rollback also failed: {rollback_err}"
                )),
            };
            self.emit_consistency_warning(operation, fact_id, &combined);
            return Err(combined);
        }

        Ok(())
    }

    fn is_inflight_session(session: &AgentSession) -> bool {
        matches!(
            session.status,
            crate::agent::session::SessionStatus::Thinking
                | crate::agent::session::SessionStatus::PendingTools
                | crate::agent::session::SessionStatus::AwaitingClarification { .. }
                | crate::agent::session::SessionStatus::AwaitingApproval { .. }
                | crate::agent::session::SessionStatus::Executing
        )
    }

    fn merge_sessions(
        mut hot_sessions: Vec<AgentSession>,
        engram_sessions: Vec<AgentSession>,
    ) -> Vec<AgentSession> {
        let mut by_id: HashMap<String, AgentSession> = hot_sessions
            .drain(..)
            .map(|session| (session.id.clone(), session))
            .collect();

        for session in engram_sessions {
            by_id
                .entry(session.id.clone())
                .and_modify(|existing| {
                    if session.updated_at > existing.updated_at {
                        *existing = session.clone();
                    }
                })
                .or_insert(session);
        }

        let mut merged: Vec<_> = by_id.into_values().collect();
        merged.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        merged
    }

    fn choose_session_authority(
        hot_session: Option<AgentSession>,
        engram_session: Option<AgentSession>,
    ) -> Option<(AgentSession, Option<&'static str>)> {
        match (hot_session, engram_session) {
            (Some(hot), Some(engram)) => {
                // Hot memory remains authoritative for actively mutating sessions so we do not
                // overwrite in-flight execution state with a durable snapshot.
                if Self::is_inflight_session(&hot) && !hot.is_archived() {
                    return Some((hot, None));
                }

                // Once a session has moved into archived / recovered territory, durable long-term
                // state should win if it is at least as fresh as hot memory.
                if engram.is_archived() && engram.updated_at >= hot.updated_at {
                    return Some((engram, Some("engram")));
                }

                if engram.updated_at > hot.updated_at {
                    Some((engram, Some("engram")))
                } else {
                    Some((hot, None))
                }
            }
            (Some(hot), None) => Some((hot, None)),
            (None, Some(engram)) => Some((engram, Some("engram"))),
            (None, None) => None,
        }
    }

    fn merge_facts(mut hot_facts: Vec<Fact>, engram_facts: Vec<Fact>) -> Vec<Fact> {
        let mut by_id: HashMap<String, Fact> = hot_facts
            .drain(..)
            .map(|fact| (fact.id.clone(), fact))
            .collect();

        for fact in engram_facts {
            by_id
                .entry(fact.id.clone())
                .and_modify(|existing| {
                    let replace = fact.updated_at > existing.updated_at
                        || (fact.updated_at == existing.updated_at
                            && fact.status != existing.status)
                        || (fact.updated_at == existing.updated_at
                            && fact.importance > existing.importance);
                    if replace {
                        *existing = fact.clone();
                    }
                })
                .or_insert(fact);
        }

        let mut merged: Vec<_> = by_id.into_values().collect();
        merged.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        merged
    }
}

#[async_trait]
impl SharedMemory for MemoryManager {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn capabilities(&self) -> MemoryCapabilities {
        <Self as Memory>::capabilities(self)
    }

    async fn store(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        message: Message,
    ) -> benshu_infra::error::Result<()> {
        <Self as Memory>::store(self, user_id, agent_id, message)
            .await
            .map_err(memory_error_to_infra)
    }

    async fn store_batch(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        messages: Vec<Message>,
    ) -> benshu_infra::error::Result<()> {
        <Self as Memory>::store_batch(self, user_id, agent_id, messages)
            .await
            .map_err(memory_error_to_infra)
    }

    async fn retrieve(&self, user_id: &str, agent_id: Option<&str>, limit: usize) -> Vec<Message> {
        <Self as Memory>::retrieve(self, user_id, agent_id, limit).await
    }

    async fn retrieve_full_history(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
    ) -> benshu_infra::error::Result<Vec<Message>> {
        <Self as Memory>::retrieve_full_history(self, user_id, agent_id)
            .await
            .map_err(memory_error_to_infra)
    }

    async fn clear(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
    ) -> benshu_infra::error::Result<()> {
        <Self as Memory>::clear(self, user_id, agent_id)
            .await
            .map_err(memory_error_to_infra)
    }

    async fn undo(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
    ) -> benshu_infra::error::Result<Option<Message>> {
        <Self as Memory>::undo(self, user_id, agent_id)
            .await
            .map_err(memory_error_to_infra)
    }

    async fn store_session(&self, session: AgentSession) -> benshu_infra::error::Result<()> {
        <Self as Memory>::store_session(self, session)
            .await
            .map_err(memory_error_to_infra)
    }

    async fn retrieve_session(
        &self,
        session_id: &str,
    ) -> benshu_infra::error::Result<Option<AgentSession>> {
        <Self as Memory>::retrieve_session(self, session_id)
            .await
            .map_err(memory_error_to_infra)
    }

    async fn delete_session(&self, session_id: &str) -> benshu_infra::error::Result<()> {
        <Self as Memory>::delete_session(self, session_id)
            .await
            .map_err(memory_error_to_infra)
    }

    async fn store_fact(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        fact: Fact,
    ) -> benshu_infra::error::Result<()> {
        <Self as Memory>::store_fact(self, user_id, agent_id, fact)
            .await
            .map_err(memory_error_to_infra)
    }

    async fn retrieve_facts(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
    ) -> benshu_infra::error::Result<Vec<Fact>> {
        <Self as Memory>::retrieve_facts(self, user_id, agent_id)
            .await
            .map_err(memory_error_to_infra)
    }

    async fn delete_fact(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        fact_id: &str,
    ) -> benshu_infra::error::Result<()> {
        <Self as Memory>::delete_fact(self, user_id, agent_id, fact_id)
            .await
            .map_err(memory_error_to_infra)
    }

    async fn find_related_facts(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        fact_id: &str,
        depth: usize,
    ) -> benshu_infra::error::Result<Vec<Fact>> {
        <Self as Memory>::find_related_facts(self, user_id, agent_id, fact_id, depth)
            .await
            .map_err(memory_error_to_infra)
    }

    async fn search(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        query: &str,
        limit: usize,
    ) -> benshu_infra::error::Result<Vec<Document>> {
        <Self as Memory>::search(self, user_id, agent_id, query, limit)
            .await
            .map_err(memory_error_to_infra)
    }

    async fn store_knowledge(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        title: &str,
        content: &str,
        category: &str,
        is_unverified: bool,
    ) -> benshu_infra::error::Result<()> {
        <Self as Memory>::store_knowledge(
            self,
            user_id,
            agent_id,
            title,
            content,
            category,
            is_unverified,
        )
        .await
        .map_err(memory_error_to_infra)
    }

    async fn store_multimodal_memory(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        record: MultimodalMemoryRecord,
    ) -> benshu_infra::error::Result<Document> {
        <Self as Memory>::store_multimodal_memory(self, user_id, agent_id, record)
            .await
            .map_err(memory_error_to_infra)
    }

    async fn update_utility(
        &self,
        collection: &str,
        fact_id: &str,
        increment: f32,
    ) -> benshu_infra::error::Result<()> {
        <Self as Memory>::update_utility(self, collection, fact_id, increment)
            .await
            .map_err(memory_error_to_infra)
    }

    async fn update_fact_importance(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        fact_id: &str,
        importance: f32,
    ) -> benshu_infra::error::Result<()> {
        <Self as Memory>::update_fact_importance(self, user_id, agent_id, fact_id, importance)
            .await
            .map_err(memory_error_to_infra)
    }

    async fn set_fact_protection(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        fact_id: &str,
        protection: FactProtection,
    ) -> benshu_infra::error::Result<()> {
        <Self as Memory>::set_fact_protection(self, user_id, agent_id, fact_id, protection)
            .await
            .map_err(memory_error_to_infra)
    }

    async fn age_vectors(
        &self,
        collection: &str,
        older_than_days: usize,
    ) -> benshu_infra::error::Result<()> {
        <Self as Memory>::age_vectors(self, collection, older_than_days)
            .await
            .map_err(memory_error_to_infra)
    }

    async fn maintenance(&self) -> benshu_infra::error::Result<()> {
        <Self as Memory>::maintenance(self)
            .await
            .map_err(memory_error_to_infra)
    }

    fn set_emitter(&self, emitter: Arc<dyn benshu_infra::traits::memory::MemoryEmitter>) {
        <Self as Memory>::set_emitter(self, emitter);
    }

    async fn fetch_document(
        &self,
        collection: &str,
        path: &str,
    ) -> benshu_infra::error::Result<Option<Document>> {
        <Self as Memory>::fetch_document(self, collection, path)
            .await
            .map_err(memory_error_to_infra)
    }

    async fn get_global_cognitive_status(&self) -> benshu_infra::error::Result<String> {
        <Self as Memory>::get_global_cognitive_status(self)
            .await
            .map_err(memory_error_to_infra)
    }
}

#[async_trait]
impl Memory for MemoryManager {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn capabilities(&self) -> MemoryCapabilities {
        let hot = self.hot.capabilities();
        let engram = self.engram.capabilities();
        MemoryCapabilities {
            episodic_messages: hot.episodic_messages || engram.episodic_messages,
            sessions: hot.sessions || engram.sessions,
            facts: hot.facts || engram.facts,
            search: hot.search || engram.search,
            knowledge_store: hot.knowledge_store || engram.knowledge_store,
            experiences: hot.experiences || engram.experiences,
            metadata: hot.metadata || engram.metadata,
        }
    }
    async fn store(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        message: Message,
    ) -> crate::error::Result<()> {
        self.hot.store(user_id, agent_id, message).await
    }

    async fn store_batch(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        messages: Vec<Message>,
    ) -> crate::error::Result<()> {
        self.hot.store_batch(user_id, agent_id, messages).await
    }

    async fn retrieve(&self, user_id: &str, agent_id: Option<&str>, limit: usize) -> Vec<Message> {
        self.hot.retrieve(user_id, agent_id, limit).await
    }

    async fn retrieve_full_history(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
    ) -> crate::error::Result<Vec<Message>> {
        self.hot.retrieve_full_history(user_id, agent_id).await
    }

    async fn clear(&self, user_id: &str, agent_id: Option<&str>) -> crate::error::Result<()> {
        self.hot.clear(user_id, agent_id).await
    }

    async fn undo(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
    ) -> crate::error::Result<Option<Message>> {
        self.hot.undo(user_id, agent_id).await
    }

    async fn store_session(&self, session: AgentSession) -> crate::error::Result<()> {
        self.store_session_consistently(session, "store_session")
            .await
    }

    async fn retrieve_session(
        &self,
        session_id: &str,
    ) -> crate::error::Result<Option<AgentSession>> {
        let hot_session = self.hot.retrieve_session(session_id).await?;
        let engram_session = self.engram.retrieve_session(session_id).await?;
        let chosen = Self::choose_session_authority(hot_session, engram_session);
        let Some((mut session, recovered_from)) = chosen else {
            return Ok(None);
        };

        if let Some(source) = recovered_from {
            session.mark_recovered(source);
            let archive_reason = session.lifecycle.archive_reason.clone();
            let recovered_from = session.lifecycle.recovered_from.clone();
            Self::annotate_background_session_lifecycle(
                &mut session,
                "recovered",
                archive_reason.as_deref(),
                recovered_from.as_deref(),
            );
            if let Err(err) = self
                .store_session_consistently(session.clone(), "recover_session_backfill")
                .await
            {
                tracing::warn!(
                    session_id,
                    error = %err,
                    "Failed to backfill recovered session into hot/engram memory"
                );
                self.emit_consistency_warning("recover_session_backfill", session_id, &err);
            }
        }

        Ok(Some(session))
    }

    async fn delete_session(&self, session_id: &str) -> crate::error::Result<()> {
        self.delete_session_consistently(session_id, "delete_session")
            .await
    }

    async fn archive_session(
        &self,
        session_id: &str,
        reason: Option<&str>,
        retention_until: Option<chrono::DateTime<chrono::Utc>>,
    ) -> crate::error::Result<Option<AgentSession>> {
        let hot_session = self.hot.retrieve_session(session_id).await?;
        let engram_session = self.engram.retrieve_session(session_id).await?;
        let Some((mut session, _)) = Self::choose_session_authority(hot_session, engram_session)
        else {
            return Ok(None);
        };

        session.archive(reason.map(str::to_string), retention_until);
        let archive_reason = session.lifecycle.archive_reason.clone();
        Self::annotate_background_session_lifecycle(
            &mut session,
            "archived",
            archive_reason.as_deref(),
            None,
        );
        self.store_session_consistently(session.clone(), "archive_session")
            .await?;
        Ok(Some(session))
    }

    async fn recover_session(
        &self,
        session_id: &str,
        source: &str,
    ) -> crate::error::Result<Option<AgentSession>> {
        let hot_session = self.hot.retrieve_session(session_id).await?;
        let engram_session = self.engram.retrieve_session(session_id).await?;
        let Some((mut session, chosen_source)) =
            Self::choose_session_authority(hot_session, engram_session)
        else {
            return Ok(None);
        };

        session.mark_recovered(chosen_source.unwrap_or(source));
        let archive_reason = session.lifecycle.archive_reason.clone();
        let recovered_from = session.lifecycle.recovered_from.clone();
        Self::annotate_background_session_lifecycle(
            &mut session,
            "recovered",
            archive_reason.as_deref(),
            recovered_from.as_deref(),
        );
        self.store_session_consistently(session.clone(), "recover_session")
            .await?;
        Ok(Some(session))
    }

    async fn prune_expired_sessions(
        &self,
        now: chrono::DateTime<chrono::Utc>,
    ) -> crate::error::Result<usize> {
        let sessions = Self::merge_sessions(
            self.hot.list_sessions().await?,
            self.engram.list_sessions().await?,
        );
        let mut pruned = 0usize;
        for session in sessions {
            if session.retention_expired_at(now) {
                self.delete_session_consistently(&session.id, "prune_expired_sessions")
                    .await?;
                pruned += 1;
            }
        }
        Ok(pruned)
    }

    async fn store_fact(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        fact: Fact,
    ) -> crate::error::Result<()> {
        self.store_fact_consistently(user_id, agent_id, fact, "store_fact")
            .await
    }

    async fn retrieve_facts(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
    ) -> crate::error::Result<Vec<Fact>> {
        let hot_facts = self.hot.retrieve_facts(user_id, agent_id).await?;
        let engram_facts = self.engram.retrieve_facts(user_id, agent_id).await?;
        Ok(Self::merge_facts(hot_facts, engram_facts))
    }

    async fn delete_fact(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        fact_id: &str,
    ) -> crate::error::Result<()> {
        self.delete_fact_consistently(user_id, agent_id, fact_id, "delete_fact")
            .await
    }

    async fn find_related_facts(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        fact_id: &str,
        depth: usize,
    ) -> crate::error::Result<Vec<Fact>> {
        let related = self
            .hot
            .find_related_facts(user_id, agent_id, fact_id, depth)
            .await?;
        if !related.is_empty() {
            return Ok(related);
        }
        self.engram
            .find_related_facts(user_id, agent_id, fact_id, depth)
            .await
    }

    async fn maintenance(&self) -> crate::error::Result<()> {
        self.hot.maintenance().await?;
        self.engram.maintenance().await?;
        let _ = <Self as Memory>::prune_expired_sessions(self, chrono::Utc::now()).await?;
        Ok(())
    }

    fn record_interaction(&self) {
        self.hot.record_interaction();
    }

    fn last_interaction_elapsed(&self) -> std::time::Duration {
        self.hot.last_interaction_elapsed()
    }

    fn set_emitter(&self, emitter: Arc<dyn benshu_infra::traits::memory::MemoryEmitter>) {
        self.hot.set_emitter(emitter.clone());
        self.engram.set_emitter(emitter);
    }

    fn emit_event(
        &self,
        event: benshu_infra::traits::memory::MemoryEvent,
        level: benshu_infra::traits::memory::EventLevel,
    ) {
        self.hot.emit_event(event, level);
    }

    async fn search(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        query: &str,
        limit: usize,
    ) -> crate::error::Result<Vec<Document>> {
        self.engram.search(user_id, agent_id, query, limit).await
    }

    async fn store_knowledge(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        title: &str,
        content: &str,
        category: &str,
        is_unverified: bool,
    ) -> crate::error::Result<()> {
        self.engram
            .store_knowledge(user_id, agent_id, title, content, category, is_unverified)
            .await
    }

    async fn store_multimodal_memory(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        record: MultimodalMemoryRecord,
    ) -> crate::error::Result<Document> {
        let document = self
            .engram
            .store_multimodal_memory(user_id, agent_id, record.clone())
            .await?;

        if let Some(derived_fact) = record.derived_fact {
            let mut fact =
                crate::agent::memory::Fact::new(derived_fact.content, derived_fact.category);
            fact.importance = derived_fact.importance.clamp(0.0, 1.0);
            fact.verified = derived_fact.verified;
            fact.status = if derived_fact.verified {
                crate::agent::memory::FactStatus::Verified
            } else {
                crate::agent::memory::FactStatus::Pending
            };
            fact.confidence = 0.8;
            fact.source = Some(format!(
                "multimodal:{}:{}",
                document.collection.as_deref().unwrap_or("multimodal"),
                document.path.as_deref().unwrap_or("unknown")
            ));
            <Self as Memory>::store_fact(self, user_id, agent_id, fact).await?;
        }

        self.hot
            .set_metadata(
                "brain.memory.multimodal.last_collection",
                document.collection.as_deref().unwrap_or("multimodal"),
            )
            .await?;
        self.hot
            .set_metadata(
                "brain.memory.multimodal.last_path",
                document.path.as_deref().unwrap_or("unknown"),
            )
            .await?;

        Ok(document)
    }

    async fn list_unverified(
        &self,
        agent_id: Option<&str>,
        limit: usize,
    ) -> crate::error::Result<Vec<Fact>> {
        self.engram.list_unverified(agent_id, limit).await
    }

    async fn mark_verified(&self, fact_id: &str) -> crate::error::Result<()> {
        self.engram.mark_verified(fact_id).await?;
        if let Err(err) = self.hot.mark_verified(fact_id).await {
            tracing::warn!(
                fact_id,
                error = %err,
                "Durable fact verification succeeded but hot cache verification sync failed"
            );
            self.emit_consistency_warning("mark_verified", fact_id, &err);
        }
        Ok(())
    }

    async fn mark_pending_review(
        &self,
        fact_id: &str,
        summary: Option<&str>,
    ) -> crate::error::Result<()> {
        self.engram.mark_pending_review(fact_id, summary).await?;
        if let Err(err) = self.hot.mark_pending_review(fact_id, summary).await {
            tracing::warn!(
                fact_id,
                error = %err,
                "Durable fact pending-review succeeded but hot cache review sync failed"
            );
            self.emit_consistency_warning("mark_pending_review", fact_id, &err);
        }
        Ok(())
    }

    async fn request_fact_review(
        &self,
        fact_id: &str,
        payload: FactReviewPayload,
    ) -> crate::error::Result<()> {
        self.engram
            .request_fact_review(fact_id, payload.clone())
            .await?;
        if let Err(err) = self.hot.request_fact_review(fact_id, payload).await {
            tracing::warn!(
                fact_id,
                error = %err,
                "Durable fact review request succeeded but hot cache review sync failed"
            );
            self.emit_consistency_warning("request_fact_review", fact_id, &err);
        }
        Ok(())
    }

    async fn get_fact_review_payload(
        &self,
        fact_id: &str,
    ) -> crate::error::Result<Option<FactReviewPayload>> {
        let hot = self.hot.get_fact_review_payload(fact_id).await?;
        if hot.is_some() {
            return Ok(hot);
        }
        self.engram.get_fact_review_payload(fact_id).await
    }

    async fn resolve_pending_review(
        &self,
        fact_id: &str,
        resolution: FactReviewResolution,
    ) -> crate::error::Result<()> {
        self.engram
            .resolve_pending_review(fact_id, resolution.clone())
            .await?;
        if let Err(err) = self
            .hot
            .resolve_pending_review(fact_id, resolution.clone())
            .await
        {
            tracing::warn!(
                fact_id,
                error = %err,
                "Durable fact pending-review resolution succeeded but hot cache resolution sync failed"
            );
            self.emit_consistency_warning("resolve_pending_review", fact_id, &err);
        }
        Ok(())
    }

    async fn mark_pruned(&self, fact_id: &str) -> crate::error::Result<()> {
        self.engram.mark_pruned(fact_id).await?;
        if let Err(err) = self.hot.mark_pruned(fact_id).await {
            tracing::warn!(
                fact_id,
                error = %err,
                "Durable fact prune succeeded but hot cache prune sync failed"
            );
            self.emit_consistency_warning("mark_pruned", fact_id, &err);
        }
        Ok(())
    }

    async fn update_utility(
        &self,
        collection: &str,
        fact_id: &str,
        increment: f32,
    ) -> crate::error::Result<()> {
        self.engram
            .update_utility(collection, fact_id, increment)
            .await
    }

    async fn age_vectors(
        &self,
        collection: &str,
        older_than_days: usize,
    ) -> crate::error::Result<()> {
        self.engram.age_vectors(collection, older_than_days).await
    }

    async fn promote_vectors(
        &self,
        collection: &str,
        level: QuantLevel,
    ) -> crate::error::Result<()> {
        self.engram.promote_vectors(collection, level).await
    }

    async fn update_fact_importance(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        fact_id: &str,
        importance: f32,
    ) -> crate::error::Result<()> {
        self.update_fact_importance_consistently(
            user_id,
            agent_id,
            fact_id,
            importance,
            "update_fact_importance",
        )
        .await
    }

    async fn set_fact_protection(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        fact_id: &str,
        protection: FactProtection,
    ) -> crate::error::Result<()> {
        self.update_fact_protection_consistently(
            user_id,
            agent_id,
            fact_id,
            protection,
            "set_fact_protection",
        )
        .await
    }

    async fn search_experiences(
        &self,
        query: &str,
        limit: usize,
    ) -> crate::error::Result<Vec<serde_json::Value>> {
        self.engram.search_experiences(query, limit).await
    }

    async fn search_anti_patterns(
        &self,
        query: &str,
        limit: usize,
    ) -> crate::error::Result<Vec<serde_json::Value>> {
        self.engram.search_anti_patterns(query, limit).await
    }

    async fn search_cognitive_guidance(
        &self,
        query: &str,
        limit: usize,
    ) -> crate::error::Result<(Vec<serde_json::Value>, Vec<serde_json::Value>)> {
        self.engram.search_cognitive_guidance(query, limit).await
    }

    async fn store_experience(&self, experience: serde_json::Value) -> crate::error::Result<()> {
        self.engram.store_experience(experience).await
    }

    async fn store_anti_pattern(
        &self,
        anti_pattern: serde_json::Value,
    ) -> crate::error::Result<()> {
        self.engram.store_anti_pattern(anti_pattern).await
    }

    async fn delete_experience(&self, id: &str) -> crate::error::Result<()> {
        self.engram.delete_experience(id).await
    }

    async fn delete_anti_pattern(&self, id: &str) -> crate::error::Result<()> {
        self.engram.delete_anti_pattern(id).await
    }

    async fn increment_experience_utility(
        &self,
        id: &str,
        increment: f64,
    ) -> crate::error::Result<()> {
        self.engram
            .increment_experience_utility(id, increment)
            .await
    }

    async fn increment_anti_pattern_utility(
        &self,
        id: &str,
        increment: f64,
    ) -> crate::error::Result<()> {
        self.engram
            .increment_anti_pattern_utility(id, increment)
            .await
    }

    async fn get_experience(&self, id: &str) -> crate::error::Result<Option<serde_json::Value>> {
        self.engram.get_experience(id).await
    }

    async fn get_global_cognitive_status(&self) -> crate::error::Result<String> {
        let hot_status = self.hot.get_global_cognitive_status().await?;
        let engram_status = self.engram.get_global_cognitive_status().await?;

        if hot_status != "Stable" || engram_status != "Stable" {
            Ok(format!(
                "Mixed Status [Hot: {}, Engram: {}]",
                hot_status, engram_status
            ))
        } else {
            Ok("Stable".into())
        }
    }

    fn set_security(&self, security: Arc<dyn benshu_infra::traits::security::SecurityHandler>) {
        self.hot.set_security(security.clone());
        self.engram.set_security(security);
    }

    fn security(&self) -> Option<Arc<dyn benshu_infra::traits::security::SecurityHandler>> {
        self.hot.security()
    }

    async fn list_sessions(
        &self,
    ) -> crate::error::Result<Vec<crate::agent::session::AgentSession>> {
        let hot_sessions = self.hot.list_sessions().await?;
        let engram_sessions = self.engram.list_sessions().await?;
        Ok(Self::merge_sessions(hot_sessions, engram_sessions))
    }

    async fn prune_messages(&self, older_than: std::time::Duration) -> crate::error::Result<usize> {
        self.hot.prune_messages(older_than).await
    }

    async fn mark_cancelled(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        reason: &str,
    ) -> crate::error::Result<()> {
        self.hot.mark_cancelled(user_id, agent_id, reason).await
    }

    async fn fetch_document(
        &self,
        collection: &str,
        path: &str,
    ) -> crate::error::Result<Option<Document>> {
        self.engram.fetch_document(collection, path).await
    }

    async fn update_summary(
        &self,
        collection: &str,
        path: &str,
        summary: &str,
    ) -> crate::error::Result<()> {
        self.engram.update_summary(collection, path, summary).await
    }

    async fn get_metadata(&self, key: &str) -> crate::error::Result<Option<String>> {
        let manager_value = match key {
            "brain.memory.authority.sessions" => {
                Some("hot_for_inflight__engram_for_archived_recovery".to_string())
            }
            "brain.memory.authority.facts" => {
                Some("hot_for_mutation__engram_for_durable_lookup".to_string())
            }
            "brain.memory.authority.documents" => Some(
                "brain_policy_controls_persistence__engram_holds_durable_documents".to_string(),
            ),
            "brain.memory.relation.default_max_depth" => {
                Some(RELATION_QUERY_DEFAULT_MAX_DEPTH.to_string())
            }
            "brain.memory.relation.hard_cap_depth" => {
                Some(RELATION_QUERY_HARD_CAP_DEPTH.to_string())
            }
            "brain.memory.relation.default_max_visited_nodes" => {
                Some(RELATION_QUERY_DEFAULT_MAX_VISITED_NODES.to_string())
            }
            "brain.memory.relation.default_max_returned_edges" => {
                Some(RELATION_QUERY_DEFAULT_MAX_RETURNED_EDGES.to_string())
            }
            _ => None,
        };
        if manager_value.is_some() {
            return Ok(manager_value);
        }

        if key.starts_with("engram.") {
            return self.engram.get_metadata(key).await;
        }

        if key.starts_with("hot.") {
            return self.hot.get_metadata(key).await;
        }

        let hot_value = self.hot.get_metadata(key).await?;
        if hot_value.is_some() {
            return Ok(hot_value);
        }

        self.engram.get_metadata(key).await
    }

    async fn set_metadata(&self, key: &str, value: &str) -> crate::error::Result<()> {
        if key.starts_with("engram.") {
            return self.engram.set_metadata(key, value).await;
        }
        self.hot.set_metadata(key, value).await
    }
}

pub struct LearnedMemoryInjector {
    memory: Arc<dyn Memory>,
    max_docs: usize,
    min_query_chars: usize,
    min_top_score: f32,
}

impl LearnedMemoryInjector {
    const LONG_TERM_COLLECTIONS: [&'static str; 12] = [
        "identity",
        "personal",
        "preference",
        "preferences",
        "plan",
        "plans",
        "knowledge",
        "constraint",
        "constraints",
        "work",
        "facts",
        "kernel_facts",
    ];

    const RECALL_SIGNALS: &'static [&'static str] = &[
        "remember",
        "remind",
        "reminder",
        "recall",
        "earlier",
        "previous",
        "before",
        "last time",
        "continue",
        "pick up",
        "pick this up",
        "we discussed",
        "we decided",
        "you mentioned",
        "history",
        "context",
        "preference",
        "上次",
        "之前",
        "刚才",
        "继续",
        "接着",
        "还记得",
        "提过",
        "聊到",
        "偏好",
        "记忆",
        "提醒",
        "记得",
    ];

    pub fn new(memory: Arc<dyn Memory>) -> Self {
        Self {
            memory,
            max_docs: 3,
            min_query_chars: 12,
            min_top_score: 0.15,
        }
    }

    pub fn with_limits(
        memory: Arc<dyn Memory>,
        _recent_query_window: usize,
        max_docs: usize,
    ) -> Self {
        Self {
            memory,
            max_docs: max_docs.max(1),
            min_query_chars: 12,
            min_top_score: 0.15,
        }
    }

    fn latest_user_text(&self, history: &[crate::agent::message::Message]) -> Option<String> {
        history
            .iter()
            .rev()
            .find(|msg| matches!(msg.role, crate::agent::message::Role::User))
            .and_then(|msg| {
                let text = msg.text().trim().to_string();
                (!text.is_empty()).then_some(text)
            })
    }

    fn needs_memory_gap_fill(&self, history: &[crate::agent::message::Message]) -> bool {
        let Some(latest_user_text) = self.latest_user_text(history) else {
            return false;
        };

        let normalized = latest_user_text.to_ascii_lowercase();
        if !Self::RECALL_SIGNALS
            .iter()
            .any(|signal| normalized.contains(signal))
        {
            return false;
        }

        !history.iter().rev().take(6).any(|msg| {
            matches!(
                msg.metadata.get("tool_name").map(String::as_str),
                Some("search_history" | "tiered_search")
            ) || msg.metadata.contains_key("retrieved_from")
                || msg.metadata.contains_key("recall_source")
        })
    }

    fn sanitize_recall_query(&self, query: &str) -> String {
        let mut sanitized = query.to_string();
        for signal in Self::RECALL_SIGNALS {
            sanitized = sanitized.replace(signal, " ");
            sanitized = sanitized.replace(&signal.to_ascii_uppercase(), " ");
        }
        for filler in [
            "我们",
            "我",
            "你",
            "那个",
            "这个",
            "一下",
            "一下子",
            "吗",
            "呢",
            "呀",
            "吧",
            "啊",
        ] {
            sanitized = sanitized.replace(filler, " ");
        }

        sanitized
            .chars()
            .map(|ch| match ch {
                '？' | '?' | '！' | '!' | '。' | '.' | '，' | ',' | '；' | ';' | '：' | ':' => {
                    ' '
                }
                _ => ch,
            })
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn build_query(&self, history: &[crate::agent::message::Message]) -> Option<String> {
        if !self.needs_memory_gap_fill(history) {
            return None;
        }

        let original_query = self.latest_user_text(history)?;
        let sanitized_query = self.sanitize_recall_query(&original_query);
        if sanitized_query.chars().count() >= self.min_query_chars {
            return Some(sanitized_query);
        }
        (original_query.chars().count() >= self.min_query_chars).then_some(original_query)
    }

    fn is_long_term_recall_doc(&self, doc: &crate::knowledge::rag::Document) -> bool {
        if doc
            .metadata
            .get("memory_tier")
            .map(|value| value.eq_ignore_ascii_case("long_term"))
            .unwrap_or(false)
        {
            return true;
        }

        if doc
            .metadata
            .get("long_term")
            .map(|value| value.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
        {
            return true;
        }

        if let Some(collection) = doc.collection.as_deref() {
            let normalized = collection.trim().to_ascii_lowercase();
            if Self::LONG_TERM_COLLECTIONS
                .iter()
                .any(|allowed| normalized == *allowed)
            {
                return true;
            }
        }

        doc.path
            .as_deref()
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .is_some_and(|path| path.starts_with("manual/") || path.starts_with("knowledge/"))
    }
}

#[async_trait]
impl crate::agent::context::ContextInjector for LearnedMemoryInjector {
    async fn inject(
        &self,
        history: &[crate::agent::message::Message],
    ) -> crate::error::Result<Vec<crate::agent::message::Message>> {
        if !self.memory.capabilities().search {
            return Ok(vec![]);
        }
        if let Some(query) = self.build_query(history) {
            let mut effective_query = query.clone();
            let mut docs = self
                .memory
                .search("", None, &query, self.max_docs.saturating_mul(3))
                .await?;
            if docs.is_empty() {
                if let Some(original_query) = self.latest_user_text(history) {
                    if original_query != query {
                        let fallback_docs = self
                            .memory
                            .search("", None, &original_query, self.max_docs.saturating_mul(3))
                            .await?;
                        if !fallback_docs.is_empty() {
                            effective_query = original_query;
                            docs = fallback_docs;
                        }
                    }
                }
            }
            let docs = docs
                .into_iter()
                .filter(|doc| self.is_long_term_recall_doc(doc))
                .take(self.max_docs)
                .collect::<Vec<_>>();

            let top_score = docs.iter().map(|doc| doc.score).fold(0.0_f32, f32::max);
            if docs.is_empty() || top_score < self.min_top_score {
                return Ok(vec![]);
            }

            let mut injections = Vec::new();
            for doc in docs {
                let summary = doc.summary.as_deref().unwrap_or("No summary").trim();
                if summary.is_empty() {
                    continue;
                }
                let mut message = crate::agent::message::Message::system(format!(
                    "### LEARNED KNOWLEDGE (RAG) - Memory Gap Fill\n- **{}**: {}",
                    doc.title.trim(),
                    summary
                ));
                if let Some(path) = doc
                    .path
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    message
                        .metadata
                        .insert("retrieved_from".to_string(), path.to_string());
                } else if let Some(collection) = doc
                    .collection
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    message
                        .metadata
                        .insert("retrieved_from".to_string(), collection.to_string());
                } else {
                    message
                        .metadata
                        .insert("retrieved_from".to_string(), doc.id.clone());
                }
                message.metadata.insert(
                    "recall_source".to_string(),
                    "learned_memory_injector".to_string(),
                );
                message
                    .metadata
                    .insert("retrieval_query".to_string(), effective_query.clone());
                injections.push(message);
            }
            return Ok(injections);
        }
        Ok(vec![])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::context::ContextInjector;
    use crate::agent::memory::{BackgroundEnvelope, InMemoryMemory, Memory};
    use crate::agent::message::Message;

    #[tokio::test]
    async fn archive_session_marks_background_lifecycle_metadata() {
        let hot: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
        let durable: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
        let manager = MemoryManager::new(hot.clone(), durable.clone());

        let mut session = AgentSession::new("background-archive".to_string());
        session.background_envelope = Some(BackgroundEnvelope::default());
        Memory::store_session(&manager, session)
            .await
            .expect("session stored");

        let retention_until = chrono::Utc::now() + chrono::Duration::days(3);
        let archived = Memory::archive_session(
            &manager,
            "background-archive",
            Some("background_window_rotation"),
            Some(retention_until),
        )
        .await
        .expect("archive succeeds")
        .expect("archived session returned");

        let background = archived
            .background_envelope
            .expect("background envelope should remain attached");
        assert_eq!(
            background
                .metadata
                .get("background_session_lifecycle_state")
                .map(String::as_str),
            Some("archived")
        );
        assert_eq!(
            background
                .metadata
                .get("background_session_archive_reason")
                .map(String::as_str),
            Some("background_window_rotation")
        );
        assert!(background
            .metadata
            .contains_key("background_session_retention_until_ms"));
    }

    #[tokio::test]
    async fn recover_session_marks_background_recovery_metadata() {
        let hot: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
        let durable: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
        let manager = MemoryManager::new(hot.clone(), durable.clone());

        let mut session = AgentSession::new("background-recover".to_string());
        session.background_envelope = Some(BackgroundEnvelope::default());
        hot.store_session(session.clone()).await.expect("hot store");
        durable.store_session(session).await.expect("durable store");

        let recovered = Memory::recover_session(&manager, "background-recover", "engram")
            .await
            .expect("recover succeeds")
            .expect("recovered session returned");

        let background = recovered
            .background_envelope
            .expect("background envelope should remain attached");
        assert_eq!(
            background
                .metadata
                .get("background_session_lifecycle_state")
                .map(String::as_str),
            Some("recovered")
        );
        assert_eq!(
            background
                .metadata
                .get("background_session_recovered_from")
                .map(String::as_str),
            Some("engram")
        );
        assert!(background
            .metadata
            .contains_key("background_session_last_recovered_at_ms"));
    }

    #[tokio::test]
    async fn learned_memory_injector_skips_plain_recent_chat() {
        let memory: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
        memory
            .store_knowledge(
                "default",
                None,
                "windows-plan",
                "The local stack is converging on Windows native routing.",
                "plans",
                true,
            )
            .await
            .expect("knowledge stored");

        let injector = LearnedMemoryInjector::new(Arc::clone(&memory));
        let history = vec![
            Message::assistant("我们继续。"),
            Message::user("今天先把这个文档改一下。"),
        ];

        let injections = injector.inject(&history).await.expect("inject succeeds");
        assert!(
            injections.is_empty(),
            "plain chat should not trigger recall"
        );
    }

    #[tokio::test]
    async fn learned_memory_injector_only_fills_explicit_recall_gaps() {
        let memory: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
        memory
            .store_knowledge(
                "default",
                None,
                "Windows 主线方案",
                "我们之前定的 Windows 主线方案是让本地栈收敛到 Windows native routing。",
                "plans",
                true,
            )
            .await
            .expect("knowledge stored");

        let injector = LearnedMemoryInjector::new(Arc::clone(&memory));
        let history = vec![
            Message::assistant("好的。"),
            Message::user("还记得 Windows 主线方案吗？"),
        ];

        let injections = injector.inject(&history).await.expect("inject succeeds");
        assert_eq!(
            injections.len(),
            1,
            "explicit recall should trigger one gap fill"
        );
        let text = injections[0].text();
        assert!(text.contains("Memory Gap Fill"));
        assert!(text.contains("Windows 主线方案"));
        assert!(injections[0].metadata.contains_key("retrieved_from"));
        assert_eq!(
            injections[0]
                .metadata
                .get("recall_source")
                .map(String::as_str),
            Some("learned_memory_injector")
        );
    }

    #[tokio::test]
    async fn learned_memory_injector_skips_when_memory_tool_already_ran() {
        let memory: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
        memory
            .store_knowledge(
                "default",
                None,
                "windows-plan",
                "We previously decided the local stack should converge on Windows native routing.",
                "plans",
                true,
            )
            .await
            .expect("knowledge stored");

        let injector = LearnedMemoryInjector::new(Arc::clone(&memory));
        let mut prior_recall = Message::tool_result("tool-call-1", "Search matches...");
        prior_recall
            .metadata
            .insert("tool_name".to_string(), "search_history".to_string());
        let history = vec![
            Message::assistant("好的。"),
            Message::user("还记得我们之前定的 Windows 主线方案吗？"),
            prior_recall,
        ];

        let injections = injector.inject(&history).await.expect("inject succeeds");
        assert!(
            injections.is_empty(),
            "auto recall should stay off when memory retrieval already happened"
        );
    }

    #[tokio::test]
    async fn learned_memory_injector_prefers_long_term_collections_over_session_scratch() {
        let memory: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
        memory
            .store_knowledge(
                "default",
                None,
                "Windows 主线方案",
                "我们之前定的 Windows 主线方案是让本地栈收敛到 Windows native routing。",
                "plans",
                true,
            )
            .await
            .expect("long-term knowledge stored");
        memory
            .store_knowledge(
                "default",
                None,
                "Recent session scratch",
                "我们之前定的 Windows 主线方案只是当前 session 的临时草稿，不要当成长期记忆。",
                "session",
                true,
            )
            .await
            .expect("session scratch stored");

        let injector = LearnedMemoryInjector::new(Arc::clone(&memory));
        let history = vec![
            Message::assistant("好的。"),
            Message::user("还记得 Windows 主线方案吗？"),
        ];

        let injections = injector.inject(&history).await.expect("inject succeeds");
        assert_eq!(
            injections.len(),
            1,
            "only long-term collections should survive automatic recall"
        );
        assert!(injections[0].text().contains("Windows 主线方案"));
        assert!(
            !injections[0].text().contains("Recent session scratch"),
            "session scratch should not be treated as long-term recall material"
        );
    }
}
