//! AgentOS State & Persistence Layer (BenShu-STATE)
//!
//! Provides the durable data model for Agent Snapshots, Task Statuses,
//! and System Metadata.

pub mod artifact;
pub mod run;
pub mod runtime_event;
pub mod session;
pub mod snapshot;
pub mod task;

pub use artifact::{
    ArtifactCleanupPolicy, ArtifactCleanupReport, ArtifactLifecycle, ArtifactManager,
    ArtifactQuery, ArtifactRecord, ArtifactScope, ARTIFACTS_TABLE,
};
pub use run::{RunManager, RunRecord, RUNS_TABLE};
pub use runtime_event::{
    missing_required_topics, repeated_event_signature, RuntimeCompletionDecision,
    RuntimeEventManager, RuntimeEventRecord, RuntimeProvenance, RuntimeReceipt,
    RUNTIME_EVENTS_TABLE,
};
pub use session::SessionManager;
pub use snapshot::{AgentSnapshot, SnapshotManager};
pub use task::{
    TaskArtifactRef, TaskBoundary, TaskCheckpoint, TaskContract, TaskEvidenceRequirement,
    TaskManager, TaskState, TaskStatus, TaskVerification, TaskVerificationVerdict,
};

use redb::Database;
use std::sync::Arc;

/// System state management container
pub struct StateProvider {
    pub snapshots: Arc<SnapshotManager>,
    pub tasks: Arc<TaskManager>,
    pub artifacts: Arc<ArtifactManager>,
    pub runs: Arc<RunManager>,
    pub runtime_events: Arc<RuntimeEventManager>,
    pub sessions: Arc<SessionManager>,
    pub db: Arc<Database>,
}

impl StateProvider {
    pub fn new(db_path: impl AsRef<std::path::Path>) -> anyhow::Result<Self> {
        let db = Arc::new(Database::create(db_path.as_ref())?);

        // Ensure tables exist on boot
        {
            let write_txn = db.begin_write()?;
            {
                let _ = write_txn.open_table(snapshot::SNAPSHOTS_TABLE)?;
                let _ = write_txn.open_table(task::TASKS_TABLE)?;
                let _ = write_txn.open_table(session::SESSIONS_TABLE)?;
                let _ = write_txn.open_table(artifact::ARTIFACTS_TABLE)?;
                let _ = write_txn.open_table(run::RUNS_TABLE)?;
                let _ = write_txn.open_table(runtime_event::RUNTIME_EVENTS_TABLE)?;
            }
            write_txn.commit()?;
        }

        Ok(Self {
            snapshots: Arc::new(SnapshotManager::new(db.clone())),
            tasks: Arc::new(TaskManager::new(db.clone())),
            artifacts: Arc::new(ArtifactManager::new(db.clone())),
            runs: Arc::new(RunManager::new(db.clone())),
            runtime_events: Arc::new(RuntimeEventManager::new(db.clone())),
            sessions: Arc::new(SessionManager::new(db.clone())),
            db,
        })
    }
}
