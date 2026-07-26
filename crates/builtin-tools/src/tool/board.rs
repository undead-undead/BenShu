use async_trait::async_trait;
use benshu_brain::agent::namespaced_memory::NamespacedMemory;
use benshu_infra::{Tool, ToolDefinition};
use serde::Deserialize;
use std::sync::Arc;

/// Tool for agents to share data in a collaborative session (Phase 9.2)
pub struct SharedBoardTool {
    memory: Arc<NamespacedMemory>,
}

impl SharedBoardTool {
    pub fn new(memory: Arc<NamespacedMemory>) -> Self {
        Self { memory }
    }
}

#[async_trait]
impl Tool for SharedBoardTool {
    fn name(&self) -> String {
        "shared_board".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Interact with the session's shared whiteboard. Use this to share structured data, \
                task status, or intermediate results with other agents in the swarm.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["write", "read", "list"], "description": "Action to perform" },
                    "key": { "type": "string", "description": "Key for the data (e.g., 'market_analysis')" },
                    "value": { "type": "string", "description": "Value to store (for write action)" },
                    "ttl_seconds": { "type": "integer", "description": "Optional TTL in seconds (default: 3600)" }
                },
                "required": ["action"]
            }),
            parameters_ts: Some("interface BoardArgs {\n  action: 'write' | 'read' | 'list';\n  key?: string;\n  value?: string;\n  ttl_seconds?: number;\n}".to_string()),
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some("Use 'session' as the namespace unless instructed otherwise. High TTL (e.g., 86400) for cross-session knowledge is discouraged here; use 'remember_this' for that.".to_string()),
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            action: String,
            key: Option<String>,
            value: Option<String>,
            ttl_seconds: Option<u64>,
        }

        let args: Args = serde_json::from_str(arguments)?;
        let namespace = "swarm_shared"; // Simplest implementation for now

        match args.action.as_str() {
            "write" => {
                let key = args
                    .key
                    .ok_or_else(|| anyhow::anyhow!("Key required for write"))?;
                let value = args
                    .value
                    .ok_or_else(|| anyhow::anyhow!("Value required for write"))?;
                let ttl = std::time::Duration::from_secs(args.ttl_seconds.unwrap_or(3600));

                self.memory
                    .store(namespace, &key, &value, Some(ttl), None)
                    .await?;
                Ok(format!("Successfully wrote to shared board: {}", key))
            }
            "read" => {
                let key = args
                    .key
                    .ok_or_else(|| anyhow::anyhow!("Key required for read"))?;
                let value = self.memory.read(namespace, &key).await?;
                match value {
                    Some(v) => Ok(format!("Shared Board [{}]: {}", key, v)),
                    None => Ok(format!(
                        "Key '{}' not found or expired on shared board.",
                        key
                    )),
                }
            }
            "list" => {
                let keys = self.memory.list_keys(namespace).await?;
                if keys.is_empty() {
                    Ok("Shared board is currently empty.".to_string())
                } else {
                    Ok(format!("Shared Board Keys: {}", keys.join(", ")))
                }
            }
            _ => Err(anyhow::anyhow!("Invalid action")),
        }
    }
}
