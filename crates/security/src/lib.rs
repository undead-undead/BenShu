//! Security module for BenShu.
//!
//! Provides utilities for:
//! 1. Secret Leak Detection (redacting API keys from tool outputs).
//! 2. Prompt Injection Detection (detecting adversarial inputs).
//! 3. Policy Enforcement (optional future extension).

pub mod anti_tamper;
pub mod audit;
pub mod encryption;
pub mod injection;
pub mod internal_backup;
pub mod leaks;
pub mod memory_backup;
pub mod output_auditor;
pub mod pid_guard;
pub mod policy_guard;
pub mod sandbox;
pub mod shell_firewall;
pub mod skill_verifier;
pub mod vault;
pub mod vessel;

use anyhow::Result;
use rand::rngs::OsRng;
use rand::RngCore;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;
use tracing::{debug, error, info};

pub use encryption::FactEncryptor;
pub use injection::{InjectionDetector, SanitizedOutput};
pub use leaks::{LeakAction, LeakDetection, LeakDetector};
pub use memory_backup::{
    MemoryRestoreDeleteReport, MemoryRestoreDryRunReport, MemoryRestorePointManifest,
    MemoryRestoreReceipt, SealedMemoryBackupManager,
};
pub use policy_guard::PolicyGuard;
pub use shell_firewall::ShellFirewall;
pub use vault::Vault;
pub use vessel::VesselInspector;

/// Security configuration
#[derive(Debug, Clone, PartialEq)]
pub struct SecurityConfig {
    pub leak_detection_enabled: bool,
    pub injection_check_enabled: bool,
    pub max_memory_restore_points: usize,
    pub query_protection_enabled: bool,
    pub query_protection_burst_window_secs: u64,
    pub query_protection_burst_limit: usize,
    pub query_protection_high_cost_threshold: usize,
    pub query_protection_pause_secs: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemoryRestorePolicyBasis {
    pub backup_id: String,
    pub decision_kind: String,
    pub policy_basis: String,
    pub reasons: Vec<String>,
    pub warnings: Vec<String>,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            leak_detection_enabled: true,
            injection_check_enabled: true,
            max_memory_restore_points: 5,
            query_protection_enabled: true,
            query_protection_burst_window_secs: 8,
            query_protection_burst_limit: 3,
            query_protection_high_cost_threshold: 12,
            query_protection_pause_secs: 6,
        }
    }
}

use benshu_infra::traits::security::{
    AuditLogRecord as InfraAuditLogRecord, DynamicPolicy, LeakDetection as InfraLeakDetection,
    QueryProtectionAction, QueryProtectionDecision as InfraQueryProtectionDecision,
    QueryProtectionRequest as InfraQueryProtectionRequest, SanitizedOutput as InfraSanitizedOutput,
    SecurityHandler,
};

#[derive(Debug, Clone, Default)]
struct QueryProtectionState {
    recent_hits: Vec<Instant>,
    pause_until: Option<Instant>,
}

/// Central manager for security checks.
pub struct SecurityManager {
    config: SecurityConfig,
    storage_root: PathBuf,
    leak_detector: LeakDetector,
    injection_detector: InjectionDetector,
    policy_guard: Option<PolicyGuard>,
    pub backup: internal_backup::ShadowBak,
    pub audit: audit::AuditLogger,
    pub pid_guard: pid_guard::PidGuard,
    encryptor: Option<encryption::FactEncryptor>,
    vault: Option<std::sync::Arc<Vault>>,
    dynamic_policy: std::sync::Arc<tokio::sync::RwLock<DynamicPolicy>>,
    query_protection: Mutex<HashMap<String, QueryProtectionState>>,
}

impl SecurityManager {
    pub fn new(config: SecurityConfig, external_vault: Option<std::sync::Arc<Vault>>) -> Self {
        let storage_root = if let Ok(dir) = std::env::var("BENSHU_DATA_DIR") {
            PathBuf::from(dir)
        } else {
            let current = std::env::current_dir().unwrap_or_default();
            // Fallback to local data dir if not in a dev workspace root
            if current.join("data").exists() {
                current.join("data")
            } else {
                current
            }
        };

        Self::new_with_storage_root(config, external_vault, storage_root)
    }

    pub fn new_with_storage_root(
        config: SecurityConfig,
        external_vault: Option<std::sync::Arc<Vault>>,
        storage_root: PathBuf,
    ) -> Self {
        #[cfg(test)]
        let audit_path = storage_root.join(format!("audit_{}.redb", uuid::Uuid::new_v4()));
        #[cfg(not(test))]
        let audit_path = storage_root.join("audit.redb");

        let mut manager = Self {
            config,
            storage_root: storage_root.clone(),
            leak_detector: LeakDetector::new(),
            injection_detector: InjectionDetector::new(),
            policy_guard: Some(PolicyGuard::new(&storage_root)),
            backup: internal_backup::ShadowBak::new_with_base_dir(storage_root.clone()),
            audit: audit::AuditLogger::new(&audit_path).expect("Failed to open audit log"),
            pid_guard: pid_guard::PidGuard::new(),
            encryptor: None,
            vault: external_vault,
            dynamic_policy: std::sync::Arc::new(tokio::sync::RwLock::new(DynamicPolicy::default())),
            query_protection: Mutex::new(HashMap::new()),
        };

        // Initialize Master Cognitive Key (Consolidated Vault - security)
        let master_key_name = "BENSHU_BRAIN_MASTER_KEY";

        if let Some(ref vault) = manager.vault {
            let key_bytes = match vault.get(master_key_name) {
                Ok(Some(hex_key)) => hex::decode(hex_key).unwrap_or_else(|_| [0u8; 32].to_vec()),
                _ => {
                    // Generate and store new master key on first run
                    let mut new_key = [0u8; 32];
                    OsRng.fill_bytes(&mut new_key);
                    let hex_key = hex::encode(new_key);
                    let _ = vault.set(master_key_name, &hex_key);
                    new_key.to_vec()
                }
            };

            if key_bytes.len() == 32 {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&key_bytes);
                manager.encryptor = Some(encryption::FactEncryptor::new(arr));
            }
        } else {
            // BACKWARD COMPAT: Re-check if we need to open it locally (though kernel should pass it)
            let vault_path = storage_root.join("vault.redb");
            if let Ok(vault) = vault::Vault::open(&vault_path) {
                let vault = std::sync::Arc::new(vault);
                let key_bytes = match vault.get(master_key_name) {
                    Ok(Some(hex_key)) => {
                        hex::decode(hex_key).unwrap_or_else(|_| [0u8; 32].to_vec())
                    }
                    _ => {
                        let mut new_key = [0u8; 32];
                        OsRng.fill_bytes(&mut new_key);
                        let hex_key = hex::encode(new_key);
                        let _ = vault.set(master_key_name, &hex_key);
                        new_key.to_vec()
                    }
                };
                if key_bytes.len() == 32 {
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(&key_bytes);
                    manager.encryptor = Some(encryption::FactEncryptor::new(arr));
                }
                manager.vault = Some(vault);
            } else {
                error!(
                    "CRITICAL: Security Vault not provided and could not be opened at {:?}.",
                    vault_path
                );
            }
        }

        manager
    }

    fn memory_backup_encryptor(&self) -> Result<encryption::FactEncryptor> {
        let vault = self
            .vault
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Security Vault unavailable for memory backup"))?;
        let hex_key = vault
            .get("BENSHU_BRAIN_MASTER_KEY")?
            .ok_or_else(|| anyhow::anyhow!("Brain master key missing from vault"))?;
        let key_bytes =
            hex::decode(hex_key).map_err(|e| anyhow::anyhow!("Backup key decode failed: {}", e))?;
        if key_bytes.len() != 32 {
            return Err(anyhow::anyhow!(
                "Backup key length mismatch: expected 32 bytes, got {}",
                key_bytes.len()
            ));
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&key_bytes);
        Ok(encryption::FactEncryptor::new(key))
    }

    fn memory_backup_manager(&self) -> memory_backup::SealedMemoryBackupManager {
        memory_backup::SealedMemoryBackupManager::new(
            self.storage_root.clone(),
            self.config.max_memory_restore_points,
        )
    }

    fn log_memory_backup_event(&self, action: &str, success: bool, preview: &str) {
        let entry = audit::AuditEntry {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            session_key: None,
            tool_name: format!("memory_restore_point:{}", action),
            arguments: String::new(),
            success,
            output_preview: preview.to_string(),
            backup: None,
        };
        if let Err(err) = self.audit.log(entry) {
            tracing::error!("Failed to write memory backup audit log: {}", err);
        }
    }

    pub async fn create_memory_restore_point(&self) -> Result<MemoryRestorePointManifest> {
        let encryptor = self.memory_backup_encryptor()?;
        let manifest = self
            .memory_backup_manager()
            .create_restore_point(&encryptor)
            .await?;
        self.log_memory_backup_event(
            "create",
            true,
            &format!(
                "backup_id={} files={} bytes={}",
                manifest.backup_id, manifest.file_count, manifest.total_bytes
            ),
        );
        Ok(manifest)
    }

    pub async fn inspect_memory_restore_point(
        &self,
        backup_id: &str,
    ) -> Result<MemoryRestorePointManifest> {
        self.memory_backup_manager()
            .inspect_restore_point(backup_id)
            .await
    }

    pub async fn list_memory_restore_points(&self) -> Result<Vec<MemoryRestorePointManifest>> {
        self.memory_backup_manager().list_restore_points().await
    }

    pub async fn restore_memory_restore_point(
        &self,
        backup_id: &str,
    ) -> Result<MemoryRestoreReceipt> {
        let encryptor = self.memory_backup_encryptor()?;
        let receipt = self
            .memory_backup_manager()
            .restore_restore_point(backup_id, &encryptor)
            .await?;
        self.log_memory_backup_event(
            "restore",
            true,
            &format!(
                "backup_id={} receipt_id={} files={} bytes={}",
                receipt.backup_id,
                receipt.receipt_id,
                receipt.restored_files,
                receipt.restored_bytes
            ),
        );
        Ok(receipt)
    }

    pub async fn dry_run_memory_restore_point(
        &self,
        backup_id: &str,
    ) -> Result<MemoryRestoreDryRunReport> {
        let encryptor = self.memory_backup_encryptor()?;
        self.memory_backup_manager()
            .dry_run_restore_point(backup_id, &encryptor)
            .await
    }

    pub async fn inspect_memory_restore_receipt(
        &self,
        backup_id: &str,
        receipt_id: &str,
    ) -> Result<MemoryRestoreReceipt> {
        self.memory_backup_manager()
            .inspect_restore_receipt(backup_id, receipt_id)
            .await
    }

    pub async fn list_memory_restore_receipts(
        &self,
        backup_id: &str,
    ) -> Result<Vec<MemoryRestoreReceipt>> {
        self.memory_backup_manager()
            .list_restore_receipts(backup_id)
            .await
    }

    pub async fn delete_memory_restore_point(
        &self,
        backup_id: &str,
        dry_run: bool,
    ) -> Result<MemoryRestoreDeleteReport> {
        let report = self
            .memory_backup_manager()
            .delete_restore_point(backup_id, dry_run)
            .await?;
        self.log_memory_backup_event(
            "delete",
            true,
            &format!(
                "backup_id={} dry_run={} files={} bytes={} receipts={}",
                report.backup_id,
                report.dry_run,
                report.file_count,
                report.total_bytes,
                report.receipt_count
            ),
        );
        Ok(report)
    }

    pub async fn explain_memory_restore_policy(
        &self,
        backup_id: &str,
    ) -> Result<MemoryRestorePolicyBasis> {
        let report = self.dry_run_memory_restore_point(backup_id).await?;
        let manifest = self.inspect_memory_restore_point(backup_id).await?;
        let mut reasons = vec![
            "restore_only_sealed_backup".to_string(),
            format!("contract_version={}", manifest.contract_version),
            format!("fingerprint={}", manifest.encryption_key_fingerprint),
            format!("file_count={}", manifest.file_count),
        ];
        let mut warnings = Vec::new();

        if report.valid {
            reasons.push("dry_run_valid".to_string());
            reasons.push(format!(
                "restorable_files={}/{}",
                report.restorable_files, report.file_count
            ));
        } else {
            warnings.push("dry_run_invalid".to_string());
        }

        if !report.missing_payloads.is_empty() {
            warnings.push(format!(
                "missing_payloads={}",
                report.missing_payloads.join(",")
            ));
        }
        if !report.integrity_mismatches.is_empty() {
            warnings.push(format!(
                "integrity_mismatches={}",
                report.integrity_mismatches.join(",")
            ));
        }

        Ok(MemoryRestorePolicyBasis {
            backup_id: backup_id.to_string(),
            decision_kind: if report.valid { "permit" } else { "deny" }.to_string(),
            policy_basis: "sealed_restore_point_validation".to_string(),
            reasons,
            warnings,
        })
    }

    /// Verify if the requester matches the parent PID
    pub fn verify_parent(&self, claimed_pid: u32) -> bool {
        self.pid_guard.verify_parent(claimed_pid)
    }

    /// Verify a request signature for anti-tampering
    pub fn verify_signature(&self, secret: &str, message: &str, signature: &str) -> bool {
        anti_tamper::AntiTamper::verify(secret.as_bytes(), message.as_bytes(), signature)
    }

    /// Pre-check a tool call against Wasm policy
    pub async fn pre_check_tool(&self, tool_name: &str, args: &str) -> Result<()> {
        if let Some(ref guard) = self.policy_guard {
            guard.pre_check(tool_name, args).await
        } else {
            Ok(())
        }
    }

    /// Post-filter a tool result through Wasm policy
    pub async fn post_filter_result(&self, result: &str) -> String {
        if let Some(ref guard) = self.policy_guard {
            guard.post_filter(result).await
        } else {
            result.to_string()
        }
    }

    fn normalize_query_signature(query: &str) -> String {
        query
            .split_whitespace()
            .map(|part| part.to_ascii_lowercase())
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn estimate_query_cost(query: &str, requested_limit: usize) -> usize {
        let char_cost = (query.chars().count().max(1) / 24).max(1);
        let term_cost = query.split_whitespace().count().max(1);
        let result_cost = (requested_limit / 2).max(1);
        char_cost.max(term_cost).saturating_add(result_cost)
    }

    fn log_query_protection(&self, decision: &InfraQueryProtectionDecision) {
        if decision.action == QueryProtectionAction::Allow {
            return;
        }

        self.log_action(
            None,
            &format!("query_protection:{}", decision.surface),
            &format!(
                "signature={} cost={} reasons={}",
                decision.query_signature,
                decision.estimated_cost,
                decision.reasons.join(",")
            ),
            true,
            &decision.user_message,
            None,
        );
    }

    pub fn protect_query(
        &self,
        request: &InfraQueryProtectionRequest,
    ) -> InfraQueryProtectionDecision {
        let query_signature = Self::normalize_query_signature(&request.query);
        let estimated_cost = request
            .estimated_cost
            .unwrap_or_else(|| Self::estimate_query_cost(&request.query, request.requested_limit));

        if !self.config.query_protection_enabled || query_signature.is_empty() {
            return InfraQueryProtectionDecision {
                action: QueryProtectionAction::Allow,
                surface: request.surface.clone(),
                query_signature,
                estimated_cost,
                retry_after_ms: None,
                protect_user: true,
                protect_system: true,
                reasons: Vec::new(),
                user_message: "query allowed".to_string(),
            };
        }

        let now = Instant::now();
        let burst_window =
            std::time::Duration::from_secs(self.config.query_protection_burst_window_secs);
        let pause_duration =
            std::time::Duration::from_secs(self.config.query_protection_pause_secs);
        let mut state = self
            .query_protection
            .lock()
            .expect("query protection lock poisoned");
        let entry = state
            .entry(query_signature.clone())
            .or_insert_with(QueryProtectionState::default);
        entry
            .recent_hits
            .retain(|timestamp| now.duration_since(*timestamp) <= burst_window);

        if let Some(pause_until) = entry.pause_until {
            if pause_until > now {
                let retry_after_ms = pause_until.duration_since(now).as_millis() as u64;
                let decision = InfraQueryProtectionDecision {
                    action: QueryProtectionAction::PauseCurrentPath,
                    surface: request.surface.clone(),
                    query_signature,
                    estimated_cost,
                    retry_after_ms: Some(retry_after_ms),
                    protect_user: true,
                    protect_system: true,
                    reasons: vec![
                        "deep_query_path_temporarily_paused".to_string(),
                        "burst_protection_active".to_string(),
                    ],
                    user_message: "Deep retrieval is temporarily paused to avoid runaway query storms. Lightweight results can still continue, and you can retry shortly.".to_string(),
                };
                drop(state);
                self.log_query_protection(&decision);
                return decision;
            }
            entry.pause_until = None;
        }

        entry.recent_hits.push(now);
        let repeated_hits = entry.recent_hits.len();
        let high_cost = estimated_cost >= self.config.query_protection_high_cost_threshold;

        let decision = if high_cost && repeated_hits >= self.config.query_protection_burst_limit {
            entry.pause_until = Some(now + pause_duration);
            InfraQueryProtectionDecision {
                action: QueryProtectionAction::PauseCurrentPath,
                surface: request.surface.clone(),
                query_signature,
                estimated_cost,
                retry_after_ms: Some(pause_duration.as_millis() as u64),
                protect_user: true,
                protect_system: true,
                reasons: vec![
                    "repeated_high_cost_query".to_string(),
                    "deep_query_path_paused".to_string(),
                ],
                user_message: "Deep retrieval was paused for a few seconds to protect your session from repetitive high-cost query loops.".to_string(),
            }
        } else if high_cost && repeated_hits >= 2 {
            InfraQueryProtectionDecision {
                action: QueryProtectionAction::Degrade,
                surface: request.surface.clone(),
                query_signature,
                estimated_cost,
                retry_after_ms: None,
                protect_user: true,
                protect_system: true,
                reasons: vec![
                    "repeated_high_cost_query".to_string(),
                    "prefer_lightweight_path".to_string(),
                ],
                user_message: "Deep retrieval was temporarily downgraded to a lighter path to avoid unnecessary cost and runaway repetition.".to_string(),
            }
        } else {
            InfraQueryProtectionDecision {
                action: QueryProtectionAction::Allow,
                surface: request.surface.clone(),
                query_signature,
                estimated_cost,
                retry_after_ms: None,
                protect_user: true,
                protect_system: true,
                reasons: Vec::new(),
                user_message: "query allowed".to_string(),
            }
        };
        drop(state);
        self.log_query_protection(&decision);
        decision
    }
}

#[async_trait::async_trait]
impl SecurityHandler for SecurityManager {
    /// Scan input text (usually from User) for prompt injection attempts.
    /// Returns sanitized text (wrapped in markers) if enabled.
    fn check_input(&self, text: &str) -> InfraSanitizedOutput {
        if self.config.injection_check_enabled {
            let res = self.injection_detector.check_injection(text);
            InfraSanitizedOutput {
                content: res.content,
                warnings: res.warnings,
                was_modified: res.was_modified,
            }
        } else {
            InfraSanitizedOutput {
                content: text.to_string(),
                warnings: vec![],
                was_modified: false,
            }
        }
    }

    /// Returns redacted text and detections.
    fn check_output(&self, text: &str) -> (String, Vec<InfraLeakDetection>) {
        if self.config.leak_detection_enabled {
            let (content, detections) = self.leak_detector.redact(text);
            let mapped = detections
                .into_iter()
                .map(|d| InfraLeakDetection {
                    pattern_name: d.pattern_name,
                    redacted_value: d.redacted_value,
                })
                .collect();
            (content, mapped)
        } else {
            (text.to_string(), vec![])
        }
    }

    fn log_action(
        &self,
        session_key: Option<&str>,
        tool_name: &str,
        arguments: &str,
        success: bool,
        output_preview: &str,
        backup: Option<benshu_infra::skill::BackupInfo>,
    ) {
        let entry = audit::AuditEntry {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            session_key: session_key.map(|s| s.to_string()),
            tool_name: tool_name.to_string(),
            arguments: arguments.to_string(),
            success,
            output_preview: output_preview.to_string(),
            backup,
        };
        if let Err(e) = self.audit.log(entry) {
            tracing::error!("Failed to write to audit log: {}", e);
        }
    }

    async fn retrieve_audit_logs(&self, limit: usize) -> anyhow::Result<Vec<InfraAuditLogRecord>> {
        let entries = self.audit.retrieve_recent(limit)?;
        Ok(entries
            .into_iter()
            .map(|e| InfraAuditLogRecord {
                timestamp: e.timestamp,
                session_key: e.session_key,
                tool_name: e.tool_name,
                arguments: e.arguments,
                success: e.success,
                output_preview: e.output_preview,
                backup: e.backup,
            })
            .collect())
    }

    async fn pre_check_tool(&self, tool_name: &str, arguments: &str) -> anyhow::Result<()> {
        self.pre_check_tool(tool_name, arguments).await
    }

    async fn post_filter_result(&self, result: &str) -> String {
        self.post_filter_result(result).await
    }

    fn encrypt_fact(&self, plaintext: &str) -> anyhow::Result<String> {
        if let Some(enc) = &self.encryptor {
            enc.encrypt(plaintext)
        } else {
            Ok(plaintext.to_string())
        }
    }

    fn decrypt_fact(&self, encrypted: &str) -> anyhow::Result<String> {
        if let Some(enc) = &self.encryptor {
            enc.decrypt(encrypted)
        } else {
            Ok(encrypted.to_string())
        }
    }

    async fn store_secret(&self, key: &str, value: &str) -> anyhow::Result<()> {
        if let Some(ref vault) = self.vault {
            vault.set(key, value).map_err(|e| anyhow::anyhow!(e))
        } else {
            Err(anyhow::anyhow!("Vault not initialized"))
        }
    }

    async fn get_secret(&self, key: &str) -> anyhow::Result<Option<String>> {
        if let Some(ref vault) = self.vault {
            vault.get(key).map_err(|e| anyhow::anyhow!(e))
        } else {
            Ok(None)
        }
    }

    async fn delete_secret(&self, key: &str) -> anyhow::Result<()> {
        if let Some(ref vault) = self.vault {
            vault.delete(key).map_err(|e| anyhow::anyhow!(e))
        } else {
            Ok(())
        }
    }

    async fn list_secrets(&self) -> anyhow::Result<Vec<String>> {
        if let Some(ref vault) = self.vault {
            vault.list_keys().map_err(|e| anyhow::anyhow!(e))
        } else {
            Ok(Vec::new())
        }
    }

    async fn update_sandbox_policy(
        &self,
        action: &str,
        params: serde_json::Value,
    ) -> anyhow::Result<()> {
        info!(
            "Updating sandbox policy: action={}, params={}",
            action, params
        );

        let mut policy = self.dynamic_policy.write().await;
        match action {
            "expand_path" => {
                if let Some(path) = params.get("path").and_then(|v| v.as_str()) {
                    policy.allowed_paths.push(PathBuf::from(path));
                }
            }
            "restrict_net" => {
                if let Some(p) = params.get("policy").and_then(|v| v.as_str()) {
                    policy.block_network = p == "none" || p == "local_only";
                }
            }
            "execution_policy" => {
                if let Some(allow) = params.get("allow_binary").and_then(|v| v.as_bool()) {
                    policy.allow_binary_exec = allow;
                }
            }
            _ => {}
        }

        Ok(())
    }

    async fn get_sandbox_status(&self) -> anyhow::Result<serde_json::Value> {
        let active = crate::sandbox::ACTIVE_SANDBOXES.len();
        let mut hardened = 0usize;
        let mut partial = 0usize;
        let mut degraded = 0usize;
        for entry in crate::sandbox::ACTIVE_SANDBOXES.iter() {
            match entry.value().isolation_state.as_str() {
                "hardened" => hardened += 1,
                "partial" => partial += 1,
                _ => degraded += 1,
            }
        }

        let status = if active == 0 {
            if cfg!(target_os = "linux") || cfg!(target_os = "macos") || cfg!(target_os = "windows")
            {
                "READY"
            } else {
                "DEGRADED"
            }
        } else if degraded > 0 {
            "DEGRADED"
        } else if partial > 0 {
            "PARTIAL"
        } else if hardened > 0 {
            "HARDENED"
        } else {
            "READY"
        };
        let policy = self.dynamic_policy.read().await;
        Ok(serde_json::json!({
            "active_processes": active,
            "engine": if cfg!(target_os = "linux") { "bwrap" } else if cfg!(target_os = "macos") { "sandbox-exec" } else { "job-objects" },
            "status": status,
            "hardened_processes": hardened,
            "partial_processes": partial,
            "degraded_processes": degraded,
            "policy": *policy
        }))
    }

    async fn reset_sandbox_policy(&self) -> anyhow::Result<()> {
        info!("Resetting sandbox policy to system defaults (HIGHEST_IMMUNITY)");
        *self.dynamic_policy.write().await = DynamicPolicy::default();
        Ok(())
    }

    fn get_dynamic_policy(&self) -> DynamicPolicy {
        self.dynamic_policy
            .try_read()
            .map(|p| p.clone())
            .unwrap_or_default()
    }

    fn protect_query(&self, request: &InfraQueryProtectionRequest) -> InfraQueryProtectionDecision {
        self.protect_query(request)
    }
}

impl Default for SecurityManager {
    fn default() -> Self {
        Self::new(SecurityConfig::default(), None)
    }
}

#[cfg(test)]
mod bypass_tests;
#[cfg(test)]
mod tests;
