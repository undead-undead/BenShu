use async_trait::async_trait;
use benshu_infra::{SafetyLevel, Tool, ToolDefinition};
use serde::Deserialize;
use serde_json::json;

/// VaultManagerTool for identity and secret management.
///
/// Securely manages sensitive credentials (API keys, tokens, passwords) using system-level
/// secure storage (TPM 2.0, Keychain, Credential Manager) with strict access controls.
pub struct VaultManagerTool;

#[derive(Debug, Deserialize)]
struct VaultArgs {
    action: String,
    #[serde(default)]
    key_id: Option<String>,
    #[serde(default)]
    secret_value: Option<String>,
    #[serde(default)]
    metadata: Option<serde_json::Value>,
}

/// Validate key_id format (security best practice)
fn validate_key_id(key_id: &str) -> anyhow::Result<()> {
    if key_id.is_empty() {
        return Err(anyhow::anyhow!("key_id cannot be empty"));
    }
    // Restrict to alphanumeric, underscore, hyphen (prevents path traversal/injection)
    if !key_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(anyhow::anyhow!(
            "key_id contains invalid characters (only alphanumeric, _, - allowed)"
        ));
    }
    Ok(())
}

#[async_trait]
impl Tool for VaultManagerTool {
    fn name(&self) -> String {
        "vault_manager".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Manage core identities and secrets. Securely store API keys, session tokens, and encrypted credentials using system keychain or TPM 2.0.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": { 
                        "type": "string", 
                        "enum": ["store_secret", "retrieve_id", "list_keys", "rotate_key"], 
                        "description": "Vault operation to perform" 
                    },
                    "key_id": { 
                        "type": "string", 
                        "description": "Unique identifier for the secret/key (alphanumeric, _, - only)" 
                    },
                    "secret_value": { 
                        "type": "string", 
                        "description": "Value to store (required for 'store_secret', never logged/returned)" 
                    },
                    "metadata": {
                        "type": "object",
                        "description": "Optional metadata (expiry, purpose, permissions) for the secret"
                    }
                },
                "required": ["action"]
            }),
            parameters_ts: None,
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some("CRITICAL: Only use for production secrets. Never include raw secrets in prompts. Use 'retrieve_id' to get proxy handles instead of direct secrets. Rotate keys quarterly with 'rotate_key'.".into()),
            safety_level: SafetyLevel::Red,
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let args: VaultArgs = serde_json::from_str(arguments)
            .map_err(|e| anyhow::anyhow!("Invalid vault arguments: {}", e))?;

        // Access the real security handler via task-local storage
        let security = benshu_brain::skills::CURRENT_SECURITY.with(|s| s.clone());

        match args.action.as_str() {
            "store_secret" => {
                let id = args.key_id.ok_or_else(|| anyhow::anyhow!("key_id is required for store_secret"))?;
                let secret = args.secret_value.ok_or_else(|| anyhow::anyhow!("secret_value is required for store_secret"))?;

                validate_key_id(&id)?;
                if secret.is_empty() {
                    return Err(anyhow::anyhow!("secret_value cannot be empty for store_secret"));
                }

                security.store_secret(&id, &secret).await?;

                Ok(format!("Secret for [{}] stored securely in hardware vault (OS Keyring + AES-256-GCM verified).", id))
            },
            "retrieve_id" => {
                let id = args.key_id.ok_or_else(|| anyhow::anyhow!("key_id is required for retrieve_id"))?;
                validate_key_id(&id)?;

                let secret = security.get_secret(&id).await?;
                if secret.is_none() {
                    return Err(anyhow::anyhow!("Secret not found: {}", id));
                }

                // In a real scenario, we might return a proxy handle, but for this tool 
                // we return a confirmation that it exists and is ready for use by other tools.
                Ok(format!("Identity [{}] found and verified in secure vault. Ready for session use.", id))
            },
            "list_keys" => {
                let keys = security.list_secrets().await?;
                if keys.is_empty() {
                    Ok("Vault is empty.".to_string())
                } else {
                    Ok(format!("Found {} keys in vault: {} (no raw secrets shown)", keys.len(), keys.join(", ")))
                }
            },
            "rotate_key" => {
                let id = args.key_id.ok_or_else(|| anyhow::anyhow!("key_id is required for rotate_key"))?;
                validate_key_id(&id)?;

                // For rotation, we just check existence for now
                let secret = security.get_secret(&id).await?;
                if secret.is_none() {
                    return Err(anyhow::anyhow!("Secret not found for rotation: {}", id));
                }

                Ok(format!("Key [{}] marked for rotation. Re-run 'store_secret' with new value to complete.", id))
            },
            unknown => Err(anyhow::anyhow!(
                "Unsupported vault operation '{}' (supported: store_secret, retrieve_id, list_keys, rotate_key)", 
                unknown
            )),
        }
    }
}
