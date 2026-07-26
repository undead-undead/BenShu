use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, Weak};

use fs2::FileExt;
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex, OwnedMutexGuard};

use super::{atomic_write_file, NovelStudioArgs, NovelStudioTool};

type ProjectMutex = Mutex<()>;
const PROJECT_LOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(30);
const PROJECT_HEARTBEAT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(10);

pub(crate) struct ProjectOperationGuard {
    _local: OwnedMutexGuard<()>,
    file: File,
    heartbeat_path: PathBuf,
    heartbeat_stop: Option<tokio::sync::oneshot::Sender<()>>,
    heartbeat_task: Option<tokio::task::JoinHandle<()>>,
}

impl Drop for ProjectOperationGuard {
    fn drop(&mut self) {
        if let Some(stop) = self.heartbeat_stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.heartbeat_task.take() {
            task.abort();
        }
        let _ = std::fs::remove_file(&self.heartbeat_path);
        let _ = FileExt::unlock(&self.file);
    }
}

fn project_lock_registry() -> &'static parking_lot::Mutex<HashMap<PathBuf, Weak<ProjectMutex>>> {
    static REGISTRY: OnceLock<parking_lot::Mutex<HashMap<PathBuf, Weak<ProjectMutex>>>> =
        OnceLock::new();
    REGISTRY.get_or_init(|| parking_lot::Mutex::new(HashMap::new()))
}

impl NovelStudioTool {
    pub(crate) async fn lock_project_workflow(
        &self,
        project_path: &str,
    ) -> anyhow::Result<ProjectOperationGuard> {
        let project = self
            .resolve_workspace_path(project_path.trim())?
            .join(".workflow-lease");
        self.acquire_project_lock(project).await
    }

    pub(super) async fn lock_project_operation(
        &self,
        args: &NovelStudioArgs,
    ) -> anyhow::Result<Option<ProjectOperationGuard>> {
        let Some(key) = self.project_operation_lock_key(args) else {
            return Ok(None);
        };
        self.acquire_project_lock(key).await.map(Some)
    }

    pub(super) async fn lock_project_creation(
        &self,
        output_root: PathBuf,
        title: &str,
    ) -> anyhow::Result<ProjectOperationGuard> {
        let normalized_title = super::normalize_project_lookup_key(title);
        self.acquire_project_lock(output_root.join(format!(
            ".create-{}",
            if normalized_title.is_empty() {
                "untitled"
            } else {
                normalized_title.as_str()
            }
        )))
        .await
    }

    async fn acquire_project_lock(&self, key: PathBuf) -> anyhow::Result<ProjectOperationGuard> {
        let lock = {
            let mut registry = project_lock_registry().lock();
            registry.retain(|_, lock| lock.strong_count() > 0);
            if let Some(lock) = registry.get(&key).and_then(Weak::upgrade) {
                lock
            } else {
                let lock = Arc::new(ProjectMutex::new(()));
                registry.insert(key.clone(), Arc::downgrade(&lock));
                lock
            }
        };
        let local = tokio::time::timeout(PROJECT_LOCK_WAIT, lock.lock_owned())
            .await
            .map_err(|_| anyhow::anyhow!("project_busy: local project lock wait exceeded 30s"))?;
        let lock_root = self.workspace.join(".runtime-locks").join("novel-studio");
        let lock_name = hex::encode(Sha256::digest(key.to_string_lossy().as_bytes()));
        let lock_path = lock_root.join(format!("{lock_name}.lock"));
        let heartbeat_path = lock_root.join(format!("{lock_name}.heartbeat.json"));
        let file = tokio::task::spawn_blocking(move || -> anyhow::Result<File> {
            std::fs::create_dir_all(&lock_root)?;
            let file = OpenOptions::new()
                .create(true)
                .read(true)
                .write(true)
                .open(&lock_path)?;
            let started = std::time::Instant::now();
            loop {
                match file.try_lock_exclusive() {
                    Ok(()) => return Ok(file),
                    Err(error) if started.elapsed() < PROJECT_LOCK_WAIT => {
                        let _ = error;
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                    Err(error) => {
                        anyhow::bail!(
                            "project_busy: filesystem project lock wait exceeded 30s: {error}"
                        )
                    }
                }
            }
        })
        .await
        .map_err(|error| anyhow::anyhow!("project lock worker failed: {error}"))??;
        let recovered_stale_heartbeat = heartbeat_path.exists();
        write_project_heartbeat(&heartbeat_path, recovered_stale_heartbeat).await?;
        let (heartbeat_stop, mut heartbeat_stopped) = tokio::sync::oneshot::channel();
        let heartbeat_task_path = heartbeat_path.clone();
        let heartbeat_task = tokio::spawn(async move {
            let mut interval = tokio::time::interval_at(
                tokio::time::Instant::now() + PROJECT_HEARTBEAT_INTERVAL,
                PROJECT_HEARTBEAT_INTERVAL,
            );
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        let _ = write_project_heartbeat(&heartbeat_task_path, false).await;
                    }
                    _ = &mut heartbeat_stopped => break,
                }
            }
        });
        Ok(ProjectOperationGuard {
            _local: local,
            file,
            heartbeat_path,
            heartbeat_stop: Some(heartbeat_stop),
            heartbeat_task: Some(heartbeat_task),
        })
    }

    fn project_operation_lock_key(&self, args: &NovelStudioArgs) -> Option<PathBuf> {
        if !args.draft_path.trim().is_empty()
            && matches!(
                args.action.as_str(),
                "update_draft" | "show_draft" | "approve_draft" | "discard_draft"
            )
        {
            return self.resolve_workspace_path(args.draft_path.trim()).ok();
        }
        if args.project_path.trim().is_empty() {
            return None;
        }
        self.require_project_path(args)
            .or_else(|_| self.resolve_workspace_path(args.project_path.trim()))
            .ok()
    }
}

async fn write_project_heartbeat(
    heartbeat_path: &std::path::Path,
    recovered_stale_heartbeat: bool,
) -> anyhow::Result<()> {
    atomic_write_file(
        heartbeat_path.to_path_buf(),
        serde_json::to_string_pretty(&serde_json::json!({
            "pid": std::process::id(),
            "updated_at": chrono::Utc::now().to_rfc3339(),
            "lease": "novel_studio_project_operation",
            "lease_authority": "filesystem_exclusive_lock",
            "recovered_stale_heartbeat": recovered_stale_heartbeat
        }))?,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stale_heartbeat_is_recovered_and_guard_drop_cannot_recreate_it() {
        let workspace = tempfile::tempdir().expect("workspace");
        let tool = NovelStudioTool::new(workspace.path().to_path_buf(), "tester");
        let project = workspace.path().join("novel");
        tokio::fs::create_dir_all(&project)
            .await
            .expect("project directory");

        let guard = tool
            .lock_project_workflow(project.to_string_lossy().as_ref())
            .await
            .expect("first lock");
        let heartbeat_path = guard.heartbeat_path.clone();
        drop(guard);
        tokio::task::yield_now().await;
        assert!(!heartbeat_path.exists());

        tokio::fs::write(&heartbeat_path, r#"{"pid":0,"stale":true}"#)
            .await
            .expect("stale heartbeat");
        let recovered = tool
            .lock_project_workflow(project.to_string_lossy().as_ref())
            .await
            .expect("recovered lock");
        let heartbeat = tokio::fs::read_to_string(&recovered.heartbeat_path)
            .await
            .expect("current heartbeat");
        assert!(heartbeat.contains(r#""recovered_stale_heartbeat": true"#));

        drop(recovered);
        tokio::task::yield_now().await;
        assert!(!heartbeat_path.exists());
    }
}
