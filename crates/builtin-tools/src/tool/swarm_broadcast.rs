use async_trait::async_trait;
use benshu_comm::client::CommClient;
use benshu_comm::protocol::a2a::A2AMessage;
use benshu_comm::protocol::Address;
use benshu_infra::{Tool, ToolDefinition};
use serde::{Deserialize, Serialize};

/// Tool for broadcasting tasks across the wider A2A network (Phase 9.3)
pub struct SwarmBroadcastTool {
    comm: CommClient,
}

impl SwarmBroadcastTool {
    pub fn new(comm: CommClient) -> Self {
        Self { comm }
    }
}

#[async_trait]
impl Tool for SwarmBroadcastTool {
    fn name(&self) -> String {
        "swarm_broadcast".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Broadcast a task request across the wider A2A network. \
                Use this only when direct delegation or normal specialist discovery is insufficient.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "task": { "type": "string", "description": "The task description" },
                    "required_capabilities": { 
                        "type": "array", 
                        "items": { "type": "string" },
                        "description": "List of required capabilities (e.g., ['gpu', 'high_memory', 'python_expert'])" 
                    }
                },
                "required": ["task"]
            }),
            parameters_ts: Some("interface BroadcastArgs {\n  task: string;\n  required_capabilities?: string[];\n}".to_string()),
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some("Low-frequency tool. Prefer direct `delegate`, `handover`, or `shared_board` first. Use broadcast only when the work truly requires wide discovery or exceptional fan-out.".to_string()),
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            task: String,
            #[serde(default)]
            required_capabilities: Vec<String>,
        }

        let args: Args = serde_json::from_str(arguments)?;

        let msg = A2AMessage::new_request("self", args.task, args.required_capabilities);
        let request_id = msg.request_id().unwrap_or_default().to_string();
        let payload = serde_json::to_vec(&msg)?;

        self.comm
            .send_msg(Address::System("all".to_string()), payload)
            .await?;

        Ok(format!("Task broadcasted successfully via the A2A comm core. Request ID: {}. Local scheduler is tracking bids.", request_id))
    }
}
