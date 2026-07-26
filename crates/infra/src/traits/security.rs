use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Trait for retrieving secrets from various backends.
pub trait SecretVault: Send + Sync {
    /// Retrieve a secret by key. Returns None if not found.
    fn get(&self, key: &str) -> crate::error::Result<Option<String>>;

    /// Set a secret if supported by the backend.
    fn set(&self, _key: &str, _value: &str) -> crate::error::Result<()> {
        Err(crate::error::Error::internal(
            "Setting secrets not supported by this vault",
        ))
    }

    /// Delete a secret if supported by the backend.
    fn delete(&self, _key: &str) -> crate::error::Result<()> {
        Err(crate::error::Error::internal(
            "Deleting secrets not supported by this vault",
        ))
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QueryProtectionAction {
    Allow,
    Degrade,
    PauseCurrentPath,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryProtectionRequest {
    pub surface: String,
    pub query: String,
    pub requested_limit: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_cost: Option<usize>,
    #[serde(default)]
    pub prefers_deep_retrieval: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryProtectionDecision {
    pub action: QueryProtectionAction,
    pub surface: String,
    pub query_signature: String,
    pub estimated_cost: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<u64>,
    #[serde(default)]
    pub protect_user: bool,
    #[serde(default)]
    pub protect_system: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<String>,
    pub user_message: String,
}

/// Output of a security input check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SanitizedOutput {
    pub content: String,
    pub warnings: Vec<String>,
    pub was_modified: bool,
}

/// A detection of a potential secret leak.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeakDetection {
    pub pattern_name: String,
    pub redacted_value: String,
}

/// A record in the audit log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLogRecord {
    pub timestamp: u64,
    pub session_key: Option<String>,
    pub tool_name: String,
    pub arguments: String,
    pub success: bool,
    pub output_preview: String,
    pub backup: Option<crate::skill::BackupInfo>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DynamicPolicy {
    pub allowed_paths: Vec<std::path::PathBuf>,
    pub block_network: bool,
    pub allow_binary_exec: bool,
}

/// Trait for enforcing security policies on inputs and outputs.
#[async_trait]
pub trait SecurityHandler: Send + Sync {
    /// Scan input text for potential threats (e.g., prompt injection).
    fn check_input(&self, text: &str) -> SanitizedOutput;
    /// Scan output text for potential leaks (e.g., API keys).
    fn check_output(&self, text: &str) -> (String, Vec<LeakDetection>);
    /// Log an agent action to the immutable audit database.
    fn log_action(
        &self,
        session_key: Option<&str>,
        tool_name: &str,
        arguments: &str,
        success: bool,
        output_preview: &str,
        backup: Option<crate::skill::BackupInfo>,
    );
    /// Retrieve recent audit logs for distillation.
    async fn retrieve_audit_logs(&self, limit: usize) -> anyhow::Result<Vec<AuditLogRecord>>;

    /// Pre-check a tool call for potential policy violations.
    async fn pre_check_tool(&self, _tool_name: &str, _arguments: &str) -> anyhow::Result<()> {
        Ok(())
    }
    /// Post-filter a tool result through security policies.
    async fn post_filter_result(&self, result: &str) -> String {
        result.to_string()
    }

    /// Encrypt a fact content (Selective Encryption)
    fn encrypt_fact(&self, plaintext: &str) -> anyhow::Result<String> {
        Ok(plaintext.to_string())
    }
    /// Decrypt a fact content (Selective Encryption)
    fn decrypt_fact(&self, encrypted: &str) -> anyhow::Result<String> {
        Ok(encrypted.to_string())
    }

    // --- Vault management ---
    async fn store_secret(&self, _key: &str, _value: &str) -> anyhow::Result<()> {
        Err(anyhow::anyhow!("Vault not implemented"))
    }
    async fn get_secret(&self, _key: &str) -> anyhow::Result<Option<String>> {
        Ok(None)
    }
    async fn delete_secret(&self, _key: &str) -> anyhow::Result<()> {
        Ok(())
    }
    async fn list_secrets(&self) -> anyhow::Result<Vec<String>> {
        Ok(Vec::new())
    }

    // --- Sandbox control ---
    async fn update_sandbox_policy(
        &self,
        _action: &str,
        _params: serde_json::Value,
    ) -> anyhow::Result<()> {
        Ok(())
    }
    async fn get_sandbox_status(&self) -> anyhow::Result<serde_json::Value> {
        Ok(serde_json::Value::Null)
    }
    async fn reset_sandbox_policy(&self) -> anyhow::Result<()> {
        Ok(())
    }
    fn get_dynamic_policy(&self) -> DynamicPolicy {
        DynamicPolicy::default()
    }

    fn protect_query(&self, request: &QueryProtectionRequest) -> QueryProtectionDecision {
        QueryProtectionDecision {
            action: QueryProtectionAction::Allow,
            surface: request.surface.clone(),
            query_signature: request.query.trim().to_string(),
            estimated_cost: request.estimated_cost.unwrap_or_default(),
            retry_after_ms: None,
            protect_user: true,
            protect_system: true,
            reasons: Vec::new(),
            user_message: "query allowed".to_string(),
        }
    }
}

/// Trait for inspecting unpacked `.vessel` packages for security violations.
#[async_trait]
pub trait VesselInspector: Send + Sync {
    /// Inspect the unpacked agent and identity files.
    async fn inspect_agent(&self, extract_to: &Path) -> crate::error::Result<()>;
}
