use anyhow::Result;
use chrono::Utc;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{error, info, warn};

/// Manages shadow backups and rollback operations for destructive tool calls.
pub struct ShadowBak {
    backup_root: PathBuf,
}

impl ShadowBak {
    pub fn new() -> Self {
        let base_dir = std::env::var("BENSHU_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::current_dir().unwrap_or_default());
        Self::new_with_base_dir(base_dir)
    }

    pub fn new_with_base_dir(base_dir: PathBuf) -> Self {
        let backup_root = base_dir.join("data").join("backups");
        Self { backup_root }
    }

    /// Create a shadow backup of a file before modification.
    pub async fn backup(&self, original_path: &Path) -> Result<Option<PathBuf>> {
        if !original_path.exists() || !original_path.is_file() {
            return Ok(None);
        }

        if !self.backup_root.exists() {
            fs::create_dir_all(&self.backup_root).await?;
        }

        // Generate a unique backup name: <filename>_<timestamp>_<id>.bak
        let file_name = original_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
        let hash = format!(
            "{:x}",
            md5::compute(original_path.to_string_lossy().as_bytes())
        );
        let backup_name = format!("{}_{}_{}.bak", file_name, timestamp, &hash[..8]);
        let backup_path = self.backup_root.join(backup_name);

        info!(
            "Creating shadow backup for {:?} -> {:?}",
            original_path, backup_path
        );
        fs::copy(original_path, &backup_path).await?;

        Ok(Some(backup_path))
    }

    /// Restore a file from a backup.
    pub async fn rollback(&self, original_path: &Path, backup_path: &Path) -> Result<()> {
        if !backup_path.exists() {
            anyhow::bail!("Backup file not found: {:?}", backup_path);
        }

        info!(
            "Rolling back file: {:?} <- {:?}",
            original_path, backup_path
        );
        fs::copy(backup_path, original_path).await?;
        Ok(())
    }

    /// Clean up old backups (older than 7 days)
    pub async fn cleanup(&self, days: i64) -> Result<()> {
        if !self.backup_root.exists() {
            return Ok(());
        }

        let mut entries = fs::read_dir(&self.backup_root).await?;
        let now = Utc::now();

        while let Some(entry) = entries.next_entry().await? {
            let metadata = entry.metadata().await?;
            if let Ok(modified) = metadata.modified() {
                let modified: chrono::DateTime<Utc> = modified.into();
                if now.signed_duration_since(modified).num_days() > days {
                    let _ = fs::remove_file(entry.path()).await;
                }
            }
        }
        Ok(())
    }
}

impl Default for ShadowBak {
    fn default() -> Self {
        Self::new()
    }
}
