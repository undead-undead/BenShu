use async_trait::async_trait;
use benshu_infra::ResourceSensor;
use benshu_infra::{Tool, ToolDefinition};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

/// SystemMonitorTool for metabolic and resource sensing.
pub struct SystemMonitorTool {
    sensor: Arc<parking_lot::RwLock<benshu_infra::sensor::CapabilitySensor>>,
}

impl SystemMonitorTool {
    pub fn new(sensor: Arc<parking_lot::RwLock<benshu_infra::sensor::CapabilitySensor>>) -> Self {
        Self { sensor }
    }
}

#[async_trait]
impl Tool for SystemMonitorTool {
    fn name(&self) -> String {
        "system_monitor".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Monitor system resources (CPU, Memory, Disk, etc.). Use this before spawning sub-agents or performing heavy tasks to ensure system stability.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "detailed": { "type": "boolean", "description": "Whether to return detailed per-component stats" }
                }
            }),
            parameters_ts: None,
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some("Check cpu_usage and free_memory_pct. Avoid spawning sub-agents if cpu_usage > 85% or free_memory < 10%.".into()),
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            #[serde(default)]
            detailed: bool,
        }
        let args: Args = serde_json::from_str(arguments).unwrap_or(Args { detailed: false });

        let stats = {
            let mut sensor = self.sensor.write();
            sensor.check_resources(args.detailed)
        };

        Ok(serde_json::to_string_pretty(&stats)?)
    }
}
