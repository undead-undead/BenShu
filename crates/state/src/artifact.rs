use chrono::{DateTime, Utc};
use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

pub const ARTIFACTS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("agent_artifacts");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactScope {
    Uploads,
    Workspace,
    Outputs,
    Artifacts,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactLifecycle {
    Ephemeral,
    Session,
    Durable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactRecord {
    pub artifact_id: String,
    pub kind: String,
    pub uri: String,
    pub scope: ArtifactScope,
    pub lifecycle: ArtifactLifecycle,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub agent_id: String,
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
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub virtual_path: Option<String>,
    pub source_kind: String,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<ArtifactScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle: Option<ArtifactLifecycle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactCleanupPolicy {
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default)]
    pub orphan_only: bool,
    #[serde(default)]
    pub prune_missing_local_files: bool,
    #[serde(default)]
    pub delete_local_files: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<ArtifactScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ephemeral_max_age_hours: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_max_age_hours: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub durable_max_age_days: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_delete: Option<usize>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactCleanupReport {
    pub dry_run: bool,
    pub scanned: usize,
    pub matched: usize,
    pub deleted: usize,
    pub kept: usize,
    pub orphan_matched: usize,
    pub missing_local_file_matched: usize,
    pub deleted_local_files: usize,
    pub skipped_external_files: usize,
    pub skipped_durable_without_policy: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deleted_artifact_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deleted_file_paths: Vec<String>,
}

pub struct ArtifactManager {
    db: Arc<Database>,
}

impl ArtifactManager {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    pub async fn save(&self, artifact: ArtifactRecord) -> anyhow::Result<()> {
        let id = artifact.artifact_id.clone();
        let data = serde_json::to_vec(&artifact)?;
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(ARTIFACTS_TABLE)?;
            table.insert(id.as_str(), data.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    pub async fn load(&self, artifact_id: &str) -> anyhow::Result<Option<ArtifactRecord>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(ARTIFACTS_TABLE)?;
        let value = table.get(artifact_id)?;
        if let Some(data) = value {
            Ok(Some(serde_json::from_slice(data.value())?))
        } else {
            Ok(None)
        }
    }

    pub async fn delete(&self, artifact_id: &str) -> anyhow::Result<bool> {
        let write_txn = self.db.begin_write()?;
        let removed = {
            let mut table = write_txn.open_table(ARTIFACTS_TABLE)?;
            let removed = table.remove(artifact_id)?.is_some();
            removed
        };
        write_txn.commit()?;
        Ok(removed)
    }

    pub async fn list(&self) -> anyhow::Result<Vec<ArtifactRecord>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(ARTIFACTS_TABLE)?;
        let mut artifacts = Vec::new();
        for entry in table.iter()? {
            let (_, value) = entry?;
            let artifact: ArtifactRecord = serde_json::from_slice(value.value())?;
            artifacts.push(artifact);
        }
        artifacts.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| left.artifact_id.cmp(&right.artifact_id))
        });
        Ok(artifacts)
    }

    pub async fn query(&self, query: &ArtifactQuery) -> anyhow::Result<Vec<ArtifactRecord>> {
        let mut artifacts = self.list().await?;
        artifacts.retain(|artifact| {
            if let Some(thread_id) = &query.thread_id {
                if artifact.thread_id.as_ref() != Some(thread_id) {
                    return false;
                }
            }
            if let Some(session_id) = &query.session_id {
                if artifact.session_id.as_ref() != Some(session_id) {
                    return false;
                }
            }
            if let Some(task_id) = query.task_id {
                if artifact.task_id != Some(task_id) {
                    return false;
                }
            }
            if let Some(run_id) = query.run_id {
                if artifact.run_id != Some(run_id) {
                    return false;
                }
            }
            if let Some(trace_id) = query.trace_id {
                if artifact.trace_id != Some(trace_id) {
                    return false;
                }
            }
            if let Some(scope) = &query.scope {
                if &artifact.scope != scope {
                    return false;
                }
            }
            if let Some(lifecycle) = &query.lifecycle {
                if &artifact.lifecycle != lifecycle {
                    return false;
                }
            }
            if let Some(source_kind) = &query.source_kind {
                if &artifact.source_kind != source_kind {
                    return false;
                }
            }
            true
        });
        if let Some(limit) = query.limit {
            artifacts.truncate(limit);
        }
        Ok(artifacts)
    }

    pub async fn cleanup(
        &self,
        policy: &ArtifactCleanupPolicy,
    ) -> anyhow::Result<ArtifactCleanupReport> {
        let now = Utc::now();
        let artifacts = self.list().await?;
        let mut report = ArtifactCleanupReport {
            dry_run: policy.dry_run,
            scanned: artifacts.len(),
            ..ArtifactCleanupReport::default()
        };
        let mut deleted_ids = Vec::new();
        let mut deleted_file_paths = Vec::new();

        for artifact in artifacts {
            if let Some(scope) = &policy.scope {
                if &artifact.scope != scope {
                    report.kept += 1;
                    continue;
                }
            }
            if let Some(source_kind) = &policy.source_kind {
                if &artifact.source_kind != source_kind {
                    report.kept += 1;
                    continue;
                }
            }

            let is_orphan = artifact.task_id.is_none()
                && artifact.run_id.is_none()
                && artifact.trace_id.is_none()
                && artifact.session_id.is_none()
                && artifact.thread_id.is_none();
            if policy.orphan_only && !is_orphan {
                report.kept += 1;
                continue;
            }

            let local_file_path = Self::local_file_path(&artifact);
            let missing_local_file = local_file_path.as_ref().is_some_and(|path| !path.exists());
            if missing_local_file && !policy.prune_missing_local_files {
                report.kept += 1;
                continue;
            }

            let artifact_age = now.signed_duration_since(artifact.updated_at);
            let eligible = if missing_local_file {
                true
            } else {
                match artifact.lifecycle {
                    ArtifactLifecycle::Ephemeral => policy
                        .ephemeral_max_age_hours
                        .map(|hours| artifact_age >= chrono::Duration::hours(hours))
                        .unwrap_or(false),
                    ArtifactLifecycle::Session => policy
                        .session_max_age_hours
                        .map(|hours| artifact_age >= chrono::Duration::hours(hours))
                        .unwrap_or(false),
                    ArtifactLifecycle::Durable => {
                        if let Some(days) = policy.durable_max_age_days {
                            artifact_age >= chrono::Duration::days(days)
                        } else {
                            report.skipped_durable_without_policy += 1;
                            false
                        }
                    }
                }
            };

            if !eligible {
                report.kept += 1;
                continue;
            }

            report.matched += 1;
            if is_orphan {
                report.orphan_matched += 1;
            }
            if missing_local_file {
                report.missing_local_file_matched += 1;
            }

            if let Some(max_delete) = policy.max_delete {
                if deleted_ids.len() >= max_delete {
                    report.kept += 1;
                    continue;
                }
            }

            let local_path_for_delete = if policy.delete_local_files {
                local_file_path.clone()
            } else {
                None
            };

            if !policy.dry_run {
                if self.delete(&artifact.artifact_id).await? {
                    if policy.delete_local_files {
                        match local_path_for_delete {
                            Some(path) if path.exists() => {
                                if tokio::fs::remove_file(&path).await.is_ok() {
                                    report.deleted_local_files += 1;
                                    deleted_file_paths.push(path.display().to_string());
                                }
                            }
                            Some(_) => {}
                            None if Self::looks_external_uri(&artifact.uri) => {
                                report.skipped_external_files += 1;
                            }
                            None => {}
                        }
                    }
                    deleted_ids.push(artifact.artifact_id);
                } else {
                    report.kept += 1;
                }
            } else {
                if policy.delete_local_files {
                    match local_path_for_delete {
                        Some(path) => {
                            report.deleted_local_files += 1;
                            deleted_file_paths.push(path.display().to_string());
                        }
                        None if Self::looks_external_uri(&artifact.uri) => {
                            report.skipped_external_files += 1;
                        }
                        None => {}
                    }
                }
                deleted_ids.push(artifact.artifact_id);
            }
        }

        report.deleted = deleted_ids.len();
        report.deleted_artifact_ids = deleted_ids;
        report.deleted_file_paths = deleted_file_paths;
        Ok(report)
    }

    fn local_file_path(artifact: &ArtifactRecord) -> Option<PathBuf> {
        if let Some(rest) = artifact.uri.strip_prefix("file://") {
            if rest.is_empty() {
                return None;
            }
            return Some(PathBuf::from(rest));
        }
        let uri_path = Path::new(&artifact.uri);
        if uri_path.is_absolute() {
            return Some(uri_path.to_path_buf());
        }
        None
    }

    fn looks_external_uri(uri: &str) -> bool {
        uri.contains("://") && !uri.starts_with("file://")
    }

    pub fn classify_scope(uri: &str, virtual_path: Option<&str>) -> ArtifactScope {
        let candidate = virtual_path.unwrap_or(uri).to_ascii_lowercase();
        if candidate.contains("uploads") {
            ArtifactScope::Uploads
        } else if candidate.contains("workspace") {
            ArtifactScope::Workspace
        } else if candidate.contains("outputs") {
            ArtifactScope::Outputs
        } else {
            ArtifactScope::Artifacts
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use tempfile::TempDir;

    #[tokio::test]
    async fn artifact_manager_round_trips_and_filters() {
        let temp = TempDir::new().expect("temp dir");
        let db = Arc::new(Database::create(temp.path().join("state.redb")).expect("db"));
        {
            let write_txn = db.begin_write().expect("write txn");
            {
                let _ = write_txn.open_table(ARTIFACTS_TABLE).expect("table");
            }
            write_txn.commit().expect("commit");
        }
        let manager = ArtifactManager::new(db);

        let first = ArtifactRecord {
            artifact_id: "artifact-1".to_string(),
            kind: "pdf_preview".to_string(),
            uri: "artifacts://thread-main/page-1.png".to_string(),
            scope: ArtifactManager::classify_scope(
                "artifacts://thread-main/page-1.png",
                Some("artifacts/page-1.png"),
            ),
            lifecycle: ArtifactLifecycle::Session,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            agent_id: "benshu".to_string(),
            task_id: Some(Uuid::new_v4()),
            run_id: Some(Uuid::new_v4()),
            trace_id: Some(Uuid::new_v4()),
            session_id: Some("session-1".to_string()),
            thread_id: Some("thread-main".to_string()),
            tool_name: Some("pdf_parse".to_string()),
            media_type: Some("image/png".to_string()),
            virtual_path: Some("artifacts/page-1.png".to_string()),
            source_kind: "run_trace".to_string(),
            metadata: HashMap::from([("page".to_string(), "1".to_string())]),
        };
        let second = ArtifactRecord {
            artifact_id: "artifact-2".to_string(),
            kind: "user_upload".to_string(),
            uri: "file:///tmp/uploads/source.pdf".to_string(),
            scope: ArtifactManager::classify_scope(
                "file:///tmp/uploads/source.pdf",
                Some("uploads/source.pdf"),
            ),
            lifecycle: ArtifactLifecycle::Durable,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            agent_id: "researcher".to_string(),
            task_id: None,
            run_id: None,
            trace_id: None,
            session_id: Some("session-1".to_string()),
            thread_id: Some("thread-main".to_string()),
            tool_name: None,
            media_type: Some("application/pdf".to_string()),
            virtual_path: Some("uploads/source.pdf".to_string()),
            source_kind: "task_state".to_string(),
            metadata: HashMap::new(),
        };

        manager.save(first.clone()).await.expect("save first");
        manager.save(second.clone()).await.expect("save second");

        assert_eq!(
            manager
                .load("artifact-1")
                .await
                .expect("load")
                .expect("artifact"),
            first
        );

        let thread_hits = manager
            .query(&ArtifactQuery {
                thread_id: Some("thread-main".to_string()),
                limit: Some(10),
                ..Default::default()
            })
            .await
            .expect("query");
        assert_eq!(thread_hits.len(), 2);

        let upload_hits = manager
            .query(&ArtifactQuery {
                scope: Some(ArtifactScope::Uploads),
                limit: Some(10),
                ..Default::default()
            })
            .await
            .expect("query uploads");
        assert_eq!(upload_hits.len(), 1);
        assert_eq!(upload_hits[0].artifact_id, "artifact-2");
    }

    #[tokio::test]
    async fn artifact_cleanup_respects_lifecycle_policy() {
        let temp = TempDir::new().expect("temp dir");
        let db = Arc::new(Database::create(temp.path().join("state.redb")).expect("db"));
        {
            let write_txn = db.begin_write().expect("write txn");
            {
                let _ = write_txn.open_table(ARTIFACTS_TABLE).expect("table");
            }
            write_txn.commit().expect("commit");
        }
        let manager = ArtifactManager::new(db);
        let stale_time = Utc::now() - Duration::hours(48);
        let fresh_time = Utc::now() - Duration::hours(1);

        manager
            .save(ArtifactRecord {
                artifact_id: "ephemeral-old".to_string(),
                kind: "tmp".to_string(),
                uri: "file:///tmp/workspace/tmp.txt".to_string(),
                scope: ArtifactScope::Workspace,
                lifecycle: ArtifactLifecycle::Ephemeral,
                created_at: stale_time,
                updated_at: stale_time,
                agent_id: "coder".to_string(),
                task_id: None,
                run_id: None,
                trace_id: None,
                session_id: Some("session-a".to_string()),
                thread_id: Some("thread-a".to_string()),
                tool_name: None,
                media_type: Some("text/plain".to_string()),
                virtual_path: Some("workspace/tmp.txt".to_string()),
                source_kind: "task_state".to_string(),
                metadata: HashMap::new(),
            })
            .await
            .expect("save ephemeral old");
        manager
            .save(ArtifactRecord {
                artifact_id: "session-old".to_string(),
                kind: "report".to_string(),
                uri: "artifacts://thread-a/report.md".to_string(),
                scope: ArtifactScope::Artifacts,
                lifecycle: ArtifactLifecycle::Session,
                created_at: stale_time,
                updated_at: stale_time,
                agent_id: "coder".to_string(),
                task_id: None,
                run_id: None,
                trace_id: None,
                session_id: Some("session-a".to_string()),
                thread_id: Some("thread-a".to_string()),
                tool_name: None,
                media_type: Some("text/markdown".to_string()),
                virtual_path: Some("artifacts/report.md".to_string()),
                source_kind: "run_trace".to_string(),
                metadata: HashMap::new(),
            })
            .await
            .expect("save session old");
        manager
            .save(ArtifactRecord {
                artifact_id: "durable-old".to_string(),
                kind: "upload".to_string(),
                uri: "file:///tmp/uploads/source.pdf".to_string(),
                scope: ArtifactScope::Uploads,
                lifecycle: ArtifactLifecycle::Durable,
                created_at: stale_time,
                updated_at: stale_time,
                agent_id: "coder".to_string(),
                task_id: None,
                run_id: None,
                trace_id: None,
                session_id: Some("session-a".to_string()),
                thread_id: Some("thread-a".to_string()),
                tool_name: None,
                media_type: Some("application/pdf".to_string()),
                virtual_path: Some("uploads/source.pdf".to_string()),
                source_kind: "task_state".to_string(),
                metadata: HashMap::new(),
            })
            .await
            .expect("save durable old");
        manager
            .save(ArtifactRecord {
                artifact_id: "ephemeral-fresh".to_string(),
                kind: "tmp".to_string(),
                uri: "file:///tmp/workspace/fresh.txt".to_string(),
                scope: ArtifactScope::Workspace,
                lifecycle: ArtifactLifecycle::Ephemeral,
                created_at: fresh_time,
                updated_at: fresh_time,
                agent_id: "coder".to_string(),
                task_id: None,
                run_id: None,
                trace_id: None,
                session_id: Some("session-a".to_string()),
                thread_id: Some("thread-a".to_string()),
                tool_name: None,
                media_type: Some("text/plain".to_string()),
                virtual_path: Some("workspace/fresh.txt".to_string()),
                source_kind: "task_state".to_string(),
                metadata: HashMap::new(),
            })
            .await
            .expect("save ephemeral fresh");

        let report = manager
            .cleanup(&ArtifactCleanupPolicy {
                ephemeral_max_age_hours: Some(24),
                session_max_age_hours: Some(24),
                max_delete: Some(10),
                ..ArtifactCleanupPolicy::default()
            })
            .await
            .expect("cleanup");
        assert_eq!(report.deleted, 2);
        assert_eq!(report.skipped_durable_without_policy, 1);
        assert!(manager.load("ephemeral-old").await.expect("load").is_none());
        assert!(manager.load("session-old").await.expect("load").is_none());
        assert!(manager.load("durable-old").await.expect("load").is_some());
        assert!(manager
            .load("ephemeral-fresh")
            .await
            .expect("load")
            .is_some());
    }

    #[tokio::test]
    async fn artifact_cleanup_dry_run_does_not_delete_records() {
        let temp = TempDir::new().expect("temp dir");
        let db = Arc::new(Database::create(temp.path().join("state.redb")).expect("db"));
        {
            let write_txn = db.begin_write().expect("write txn");
            {
                let _ = write_txn.open_table(ARTIFACTS_TABLE).expect("table");
            }
            write_txn.commit().expect("commit");
        }
        let manager = ArtifactManager::new(db);
        let stale_time = Utc::now() - Duration::hours(72);
        manager
            .save(ArtifactRecord {
                artifact_id: "ephemeral-dry-run".to_string(),
                kind: "tmp".to_string(),
                uri: "file:///tmp/workspace/tmp.txt".to_string(),
                scope: ArtifactScope::Workspace,
                lifecycle: ArtifactLifecycle::Ephemeral,
                created_at: stale_time,
                updated_at: stale_time,
                agent_id: "coder".to_string(),
                task_id: None,
                run_id: None,
                trace_id: None,
                session_id: None,
                thread_id: None,
                tool_name: None,
                media_type: Some("text/plain".to_string()),
                virtual_path: Some("workspace/tmp.txt".to_string()),
                source_kind: "task_state".to_string(),
                metadata: HashMap::new(),
            })
            .await
            .expect("save");

        let report = manager
            .cleanup(&ArtifactCleanupPolicy {
                dry_run: true,
                ephemeral_max_age_hours: Some(24),
                ..ArtifactCleanupPolicy::default()
            })
            .await
            .expect("cleanup");
        assert_eq!(report.deleted, 1);
        assert_eq!(
            report.deleted_artifact_ids,
            vec!["ephemeral-dry-run".to_string()]
        );
        assert!(manager
            .load("ephemeral-dry-run")
            .await
            .expect("load")
            .is_some());
    }

    #[tokio::test]
    async fn artifact_cleanup_can_prune_missing_local_orphan_records() {
        let temp = TempDir::new().expect("temp dir");
        let db = Arc::new(Database::create(temp.path().join("state.redb")).expect("db"));
        {
            let write_txn = db.begin_write().expect("write txn");
            {
                let _ = write_txn.open_table(ARTIFACTS_TABLE).expect("table");
            }
            write_txn.commit().expect("commit");
        }
        let manager = ArtifactManager::new(db);
        let stale_time = Utc::now() - Duration::hours(72);
        let missing_path = temp.path().join("missing.txt");
        manager
            .save(ArtifactRecord {
                artifact_id: "missing-orphan".to_string(),
                kind: "tmp".to_string(),
                uri: format!("file://{}", missing_path.display()),
                scope: ArtifactScope::Workspace,
                lifecycle: ArtifactLifecycle::Ephemeral,
                created_at: stale_time,
                updated_at: stale_time,
                agent_id: "coder".to_string(),
                task_id: None,
                run_id: None,
                trace_id: None,
                session_id: None,
                thread_id: None,
                tool_name: None,
                media_type: Some("text/plain".to_string()),
                virtual_path: Some("workspace/missing.txt".to_string()),
                source_kind: "task_state".to_string(),
                metadata: HashMap::new(),
            })
            .await
            .expect("save");

        let report = manager
            .cleanup(&ArtifactCleanupPolicy {
                orphan_only: true,
                prune_missing_local_files: true,
                ..ArtifactCleanupPolicy::default()
            })
            .await
            .expect("cleanup");
        assert_eq!(report.deleted, 1);
        assert_eq!(report.orphan_matched, 1);
        assert_eq!(report.missing_local_file_matched, 1);
        assert!(manager
            .load("missing-orphan")
            .await
            .expect("load")
            .is_none());
    }

    #[tokio::test]
    async fn artifact_cleanup_can_delete_local_files() {
        let temp = TempDir::new().expect("temp dir");
        let db = Arc::new(Database::create(temp.path().join("state.redb")).expect("db"));
        {
            let write_txn = db.begin_write().expect("write txn");
            {
                let _ = write_txn.open_table(ARTIFACTS_TABLE).expect("table");
            }
            write_txn.commit().expect("commit");
        }
        let manager = ArtifactManager::new(db);
        let stale_time = Utc::now() - Duration::hours(72);
        let file_path = temp.path().join("artifact-output.txt");
        tokio::fs::write(&file_path, "artifact payload")
            .await
            .expect("write payload");
        manager
            .save(ArtifactRecord {
                artifact_id: "local-file".to_string(),
                kind: "output".to_string(),
                uri: format!("file://{}", file_path.display()),
                scope: ArtifactScope::Outputs,
                lifecycle: ArtifactLifecycle::Ephemeral,
                created_at: stale_time,
                updated_at: stale_time,
                agent_id: "coder".to_string(),
                task_id: None,
                run_id: None,
                trace_id: None,
                session_id: Some("session-a".to_string()),
                thread_id: Some("thread-a".to_string()),
                tool_name: None,
                media_type: Some("text/plain".to_string()),
                virtual_path: Some("outputs/artifact-output.txt".to_string()),
                source_kind: "run_trace".to_string(),
                metadata: HashMap::new(),
            })
            .await
            .expect("save");

        let report = manager
            .cleanup(&ArtifactCleanupPolicy {
                delete_local_files: true,
                ephemeral_max_age_hours: Some(24),
                ..ArtifactCleanupPolicy::default()
            })
            .await
            .expect("cleanup");
        assert_eq!(report.deleted, 1);
        assert_eq!(report.deleted_local_files, 1);
        assert!(!file_path.exists());
    }
}
