use chrono::{DateTime, Utc};
use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

pub const TASKS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("agent_tasks");

/// Persistence status of an asynchronous task (OS-level State)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TaskStatus {
    Pending,
    Queued,
    Running,
    Completed,
    Failed(String),
    Cancelled,
    Paused(DateTime<Utc>),
    AwaitingApproval {
        approval_kind: String,
        summary: String,
    },
    Blocked {
        reason: String,
    },
    Deferred {
        until: DateTime<Utc>,
        reason: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TaskContract {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_language: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decisions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub boundaries: Vec<TaskBoundary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub completion_criteria: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_events: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_requirements: Vec<TaskEvidenceRequirement>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lint_warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskBoundary {
    pub scope: String,
    pub rule: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskEvidenceRequirement {
    pub topic: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum_count: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskVerificationVerdict {
    Pass,
    Fail,
    Skip,
    Uncertain,
    PendingReview,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskVerification {
    pub verdict: TaskVerificationVerdict,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing_events: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_event_ids: Vec<Uuid>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskArtifactRef {
    pub artifact_id: String,
    pub kind: String,
    pub uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskCheckpoint {
    pub step: u32,
    pub label: String,
    pub recorded_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

/// Durable Task State (AgentOS Job persistence)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskState {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub status: TaskStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Task input parameters
    pub payload: serde_json::Value,
    /// Partial or final task results
    pub result: Option<serde_json::Value>,
    /// Step counter for multi-stage tasks (Fission)
    pub current_step: u32,
    pub total_steps: Option<u32>,
    /// Priority level (-128 to 127)
    pub priority: i8,
    /// Owner agent ID
    pub agent_id: String,
    /// Schema version for long-lived durable records
    #[serde(default = "default_task_contract_version")]
    pub contract_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_task_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_task_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub witness_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_receipt_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract: Option<TaskContract>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification: Option<TaskVerification>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub evidence: HashMap<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delegation_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delegation_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delegated_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delegated_to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delegation_return_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<TaskArtifactRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checkpoints: Vec<TaskCheckpoint>,
}

fn default_task_contract_version() -> u32 {
    1
}

impl TaskState {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        payload: serde_json::Value,
        agent_id: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            description: description.into(),
            status: TaskStatus::Pending,
            created_at: now,
            updated_at: now,
            payload,
            result: None,
            current_step: 0,
            total_steps: None,
            priority: 0,
            agent_id: agent_id.into(),
            contract_version: default_task_contract_version(),
            session_id: None,
            thread_id: None,
            run_id: None,
            trace_id: None,
            parent_task_id: None,
            root_task_id: None,
            witness_id: None,
            approval_receipt_id: None,
            contract: None,
            verification: None,
            evidence: HashMap::new(),
            delegation_request_id: None,
            delegation_state: None,
            delegated_by: None,
            delegated_to: None,
            delegation_return_mode: None,
            tags: Vec::new(),
            artifacts: Vec::new(),
            checkpoints: Vec::new(),
        }
    }
}

fn task_status_is_terminal(status: &TaskStatus) -> bool {
    matches!(
        status,
        TaskStatus::Completed
            | TaskStatus::Failed(_)
            | TaskStatus::Cancelled
            | TaskStatus::Blocked { .. }
    )
}

fn merge_completion_fields(existing: &TaskState, next: &mut TaskState) {
    if task_status_is_terminal(&existing.status) && !task_status_is_terminal(&next.status) {
        next.status = existing.status.clone();
    }
    if next.result.is_none() {
        next.result = existing.result.clone();
    }
    if next.verification.is_none() {
        next.verification = existing.verification.clone();
    }
    if next.artifacts.is_empty() {
        next.artifacts = existing.artifacts.clone();
    }
    for (key, value) in &existing.evidence {
        next.evidence
            .entry(key.clone())
            .or_insert_with(|| value.clone());
    }
}

pub struct TaskManager {
    db: Arc<Database>,
}

impl TaskManager {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    pub async fn save(&self, task: TaskState) -> anyhow::Result<()> {
        let id = task.id.to_string();
        let data = serde_json::to_vec(&task)?;
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(TASKS_TABLE)?;
            table.insert(id.as_str(), data.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    pub async fn save_preserving_completion_fields(
        &self,
        mut task: TaskState,
    ) -> anyhow::Result<()> {
        let id = task.id.to_string();
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(TASKS_TABLE)?;
            if let Some(existing) = table.get(id.as_str())? {
                let existing: TaskState = serde_json::from_slice(existing.value())?;
                merge_completion_fields(&existing, &mut task);
            }
            let data = serde_json::to_vec(&task)?;
            table.insert(id.as_str(), data.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    pub async fn load(&self, task_id: &str) -> anyhow::Result<Option<TaskState>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(TASKS_TABLE)?;
        let value = table.get(task_id)?;
        if let Some(data) = value {
            let task: TaskState = serde_json::from_slice(data.value())?;
            Ok(Some(task))
        } else {
            Ok(None)
        }
    }

    pub async fn list(&self) -> anyhow::Result<Vec<TaskState>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(TASKS_TABLE)?;
        let mut tasks = Vec::new();
        for entry in table.iter()? {
            let (_, value) = entry?;
            let task: TaskState = serde_json::from_slice(value.value())?;
            tasks.push(task);
        }
        Ok(tasks)
    }

    pub async fn list_by_session(&self, session_id: &str) -> anyhow::Result<Vec<TaskState>> {
        let mut tasks = self.list().await?;
        tasks.retain(|task| task.session_id.as_deref() == Some(session_id));
        tasks.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        Ok(tasks)
    }

    pub async fn upsert_artifact_refs(
        &self,
        task_id: Uuid,
        artifact_refs: Vec<TaskArtifactRef>,
    ) -> anyhow::Result<Option<TaskState>> {
        let Some(mut task) = self.load(&task_id.to_string()).await? else {
            return Ok(None);
        };

        let mut changed = false;
        for artifact in artifact_refs {
            if task
                .artifacts
                .iter()
                .any(|existing| existing.artifact_id == artifact.artifact_id)
            {
                continue;
            }
            task.artifacts.push(artifact);
            changed = true;
        }

        if changed {
            task.updated_at = Utc::now();
            self.save(task.clone()).await?;
        }

        Ok(Some(task))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    #[test]
    fn task_state_round_trip_preserves_relationship_fields() {
        let mut task = TaskState::new(
            "ingest",
            "ingest artifacts",
            json!({"path": "a.pdf"}),
            "agent-1",
        );
        task.status = TaskStatus::AwaitingApproval {
            approval_kind: "external_write".to_string(),
            summary: "Need approval before publishing".to_string(),
        };
        task.session_id = Some("session-123".to_string());
        task.thread_id = Some("thread-main".to_string());
        task.run_id = Some(Uuid::new_v4());
        task.trace_id = Some(Uuid::new_v4());
        task.parent_task_id = Some(Uuid::new_v4());
        task.root_task_id = Some(Uuid::new_v4());
        task.witness_id = Some(Uuid::new_v4());
        task.approval_receipt_id = Some(Uuid::new_v4());
        task.delegation_request_id = Some("request-1".to_string());
        task.delegation_state = Some("running".to_string());
        task.delegated_by = Some("benshu".to_string());
        task.delegated_to = Some("researcher".to_string());
        task.delegation_return_mode = Some("return_to_owner".to_string());
        task.tags = vec!["p1".to_string(), "document".to_string()];
        task.artifacts.push(TaskArtifactRef {
            artifact_id: "artifact-1".to_string(),
            kind: "witness_bundle".to_string(),
            uri: "artifacts://runs/run-1/witness.json".to_string(),
            media_type: Some("application/json".to_string()),
        });
        task.checkpoints.push(TaskCheckpoint {
            step: 1,
            label: "retrieval".to_string(),
            recorded_at: Utc::now(),
            summary: Some("seeded retrieval plan".to_string()),
        });

        let encoded = serde_json::to_value(&task).expect("serialize task");
        let decoded: TaskState = serde_json::from_value(encoded).expect("deserialize task");

        assert_eq!(decoded.session_id.as_deref(), Some("session-123"));
        assert_eq!(decoded.thread_id.as_deref(), Some("thread-main"));
        assert_eq!(decoded.tags.len(), 2);
        assert_eq!(decoded.artifacts.len(), 1);
        assert_eq!(decoded.checkpoints.len(), 1);
        assert!(matches!(
            decoded.status,
            TaskStatus::AwaitingApproval { .. }
        ));
    }

    #[test]
    fn task_state_deserializes_legacy_payload_without_new_fields() {
        let legacy = json!({
            "id": Uuid::new_v4(),
            "name": "legacy-task",
            "description": "old durable record",
            "status": "Pending",
            "created_at": Utc::now(),
            "updated_at": Utc::now(),
            "payload": {"legacy": true},
            "result": null,
            "current_step": 0,
            "total_steps": null,
            "priority": 0,
            "agent_id": "agent-legacy"
        });

        let decoded: TaskState = serde_json::from_value(legacy).expect("deserialize legacy task");

        assert_eq!(decoded.contract_version, 1);
        assert!(decoded.session_id.is_none());
        assert!(decoded.artifacts.is_empty());
        assert!(matches!(decoded.status, TaskStatus::Pending));
    }

    #[tokio::test]
    async fn list_by_session_filters_and_sorts_tasks() {
        let temp_dir = TempDir::new().expect("temp dir");
        let db_path = temp_dir.path().join("list-by-session.redb");
        let db = Arc::new(Database::create(&db_path).expect("create db"));
        {
            let write_txn = db.begin_write().expect("begin write");
            {
                let _ = write_txn.open_table(TASKS_TABLE).expect("open table");
            }
            write_txn.commit().expect("commit");
        }
        let manager = TaskManager::new(db);

        let mut first = TaskState::new("task-1", "first", json!({}), "agent");
        first.session_id = Some("session-a".to_string());
        first.updated_at = Utc::now();

        let mut second = TaskState::new("task-2", "second", json!({}), "agent");
        second.session_id = Some("session-a".to_string());
        second.updated_at = first.updated_at + chrono::Duration::seconds(5);

        let mut third = TaskState::new("task-3", "third", json!({}), "agent");
        third.session_id = Some("session-b".to_string());

        manager.save(first.clone()).await.expect("save first");
        manager.save(second.clone()).await.expect("save second");
        manager.save(third).await.expect("save third");

        let filtered = manager
            .list_by_session("session-a")
            .await
            .expect("list by session");

        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].id, second.id);
        assert_eq!(filtered[1].id, first.id);
    }

    #[tokio::test]
    async fn upsert_artifact_refs_adds_new_refs_without_duplication() {
        let temp_dir = TempDir::new().expect("temp dir");
        let db_path = temp_dir.path().join("task-artifacts.redb");
        let db = Arc::new(Database::create(&db_path).expect("create db"));
        {
            let write_txn = db.begin_write().expect("begin write");
            {
                let _ = write_txn.open_table(TASKS_TABLE).expect("open table");
            }
            write_txn.commit().expect("commit");
        }
        let manager = TaskManager::new(db);

        let task = TaskState::new("task-1", "artifact sync", json!({}), "agent");
        let task_id = task.id;
        manager.save(task).await.expect("save task");

        let refs = vec![
            TaskArtifactRef {
                artifact_id: "artifact-a".to_string(),
                kind: "witness_bundle".to_string(),
                uri: "artifacts://runs/run-1/witness.json".to_string(),
                media_type: Some("application/json".to_string()),
            },
            TaskArtifactRef {
                artifact_id: "artifact-b".to_string(),
                kind: "trace_log".to_string(),
                uri: "artifacts://runs/run-1/trace.json".to_string(),
                media_type: Some("application/json".to_string()),
            },
        ];

        let synced = manager
            .upsert_artifact_refs(task_id, refs.clone())
            .await
            .expect("sync refs")
            .expect("task exists");
        assert_eq!(synced.artifacts.len(), 2);

        let synced_again = manager
            .upsert_artifact_refs(task_id, refs)
            .await
            .expect("sync refs again")
            .expect("task exists");
        assert_eq!(synced_again.artifacts.len(), 2);
    }
}
