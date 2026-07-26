use async_trait::async_trait;
use benshu_inference::QuantLevel;
use benshu_infra::error::{Error, Result};
use benshu_infra::traits::memory::{EventLevel, MemoryEmitter, MemoryEvent};
use benshu_infra::SecurityHandler;
use benshu_memory_core::{
    Document, Fact, FactProtection, FactReviewPayload, FactReviewResolution,
    FactReviewResolutionOutcome, MemoryCapabilities, MultimodalMemoryRecord,
};
use benshu_protocol_core::{AgentSession, Message};
use std::sync::{Arc, Weak};

/// Unified memory service contract shared by brain, durable stores, and tools.
#[async_trait]
pub trait Memory: Send + Sync {
    /// Allow downcasting to concrete types.
    fn as_any(&self) -> &dyn std::any::Any;

    /// Report supported capability families so callers do not infer support from no-op return values.
    fn capabilities(&self) -> MemoryCapabilities {
        MemoryCapabilities::default()
    }

    /// Store a message.
    async fn store(&self, user_id: &str, agent_id: Option<&str>, message: Message) -> Result<()>;

    /// Store multiple messages efficiently.
    async fn store_batch(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        messages: Vec<Message>,
    ) -> Result<()>;

    /// Retrieve recent messages.
    async fn retrieve(&self, user_id: &str, agent_id: Option<&str>, limit: usize) -> Vec<Message>;

    /// Retrieve full message history.
    async fn retrieve_full_history(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
    ) -> Result<Vec<Message>>;

    /// Clear memory.
    async fn clear(&self, user_id: &str, agent_id: Option<&str>) -> Result<()>;

    /// Undo last message.
    async fn undo(&self, user_id: &str, agent_id: Option<&str>) -> Result<Option<Message>>;

    /// Session management.
    async fn store_session(&self, session: AgentSession) -> Result<()>;
    async fn retrieve_session(&self, session_id: &str) -> Result<Option<AgentSession>>;
    async fn delete_session(&self, session_id: &str) -> Result<()>;

    async fn archive_session(
        &self,
        session_id: &str,
        reason: Option<&str>,
        retention_until: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<Option<AgentSession>> {
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
    ) -> Result<Option<AgentSession>> {
        let Some(mut session) = self.retrieve_session(session_id).await? else {
            return Ok(None);
        };
        session.mark_recovered(source);
        self.store_session(session.clone()).await?;
        Ok(Some(session))
    }

    async fn prune_expired_sessions(&self, now: chrono::DateTime<chrono::Utc>) -> Result<usize> {
        let mut pruned = 0usize;
        for session in self.list_sessions().await? {
            if session.retention_expired_at(now) {
                self.delete_session(&session.id).await?;
                pruned += 1;
            }
        }
        Ok(pruned)
    }

    /// Knowledge management.
    async fn store_fact(&self, user_id: &str, agent_id: Option<&str>, fact: Fact) -> Result<()>;
    async fn retrieve_facts(&self, user_id: &str, agent_id: Option<&str>) -> Result<Vec<Fact>>;
    async fn delete_fact(&self, user_id: &str, agent_id: Option<&str>, fact_id: &str)
        -> Result<()>;
    async fn find_related_facts(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        fact_id: &str,
        depth: usize,
    ) -> Result<Vec<Fact>>;

    async fn search(
        &self,
        _user_id: &str,
        _agent_id: Option<&str>,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<Document>> {
        Ok(Vec::new())
    }

    async fn store_knowledge(
        &self,
        _user_id: &str,
        _agent_id: Option<&str>,
        _title: &str,
        _content: &str,
        _category: &str,
        _is_unverified: bool,
    ) -> Result<()> {
        Ok(())
    }

    async fn store_multimodal_memory(
        &self,
        _user_id: &str,
        _agent_id: Option<&str>,
        _record: MultimodalMemoryRecord,
    ) -> Result<Document> {
        Err(Error::Internal(
            "multimodal memory writeback not supported by this backend".to_string(),
        ))
    }

    /// Verification workflow.
    async fn list_unverified(&self, _agent_id: Option<&str>, _limit: usize) -> Result<Vec<Fact>> {
        Ok(Vec::new())
    }

    async fn mark_verified(&self, _fact_id: &str) -> Result<()> {
        Ok(())
    }

    async fn mark_pending_review(&self, _fact_id: &str, _summary: Option<&str>) -> Result<()> {
        Ok(())
    }

    async fn request_fact_review(&self, fact_id: &str, payload: FactReviewPayload) -> Result<()> {
        self.mark_pending_review(fact_id, payload.challenger_summary.as_deref())
            .await
    }

    async fn get_fact_review_payload(&self, _fact_id: &str) -> Result<Option<FactReviewPayload>> {
        Ok(None)
    }

    async fn resolve_pending_review(
        &self,
        fact_id: &str,
        resolution: FactReviewResolution,
    ) -> Result<()> {
        match resolution.outcome {
            FactReviewResolutionOutcome::Verified => self.mark_verified(fact_id).await,
            FactReviewResolutionOutcome::Pruned => self.mark_pruned(fact_id).await,
            FactReviewResolutionOutcome::PendingReview => {
                self.mark_pending_review(fact_id, resolution.resolution_reason.as_deref())
                    .await
            }
        }
    }

    async fn mark_pruned(&self, _fact_id: &str) -> Result<()> {
        Ok(())
    }

    /// Utility tracking.
    async fn update_utility(
        &self,
        _collection: &str,
        _fact_id: &str,
        _increment: f32,
    ) -> Result<()>;

    /// Memory aging and promotion.
    async fn age_vectors(&self, _collection: &str, _older_than_days: usize) -> Result<()>;
    async fn promote_vectors(&self, _collection: &str, _level: QuantLevel) -> Result<()> {
        Ok(())
    }

    async fn update_fact_importance(
        &self,
        _user_id: &str,
        _agent_id: Option<&str>,
        _fact_id: &str,
        _importance: f32,
    ) -> Result<()> {
        Ok(())
    }

    async fn set_fact_protection(
        &self,
        _user_id: &str,
        _agent_id: Option<&str>,
        _fact_id: &str,
        _protection: FactProtection,
    ) -> Result<()> {
        Ok(())
    }

    /// Maintenance.
    async fn maintenance(&self) -> Result<()>;

    /// Metadata.
    async fn get_metadata(&self, key: &str) -> Result<Option<String>> {
        let _ = key;
        Ok(None)
    }

    async fn set_metadata(&self, key: &str, value: &str) -> Result<()> {
        let _ = (key, value);
        Ok(())
    }

    /// Security.
    fn set_security(&self, _security: Arc<dyn SecurityHandler>) {}
    fn security(&self) -> Option<Arc<dyn SecurityHandler>> {
        None
    }

    /// Experience and anti-pattern search.
    async fn search_experiences(
        &self,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<serde_json::Value>> {
        Ok(Vec::new())
    }

    async fn search_anti_patterns(
        &self,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<serde_json::Value>> {
        Ok(Vec::new())
    }

    async fn search_cognitive_guidance(
        &self,
        _query: &str,
        _limit: usize,
    ) -> Result<(Vec<serde_json::Value>, Vec<serde_json::Value>)> {
        Ok((Vec::new(), Vec::new()))
    }

    async fn store_experience(&self, _experience: serde_json::Value) -> Result<()> {
        Ok(())
    }

    async fn store_anti_pattern(&self, _anti_pattern: serde_json::Value) -> Result<()> {
        Ok(())
    }

    async fn delete_experience(&self, _id: &str) -> Result<()> {
        Ok(())
    }

    async fn delete_anti_pattern(&self, _id: &str) -> Result<()> {
        Ok(())
    }

    async fn increment_experience_utility(&self, _id: &str, _increment: f64) -> Result<()> {
        Ok(())
    }

    async fn increment_anti_pattern_utility(&self, _id: &str, _increment: f64) -> Result<()> {
        Ok(())
    }

    async fn get_experience(&self, _id: &str) -> Result<Option<serde_json::Value>> {
        Ok(None)
    }

    async fn get_anti_pattern(&self, _id: &str) -> Result<Option<serde_json::Value>> {
        Ok(None)
    }

    /// Interaction tracking.
    fn record_interaction(&self) {}
    fn last_interaction_elapsed(&self) -> std::time::Duration {
        std::time::Duration::from_secs(0)
    }

    /// Observability.
    fn set_emitter(&self, emitter: Arc<dyn MemoryEmitter>);
    fn emit_event(&self, _event: MemoryEvent, _level: EventLevel) {}

    /// Maintenance extras.
    async fn prune_messages(&self, _older_than: std::time::Duration) -> Result<usize> {
        Ok(0)
    }

    async fn list_sessions(&self) -> Result<Vec<AgentSession>> {
        Ok(Vec::new())
    }

    /// Cancellation.
    async fn mark_cancelled(
        &self,
        _user_id: &str,
        _agent_id: Option<&str>,
        _reason: &str,
    ) -> Result<()> {
        Ok(())
    }

    /// Get global status for multi-agent coordination.
    async fn get_global_cognitive_status(&self) -> Result<String> {
        Ok("Stable".into())
    }

    /// Fetch a specific document by path.
    async fn fetch_document(&self, _collection: &str, _path: &str) -> Result<Option<Document>> {
        Ok(None)
    }

    /// Scheduler integration.
    fn link_scheduler(&self, _scheduler: Weak<benshu_scheduler::Scheduler>) {}

    /// Document summary updates.
    async fn update_summary(&self, _collection: &str, _path: &str, _summary: &str) -> Result<()> {
        Ok(())
    }
}
