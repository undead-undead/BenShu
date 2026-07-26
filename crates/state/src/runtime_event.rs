use chrono::{DateTime, Utc};
use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

pub const RUNTIME_EVENTS_TABLE: TableDefinition<&str, &[u8]> =
    TableDefinition::new("runtime_events");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RuntimeProvenance {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_uri: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture_method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub captured_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeReceipt {
    pub receipt_id: Uuid,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocker: Option<String>,
}

impl RuntimeReceipt {
    pub fn new(status: impl Into<String>) -> Self {
        Self {
            receipt_id: Uuid::new_v4(),
            status: status.into(),
            started_at: None,
            finished_at: Some(Utc::now()),
            actor: None,
            action: None,
            input_fingerprint: None,
            output_fingerprint: None,
            output_preview: None,
            blocker: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeEventRecord {
    pub event_id: Uuid,
    pub topic: String,
    pub occurred_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_event_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<RuntimeReceipt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<RuntimeProvenance>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifact_ids: Vec<String>,
    #[serde(default)]
    pub payload: serde_json::Value,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, String>,
}

impl RuntimeEventRecord {
    pub fn new(topic: impl Into<String>) -> Self {
        Self {
            event_id: Uuid::new_v4(),
            topic: topic.into(),
            occurred_at: Utc::now(),
            task_id: None,
            run_id: None,
            trace_id: None,
            session_id: None,
            thread_id: None,
            actor: None,
            scope: None,
            parent_event_id: None,
            receipt: None,
            provenance: None,
            artifact_ids: Vec::new(),
            payload: serde_json::Value::Object(Default::default()),
            metadata: HashMap::new(),
        }
    }

    pub fn with_task(mut self, task_id: Uuid) -> Self {
        self.task_id = Some(task_id);
        self
    }

    pub fn with_actor(mut self, actor: impl Into<String>) -> Self {
        self.actor = Some(actor.into());
        self
    }

    pub fn with_payload(mut self, payload: serde_json::Value) -> Self {
        self.payload = payload;
        self
    }

    pub fn with_receipt(mut self, receipt: RuntimeReceipt) -> Self {
        self.receipt = Some(receipt);
        self
    }

    pub fn signature(&self) -> String {
        let step = self
            .payload
            .get("step")
            .and_then(|value| value.as_u64())
            .map(|value| value.to_string())
            .unwrap_or_default();
        let actor = self.actor.as_deref().unwrap_or_default();
        let scope = self.scope.as_deref().unwrap_or_default();
        format!("{}|{}|{}|{}", self.topic, actor, scope, step)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeCompletionDecision {
    Complete,
    Incomplete { missing_topics: Vec<String> },
}

pub struct RuntimeEventManager {
    db: Arc<Database>,
}

impl RuntimeEventManager {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    pub async fn append(&self, event: RuntimeEventRecord) -> anyhow::Result<RuntimeEventRecord> {
        let key = event_key(event.occurred_at, event.event_id);
        let data = serde_json::to_vec(&event)?;
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(RUNTIME_EVENTS_TABLE)?;
            table.insert(key.as_str(), data.as_slice())?;
        }
        write_txn.commit()?;
        Ok(event)
    }

    pub async fn list(&self) -> anyhow::Result<Vec<RuntimeEventRecord>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(RUNTIME_EVENTS_TABLE)?;
        let mut events = Vec::new();
        for entry in table.iter()? {
            let (_, value) = entry?;
            let event: RuntimeEventRecord = serde_json::from_slice(value.value())?;
            events.push(event);
        }
        events.sort_by(|left, right| {
            left.occurred_at
                .cmp(&right.occurred_at)
                .then_with(|| left.event_id.cmp(&right.event_id))
        });
        Ok(events)
    }

    pub async fn list_by_task(&self, task_id: Uuid) -> anyhow::Result<Vec<RuntimeEventRecord>> {
        let mut events = self.list().await?;
        events.retain(|event| event.task_id == Some(task_id));
        Ok(events)
    }

    pub async fn list_by_run(&self, run_id: Uuid) -> anyhow::Result<Vec<RuntimeEventRecord>> {
        let mut events = self.list().await?;
        events.retain(|event| event.run_id == Some(run_id));
        Ok(events)
    }

    pub async fn completion_decision(
        &self,
        task_id: Uuid,
        required_topics: &[String],
    ) -> anyhow::Result<RuntimeCompletionDecision> {
        let events = self.list_by_task(task_id).await?;
        let missing_topics = missing_required_topics(&events, required_topics);
        if missing_topics.is_empty() {
            Ok(RuntimeCompletionDecision::Complete)
        } else {
            Ok(RuntimeCompletionDecision::Incomplete { missing_topics })
        }
    }
}

pub fn missing_required_topics(
    events: &[RuntimeEventRecord],
    required_topics: &[String],
) -> Vec<String> {
    required_topics
        .iter()
        .filter(|topic| !events.iter().any(|event| &event.topic == *topic))
        .cloned()
        .collect()
}

pub fn repeated_event_signature(
    events: &[RuntimeEventRecord],
    recent_limit: usize,
    repeat_threshold: usize,
) -> Option<String> {
    if repeat_threshold == 0 {
        return None;
    }
    let start = events.len().saturating_sub(recent_limit);
    let mut counts: HashMap<String, usize> = HashMap::new();
    for event in &events[start..] {
        let signature = event.signature();
        let count = counts
            .entry(signature.clone())
            .and_modify(|count| *count += 1)
            .or_insert(1);
        if *count >= repeat_threshold {
            return Some(signature);
        }
    }
    None
}

fn event_key(occurred_at: DateTime<Utc>, event_id: Uuid) -> String {
    format!("{}:{}", occurred_at.format("%Y%m%d%H%M%S%.9f"), event_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_manager() -> (TempDir, RuntimeEventManager) {
        let temp = TempDir::new().expect("tempdir");
        let db = Arc::new(Database::create(temp.path().join("state.redb")).expect("db"));
        {
            let write_txn = db.begin_write().expect("write txn");
            {
                let _ = write_txn.open_table(RUNTIME_EVENTS_TABLE).expect("table");
            }
            write_txn.commit().expect("commit");
        }
        (temp, RuntimeEventManager::new(db))
    }

    #[tokio::test]
    async fn event_manager_persists_task_scoped_receipts() {
        let (_temp, manager) = test_manager();
        let task_id = Uuid::new_v4();
        let receipt = RuntimeReceipt::new("completed");
        let event = RuntimeEventRecord::new("tool.completed")
            .with_task(task_id)
            .with_actor("worker")
            .with_receipt(receipt.clone())
            .with_payload(serde_json::json!({"step": 1}));

        manager.append(event).await.expect("append");
        let events = manager.list_by_task(task_id).await.expect("list");

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].topic, "tool.completed");
        assert_eq!(events[0].receipt.as_ref(), Some(&receipt));
    }

    #[tokio::test]
    async fn completion_decision_reports_missing_topics() {
        let (_temp, manager) = test_manager();
        let task_id = Uuid::new_v4();
        manager
            .append(RuntimeEventRecord::new("step.completed").with_task(task_id))
            .await
            .expect("append");

        let decision = manager
            .completion_decision(
                task_id,
                &["step.completed".to_string(), "artifact.written".to_string()],
            )
            .await
            .expect("decision");

        assert_eq!(
            decision,
            RuntimeCompletionDecision::Incomplete {
                missing_topics: vec!["artifact.written".to_string()]
            }
        );
    }

    #[test]
    fn repeated_event_signature_detects_looping_progress() {
        let task_id = Uuid::new_v4();
        let events = (0..4)
            .map(|_| {
                RuntimeEventRecord::new("delegate.called")
                    .with_task(task_id)
                    .with_actor("researcher")
                    .with_payload(serde_json::json!({"step": 1}))
            })
            .collect::<Vec<_>>();

        let signature = repeated_event_signature(&events, 8, 3);
        assert_eq!(signature, Some("delegate.called|researcher||1".to_string()));
    }
}
