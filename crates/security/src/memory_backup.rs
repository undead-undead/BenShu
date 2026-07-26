use crate::encryption::FactEncryptor;
use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub const MEMORY_BACKUP_CONTRACT_VERSION: &str = "1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryBackupFileEntry {
    pub label: String,
    pub relative_path: String,
    pub payload_path: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryRestorePointManifest {
    pub backup_id: String,
    pub product: String,
    pub contract_version: String,
    pub created_at: DateTime<Utc>,
    pub storage_root_hint: String,
    pub encryption_key_fingerprint: String,
    pub file_count: usize,
    pub total_bytes: u64,
    pub files: Vec<MemoryBackupFileEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryRestoreReceipt {
    pub receipt_id: String,
    pub backup_id: String,
    pub restored_at: DateTime<Utc>,
    pub contract_version: String,
    pub encryption_key_fingerprint: String,
    pub restored_files: usize,
    pub restored_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryRestoreDryRunReport {
    pub backup_id: String,
    pub checked_at: DateTime<Utc>,
    pub contract_version: String,
    pub encryption_key_fingerprint: String,
    pub valid: bool,
    pub file_count: usize,
    pub total_bytes: u64,
    pub restorable_files: usize,
    pub missing_payloads: Vec<String>,
    pub integrity_mismatches: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryRestoreDeleteReport {
    pub backup_id: String,
    pub deleted_at: DateTime<Utc>,
    pub dry_run: bool,
    pub file_count: usize,
    pub total_bytes: u64,
    pub receipt_count: usize,
}

pub struct SealedMemoryBackupManager {
    storage_root: PathBuf,
    restore_root: PathBuf,
    max_restore_points: usize,
}

impl SealedMemoryBackupManager {
    pub fn new(storage_root: PathBuf, max_restore_points: usize) -> Self {
        let restore_root = storage_root.join("data").join("memory_restore_points");
        Self {
            storage_root,
            restore_root,
            max_restore_points,
        }
    }

    pub async fn create_restore_point(
        &self,
        encryptor: &FactEncryptor,
    ) -> Result<MemoryRestorePointManifest> {
        let targets = self.discover_targets()?;
        if targets.is_empty() {
            return Err(anyhow!("No durable memory files found to back up"));
        }

        let backup_id = uuid::Uuid::new_v4().to_string();
        let backup_dir = self.restore_root.join(&backup_id);
        let payload_root = backup_dir.join("payloads");
        tokio::fs::create_dir_all(&payload_root).await?;

        let mut entries = Vec::new();
        let mut total_bytes = 0u64;

        for (label, absolute_path, relative_path) in targets {
            let plaintext = tokio::fs::read(&absolute_path).await?;
            let sealed = encryptor.encrypt_bytes(&plaintext)?;
            let payload_path = payload_root.join(format!("{relative_path}.sealed"));
            if let Some(parent) = payload_path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::write(&payload_path, sealed).await?;

            let size_bytes = plaintext.len() as u64;
            total_bytes += size_bytes;
            entries.push(MemoryBackupFileEntry {
                label,
                relative_path: relative_path.clone(),
                payload_path: payload_path
                    .strip_prefix(&backup_dir)
                    .unwrap_or(&payload_path)
                    .to_string_lossy()
                    .replace('\\', "/"),
                size_bytes,
                sha256: hex::encode(Sha256::digest(&plaintext)),
            });
        }

        entries.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));

        let manifest = MemoryRestorePointManifest {
            backup_id: backup_id.clone(),
            product: "BenShu".to_string(),
            contract_version: MEMORY_BACKUP_CONTRACT_VERSION.to_string(),
            created_at: Utc::now(),
            storage_root_hint: self.storage_root.display().to_string(),
            encryption_key_fingerprint: encryptor.fingerprint(),
            file_count: entries.len(),
            total_bytes,
            files: entries,
        };

        let manifest_path = backup_dir.join("manifest.json");
        tokio::fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest)?).await?;
        self.prune_old_restore_points().await?;
        Ok(manifest)
    }

    pub async fn inspect_restore_point(
        &self,
        backup_id: &str,
    ) -> Result<MemoryRestorePointManifest> {
        let manifest_path = self.restore_root.join(backup_id).join("manifest.json");
        let data = tokio::fs::read(&manifest_path).await?;
        Ok(serde_json::from_slice(&data)?)
    }

    pub async fn list_restore_points(&self) -> Result<Vec<MemoryRestorePointManifest>> {
        if !self.restore_root.exists() {
            return Ok(Vec::new());
        }

        let mut manifests = Vec::new();
        let mut entries = tokio::fs::read_dir(&self.restore_root).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let manifest_path = path.join("manifest.json");
            if !manifest_path.is_file() {
                continue;
            }

            let Ok(data) = tokio::fs::read(&manifest_path).await else {
                continue;
            };
            let Ok(manifest) = serde_json::from_slice::<MemoryRestorePointManifest>(&data) else {
                continue;
            };
            manifests.push(manifest);
        }

        manifests.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(manifests)
    }

    async fn prune_old_restore_points(&self) -> Result<()> {
        if self.max_restore_points == 0 {
            return Ok(());
        }

        let manifests = self.list_restore_points().await?;
        for manifest in manifests.iter().skip(self.max_restore_points) {
            let backup_dir = self.restore_root.join(&manifest.backup_id);
            if backup_dir.exists() {
                tokio::fs::remove_dir_all(backup_dir).await?;
            }
        }

        Ok(())
    }

    pub async fn restore_restore_point(
        &self,
        backup_id: &str,
        encryptor: &FactEncryptor,
    ) -> Result<MemoryRestoreReceipt> {
        let manifest = self.inspect_restore_point(backup_id).await?;
        if manifest.contract_version != MEMORY_BACKUP_CONTRACT_VERSION {
            return Err(anyhow!(
                "Unsupported memory backup contract version: {}",
                manifest.contract_version
            ));
        }
        if manifest.encryption_key_fingerprint != encryptor.fingerprint() {
            return Err(anyhow!(
                "Restore key fingerprint mismatch for backup {}",
                manifest.backup_id
            ));
        }

        let backup_dir = self.restore_root.join(backup_id);
        let mut restored_bytes = 0u64;

        for entry in &manifest.files {
            let payload = tokio::fs::read(backup_dir.join(&entry.payload_path)).await?;
            let plaintext = encryptor.decrypt_bytes(&payload)?;
            let plaintext_hash = hex::encode(Sha256::digest(&plaintext));
            if plaintext_hash != entry.sha256 {
                return Err(anyhow!(
                    "Payload integrity mismatch for {}",
                    entry.relative_path
                ));
            }

            let target_path = self.storage_root.join(&entry.relative_path);
            if let Some(parent) = target_path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::write(&target_path, &plaintext).await?;
            restored_bytes += plaintext.len() as u64;
        }

        let receipt = MemoryRestoreReceipt {
            receipt_id: uuid::Uuid::new_v4().to_string(),
            backup_id: manifest.backup_id.clone(),
            restored_at: Utc::now(),
            contract_version: MEMORY_BACKUP_CONTRACT_VERSION.to_string(),
            encryption_key_fingerprint: manifest.encryption_key_fingerprint.clone(),
            restored_files: manifest.file_count,
            restored_bytes,
        };

        let receipts_dir = backup_dir.join("receipts");
        tokio::fs::create_dir_all(&receipts_dir).await?;
        tokio::fs::write(
            receipts_dir.join(format!("{}.json", receipt.receipt_id)),
            serde_json::to_vec_pretty(&receipt)?,
        )
        .await?;

        Ok(receipt)
    }

    pub async fn dry_run_restore_point(
        &self,
        backup_id: &str,
        encryptor: &FactEncryptor,
    ) -> Result<MemoryRestoreDryRunReport> {
        let manifest = self.inspect_restore_point(backup_id).await?;
        if manifest.contract_version != MEMORY_BACKUP_CONTRACT_VERSION {
            return Err(anyhow!(
                "Unsupported memory backup contract version: {}",
                manifest.contract_version
            ));
        }
        if manifest.encryption_key_fingerprint != encryptor.fingerprint() {
            return Err(anyhow!(
                "Restore key fingerprint mismatch for backup {}",
                manifest.backup_id
            ));
        }

        let backup_dir = self.restore_root.join(backup_id);
        let mut restorable_files = 0usize;
        let mut missing_payloads = Vec::new();
        let mut integrity_mismatches = Vec::new();

        for entry in &manifest.files {
            let payload_path = backup_dir.join(&entry.payload_path);
            let Ok(payload) = tokio::fs::read(&payload_path).await else {
                missing_payloads.push(entry.relative_path.clone());
                continue;
            };
            let Ok(plaintext) = encryptor.decrypt_bytes(&payload) else {
                integrity_mismatches.push(entry.relative_path.clone());
                continue;
            };
            let plaintext_hash = hex::encode(Sha256::digest(&plaintext));
            if plaintext_hash != entry.sha256 {
                integrity_mismatches.push(entry.relative_path.clone());
                continue;
            }
            restorable_files += 1;
        }

        Ok(MemoryRestoreDryRunReport {
            backup_id: manifest.backup_id,
            checked_at: Utc::now(),
            contract_version: MEMORY_BACKUP_CONTRACT_VERSION.to_string(),
            encryption_key_fingerprint: manifest.encryption_key_fingerprint,
            valid: missing_payloads.is_empty() && integrity_mismatches.is_empty(),
            file_count: manifest.file_count,
            total_bytes: manifest.total_bytes,
            restorable_files,
            missing_payloads,
            integrity_mismatches,
        })
    }

    pub async fn inspect_restore_receipt(
        &self,
        backup_id: &str,
        receipt_id: &str,
    ) -> Result<MemoryRestoreReceipt> {
        let receipt_path = self
            .restore_root
            .join(backup_id)
            .join("receipts")
            .join(format!("{receipt_id}.json"));
        let data = tokio::fs::read(receipt_path).await?;
        Ok(serde_json::from_slice(&data)?)
    }

    pub async fn list_restore_receipts(
        &self,
        backup_id: &str,
    ) -> Result<Vec<MemoryRestoreReceipt>> {
        let receipts_dir = self.restore_root.join(backup_id).join("receipts");
        if !receipts_dir.exists() {
            return Ok(Vec::new());
        }

        let mut receipts = Vec::new();
        let mut entries = tokio::fs::read_dir(receipts_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Ok(data) = tokio::fs::read(&path).await else {
                continue;
            };
            let Ok(receipt) = serde_json::from_slice::<MemoryRestoreReceipt>(&data) else {
                continue;
            };
            receipts.push(receipt);
        }

        receipts.sort_by(|a, b| b.restored_at.cmp(&a.restored_at));
        Ok(receipts)
    }

    pub async fn delete_restore_point(
        &self,
        backup_id: &str,
        dry_run: bool,
    ) -> Result<MemoryRestoreDeleteReport> {
        let manifest = self.inspect_restore_point(backup_id).await?;
        let receipt_count = self.list_restore_receipts(backup_id).await?.len();
        let report = MemoryRestoreDeleteReport {
            backup_id: manifest.backup_id.clone(),
            deleted_at: Utc::now(),
            dry_run,
            file_count: manifest.file_count,
            total_bytes: manifest.total_bytes,
            receipt_count,
        };

        if !dry_run {
            let backup_dir = self.restore_root.join(backup_id);
            tokio::fs::remove_dir_all(backup_dir).await?;
        }

        Ok(report)
    }

    fn discover_targets(&self) -> Result<Vec<(String, PathBuf, String)>> {
        let mut targets = Vec::new();
        self.push_file_target(
            &mut targets,
            "stm_hot",
            self.storage_root.join("short_term_memory.redb"),
        )?;
        self.push_file_target(
            &mut targets,
            "audit_log",
            self.storage_root.join("audit.redb"),
        )?;
        self.push_file_target(
            &mut targets,
            "system_experience",
            self.storage_root.join("experience.redb"),
        )?;

        let search_root = self.storage_root.join("search");
        if search_root.exists() {
            self.collect_directory_targets("engram_search", &search_root, &mut targets)?;
        }

        Ok(targets)
    }

    fn push_file_target(
        &self,
        targets: &mut Vec<(String, PathBuf, String)>,
        label: &str,
        absolute_path: PathBuf,
    ) -> Result<()> {
        if absolute_path.is_file() {
            let relative = absolute_path
                .strip_prefix(&self.storage_root)
                .unwrap_or(&absolute_path)
                .to_string_lossy()
                .replace('\\', "/");
            targets.push((label.to_string(), absolute_path, relative));
        }
        Ok(())
    }

    fn collect_directory_targets(
        &self,
        label_prefix: &str,
        directory: &Path,
        targets: &mut Vec<(String, PathBuf, String)>,
    ) -> Result<()> {
        if !directory.exists() {
            return Ok(());
        }

        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                self.collect_directory_targets(label_prefix, &path, targets)?;
                continue;
            }
            if !path.is_file() {
                continue;
            }

            let relative = path
                .strip_prefix(&self.storage_root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let label = format!("{label_prefix}:{}", relative);
            targets.push((label, path, relative));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SecurityConfig, SecurityManager, Vault};
    use std::sync::Arc;

    #[tokio::test]
    async fn sealed_memory_backup_round_trips_durable_files() {
        let temp = tempfile::tempdir().expect("tempdir");
        let storage_root = temp.path().join("agentos");
        std::fs::create_dir_all(storage_root.join("search")).expect("search dir");
        std::fs::write(storage_root.join("short_term_memory.redb"), b"stm-v1").expect("stm");
        std::fs::write(storage_root.join("audit.redb"), b"audit-v1").expect("audit");
        std::fs::write(storage_root.join("experience.redb"), b"experience-v1").expect("experience");
        std::fs::write(storage_root.join("search").join("engram.db"), b"engram-v1")
            .expect("engram");
        std::fs::write(
            storage_root.join("search").join("index.snapshot"),
            b"snapshot-v1",
        )
        .expect("snapshot");

        let vault = Arc::new(Vault::open(storage_root.join("vault.redb")).expect("vault"));
        let security = SecurityManager::new_with_storage_root(
            SecurityConfig::default(),
            Some(vault),
            storage_root.clone(),
        );

        let manifest = security
            .create_memory_restore_point()
            .await
            .expect("create restore point");
        assert_eq!(manifest.contract_version, MEMORY_BACKUP_CONTRACT_VERSION);
        assert!(manifest.file_count >= 4);

        std::fs::write(storage_root.join("short_term_memory.redb"), b"stm-v2").expect("mutate");
        std::fs::write(storage_root.join("experience.redb"), b"experience-v2")
            .expect("mutate experience");
        std::fs::write(storage_root.join("search").join("engram.db"), b"engram-v2")
            .expect("mutate engram");

        let inspected = security
            .inspect_memory_restore_point(&manifest.backup_id)
            .await
            .expect("inspect restore point");
        assert_eq!(inspected.backup_id, manifest.backup_id);

        let listed = security
            .list_memory_restore_points()
            .await
            .expect("list restore points");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].backup_id, manifest.backup_id);

        let receipt = security
            .restore_memory_restore_point(&manifest.backup_id)
            .await
            .expect("restore point");
        assert_eq!(receipt.backup_id, manifest.backup_id);
        assert_eq!(
            std::fs::read(storage_root.join("short_term_memory.redb")).expect("read stm"),
            b"stm-v1"
        );
        assert_eq!(
            std::fs::read(storage_root.join("experience.redb")).expect("read experience"),
            b"experience-v1"
        );
        assert_eq!(
            std::fs::read(storage_root.join("search").join("engram.db")).expect("read engram"),
            b"engram-v1"
        );
    }

    #[tokio::test]
    async fn sealed_memory_backup_dry_run_and_receipt_listing_work() {
        let temp = tempfile::tempdir().expect("tempdir");
        let storage_root = temp.path().join("agentos");
        std::fs::create_dir_all(storage_root.join("search")).expect("search dir");
        std::fs::write(storage_root.join("short_term_memory.redb"), b"stm-v1").expect("stm");
        std::fs::write(storage_root.join("audit.redb"), b"audit-v1").expect("audit");

        let vault = Arc::new(Vault::open(storage_root.join("vault.redb")).expect("vault"));
        let security = SecurityManager::new_with_storage_root(
            SecurityConfig::default(),
            Some(vault),
            storage_root.clone(),
        );

        let manifest = security
            .create_memory_restore_point()
            .await
            .expect("create restore point");
        let dry_run = security
            .dry_run_memory_restore_point(&manifest.backup_id)
            .await
            .expect("dry run restore point");
        assert!(dry_run.valid);
        assert_eq!(dry_run.file_count, manifest.file_count);

        let receipt = security
            .restore_memory_restore_point(&manifest.backup_id)
            .await
            .expect("restore point");

        let receipts = security
            .list_memory_restore_receipts(&manifest.backup_id)
            .await
            .expect("list restore receipts");
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0].receipt_id, receipt.receipt_id);

        let inspected = security
            .inspect_memory_restore_receipt(&manifest.backup_id, &receipt.receipt_id)
            .await
            .expect("inspect restore receipt");
        assert_eq!(inspected.receipt_id, receipt.receipt_id);
    }

    #[tokio::test]
    async fn sealed_memory_backup_delete_report_and_removal_work() {
        let temp = tempfile::tempdir().expect("tempdir");
        let storage_root = temp.path().join("agentos");
        std::fs::create_dir_all(storage_root.join("search")).expect("search dir");
        std::fs::write(storage_root.join("short_term_memory.redb"), b"stm-v1").expect("stm");
        std::fs::write(storage_root.join("audit.redb"), b"audit-v1").expect("audit");

        let vault = Arc::new(Vault::open(storage_root.join("vault.redb")).expect("vault"));
        let security = SecurityManager::new_with_storage_root(
            SecurityConfig::default(),
            Some(vault),
            storage_root.clone(),
        );

        let manifest = security
            .create_memory_restore_point()
            .await
            .expect("create restore point");
        let dry_run_report = security
            .delete_memory_restore_point(&manifest.backup_id, true)
            .await
            .expect("dry-run delete");
        assert!(dry_run_report.dry_run);
        assert_eq!(dry_run_report.backup_id, manifest.backup_id);
        assert_eq!(
            security
                .inspect_memory_restore_point(&manifest.backup_id)
                .await
                .expect("manifest still exists after dry-run")
                .backup_id,
            manifest.backup_id
        );

        let delete_report = security
            .delete_memory_restore_point(&manifest.backup_id, false)
            .await
            .expect("delete restore point");
        assert!(!delete_report.dry_run);
        assert_eq!(delete_report.backup_id, manifest.backup_id);
        assert!(
            security
                .inspect_memory_restore_point(&manifest.backup_id)
                .await
                .is_err(),
            "manifest should be gone after delete"
        );
    }

    #[tokio::test]
    async fn sealed_memory_backup_prunes_restore_points_by_retention_limit() {
        let temp = tempfile::tempdir().expect("tempdir");
        let storage_root = temp.path().join("agentos");
        std::fs::create_dir_all(storage_root.join("search")).expect("search dir");
        std::fs::write(storage_root.join("short_term_memory.redb"), b"stm-v1").expect("stm");
        std::fs::write(storage_root.join("audit.redb"), b"audit-v1").expect("audit");

        let vault = Arc::new(Vault::open(storage_root.join("vault.redb")).expect("vault"));
        let security = SecurityManager::new_with_storage_root(
            SecurityConfig {
                max_memory_restore_points: 2,
                ..SecurityConfig::default()
            },
            Some(vault),
            storage_root.clone(),
        );

        let first = security
            .create_memory_restore_point()
            .await
            .expect("create first restore point");
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        let second = security
            .create_memory_restore_point()
            .await
            .expect("create second restore point");
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        let third = security
            .create_memory_restore_point()
            .await
            .expect("create third restore point");

        let listed = security
            .list_memory_restore_points()
            .await
            .expect("list retained restore points");
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].backup_id, third.backup_id);
        assert_eq!(listed[1].backup_id, second.backup_id);
        assert!(
            security
                .inspect_memory_restore_point(&first.backup_id)
                .await
                .is_err(),
            "oldest restore point should be pruned"
        );
    }
}
