use serde::{Deserialize, Serialize};

/// Phase 15.3: Memory Observability Events
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MemoryEvent {
    /// New message stored in L1
    L1Stored { session_id: String, role: String },
    /// Message batch durably persisted into L2 (Redb)
    L2Persisted { session_id: String, count: usize },
    /// Topic drift detected, triggering JIT
    JitTriggered {
        previous_intent: String,
        new_intent: String,
    },
    /// Fact promoted or created in Mid-term memory
    FactCreated {
        id: String,
        category: String,
        status: String,
    },
    /// Fact entered challenger review state
    FactReviewRequested { id: String, source: String },
    /// Fact challenger review was resolved
    FactReviewResolved {
        id: String,
        outcome: String,
        resolved_by: String,
    },
    /// Consolidation backlog snapshot for memory governance
    BacklogHealth {
        pending_backlog_before: usize,
        pending_backlog_after: usize,
        pending_review_count: usize,
        high_priority_pending: usize,
        oldest_pending_at: Option<String>,
        batches_processed: usize,
        backlog_drained: bool,
        throttle_level: String,
    },
    /// Effective review budget chosen for the current consolidation cycle
    ReviewBudgetApplied {
        throttle_level: String,
        configured_batch_size: usize,
        configured_max_batches: usize,
        configured_max_estimated_tokens: usize,
        configured_max_latency_ms: u64,
        effective_batch_size: usize,
        effective_max_batches: usize,
        effective_max_estimated_tokens: usize,
        effective_max_latency_ms: u64,
    },
    /// Session persisted into long-term memory
    SessionStored {
        session_id: String,
        status: String,
        archived: bool,
    },
    /// Session deleted from long-term memory
    SessionDeleted { session_id: String, reason: String },
    /// Document summary contract updated in long-term memory
    DocumentSummaryUpdated {
        collection: String,
        path: String,
        state: String,
    },
    /// Multimodal understanding summary or generation provenance durably recorded
    MultimodalMemoryStored {
        collection: String,
        path: String,
        kind: String,
        modality: String,
        transient: bool,
    },
    /// Memory pruned during sleep cycle
    MemoryPruned { entries: usize, reason: String },
    /// STM Persistence system failed (Disk failure, permission, etc.)
    PersistenceFailure {
        path: String,
        error: String,
        is_fatal: bool,
    },
}

/// Phase 15.3: Event Importance Levels for Sovereignty Log Filtering
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventLevel {
    Debug,
    Info,
    Warn,
}

impl Default for EventLevel {
    fn default() -> Self {
        Self::Info
    }
}

/// Interface for memory observability
pub trait MemoryEmitter: Send + Sync {
    fn emit(&self, event: MemoryEvent, level: EventLevel);
}
