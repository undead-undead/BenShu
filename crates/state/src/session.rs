use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const SESSIONS_TABLE: TableDefinition<&str, &str> = TableDefinition::new("agent_sessions");

/// Manages durable session-to-agent-role mappings (Phase 11.4 Stateless Fix)
pub struct SessionManager {
    db: Arc<Database>,
}

impl SessionManager {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// Persist a session mapping: session_id -> agent_role_name
    pub async fn save_session_mapping(
        &self,
        session_id: &str,
        role_name: &str,
    ) -> anyhow::Result<()> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(SESSIONS_TABLE)?;
            table.insert(session_id, role_name)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Load a session mapping
    pub async fn load_session_mapping(&self, session_id: &str) -> anyhow::Result<Option<String>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(SESSIONS_TABLE)?;
        let value = table.get(session_id)?;
        Ok(value.map(|v| v.value().to_string()))
    }

    /// List all persisted session mappings
    pub async fn list_sessions(&self) -> anyhow::Result<Vec<(String, String)>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(SESSIONS_TABLE)?;
        let mut sessions = Vec::new();
        for entry in table.iter()? {
            let (k, v) = entry?;
            sessions.push((k.value().to_string(), v.value().to_string()));
        }
        Ok(sessions)
    }

    /// Remove a session mapping
    pub async fn remove_session(&self, session_id: &str) -> anyhow::Result<bool> {
        let write_txn = self.db.begin_write()?;
        let removed = {
            let mut table = write_txn.open_table(SESSIONS_TABLE)?;
            let opt = table.remove(session_id)?;
            opt.is_some()
        };
        write_txn.commit()?;
        Ok(removed)
    }
}
