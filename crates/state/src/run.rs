use chrono::{DateTime, Utc};
use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

pub const RUNS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("agent_runs");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RunRecord {
    pub run_id: Uuid,
    pub trace_id: Uuid,
    pub session_id: Uuid,
    pub agent_id: String,
    pub trace_status: String,
    pub started_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub witness_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trial_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suite_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub benchmark_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profiler_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifact_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, String>,
}

pub struct RunManager {
    db: Arc<Database>,
}

impl RunManager {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    pub async fn save(&self, run: RunRecord) -> anyhow::Result<()> {
        let id = run.run_id.to_string();
        let data = serde_json::to_vec(&run)?;
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(RUNS_TABLE)?;
            table.insert(id.as_str(), data.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    pub async fn load(&self, run_id: &str) -> anyhow::Result<Option<RunRecord>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(RUNS_TABLE)?;
        let value = table.get(run_id)?;
        if let Some(data) = value {
            Ok(Some(serde_json::from_slice(data.value())?))
        } else {
            Ok(None)
        }
    }

    pub async fn list(&self) -> anyhow::Result<Vec<RunRecord>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(RUNS_TABLE)?;
        let mut runs = Vec::new();
        for entry in table.iter()? {
            let (_, value) = entry?;
            let run: RunRecord = serde_json::from_slice(value.value())?;
            runs.push(run);
        }
        runs.sort_by(|left, right| {
            right
                .started_at
                .cmp(&left.started_at)
                .then_with(|| left.run_id.cmp(&right.run_id))
        });
        Ok(runs)
    }

    pub async fn list_by_session(&self, session_id: Uuid) -> anyhow::Result<Vec<RunRecord>> {
        let mut runs = self.list().await?;
        runs.retain(|run| run.session_id == session_id);
        Ok(runs)
    }

    pub async fn list_by_task(&self, task_id: Uuid) -> anyhow::Result<Vec<RunRecord>> {
        let mut runs = self.list().await?;
        runs.retain(|run| run.task_id == Some(task_id));
        Ok(runs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn run_manager_persists_and_queries_runtime_links() {
        let temp = TempDir::new().expect("tempdir");
        let db = Arc::new(Database::create(temp.path().join("state.redb")).expect("db"));
        {
            let write_txn = db.begin_write().expect("write txn");
            {
                let _ = write_txn.open_table(RUNS_TABLE).expect("open runs");
            }
            write_txn.commit().expect("commit");
        }

        let manager = RunManager::new(db);
        let session_id = Uuid::new_v4();
        let task_id = Uuid::new_v4();
        let run_id = Uuid::new_v4();
        let record = RunRecord {
            run_id,
            trace_id: run_id,
            session_id,
            agent_id: "benshu".to_string(),
            trace_status: "completed".to_string(),
            started_at: Utc::now(),
            finished_at: Some(Utc::now()),
            task_id: Some(task_id),
            thread_id: Some("thread-main".to_string()),
            provider: Some("openai".to_string()),
            model: Some("benshu-unconfigured-model".to_string()),
            witness_id: Some(Uuid::new_v4()),
            trial_id: Some(Uuid::new_v4()),
            suite_id: Some("runtime_main_path".to_string()),
            benchmark_fingerprint: Some("fp-1".to_string()),
            profiler_id: Some("profiler-run".to_string()),
            artifact_ids: vec!["artifact-a".to_string(), "artifact-b".to_string()],
            metadata: HashMap::from([("route".to_string(), "main".to_string())]),
        };

        manager.save(record.clone()).await.expect("save run");

        let loaded = manager
            .load(&run_id.to_string())
            .await
            .expect("load run")
            .expect("run record");
        assert_eq!(loaded.run_id, run_id);
        assert_eq!(loaded.task_id, Some(task_id));
        assert_eq!(loaded.artifact_ids.len(), 2);

        let by_session = manager
            .list_by_session(session_id)
            .await
            .expect("list by session");
        assert_eq!(by_session.len(), 1);

        let by_task = manager.list_by_task(task_id).await.expect("list by task");
        assert_eq!(by_task.len(), 1);
        assert_eq!(by_task[0].trace_id, run_id);
    }
}
