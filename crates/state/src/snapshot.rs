use chrono::{DateTime, Utc};
use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

pub const SNAPSHOTS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("agent_snapshots");

/// Agent-level Snapshot (Durable State)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSnapshot {
    pub id: Uuid,
    pub agent_id: String,
    pub name: String,
    pub timestamp: DateTime<Utc>,
    /// Last processed message ID or sequence number
    pub last_message_seq: u64,
    /// Model configuration at the time of snapshot
    pub model_config: HashMap<String, String>,
    /// Environment variables (Sanitized/Encrypted if needed)
    pub env_context: HashMap<String, String>,
    /// Custom metadata (Phase 21.4)
    pub metadata: serde_json::Value,
}

pub struct SnapshotManager {
    db: Arc<Database>,
}

impl SnapshotManager {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    pub async fn save(&self, snapshot: AgentSnapshot) -> anyhow::Result<()> {
        let id = snapshot.agent_id.clone();
        let data = serde_json::to_vec(&snapshot)?;
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(SNAPSHOTS_TABLE)?;
            table.insert(id.as_str(), data.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    pub async fn load(&self, agent_id: &str) -> anyhow::Result<Option<AgentSnapshot>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(SNAPSHOTS_TABLE)?;
        let value = table.get(agent_id)?;
        if let Some(data) = value {
            let snapshot: AgentSnapshot = serde_json::from_slice(data.value())?;
            Ok(Some(snapshot))
        } else {
            Ok(None)
        }
    }
}
