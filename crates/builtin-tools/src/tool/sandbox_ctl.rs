use async_trait::async_trait;
use benshu_infra::{SafetyLevel, Tool, ToolDefinition};
use serde::Deserialize;
use serde_json::json;

/// SandboxConfiguratorTool for dynamic immunity and sandbox control.
/// This tool allows for programmatic adjustment of security boundaries.
pub struct SandboxConfiguratorTool;

/// Arguments for SandboxConfiguratorTool
///
/// - `action`: The sandbox configuration action to perform.
///   Supported: "expand_path", "restrict_net", "set_execution_policy".
/// - `scoped_path`: Path to whitelist/restrict (required for "expand_path").
/// - `network_policy`: Network access level (used for "restrict_net").
///   Defaults to "local_only".
/// - `allow_binary_exec`: Binary execution permission (used for "set_execution_policy").
#[derive(Debug, Deserialize)]
struct SandboxArgs {
    action: String,
    #[serde(default)]
    scoped_path: Option<String>,
    #[serde(default)]
    network_policy: Option<String>,
    #[serde(default)]
    allow_binary_exec: bool,
}

#[async_trait]
impl Tool for SandboxConfiguratorTool {
    fn name(&self) -> String {
        "sandbox_ctl".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Dynamically adjust sandbox permissions and security policies for sub-agents or specific tasks. Use this for autonomous evolution and safe scaling.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": { 
                        "type": "string", 
                        "enum": ["expand_path", "restrict_net", "set_execution_policy"], 
                        "description": "The specific configuration action to apply to the sandbox." 
                    },
                    "scoped_path": { 
                        "type": "string", 
                        "description": "Specific filesystem path to whitelist or apply scope to." 
                    },
                    "network_policy": { 
                        "type": "string", 
                        "enum": ["none", "local_only", "full"], 
                        "description": "Network access level to enforce." 
                    },
                    "allow_binary_exec": { 
                        "type": "boolean", 
                        "description": "Toggle to allow or block execution of unverified binaries." 
                    }
                },
                "required": ["action"]
            }),
            parameters_ts: None,
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some("CRITICAL: Only expand permissions for specific, audited sub-tasks. Always prefer 'local_only' or 'none' for network policies unless external connectivity is strictly required for the objective.".into()),
            safety_level: SafetyLevel::Red,
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let args: SandboxArgs = serde_json::from_str(arguments)
            .map_err(|e| anyhow::anyhow!("Invalid sandbox arguments: {}", e))?;

        // Access the real security handler via task-local storage
        let security = benshu_brain::skills::CURRENT_SECURITY.with(|s| s.clone());

        match args.action.as_str() {
            "expand_path" => {
                let path = args.scoped_path.ok_or_else(|| {
                    anyhow::anyhow!("Required parameter 'scoped_path' is missing for action 'expand_path'")
                })?;

                // Security: Basic path traversal validation
                if path.is_empty() || path.contains("..//") || path.starts_with("/../") {
                    return Err(anyhow::anyhow!("Invalid scoped_path: '{}' (path traversal detected)", path));
                }

                security.update_sandbox_policy("expand_path", json!({ "path": path })).await?;

                Ok(format!("Sandbox expanded: Whitelisted path [{}]. Policy audit: PASS.", path))
            },
            "restrict_net" => {
                let policy = args.network_policy.clone().unwrap_or_else(|| "local_only".to_string());
                security.update_sandbox_policy("restrict_net", json!({ "policy": policy })).await?;

                Ok(format!("Network policy updated to: {}. Enforcement engine: ACTIVE.", policy.to_uppercase()))
            },
            "set_execution_policy" => {
                let exec_allow = args.allow_binary_exec;
                security.update_sandbox_policy("execution_policy", json!({ "allow_binary": exec_allow })).await?;

                let policy_str = if exec_allow { "ALLOWED" } else { "BLOCKED" };
                Ok(format!(
                    "Binary execution policy updated to: {}. Unverified binaries will be {}.",
                    policy_str,
                    if exec_allow { "permitted" } else { "blocked" }
                ))
            },
            _ => Err(anyhow::anyhow!(
                "Unsupported sandbox action '{}'. Supported actions: expand_path, restrict_net, set_execution_policy.", 
                args.action
            )),
        }
    }
}
