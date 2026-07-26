use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, Weak};

use fs2::FileExt;
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex, OwnedMutexGuard};

type ProjectMutex = Mutex<()>;

pub(super) struct ProjectOperationGuard {
    _local: OwnedMutexGuard<()>,
    file: File,
}

impl Drop for ProjectOperationGuard {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

fn registry() -> &'static parking_lot::Mutex<HashMap<PathBuf, Weak<ProjectMutex>>> {
    static REGISTRY: OnceLock<parking_lot::Mutex<HashMap<PathBuf, Weak<ProjectMutex>>>> =
        OnceLock::new();
    REGISTRY.get_or_init(|| parking_lot::Mutex::new(HashMap::new()))
}

pub(super) async fn acquire_project_lock(
    workspace: &Path,
    key: PathBuf,
) -> anyhow::Result<ProjectOperationGuard> {
    let lock = {
        let mut registry = registry().lock();
        registry.retain(|_, lock| lock.strong_count() > 0);
        if let Some(lock) = registry.get(&key).and_then(Weak::upgrade) {
            lock
        } else {
            let lock = Arc::new(ProjectMutex::new(()));
            registry.insert(key.clone(), Arc::downgrade(&lock));
            lock
        }
    };
    let local = lock.lock_owned().await;
    let lock_root = workspace.join(".runtime-locks").join("writing-studio");
    let lock_name = hex::encode(Sha256::digest(key.to_string_lossy().as_bytes()));
    let lock_path = lock_root.join(format!("{lock_name}.lock"));
    let file = tokio::task::spawn_blocking(move || -> anyhow::Result<File> {
        std::fs::create_dir_all(&lock_root)?;
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(lock_path)?;
        file.lock_exclusive()?;
        Ok(file)
    })
    .await
    .map_err(|error| anyhow::anyhow!("project lock worker failed: {error}"))??;
    Ok(ProjectOperationGuard {
        _local: local,
        file,
    })
}
