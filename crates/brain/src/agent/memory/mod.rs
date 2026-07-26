pub mod episodic;
pub mod facade;
pub mod knowledge;

pub use benshu_memory_core::{
    ArtifactSessionObject, BackendContextKind, BackendContextRecord, BackgroundCompressionDecision,
    BackgroundCompressionSlots, BackgroundEnvelope, BackgroundEvidenceRef, BackgroundQualitySignal,
    BackgroundRevision, FactReviewPayload, FactReviewResolution, FactReviewResolutionOutcome,
    MemoryCapabilities, MultimodalDerivedFact, MultimodalMemoryKind, MultimodalMemoryRecord,
    MultimodalSessionObject, PersonaBackgroundLayer, RecentWindowSummary,
    RelationshipBackgroundLayer, RetrievedMemoryObject, SessionBackgroundState, TaskSessionObject,
    ToolSessionObject, WebSessionObject,
};
pub use episodic::{ShortTermMemory, ShortTermMemoryConfig};
pub use facade::{BackgroundSessionPersistenceStatus, LearnedMemoryInjector, MemoryManager};
pub use knowledge::{
    Fact, FactProtection, FactStatus, Relation, RELATION_QUERY_DEFAULT_MAX_DEPTH,
    RELATION_QUERY_DEFAULT_MAX_RETURNED_EDGES, RELATION_QUERY_DEFAULT_MAX_VISITED_NODES,
    RELATION_QUERY_HARD_CAP_DEPTH,
};

use crate::agent::message::Message;
use crate::agent::session::AgentSession;
use crate::knowledge::rag::Document;
use async_trait::async_trait;
use benshu_inference::QuantLevel;
use benshu_memory_api::Memory as SharedMemory;
use std::sync::{Arc, Weak};

/// Unified Memory Facade Trait
#[async_trait]
pub trait Memory: Send + Sync {
    /// Allow downcasting to concrete types
    fn as_any(&self) -> &dyn std::any::Any;

    /// Report supported capability families so callers do not need to infer support from no-op
    /// return values.
    fn capabilities(&self) -> MemoryCapabilities {
        MemoryCapabilities::default()
    }

    /// Store a message (Episodic)
    async fn store(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        message: Message,
    ) -> crate::error::Result<()>;

    /// Store multiple messages efficiently
    async fn store_batch(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        messages: Vec<Message>,
    ) -> crate::error::Result<()>;

    /// Retrieve recent messages (Episodic)
    async fn retrieve(&self, user_id: &str, agent_id: Option<&str>, limit: usize) -> Vec<Message>;

    /// Retrieve full message history from L2 (Episodic)
    async fn retrieve_full_history(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
    ) -> crate::error::Result<Vec<Message>>;

    /// Clear memory (Episodic)
    async fn clear(&self, user_id: &str, agent_id: Option<&str>) -> crate::error::Result<()>;

    /// Undo last message (Episodic)
    async fn undo(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
    ) -> crate::error::Result<Option<Message>>;

    /// Session management
    async fn store_session(&self, session: AgentSession) -> crate::error::Result<()>;
    async fn retrieve_session(
        &self,
        session_id: &str,
    ) -> crate::error::Result<Option<AgentSession>>;
    async fn delete_session(&self, session_id: &str) -> crate::error::Result<()>;
    async fn archive_session(
        &self,
        session_id: &str,
        reason: Option<&str>,
        retention_until: Option<chrono::DateTime<chrono::Utc>>,
    ) -> crate::error::Result<Option<AgentSession>> {
        let Some(mut session) = self.retrieve_session(session_id).await? else {
            return Ok(None);
        };
        session.archive(reason.map(str::to_string), retention_until);
        self.store_session(session.clone()).await?;
        Ok(Some(session))
    }
    async fn recover_session(
        &self,
        session_id: &str,
        source: &str,
    ) -> crate::error::Result<Option<AgentSession>> {
        let Some(mut session) = self.retrieve_session(session_id).await? else {
            return Ok(None);
        };
        session.mark_recovered(source);
        self.store_session(session.clone()).await?;
        Ok(Some(session))
    }
    async fn prune_expired_sessions(
        &self,
        now: chrono::DateTime<chrono::Utc>,
    ) -> crate::error::Result<usize> {
        let mut pruned = 0usize;
        for session in self.list_sessions().await? {
            if session.retention_expired_at(now) {
                self.delete_session(&session.id).await?;
                pruned += 1;
            }
        }
        Ok(pruned)
    }

    /// Knowledge management
    async fn store_fact(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        fact: Fact,
    ) -> crate::error::Result<()>;
    async fn retrieve_facts(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
    ) -> crate::error::Result<Vec<Fact>>;
    async fn delete_fact(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        fact_id: &str,
    ) -> crate::error::Result<()>;
    async fn find_related_facts(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        fact_id: &str,
        depth: usize,
    ) -> crate::error::Result<Vec<Fact>>;

    /// Extended search (Knowledge)
    async fn search(
        &self,
        _user_id: &str,
        _agent_id: Option<&str>,
        _query: &str,
        _limit: usize,
    ) -> crate::error::Result<Vec<Document>> {
        Ok(Vec::new())
    }

    /// Storage for knowledge
    async fn store_knowledge(
        &self,
        _user_id: &str,
        _agent_id: Option<&str>,
        _title: &str,
        _content: &str,
        _category: &str,
        _is_unverified: bool,
    ) -> crate::error::Result<()> {
        Ok(())
    }

    /// Durable multimodal writeback for understanding summaries and generation provenance.
    async fn store_multimodal_memory(
        &self,
        _user_id: &str,
        _agent_id: Option<&str>,
        _record: MultimodalMemoryRecord,
    ) -> crate::error::Result<Document> {
        Err(crate::error::Error::MemoryStorage(
            "multimodal memory writeback not supported by this backend".to_string(),
        ))
    }

    /// Verification workflow
    async fn list_unverified(
        &self,
        _agent_id: Option<&str>,
        _limit: usize,
    ) -> crate::error::Result<Vec<Fact>> {
        Ok(Vec::new())
    }
    async fn mark_verified(&self, _fact_id: &str) -> crate::error::Result<()> {
        Ok(())
    }
    async fn mark_pending_review(
        &self,
        _fact_id: &str,
        _summary: Option<&str>,
    ) -> crate::error::Result<()> {
        Ok(())
    }
    async fn request_fact_review(
        &self,
        fact_id: &str,
        payload: FactReviewPayload,
    ) -> crate::error::Result<()> {
        self.mark_pending_review(fact_id, payload.challenger_summary.as_deref())
            .await
    }
    async fn get_fact_review_payload(
        &self,
        _fact_id: &str,
    ) -> crate::error::Result<Option<FactReviewPayload>> {
        Ok(None)
    }
    async fn resolve_pending_review(
        &self,
        fact_id: &str,
        resolution: FactReviewResolution,
    ) -> crate::error::Result<()> {
        match resolution.outcome {
            FactReviewResolutionOutcome::Verified => self.mark_verified(fact_id).await,
            FactReviewResolutionOutcome::Pruned => self.mark_pruned(fact_id).await,
            FactReviewResolutionOutcome::PendingReview => {
                self.mark_pending_review(fact_id, resolution.resolution_reason.as_deref())
                    .await
            }
        }
    }
    async fn mark_pruned(&self, _fact_id: &str) -> crate::error::Result<()> {
        Ok(())
    }

    /// Utility tracking
    async fn update_utility(
        &self,
        _collection: &str,
        _fact_id: &str,
        _increment: f32,
    ) -> crate::error::Result<()>;

    /// Memory aging & promotion
    async fn age_vectors(
        &self,
        _collection: &str,
        _older_than_days: usize,
    ) -> crate::error::Result<()>;
    async fn promote_vectors(
        &self,
        _collection: &str,
        _level: QuantLevel,
    ) -> crate::error::Result<()> {
        Ok(())
    }

    /// Fact Importance (Reflexion)
    async fn update_fact_importance(
        &self,
        _user_id: &str,
        _agent_id: Option<&str>,
        _fact_id: &str,
        _importance: f32,
    ) -> crate::error::Result<()> {
        Ok(())
    }

    async fn set_fact_protection(
        &self,
        _user_id: &str,
        _agent_id: Option<&str>,
        _fact_id: &str,
        _protection: FactProtection,
    ) -> crate::error::Result<()> {
        Ok(())
    }

    /// Maintenance
    async fn maintenance(&self) -> crate::error::Result<()>;

    /// Metadata
    async fn get_metadata(&self, key: &str) -> crate::error::Result<Option<String>> {
        let _ = key;
        Ok(None)
    }
    async fn set_metadata(&self, key: &str, value: &str) -> crate::error::Result<()> {
        let _ = (key, value);
        Ok(())
    }

    /// Security
    fn set_security(&self, _security: Arc<dyn crate::security::SecurityHandler>) {}
    fn security(&self) -> Option<Arc<dyn crate::security::SecurityHandler>> {
        None
    }

    /// Experience / Anti-Pattern Search
    async fn search_experiences(
        &self,
        _query: &str,
        _limit: usize,
    ) -> crate::error::Result<Vec<serde_json::Value>> {
        Ok(Vec::new())
    }
    async fn search_anti_patterns(
        &self,
        _query: &str,
        _limit: usize,
    ) -> crate::error::Result<Vec<serde_json::Value>> {
        Ok(Vec::new())
    }
    async fn search_cognitive_guidance(
        &self,
        _query: &str,
        _limit: usize,
    ) -> crate::error::Result<(Vec<serde_json::Value>, Vec<serde_json::Value>)> {
        Ok((Vec::new(), Vec::new()))
    }

    /// Experience / Anti-Pattern Storage Utility increments
    async fn store_experience(&self, _experience: serde_json::Value) -> crate::error::Result<()> {
        Ok(())
    }
    async fn store_anti_pattern(
        &self,
        _anti_pattern: serde_json::Value,
    ) -> crate::error::Result<()> {
        Ok(())
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
        Ok(())
    }
    async fn increment_anti_pattern_utility(
        &self,
        _id: &str,
        _increment: f64,
    ) -> crate::error::Result<()> {
        Ok(())
    }
    async fn get_experience(&self, _id: &str) -> crate::error::Result<Option<serde_json::Value>> {
        Ok(None)
    }
    async fn get_anti_pattern(&self, _id: &str) -> crate::error::Result<Option<serde_json::Value>> {
        Ok(None)
    }

    /// Interaction tracking
    fn record_interaction(&self) {}
    fn last_interaction_elapsed(&self) -> std::time::Duration {
        std::time::Duration::from_secs(0)
    }

    /// Observability
    fn set_emitter(&self, emitter: Arc<dyn benshu_infra::traits::memory::MemoryEmitter>);
    fn emit_event(
        &self,
        _event: benshu_infra::traits::memory::MemoryEvent,
        _level: benshu_infra::traits::memory::EventLevel,
    ) {
    }

    /// Maintenance Extras
    async fn prune_messages(
        &self,
        _older_than: std::time::Duration,
    ) -> crate::error::Result<usize> {
        Ok(0)
    }
    async fn list_sessions(
        &self,
    ) -> crate::error::Result<Vec<crate::agent::session::AgentSession>> {
        Ok(Vec::new())
    }

    /// Cancellation
    async fn mark_cancelled(
        &self,
        _user_id: &str,
        _agent_id: Option<&str>,
        _reason: &str,
    ) -> crate::error::Result<()> {
        Ok(())
    }

    /// Get global status for multi-agent coordination (crystallization)
    async fn get_global_cognitive_status(&self) -> crate::error::Result<String> {
        Ok("Stable".into())
    }

    /// Fetch a specific document by path (RAG)
    async fn fetch_document(
        &self,
        _collection: &str,
        _path: &str,
    ) -> crate::error::Result<Option<Document>> {
        Ok(None)
    }

    /// Scheduler integration (Phase 16)
    fn link_scheduler(&self, _scheduler: Weak<benshu_scheduler::Scheduler>) {}

    /// Document summary updates (Phase 16)
    async fn update_summary(
        &self,
        _collection: &str,
        _path: &str,
        _summary: &str,
    ) -> crate::error::Result<()> {
        Ok(())
    }
}

/// Adapter from the shared memory API crate into the legacy brain memory facade.
///
/// This is intentionally kept private to `brain`: cross-crate callers should pass
/// `benshu-memory-api::Memory` into `MemoryManager` instead of depending on
/// the old brain-local memory trait.
pub(crate) struct SharedMemoryAdapter {
    inner: Arc<dyn SharedMemory>,
}

impl SharedMemoryAdapter {
    pub fn new(inner: Arc<dyn SharedMemory>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl Memory for SharedMemoryAdapter {
    fn as_any(&self) -> &dyn std::any::Any {
        self.inner.as_any()
    }

    fn capabilities(&self) -> MemoryCapabilities {
        self.inner.capabilities()
    }

    async fn store(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        message: Message,
    ) -> crate::error::Result<()> {
        self.inner.store(user_id, agent_id, message).await?;
        Ok(())
    }

    async fn store_batch(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        messages: Vec<Message>,
    ) -> crate::error::Result<()> {
        self.inner.store_batch(user_id, agent_id, messages).await?;
        Ok(())
    }

    async fn retrieve(&self, user_id: &str, agent_id: Option<&str>, limit: usize) -> Vec<Message> {
        self.inner.retrieve(user_id, agent_id, limit).await
    }

    async fn retrieve_full_history(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
    ) -> crate::error::Result<Vec<Message>> {
        Ok(self.inner.retrieve_full_history(user_id, agent_id).await?)
    }

    async fn clear(&self, user_id: &str, agent_id: Option<&str>) -> crate::error::Result<()> {
        self.inner.clear(user_id, agent_id).await?;
        Ok(())
    }

    async fn undo(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
    ) -> crate::error::Result<Option<Message>> {
        Ok(self.inner.undo(user_id, agent_id).await?)
    }

    async fn store_session(&self, session: AgentSession) -> crate::error::Result<()> {
        self.inner.store_session(session).await?;
        Ok(())
    }

    async fn retrieve_session(
        &self,
        session_id: &str,
    ) -> crate::error::Result<Option<AgentSession>> {
        Ok(self.inner.retrieve_session(session_id).await?)
    }

    async fn delete_session(&self, session_id: &str) -> crate::error::Result<()> {
        self.inner.delete_session(session_id).await?;
        Ok(())
    }

    async fn store_fact(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        fact: Fact,
    ) -> crate::error::Result<()> {
        self.inner.store_fact(user_id, agent_id, fact).await?;
        Ok(())
    }

    async fn retrieve_facts(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
    ) -> crate::error::Result<Vec<Fact>> {
        Ok(self.inner.retrieve_facts(user_id, agent_id).await?)
    }

    async fn delete_fact(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        fact_id: &str,
    ) -> crate::error::Result<()> {
        self.inner.delete_fact(user_id, agent_id, fact_id).await?;
        Ok(())
    }

    async fn find_related_facts(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        fact_id: &str,
        depth: usize,
    ) -> crate::error::Result<Vec<Fact>> {
        Ok(self
            .inner
            .find_related_facts(user_id, agent_id, fact_id, depth)
            .await?)
    }

    async fn search(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        query: &str,
        limit: usize,
    ) -> crate::error::Result<Vec<Document>> {
        Ok(self.inner.search(user_id, agent_id, query, limit).await?)
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
        self.inner
            .store_knowledge(user_id, agent_id, title, content, category, is_unverified)
            .await?;
        Ok(())
    }

    async fn store_multimodal_memory(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        record: MultimodalMemoryRecord,
    ) -> crate::error::Result<Document> {
        Ok(self
            .inner
            .store_multimodal_memory(user_id, agent_id, record)
            .await?)
    }

    async fn list_unverified(
        &self,
        agent_id: Option<&str>,
        limit: usize,
    ) -> crate::error::Result<Vec<Fact>> {
        Ok(self.inner.list_unverified(agent_id, limit).await?)
    }

    async fn mark_verified(&self, fact_id: &str) -> crate::error::Result<()> {
        self.inner.mark_verified(fact_id).await?;
        Ok(())
    }

    async fn mark_pending_review(
        &self,
        fact_id: &str,
        summary: Option<&str>,
    ) -> crate::error::Result<()> {
        self.inner.mark_pending_review(fact_id, summary).await?;
        Ok(())
    }

    async fn request_fact_review(
        &self,
        fact_id: &str,
        payload: FactReviewPayload,
    ) -> crate::error::Result<()> {
        self.inner.request_fact_review(fact_id, payload).await?;
        Ok(())
    }

    async fn get_fact_review_payload(
        &self,
        fact_id: &str,
    ) -> crate::error::Result<Option<FactReviewPayload>> {
        Ok(self.inner.get_fact_review_payload(fact_id).await?)
    }

    async fn resolve_pending_review(
        &self,
        fact_id: &str,
        resolution: FactReviewResolution,
    ) -> crate::error::Result<()> {
        self.inner
            .resolve_pending_review(fact_id, resolution)
            .await?;
        Ok(())
    }

    async fn mark_pruned(&self, fact_id: &str) -> crate::error::Result<()> {
        self.inner.mark_pruned(fact_id).await?;
        Ok(())
    }

    async fn update_utility(
        &self,
        collection: &str,
        fact_id: &str,
        increment: f32,
    ) -> crate::error::Result<()> {
        self.inner
            .update_utility(collection, fact_id, increment)
            .await?;
        Ok(())
    }

    async fn age_vectors(
        &self,
        collection: &str,
        older_than_days: usize,
    ) -> crate::error::Result<()> {
        self.inner.age_vectors(collection, older_than_days).await?;
        Ok(())
    }

    async fn promote_vectors(
        &self,
        collection: &str,
        level: QuantLevel,
    ) -> crate::error::Result<()> {
        self.inner.promote_vectors(collection, level).await?;
        Ok(())
    }

    async fn update_fact_importance(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        fact_id: &str,
        importance: f32,
    ) -> crate::error::Result<()> {
        self.inner
            .update_fact_importance(user_id, agent_id, fact_id, importance)
            .await?;
        Ok(())
    }

    async fn set_fact_protection(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        fact_id: &str,
        protection: FactProtection,
    ) -> crate::error::Result<()> {
        self.inner
            .set_fact_protection(user_id, agent_id, fact_id, protection)
            .await?;
        Ok(())
    }

    async fn maintenance(&self) -> crate::error::Result<()> {
        self.inner.maintenance().await?;
        Ok(())
    }

    async fn get_metadata(&self, key: &str) -> crate::error::Result<Option<String>> {
        Ok(self.inner.get_metadata(key).await?)
    }

    async fn set_metadata(&self, key: &str, value: &str) -> crate::error::Result<()> {
        self.inner.set_metadata(key, value).await?;
        Ok(())
    }

    fn set_security(&self, security: Arc<dyn crate::security::SecurityHandler>) {
        self.inner.set_security(security);
    }

    fn security(&self) -> Option<Arc<dyn crate::security::SecurityHandler>> {
        self.inner.security()
    }

    async fn search_experiences(
        &self,
        query: &str,
        limit: usize,
    ) -> crate::error::Result<Vec<serde_json::Value>> {
        Ok(self.inner.search_experiences(query, limit).await?)
    }

    async fn search_anti_patterns(
        &self,
        query: &str,
        limit: usize,
    ) -> crate::error::Result<Vec<serde_json::Value>> {
        Ok(self.inner.search_anti_patterns(query, limit).await?)
    }

    async fn search_cognitive_guidance(
        &self,
        query: &str,
        limit: usize,
    ) -> crate::error::Result<(Vec<serde_json::Value>, Vec<serde_json::Value>)> {
        Ok(self.inner.search_cognitive_guidance(query, limit).await?)
    }

    async fn store_experience(&self, experience: serde_json::Value) -> crate::error::Result<()> {
        self.inner.store_experience(experience).await?;
        Ok(())
    }

    async fn store_anti_pattern(
        &self,
        anti_pattern: serde_json::Value,
    ) -> crate::error::Result<()> {
        self.inner.store_anti_pattern(anti_pattern).await?;
        Ok(())
    }

    async fn delete_experience(&self, id: &str) -> crate::error::Result<()> {
        self.inner.delete_experience(id).await?;
        Ok(())
    }

    async fn delete_anti_pattern(&self, id: &str) -> crate::error::Result<()> {
        self.inner.delete_anti_pattern(id).await?;
        Ok(())
    }

    async fn increment_experience_utility(
        &self,
        id: &str,
        increment: f64,
    ) -> crate::error::Result<()> {
        self.inner
            .increment_experience_utility(id, increment)
            .await?;
        Ok(())
    }

    async fn increment_anti_pattern_utility(
        &self,
        id: &str,
        increment: f64,
    ) -> crate::error::Result<()> {
        self.inner
            .increment_anti_pattern_utility(id, increment)
            .await?;
        Ok(())
    }

    async fn get_experience(&self, id: &str) -> crate::error::Result<Option<serde_json::Value>> {
        Ok(self.inner.get_experience(id).await?)
    }

    async fn get_anti_pattern(&self, id: &str) -> crate::error::Result<Option<serde_json::Value>> {
        Ok(self.inner.get_anti_pattern(id).await?)
    }

    fn record_interaction(&self) {
        self.inner.record_interaction();
    }

    fn last_interaction_elapsed(&self) -> std::time::Duration {
        self.inner.last_interaction_elapsed()
    }

    fn set_emitter(&self, emitter: Arc<dyn benshu_infra::traits::memory::MemoryEmitter>) {
        self.inner.set_emitter(emitter);
    }

    fn emit_event(
        &self,
        event: benshu_infra::traits::memory::MemoryEvent,
        level: benshu_infra::traits::memory::EventLevel,
    ) {
        self.inner.emit_event(event, level);
    }

    async fn prune_messages(&self, older_than: std::time::Duration) -> crate::error::Result<usize> {
        Ok(self.inner.prune_messages(older_than).await?)
    }

    async fn list_sessions(&self) -> crate::error::Result<Vec<AgentSession>> {
        Ok(self.inner.list_sessions().await?)
    }

    async fn mark_cancelled(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        reason: &str,
    ) -> crate::error::Result<()> {
        self.inner.mark_cancelled(user_id, agent_id, reason).await?;
        Ok(())
    }

    async fn get_global_cognitive_status(&self) -> crate::error::Result<String> {
        Ok(self.inner.get_global_cognitive_status().await?)
    }

    async fn fetch_document(
        &self,
        collection: &str,
        path: &str,
    ) -> crate::error::Result<Option<Document>> {
        Ok(self.inner.fetch_document(collection, path).await?)
    }

    fn link_scheduler(&self, scheduler: Weak<benshu_scheduler::Scheduler>) {
        self.inner.link_scheduler(scheduler);
    }

    async fn update_summary(
        &self,
        collection: &str,
        path: &str,
        summary: &str,
    ) -> crate::error::Result<()> {
        self.inner.update_summary(collection, path, summary).await?;
        Ok(())
    }
}

/// Read-only proxy for Memory (prevents mutation by sub-agents)
pub struct ReadOnlyMemory {
    inner: Arc<dyn Memory>,
}

impl ReadOnlyMemory {
    pub fn new(inner: Arc<dyn Memory>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl Memory for ReadOnlyMemory {
    fn as_any(&self) -> &dyn std::any::Any {
        self.inner.as_any()
    }

    fn capabilities(&self) -> MemoryCapabilities {
        MemoryCapabilities::read_only(self.inner.capabilities())
    }

    async fn store(
        &self,
        _user_id: &str,
        _agent_id: Option<&str>,
        _message: Message,
    ) -> crate::error::Result<()> {
        Ok(())
    }
    async fn store_batch(
        &self,
        _user_id: &str,
        _agent_id: Option<&str>,
        _messages: Vec<Message>,
    ) -> crate::error::Result<()> {
        Ok(())
    }
    async fn retrieve(&self, user_id: &str, agent_id: Option<&str>, limit: usize) -> Vec<Message> {
        self.inner.retrieve(user_id, agent_id, limit).await
    }
    async fn retrieve_full_history(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
    ) -> crate::error::Result<Vec<Message>> {
        self.inner.retrieve_full_history(user_id, agent_id).await
    }
    async fn clear(&self, _user_id: &str, _agent_id: Option<&str>) -> crate::error::Result<()> {
        Ok(())
    }
    async fn undo(
        &self,
        _user_id: &str,
        _agent_id: Option<&str>,
    ) -> crate::error::Result<Option<Message>> {
        Ok(None)
    }

    async fn store_session(&self, _session: AgentSession) -> crate::error::Result<()> {
        Ok(())
    }
    async fn retrieve_session(
        &self,
        session_id: &str,
    ) -> crate::error::Result<Option<AgentSession>> {
        self.inner.retrieve_session(session_id).await
    }
    async fn delete_session(&self, _session_id: &str) -> crate::error::Result<()> {
        Ok(())
    }
    async fn archive_session(
        &self,
        session_id: &str,
        _reason: Option<&str>,
        _retention_until: Option<chrono::DateTime<chrono::Utc>>,
    ) -> crate::error::Result<Option<AgentSession>> {
        self.inner.retrieve_session(session_id).await
    }
    async fn recover_session(
        &self,
        session_id: &str,
        _source: &str,
    ) -> crate::error::Result<Option<AgentSession>> {
        self.inner.retrieve_session(session_id).await
    }
    async fn prune_expired_sessions(
        &self,
        _now: chrono::DateTime<chrono::Utc>,
    ) -> crate::error::Result<usize> {
        Ok(0)
    }

    async fn store_fact(
        &self,
        _user_id: &str,
        _agent_id: Option<&str>,
        _fact: Fact,
    ) -> crate::error::Result<()> {
        Ok(())
    }
    async fn retrieve_facts(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
    ) -> crate::error::Result<Vec<Fact>> {
        self.inner.retrieve_facts(user_id, agent_id).await
    }
    async fn delete_fact(
        &self,
        _user_id: &str,
        _agent_id: Option<&str>,
        _fact_id: &str,
    ) -> crate::error::Result<()> {
        Ok(())
    }
    async fn find_related_facts(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        fact_id: &str,
        depth: usize,
    ) -> crate::error::Result<Vec<Fact>> {
        self.inner
            .find_related_facts(user_id, agent_id, fact_id, depth)
            .await
    }

    async fn search(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        query: &str,
        limit: usize,
    ) -> crate::error::Result<Vec<Document>> {
        self.inner.search(user_id, agent_id, query, limit).await
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
        Ok(())
    }

    async fn store_multimodal_memory(
        &self,
        _user_id: &str,
        _agent_id: Option<&str>,
        _record: MultimodalMemoryRecord,
    ) -> crate::error::Result<Document> {
        Err(crate::error::Error::PermissionDenied(
            "read-only memory cannot store multimodal writeback".to_string(),
        ))
    }

    async fn maintenance(&self) -> crate::error::Result<()> {
        Ok(())
    }

    async fn update_utility(
        &self,
        _collection: &str,
        _fact_id: &str,
        _increment: f32,
    ) -> crate::error::Result<()> {
        Ok(())
    }
    async fn age_vectors(
        &self,
        _collection: &str,
        _older_than_days: usize,
    ) -> crate::error::Result<()> {
        Ok(())
    }
    async fn promote_vectors(
        &self,
        _collection: &str,
        _level: QuantLevel,
    ) -> crate::error::Result<()> {
        Ok(())
    }
    async fn get_experience(&self, id: &str) -> crate::error::Result<Option<serde_json::Value>> {
        self.inner.get_experience(id).await
    }
    async fn get_anti_pattern(&self, id: &str) -> crate::error::Result<Option<serde_json::Value>> {
        self.inner.get_anti_pattern(id).await
    }

    async fn get_global_cognitive_status(&self) -> crate::error::Result<String> {
        self.inner.get_global_cognitive_status().await
    }

    async fn get_fact_review_payload(
        &self,
        fact_id: &str,
    ) -> crate::error::Result<Option<FactReviewPayload>> {
        self.inner.get_fact_review_payload(fact_id).await
    }

    async fn request_fact_review(
        &self,
        fact_id: &str,
        payload: FactReviewPayload,
    ) -> crate::error::Result<()> {
        self.inner.request_fact_review(fact_id, payload).await
    }

    async fn resolve_pending_review(
        &self,
        _fact_id: &str,
        _resolution: FactReviewResolution,
    ) -> crate::error::Result<()> {
        Ok(())
    }

    async fn fetch_document(
        &self,
        collection: &str,
        path: &str,
    ) -> crate::error::Result<Option<Document>> {
        self.inner.fetch_document(collection, path).await
    }

    fn record_interaction(&self) {}
    fn last_interaction_elapsed(&self) -> std::time::Duration {
        std::time::Duration::from_secs(0)
    }

    fn set_emitter(&self, emitter: Arc<dyn benshu_infra::traits::memory::MemoryEmitter>) {
        self.inner.set_emitter(emitter);
    }

    fn link_scheduler(&self, scheduler: Weak<benshu_scheduler::Scheduler>) {
        self.inner.link_scheduler(scheduler);
    }

    async fn update_summary(
        &self,
        _collection: &str,
        _path: &str,
        _summary: &str,
    ) -> crate::error::Result<()> {
        // Read-only proxy typically doesn't allow summary updates, but we can delegate if safe.
        // For now, consistent with other 'store' methods, we keep it as no-op.
        Ok(())
    }
}

use parking_lot::RwLock;
use std::collections::{HashMap, VecDeque};

/// A simple in-memory implementation of the Memory trait for testing.
pub struct InMemoryMemory {
    messages: RwLock<HashMap<String, VecDeque<Message>>>,
    facts: RwLock<HashMap<String, Vec<Fact>>>,
    sessions: RwLock<HashMap<String, crate::agent::session::AgentSession>>,
    documents: RwLock<HashMap<String, Document>>,
    experiences: RwLock<HashMap<String, serde_json::Value>>,
    review_payloads: RwLock<HashMap<String, FactReviewPayload>>,
    metadata: RwLock<HashMap<String, String>>,
    emitter: RwLock<Option<Arc<dyn benshu_infra::traits::memory::MemoryEmitter>>>,
}

impl Default for InMemoryMemory {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryMemory {
    pub fn new() -> Self {
        Self {
            messages: RwLock::new(HashMap::new()),
            facts: RwLock::new(HashMap::new()),
            sessions: RwLock::new(HashMap::new()),
            documents: RwLock::new(HashMap::new()),
            experiences: RwLock::new(HashMap::new()),
            review_payloads: RwLock::new(HashMap::new()),
            metadata: RwLock::new(HashMap::new()),
            emitter: RwLock::new(None),
        }
    }

    fn key(&self, user_id: &str, agent_id: Option<&str>) -> String {
        match agent_id {
            Some(aid) => format!("{}:{}", user_id, aid),
            None => user_id.to_string(),
        }
    }
}

#[async_trait]
impl Memory for InMemoryMemory {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn capabilities(&self) -> MemoryCapabilities {
        MemoryCapabilities {
            episodic_messages: true,
            sessions: true,
            facts: true,
            search: true,
            knowledge_store: true,
            experiences: false,
            metadata: true,
        }
    }

    async fn store(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        message: Message,
    ) -> crate::error::Result<()> {
        let key = self.key(user_id, agent_id);
        let mut messages = self.messages.write();
        messages.entry(key).or_default().push_back(message);
        Ok(())
    }

    async fn store_batch(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        messages: Vec<Message>,
    ) -> crate::error::Result<()> {
        let key = self.key(user_id, agent_id);
        let mut msg_map = self.messages.write();
        let entry = msg_map.entry(key).or_default();
        for m in messages {
            entry.push_back(m);
        }
        Ok(())
    }

    async fn retrieve(&self, user_id: &str, agent_id: Option<&str>, limit: usize) -> Vec<Message> {
        let key = self.key(user_id, agent_id);
        let messages = self.messages.read();
        messages
            .get(&key)
            .map(|v| v.iter().rev().take(limit).cloned().rev().collect())
            .unwrap_or_default()
    }

    async fn retrieve_full_history(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
    ) -> crate::error::Result<Vec<Message>> {
        let key = self.key(user_id, agent_id);
        let messages = self.messages.read();
        Ok(messages
            .get(&key)
            .map(|v| v.iter().cloned().collect())
            .unwrap_or_default())
    }

    async fn clear(&self, user_id: &str, agent_id: Option<&str>) -> crate::error::Result<()> {
        let key = self.key(user_id, agent_id);
        self.messages.write().remove(&key);
        Ok(())
    }

    async fn undo(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
    ) -> crate::error::Result<Option<Message>> {
        let key = self.key(user_id, agent_id);
        Ok(self
            .messages
            .write()
            .get_mut(&key)
            .and_then(|v| v.pop_back()))
    }

    async fn store_session(&self, session: AgentSession) -> crate::error::Result<()> {
        self.sessions.write().insert(session.id.clone(), session);
        Ok(())
    }
    async fn retrieve_session(&self, id: &str) -> crate::error::Result<Option<AgentSession>> {
        Ok(self.sessions.read().get(id).cloned())
    }
    async fn delete_session(&self, id: &str) -> crate::error::Result<()> {
        self.sessions.write().remove(id);
        Ok(())
    }

    async fn store_fact(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        fact: Fact,
    ) -> crate::error::Result<()> {
        let key = self.key(user_id, agent_id);
        self.facts.write().entry(key).or_default().push(fact);
        Ok(())
    }

    async fn retrieve_facts(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
    ) -> crate::error::Result<Vec<Fact>> {
        let key = self.key(user_id, agent_id);
        Ok(self.facts.read().get(&key).cloned().unwrap_or_default())
    }

    async fn find_related_facts(
        &self,
        uid: &str,
        aid: Option<&str>,
        fid: &str,
        d: usize,
    ) -> crate::error::Result<Vec<Fact>> {
        let key = self.key(uid, aid);
        let facts = self.facts.read().get(&key).cloned().unwrap_or_default();
        let facts_by_id = facts
            .into_iter()
            .map(|fact| (fact.id.clone(), fact))
            .collect::<std::collections::HashMap<_, _>>();
        let traversal = knowledge::traverse_related_facts_with_report(&facts_by_id, fid, d);
        {
            let mut metadata = self.metadata.write();
            for (key, value) in traversal.report.metadata_entries(fid) {
                metadata.insert(key, value);
            }
        }
        Ok(traversal.facts)
    }

    async fn maintenance(&self) -> crate::error::Result<()> {
        Ok(())
    }
    async fn update_utility(&self, _coll: &str, _id: &str, _inc: f32) -> crate::error::Result<()> {
        Ok(())
    }
    async fn age_vectors(&self, _coll: &str, _days: usize) -> crate::error::Result<()> {
        Ok(())
    }
    async fn promote_vectors(&self, _coll: &str, _level: QuantLevel) -> crate::error::Result<()> {
        Ok(())
    }
    async fn update_fact_importance(
        &self,
        _uid: &str,
        _aid: Option<&str>,
        fid: &str,
        imp: f32,
    ) -> crate::error::Result<()> {
        for facts in self.facts.write().values_mut() {
            for fact in facts {
                if fact.id == fid {
                    fact.importance = imp.clamp(0.0, 1.0);
                    fact.updated_at = chrono::Utc::now();
                }
            }
        }
        Ok(())
    }
    async fn set_fact_protection(
        &self,
        _uid: &str,
        _aid: Option<&str>,
        fid: &str,
        protection: FactProtection,
    ) -> crate::error::Result<()> {
        for facts in self.facts.write().values_mut() {
            for fact in facts {
                if fact.id == fid {
                    fact.protection = protection.clone();
                    fact.updated_at = chrono::Utc::now();
                }
            }
        }
        Ok(())
    }
    async fn get_metadata(&self, key: &str) -> crate::error::Result<Option<String>> {
        Ok(self.metadata.read().get(key).cloned())
    }
    async fn set_metadata(&self, key: &str, val: &str) -> crate::error::Result<()> {
        self.metadata
            .write()
            .insert(key.to_string(), val.to_string());
        Ok(())
    }

    async fn search(
        &self,
        _uid: &str,
        _aid: Option<&str>,
        q: &str,
        l: usize,
    ) -> crate::error::Result<Vec<Document>> {
        let query = q.to_lowercase();
        let mut docs: Vec<_> = self
            .documents
            .read()
            .values()
            .filter(|doc| {
                doc.title.to_lowercase().contains(&query)
                    || doc.content.to_lowercase().contains(&query)
            })
            .cloned()
            .collect();
        docs.truncate(l);
        Ok(docs)
    }
    async fn store_knowledge(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        title: &str,
        content: &str,
        category: &str,
        verified: bool,
    ) -> crate::error::Result<()> {
        let key = self.key(user_id, agent_id);
        let fact = Fact {
            id: uuid::Uuid::new_v4().to_string(),
            content: content.to_string(),
            category: category.to_string(),
            importance: 0.5,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            verified,
            source: None,
            confidence: 0.9,
            relations: Vec::new(),
            semantic_hash: None,
            status: if verified {
                FactStatus::Verified
            } else {
                FactStatus::Pending
            },
            protection: FactProtection::Normal,
        };
        self.facts.write().entry(key).or_default().push(fact);
        let path = format!("manual/{}", uuid::Uuid::new_v4());
        self.documents.write().insert(
            format!("{}:{}", category, path),
            Document {
                id: path.clone(),
                title: title.to_string(),
                content: content.to_string(),
                summary: None,
                collection: Some(category.to_string()),
                path: Some(path),
                metadata: HashMap::new(),
                score: 1.0,
            },
        );
        Ok(())
    }

    async fn store_multimodal_memory(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        record: MultimodalMemoryRecord,
    ) -> crate::error::Result<Document> {
        let collection = record.collection.clone();
        let path = format!("multimodal/{}/{}", record.kind_slug(), uuid::Uuid::new_v4());
        let mut metadata = record.metadata.clone();
        metadata.insert("multimodal_contract_version".to_string(), "1".to_string());
        metadata.insert(
            "multimodal_kind".to_string(),
            serde_json::to_string(&record.kind).unwrap_or_else(|_| "\"understanding\"".to_string()),
        );
        metadata.insert("multimodal_modality".to_string(), record.modality.clone());
        metadata.insert(
            "document_persistence_scope".to_string(),
            if record.transient {
                "transient".to_string()
            } else {
                "durable".to_string()
            },
        );
        metadata.insert(
            "document_context_role".to_string(),
            if record.transient {
                "transient_context".to_string()
            } else {
                "durable_document".to_string()
            },
        );
        metadata.insert(
            "document_lifecycle_state".to_string(),
            "multimodal_recorded".to_string(),
        );
        metadata.insert(
            "document_ingest_source".to_string(),
            "brain_multimodal_writeback".to_string(),
        );
        if let Some(source_path) = &record.source_path {
            metadata.insert("multimodal_source_path".to_string(), source_path.clone());
        }
        if let Some(source_url) = &record.source_url {
            metadata.insert("multimodal_source_url".to_string(), source_url.clone());
        }
        if let Some(route) = &record.route {
            metadata.insert("multimodal_route".to_string(), route.clone());
        }
        if let Some(model) = &record.model {
            metadata.insert("multimodal_model".to_string(), model.clone());
        }
        if let Some(prompt) = &record.prompt {
            metadata.insert("multimodal_prompt".to_string(), prompt.clone());
        }
        if let Some(locator) = &record.artifact_locator {
            metadata.insert("multimodal_artifact_locator".to_string(), locator.clone());
        }

        let multimodal_kind = record.kind_slug().to_string();
        let multimodal_modality = record.modality.clone();
        let multimodal_transient = record.transient;

        let document = Document {
            id: path.clone(),
            title: record.title.clone(),
            content: record.content.clone(),
            summary: Some(record.summary.clone()),
            collection: Some(collection.clone()),
            path: Some(path.clone()),
            metadata,
            score: 1.0,
        };
        self.documents
            .write()
            .insert(format!("{}:{}", collection, path), document.clone());

        if let Some(derived_fact) = record.derived_fact {
            let mut fact = Fact::new(derived_fact.content, derived_fact.category);
            fact.importance = derived_fact.importance.clamp(0.0, 1.0);
            fact.verified = derived_fact.verified;
            fact.status = if derived_fact.verified {
                FactStatus::Verified
            } else {
                FactStatus::Pending
            };
            fact.confidence = 0.8;
            fact.source = Some(format!(
                "multimodal:{}:{}",
                collection,
                document.path.as_deref().unwrap_or_default()
            ));
            self.store_fact(user_id, agent_id, fact).await?;
        }

        if let Some(emitter) = self.emitter.read().as_ref() {
            emitter.emit(
                benshu_infra::traits::memory::MemoryEvent::MultimodalMemoryStored {
                    collection,
                    path,
                    kind: multimodal_kind,
                    modality: multimodal_modality,
                    transient: multimodal_transient,
                },
                benshu_infra::traits::memory::EventLevel::Info,
            );
        }

        Ok(document)
    }

    async fn list_unverified(
        &self,
        _aid: Option<&str>,
        l: usize,
    ) -> crate::error::Result<Vec<Fact>> {
        let all_facts: Vec<Fact> = self.facts.read().values().flatten().cloned().collect();
        Ok(all_facts
            .into_iter()
            .filter(|f| !f.verified)
            .take(l)
            .collect())
    }

    async fn mark_verified(&self, fid: &str) -> crate::error::Result<()> {
        for facts in self.facts.write().values_mut() {
            for f in facts {
                if f.id == fid {
                    f.verified = true;
                    f.status = FactStatus::Verified;
                    f.updated_at = chrono::Utc::now();
                }
            }
        }
        Ok(())
    }

    async fn mark_pending_review(
        &self,
        fid: &str,
        summary: Option<&str>,
    ) -> crate::error::Result<()> {
        for facts in self.facts.write().values_mut() {
            for f in facts {
                if f.id == fid {
                    f.verified = false;
                    f.status = FactStatus::PendingReview;
                    f.updated_at = chrono::Utc::now();
                }
            }
        }
        self.review_payloads.write().insert(
            fid.to_string(),
            FactReviewPayload {
                review_reason: Some("auditor_needs_review".to_string()),
                challenger_summary: summary.map(str::to_string),
                challenger_source: Some("memory_auditor".to_string()),
                review_requested_at: Some(chrono::Utc::now()),
                resolution: None,
            },
        );
        Ok(())
    }

    async fn request_fact_review(
        &self,
        fid: &str,
        payload: FactReviewPayload,
    ) -> crate::error::Result<()> {
        for facts in self.facts.write().values_mut() {
            for f in facts {
                if f.id == fid {
                    f.verified = false;
                    f.status = FactStatus::PendingReview;
                    f.updated_at = chrono::Utc::now();
                }
            }
        }
        self.review_payloads.write().insert(
            fid.to_string(),
            FactReviewPayload {
                review_requested_at: Some(chrono::Utc::now()),
                ..payload
            },
        );
        Ok(())
    }

    async fn get_fact_review_payload(
        &self,
        fact_id: &str,
    ) -> crate::error::Result<Option<FactReviewPayload>> {
        Ok(self.review_payloads.read().get(fact_id).cloned())
    }

    async fn resolve_pending_review(
        &self,
        fact_id: &str,
        resolution: FactReviewResolution,
    ) -> crate::error::Result<()> {
        let mut payload = self
            .review_payloads
            .read()
            .get(fact_id)
            .cloned()
            .unwrap_or_default();
        payload.resolution = Some(resolution.clone());

        match resolution.outcome {
            FactReviewResolutionOutcome::Verified => self.mark_verified(fact_id).await,
            FactReviewResolutionOutcome::Pruned => self.mark_pruned(fact_id).await,
            FactReviewResolutionOutcome::PendingReview => {
                self.mark_pending_review(fact_id, payload.challenger_summary.as_deref())
                    .await
            }
        }?;
        self.review_payloads
            .write()
            .insert(fact_id.to_string(), payload);
        Ok(())
    }

    async fn mark_pruned(&self, fid: &str) -> crate::error::Result<()> {
        for facts in self.facts.write().values_mut() {
            facts.retain(|f| f.id != fid);
        }
        Ok(())
    }

    async fn delete_fact(
        &self,
        uid: &str,
        aid: Option<&str>,
        fid: &str,
    ) -> crate::error::Result<()> {
        let key = self.key(uid, aid);
        if let Some(facts) = self.facts.write().get_mut(&key) {
            facts.retain(|f| f.id != fid);
        }
        Ok(())
    }

    async fn search_experiences(
        &self,
        _q: &str,
        _l: usize,
    ) -> crate::error::Result<Vec<serde_json::Value>> {
        Ok(Vec::new())
    }
    async fn search_anti_patterns(
        &self,
        _q: &str,
        _l: usize,
    ) -> crate::error::Result<Vec<serde_json::Value>> {
        Ok(Vec::new())
    }
    async fn search_cognitive_guidance(
        &self,
        _q: &str,
        _l: usize,
    ) -> crate::error::Result<(Vec<serde_json::Value>, Vec<serde_json::Value>)> {
        Ok((Vec::new(), Vec::new()))
    }

    async fn store_experience(&self, experience: serde_json::Value) -> crate::error::Result<()> {
        if let Some(id) = experience.get("id").and_then(|v| v.as_str()) {
            self.experiences.write().insert(id.to_string(), experience);
        }
        Ok(())
    }

    async fn store_anti_pattern(&self, _ap: serde_json::Value) -> crate::error::Result<()> {
        Ok(())
    }
    async fn delete_experience(&self, id: &str) -> crate::error::Result<()> {
        self.experiences.write().remove(id);
        Ok(())
    }
    async fn delete_anti_pattern(&self, _id: &str) -> crate::error::Result<()> {
        Ok(())
    }
    async fn increment_experience_utility(&self, _id: &str, _inc: f64) -> crate::error::Result<()> {
        Ok(())
    }
    async fn increment_anti_pattern_utility(
        &self,
        _id: &str,
        _inc: f64,
    ) -> crate::error::Result<()> {
        Ok(())
    }
    async fn get_experience(&self, id: &str) -> crate::error::Result<Option<serde_json::Value>> {
        Ok(self.experiences.read().get(id).cloned())
    }
    async fn get_anti_pattern(&self, _id: &str) -> crate::error::Result<Option<serde_json::Value>> {
        Ok(None)
    }

    fn record_interaction(&self) {}
    fn last_interaction_elapsed(&self) -> std::time::Duration {
        std::time::Duration::from_secs(0)
    }
    fn set_emitter(&self, emitter: Arc<dyn benshu_infra::traits::memory::MemoryEmitter>) {
        *self.emitter.write() = Some(emitter);
    }
    fn emit_event(
        &self,
        event: benshu_infra::traits::memory::MemoryEvent,
        level: benshu_infra::traits::memory::EventLevel,
    ) {
        if let Some(emitter) = self.emitter.read().as_ref() {
            emitter.emit(event, level);
        }
    }

    async fn prune_messages(&self, _o: std::time::Duration) -> crate::error::Result<usize> {
        Ok(0)
    }
    async fn list_sessions(
        &self,
    ) -> crate::error::Result<Vec<crate::agent::session::AgentSession>> {
        Ok(self.sessions.read().values().cloned().collect())
    }
    async fn mark_cancelled(
        &self,
        _uid: &str,
        _aid: Option<&str>,
        _r: &str,
    ) -> crate::error::Result<()> {
        Ok(())
    }
    async fn get_global_cognitive_status(&self) -> crate::error::Result<String> {
        Ok("Stable".into())
    }
    async fn fetch_document(&self, coll: &str, p: &str) -> crate::error::Result<Option<Document>> {
        Ok(self
            .documents
            .read()
            .get(&format!("{}:{}", coll, p))
            .cloned())
    }
    fn link_scheduler(&self, _s: Weak<benshu_scheduler::Scheduler>) {}
    async fn update_summary(&self, coll: &str, p: &str, s: &str) -> crate::error::Result<()> {
        if let Some(doc) = self.documents.write().get_mut(&format!("{}:{}", coll, p)) {
            doc.summary = Some(s.to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn background_envelope_budget_caps_keep_layers_bounded() {
        let mut envelope = BackgroundEnvelope {
            persona_layer: Some(PersonaBackgroundLayer {
                identity_summary: Some("x".repeat(400)),
                speaking_style: Some("y".repeat(220)),
                relationship_frame: Some("z".repeat(260)),
                safety_notes: vec![
                    "a".repeat(150),
                    "b".repeat(150),
                    "c".repeat(150),
                    "d".repeat(150),
                    "e".repeat(150),
                ],
                metadata: Default::default(),
            }),
            relationship_layer: Some(RelationshipBackgroundLayer {
                user_profile_summary: Some("p".repeat(400)),
                user_preferences: (0..8).map(|_| "pref".repeat(40)).collect(),
                relationship_summary: Some("r".repeat(320)),
                long_term_topics: (0..6).map(|_| "topic".repeat(30)).collect(),
                emotional_markers: (0..6).map(|_| "emotion".repeat(20)).collect(),
                metadata: Default::default(),
            }),
            session_layer: Some(SessionBackgroundState {
                active_topics: (0..8).map(|_| "active".repeat(20)).collect(),
                backend_contexts: (0..9).map(|_| "backend".repeat(25)).collect(),
                backend_context_records: (0..9)
                    .map(|_| BackendContextRecord {
                        kind: Some(BackendContextKind::Web),
                        value: "backend".repeat(25),
                        source: Some("source".repeat(20)),
                    })
                    .collect(),
                retrieved_memory_objects: (0..7)
                    .map(|_| RetrievedMemoryObject {
                        recall_source: "relationship_memory".repeat(10),
                        recall_kind: Some("fact_lookup".repeat(12)),
                        collection: Some("memory".repeat(24)),
                        retrieval_query: Some("长期称呼偏好与协作语气".repeat(12)),
                        recall_summary: Some("最近召回的稳定协作关系提示".repeat(10)),
                    })
                    .collect(),
                web_session_objects: (0..7)
                    .map(|_| WebSessionObject {
                        url: "https://example.com/background-window".repeat(8),
                        page_title: Some("BenShu Gateway".repeat(12)),
                        task_goal: Some("review current browser result".repeat(10)),
                    })
                    .collect(),
                artifact_session_objects: (0..7)
                    .map(|_| ArtifactSessionObject {
                        path: "/tmp/spec.pdf".repeat(16),
                        collection: Some("docs".repeat(25)),
                        task_goal: Some("compare with current plan".repeat(12)),
                    })
                    .collect(),
                task_session_objects: (0..7)
                    .map(|_| TaskSessionObject {
                        state: "background_window_review".repeat(10),
                        title: Some("Agent 背景压缩主线".repeat(12)),
                        goal: Some("keep persona and relationship stable".repeat(10)),
                    })
                    .collect(),
                tool_session_objects: (0..7)
                    .map(|_| ToolSessionObject {
                        tool_name: "browser_snapshot".repeat(8),
                        result_summary: Some(
                            "current browser result enters active background".repeat(8),
                        ),
                        route: Some("browser_snapshot".repeat(10)),
                        source_ref: Some("https://example.com/background-window".repeat(8)),
                    })
                    .collect(),
                multimodal_session_objects: (0..7)
                    .map(|_| MultimodalSessionObject {
                        locator: "/tmp/dashboard.png".repeat(12),
                        route: Some("image_page_raster".repeat(10)),
                        modality: Some("image".to_string()),
                        collection: Some("desktop_capture".repeat(10)),
                        source_url: Some("https://example.com/dashboard.png".repeat(8)),
                        title: Some("dashboard screenshot".repeat(10)),
                        task_goal: Some("review current browser result".repeat(10)),
                    })
                    .collect(),
                open_loops: (0..7).map(|_| "loop".repeat(35)).collect(),
                recent_emotional_state: Some("mood".repeat(40)),
                ongoing_goals: (0..6).map(|_| "goal".repeat(30)).collect(),
                workspace_focus: Some("focus".repeat(40)),
                pending_followups: (0..7).map(|_| "follow".repeat(30)).collect(),
                summary: Some("summary".repeat(80)),
                metadata: Default::default(),
            }),
            recent_window_summary: Some(RecentWindowSummary {
                summary: "window".repeat(80),
                pruned_message_count: 20,
                covered_message_count: 40,
                metadata: Default::default(),
            }),
            source_refs: (0..12)
                .map(|idx| BackgroundEvidenceRef {
                    source_kind: "message".to_string(),
                    source_id: format!("m-{idx}"),
                    confidence: Some(0.5),
                    occurred_at: None,
                    metadata: Default::default(),
                })
                .collect(),
            ..Default::default()
        };

        envelope.apply_budget_caps();

        let persona = envelope.persona_layer.expect("persona layer");
        assert!(persona
            .identity_summary
            .as_deref()
            .is_some_and(|value| value.chars().count() <= 243));
        assert!(persona.safety_notes.len() <= 4);
        assert!(persona
            .safety_notes
            .iter()
            .all(|value| value.chars().count() <= 123));

        let relationship = envelope.relationship_layer.expect("relationship layer");
        assert!(relationship.user_preferences.len() <= 6);
        assert!(relationship.long_term_topics.len() <= 4);
        assert!(relationship.emotional_markers.len() <= 4);

        let session = envelope.session_layer.expect("session layer");
        assert!(session.active_topics.len() <= 5);
        assert!(session.backend_contexts.len() <= 8);
        assert!(session.backend_context_records.len() <= 8);
        assert!(session
            .backend_context_records
            .iter()
            .all(|record| record.value.chars().count() <= 143));
        assert!(session.retrieved_memory_objects.len() <= 6);
        assert!(session.retrieved_memory_objects.iter().all(|object| object
            .recall_source
            .chars()
            .count()
            <= 83));
        assert!(session.retrieved_memory_objects.iter().all(|object| object
            .recall_kind
            .as_deref()
            .is_none_or(|value| value.chars().count() <= 63)));
        assert!(session.web_session_objects.len() <= 6);
        assert!(session
            .web_session_objects
            .iter()
            .all(|object| object.url.chars().count() <= 143));
        assert!(session.artifact_session_objects.len() <= 6);
        assert!(session
            .artifact_session_objects
            .iter()
            .all(|object| object.path.chars().count() <= 143));
        assert!(session.task_session_objects.len() <= 6);
        assert!(session
            .task_session_objects
            .iter()
            .all(|object| object.state.chars().count() <= 103));
        assert!(session.tool_session_objects.len() <= 6);
        assert!(session
            .tool_session_objects
            .iter()
            .all(|object| object.tool_name.chars().count() <= 67));
        assert!(session.multimodal_session_objects.len() <= 6);
        assert!(session
            .multimodal_session_objects
            .iter()
            .all(|object| object.locator.chars().count() <= 143));
        assert!(session.open_loops.len() <= 5);
        assert!(session.ongoing_goals.len() <= 4);
        assert!(session.pending_followups.len() <= 5);
        assert!(session
            .summary
            .as_deref()
            .is_some_and(|value| value.chars().count() <= 323));

        let recent = envelope
            .recent_window_summary
            .expect("recent window summary");
        assert!(recent.summary.chars().count() <= 363);
        assert!(envelope.source_refs.len() <= 8);
    }

    #[test]
    fn session_background_compression_slots_round_trip_and_cap() {
        let mut session = SessionBackgroundState::default();
        session.set_compression_slots(BackgroundCompressionSlots {
            project_facts: (0..8).map(|_| "project fact ".repeat(20)).collect(),
            current_task: Some("current task ".repeat(30)),
            completed_work: (0..7).map(|_| "completed ".repeat(20)).collect(),
            pending_work: (0..7).map(|_| "pending ".repeat(20)).collect(),
            key_files: (0..10).map(|_| "key_file ".repeat(30)).collect(),
            test_results: (0..8).map(|_| "test_result ".repeat(25)).collect(),
            risks: (0..6).map(|_| "risk ".repeat(40)).collect(),
            verification_needs: (0..6).map(|_| "verify ".repeat(40)).collect(),
        });

        let slots = session.compression_slots();
        assert!(slots.project_facts.len() <= 5);
        assert!(slots.completed_work.len() <= 5);
        assert!(slots.pending_work.len() <= 5);
        assert!(slots.key_files.len() <= 8);
        assert!(slots.test_results.len() <= 6);
        assert!(slots.risks.len() <= 4);
        assert!(slots.verification_needs.len() <= 4);
        assert!(slots
            .current_task
            .as_deref()
            .is_some_and(|value| value.chars().count() <= 183));
        assert!(!session.compression_slots_are_empty());
    }

    #[tokio::test]
    async fn readonly_memory_as_any_exposes_inner_backend() {
        let inner: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
        let readonly: Arc<dyn Memory> = Arc::new(ReadOnlyMemory::new(inner));

        assert!(readonly.as_any().downcast_ref::<InMemoryMemory>().is_some());
    }

    #[tokio::test]
    async fn delete_fact_respects_memory_partition_and_mark_pruned_removes_globally() {
        let memory = InMemoryMemory::new();
        let shared_fact_id = "shared-fact-id";

        let mut alpha_fact = Fact::new("alpha fact", "relationship_background");
        alpha_fact.id = shared_fact_id.to_string();
        let mut beta_fact = Fact::new("beta fact", "relationship_background");
        beta_fact.id = shared_fact_id.to_string();

        memory
            .store_fact("alpha", Some("agent-a"), alpha_fact)
            .await
            .expect("store alpha fact");
        memory
            .store_fact("beta", Some("agent-a"), beta_fact)
            .await
            .expect("store beta fact");

        memory
            .delete_fact("alpha", Some("agent-a"), shared_fact_id)
            .await
            .expect("delete alpha fact only");

        assert!(memory
            .retrieve_facts("alpha", Some("agent-a"))
            .await
            .expect("retrieve alpha facts")
            .is_empty());
        assert_eq!(
            memory
                .retrieve_facts("beta", Some("agent-a"))
                .await
                .expect("retrieve beta facts")
                .len(),
            1
        );

        memory
            .mark_pruned(shared_fact_id)
            .await
            .expect("globally prune fact id");

        assert!(memory
            .retrieve_facts("beta", Some("agent-a"))
            .await
            .expect("retrieve beta facts after prune")
            .is_empty());
    }
}
