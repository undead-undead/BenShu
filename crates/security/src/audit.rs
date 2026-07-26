use anyhow::Result;
use chrono::Utc;
use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const AUDIT_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("audit_log");

#[derive(Debug, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp: u64,
    pub session_key: Option<String>,
    pub tool_name: String,
    pub arguments: String,
    pub success: bool,
    pub output_preview: String,
    pub backup: Option<benshu_infra::skill::BackupInfo>,
}

pub struct AuditLogger {
    db: Database,
}

impl AuditLogger {
    pub fn new(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let db = Database::builder().create(path)?;

        // Ensure table exists
        {
            let write_txn = db.begin_write()?;
            {
                let _ = write_txn.open_table(AUDIT_TABLE)?;
            }
            write_txn.commit()?;
        }

        Ok(Self { db })
    }

    pub fn log(&self, entry: AuditEntry) -> Result<()> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(AUDIT_TABLE)?;
            let data = bincode::serialize(&entry)?;
            table.insert(entry.timestamp, data.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    pub fn retrieve_recent(&self, limit: usize) -> Result<Vec<AuditEntry>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(AUDIT_TABLE)?;

        let mut entries = Vec::new();
        // Since redb is sorted, we can iterate from the end
        for result in table.iter()?.rev() {
            if let Ok((_key, value)) = result {
                if let Ok(entry) = bincode::deserialize(value.value()) {
                    entries.push(entry);
                }
            }
            if entries.len() >= limit {
                break;
            }
        }
        Ok(entries)
    }
}
