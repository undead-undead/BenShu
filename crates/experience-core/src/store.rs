use crate::matcher::{rank_experiences, ExperienceMatch, ExperienceQuery};
use crate::model::{
    current_time_ms, normalize_key, normalize_namespace, ExperienceStatus, TaskExperience,
};
use anyhow::{Context, Result};
use redb::{Database, ReadableTable, ReadableTableMetadata, TableDefinition};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Arc;

type BytesTable = TableDefinition<'static, &'static str, &'static [u8]>;
type StrTable = TableDefinition<'static, &'static str, &'static str>;

const EXPERIENCES_TABLE: BytesTable = TableDefinition::new("experiences");
const INDEX_SCOPE_TASK_TABLE: StrTable = TableDefinition::new("experience_by_scope_task");
const INDEX_WORKER_TABLE: StrTable = TableDefinition::new("experience_by_worker");
const INDEX_TOOL_TABLE: StrTable = TableDefinition::new("experience_by_tool");
const INDEX_STATUS_TABLE: StrTable = TableDefinition::new("experience_by_status");
const METADATA_TABLE: StrTable = TableDefinition::new("metadata");
const STORE_VERSION_KEY: &str = "store_version";
const STORE_VERSION_VALUE: &str = "1";
const SEP: &str = "\u{1f}";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExperienceStoreStats {
    pub path: PathBuf,
    pub total_experiences: u64,
    pub index_scope_task_entries: u64,
    pub index_worker_entries: u64,
    pub index_tool_entries: u64,
    pub index_status_entries: u64,
}

#[derive(Clone)]
pub struct ExperienceStore {
    db: Arc<Database>,
    path: PathBuf,
}

impl ExperienceStore {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("failed to create experience store dir {}", parent.display())
            })?;
        }

        let db = Database::create(&path)
            .with_context(|| format!("failed to open experience store {}", path.display()))?;
        let write_txn = db.begin_write()?;
        {
            let _ = write_txn.open_table(EXPERIENCES_TABLE)?;
            let _ = write_txn.open_table(INDEX_SCOPE_TASK_TABLE)?;
            let _ = write_txn.open_table(INDEX_WORKER_TABLE)?;
            let _ = write_txn.open_table(INDEX_TOOL_TABLE)?;
            let _ = write_txn.open_table(INDEX_STATUS_TABLE)?;
            let mut metadata = write_txn.open_table(METADATA_TABLE)?;
            metadata.insert(STORE_VERSION_KEY, STORE_VERSION_VALUE)?;
        }
        write_txn.commit()?;

        Ok(Self {
            db: Arc::new(db),
            path,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn upsert(&self, mut experience: TaskExperience) -> Result<TaskExperience> {
        let now = current_time_ms();
        experience.normalize_before_store(now);

        let write_txn = self.db.begin_write()?;
        let existing = {
            let table = write_txn.open_table(EXPERIENCES_TABLE)?;
            let existing = table
                .get(experience.id.as_str())?
                .map(|bytes| serde_json::from_slice::<TaskExperience>(bytes.value()))
                .transpose()
                .context("failed to deserialize existing experience")?;
            existing
        };

        if let Some(old) = existing.as_ref() {
            remove_indexes(&write_txn, old)?;
        }
        {
            let mut table = write_txn.open_table(EXPERIENCES_TABLE)?;
            let data = serde_json::to_vec(&experience).context("failed to serialize experience")?;
            table.insert(experience.id.as_str(), data.as_slice())?;
        }
        insert_indexes(&write_txn, &experience)?;
        write_txn.commit()?;
        Ok(experience)
    }

    pub fn get(&self, id: &str) -> Result<Option<TaskExperience>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(EXPERIENCES_TABLE)?;
        table
            .get(id)?
            .map(|bytes| serde_json::from_slice::<TaskExperience>(bytes.value()))
            .transpose()
            .context("failed to deserialize experience")
    }

    pub fn delete(&self, id: &str) -> Result<bool> {
        let write_txn = self.db.begin_write()?;
        let existing = {
            let table = write_txn.open_table(EXPERIENCES_TABLE)?;
            let existing = table
                .get(id)?
                .map(|bytes| serde_json::from_slice::<TaskExperience>(bytes.value()))
                .transpose()
                .context("failed to deserialize experience before delete")?;
            existing
        };
        let Some(existing) = existing else {
            return Ok(false);
        };
        remove_indexes(&write_txn, &existing)?;
        {
            let mut table = write_txn.open_table(EXPERIENCES_TABLE)?;
            table.remove(id)?;
        }
        write_txn.commit()?;
        Ok(true)
    }

    pub fn list(&self) -> Result<Vec<TaskExperience>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(EXPERIENCES_TABLE)?;
        let mut records = Vec::new();
        let mut iter = table.iter()?;
        while let Some(entry) = iter.next() {
            let (_, value) = entry?;
            records.push(
                serde_json::from_slice::<TaskExperience>(value.value())
                    .context("failed to deserialize experience during list")?,
            );
        }
        Ok(records)
    }

    pub fn query(&self, query: &ExperienceQuery) -> Result<Vec<ExperienceMatch>> {
        Ok(rank_experiences(self.list()?, query))
    }

    pub fn mark_selected(&self, id: &str) -> Result<Option<TaskExperience>> {
        self.update(id, |experience, now| experience.mark_selected(now))
    }

    pub fn record_preflight_result(
        &self,
        id: &str,
        passed: bool,
    ) -> Result<Option<TaskExperience>> {
        self.update(id, |experience, now| {
            experience.record_preflight_result(passed, now)
        })
    }

    pub fn record_task_result(&self, id: &str, succeeded: bool) -> Result<Option<TaskExperience>> {
        self.update(id, |experience, now| {
            experience.record_task_result(succeeded, now)
        })
    }

    pub fn prune_expired(&self, now_ms: i64) -> Result<u64> {
        let expired = self
            .list()?
            .into_iter()
            .filter(|experience| experience.is_expired_at(now_ms))
            .map(|experience| experience.id)
            .collect::<Vec<_>>();
        let mut removed = 0_u64;
        for id in expired {
            if self.delete(&id)? {
                removed = removed.saturating_add(1);
            }
        }
        Ok(removed)
    }

    pub fn stats(&self) -> Result<ExperienceStoreStats> {
        let read_txn = self.db.begin_read()?;
        Ok(ExperienceStoreStats {
            path: self.path.clone(),
            total_experiences: read_txn.open_table(EXPERIENCES_TABLE)?.len()?,
            index_scope_task_entries: read_txn.open_table(INDEX_SCOPE_TASK_TABLE)?.len()?,
            index_worker_entries: read_txn.open_table(INDEX_WORKER_TABLE)?.len()?,
            index_tool_entries: read_txn.open_table(INDEX_TOOL_TABLE)?.len()?,
            index_status_entries: read_txn.open_table(INDEX_STATUS_TABLE)?.len()?,
        })
    }

    fn update(
        &self,
        id: &str,
        mutate: impl FnOnce(&mut TaskExperience, i64),
    ) -> Result<Option<TaskExperience>> {
        let Some(mut experience) = self.get(id)? else {
            return Ok(None);
        };
        mutate(&mut experience, current_time_ms());
        self.upsert(experience).map(Some)
    }
}

fn insert_indexes(txn: &redb::WriteTransaction, experience: &TaskExperience) -> Result<()> {
    let scope_key = scoped_task_key(experience);
    {
        let mut table = txn.open_table(INDEX_SCOPE_TASK_TABLE)?;
        table.insert(scope_key.as_str(), experience.id.as_str())?;
    }
    if let Some(worker_role) = experience.worker_role.as_deref() {
        let key = index_key(
            &experience.namespace,
            &normalize_key(worker_role),
            &experience.id,
        );
        let mut table = txn.open_table(INDEX_WORKER_TABLE)?;
        table.insert(key.as_str(), experience.id.as_str())?;
    }
    {
        let mut table = txn.open_table(INDEX_TOOL_TABLE)?;
        for tool in &experience.tool_names {
            let key = index_key(&experience.namespace, &normalize_key(tool), &experience.id);
            table.insert(key.as_str(), experience.id.as_str())?;
        }
    }
    {
        let key = index_key(
            &experience.namespace,
            status_key(&experience.status),
            &experience.id,
        );
        let mut table = txn.open_table(INDEX_STATUS_TABLE)?;
        table.insert(key.as_str(), experience.id.as_str())?;
    }
    Ok(())
}

fn remove_indexes(txn: &redb::WriteTransaction, experience: &TaskExperience) -> Result<()> {
    {
        let mut table = txn.open_table(INDEX_SCOPE_TASK_TABLE)?;
        table.remove(scoped_task_key(experience).as_str())?;
    }
    if let Some(worker_role) = experience.worker_role.as_deref() {
        let key = index_key(
            &experience.namespace,
            &normalize_key(worker_role),
            &experience.id,
        );
        let mut table = txn.open_table(INDEX_WORKER_TABLE)?;
        table.remove(key.as_str())?;
    }
    {
        let mut table = txn.open_table(INDEX_TOOL_TABLE)?;
        for tool in &experience.tool_names {
            let key = index_key(&experience.namespace, &normalize_key(tool), &experience.id);
            table.remove(key.as_str())?;
        }
    }
    {
        let key = index_key(
            &experience.namespace,
            status_key(&experience.status),
            &experience.id,
        );
        let mut table = txn.open_table(INDEX_STATUS_TABLE)?;
        table.remove(key.as_str())?;
    }
    Ok(())
}

fn scoped_task_key(experience: &TaskExperience) -> String {
    [
        normalize_namespace(&experience.namespace),
        experience.scope.as_key(),
        task_hash(&experience.task_signature),
        experience.id.clone(),
    ]
    .join(SEP)
}

fn index_key(namespace: &str, value: &str, id: &str) -> String {
    [
        normalize_namespace(namespace),
        normalize_key(value),
        id.to_string(),
    ]
    .join(SEP)
}

fn status_key(status: &ExperienceStatus) -> &'static str {
    match status {
        ExperienceStatus::Candidate => "candidate",
        ExperienceStatus::Active => "active",
        ExperienceStatus::Retired => "retired",
    }
}

fn task_hash(task: &str) -> String {
    let digest = Sha256::digest(task.trim().to_lowercase().as_bytes());
    digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        EvidenceRefs, ExperienceScope, ExperienceStep, PreflightCheck, PreflightKind,
    };

    fn sample_experience() -> TaskExperience {
        let mut exp = TaskExperience::new(
            "公网网页检索后保存素材并写长文",
            "web collection to writing workflow",
            ExperienceScope::Web,
        );
        exp.worker_role = Some("researcher".to_string());
        exp.tool_names = vec![
            "browser_browse".to_string(),
            "knowledge_import_url".to_string(),
        ];
        exp.successful_steps.push(ExperienceStep {
            label: "observe".to_string(),
            action: "Open the page and verify the page title and list items before collecting."
                .to_string(),
            evidence_ref: Some("trace:abc".to_string()),
        });
        exp.required_preflight.push(PreflightCheck {
            kind: PreflightKind::DomStable,
            target: "target page".to_string(),
            description: "Page contains concrete list items, not only navigation.".to_string(),
            required: true,
        });
        exp.evidence_refs = EvidenceRefs {
            trace_id: Some("trace-1".to_string()),
            witness_id: Some("witness-1".to_string()),
            ..Default::default()
        };
        exp.confidence = 0.7;
        exp
    }

    #[test]
    fn redb_store_roundtrips_and_queries_without_touching_knowledge_namespace() {
        let dir = tempfile::tempdir().unwrap();
        let store = ExperienceStore::open(dir.path().join("experience.redb")).unwrap();
        let stored = store.upsert(sample_experience()).unwrap();

        let loaded = store.get(&stored.id).unwrap().unwrap();
        assert_eq!(loaded.namespace, "system_experience");
        assert_eq!(loaded.evidence_refs.trace_id.as_deref(), Some("trace-1"));

        let mut query = ExperienceQuery::new("需要抓取网页列表之后导入知识库再写作");
        query.scope = Some(ExperienceScope::Web);
        query.worker_role = Some("researcher".to_string());
        query.tool_name = Some("browser_browse".to_string());
        let matches = store.query(&query).unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].experience.id, stored.id);
        assert!(matches[0].score > 0.5);

        let stats = store.stats().unwrap();
        assert_eq!(stats.total_experiences, 1);
        assert_eq!(stats.index_scope_task_entries, 1);
        assert_eq!(stats.index_worker_entries, 1);
        assert_eq!(stats.index_tool_entries, 2);
    }

    #[test]
    fn expired_experiences_are_not_reused_unless_requested() {
        let dir = tempfile::tempdir().unwrap();
        let store = ExperienceStore::open(dir.path().join("experience.redb")).unwrap();
        let mut exp = sample_experience();
        exp.expires_at_ms = Some(10);
        let stored = store.upsert(exp).unwrap();

        let mut query = ExperienceQuery::new("网页检索写作");
        query.now_ms = 100;
        assert!(store.query(&query).unwrap().is_empty());

        query.include_expired = true;
        let matches = store.query(&query).unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].experience.id, stored.id);
    }

    #[test]
    fn failures_reduce_confidence_and_can_retire_record() {
        let dir = tempfile::tempdir().unwrap();
        let store = ExperienceStore::open(dir.path().join("experience.redb")).unwrap();
        let stored = store.upsert(sample_experience()).unwrap();

        let after_preflight = store
            .record_preflight_result(&stored.id, false)
            .unwrap()
            .unwrap();
        assert!(after_preflight.confidence < stored.confidence);

        let mut current = after_preflight;
        for _ in 0..5 {
            current = store
                .record_task_result(&current.id, false)
                .unwrap()
                .unwrap();
        }
        assert_eq!(current.status, ExperienceStatus::Retired);
        assert!(!current.is_reusable_at(current.updated_at_ms));
    }
}
