use async_trait::async_trait;
use benshu_infra::{Tool, ToolDefinition};
use serde::{Deserialize, Serialize};
/// Tool for requesting a peer audit across the A2A specialist network (Phase 9.3)
pub struct MultiAgentAuditTool {
    coordinator: std::sync::Weak<benshu_brain::agent::multi_agent::Coordinator>,
}

impl MultiAgentAuditTool {
    pub fn new(
        coordinator: std::sync::Weak<benshu_brain::agent::multi_agent::Coordinator>,
    ) -> Self {
        Self { coordinator }
    }
}

#[async_trait]
impl Tool for MultiAgentAuditTool {
    fn name(&self) -> String {
        "consensus_audit".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Request a mandatory safety audit from a peer agent in the A2A specialist network (e.g., risk_analyst) for high-risk actions. \
                Use this when your current risk_score > 0.8 or before executing critical system changes.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "action_to_audit": { "type": "string", "description": "The command or code you intend to execute" },
                    "rationale": { "type": "string", "description": "Why you think this action is necessary" }
                },
                "required": ["action_to_audit", "rationale"]
            }),
            parameters_ts: Some("interface AuditArgs {\n  action_to_audit: string;\n  rationale: string;\n}".to_string()),
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some("You MUST provide the full context of the intended action. If the auditor rejects, you must find a safer alternative or ask the user for explicit approval.".to_string()),
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Args {
            action_to_audit: String,
            rationale: String,
        }
        let args: Args = serde_json::from_str(arguments)?;

        let coordinator = self
            .coordinator
            .upgrade()
            .ok_or_else(|| anyhow::anyhow!("Coordinator lost"))?;

        // Find Risk Analyst
        let auditor = coordinator
            .get(&benshu_brain::agent::multi_agent::AgentRole::RiskAnalyst)
            .or_else(|| coordinator.get(&benshu_brain::agent::multi_agent::AgentRole::Researcher)) // Fallback
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "No safety auditor (Risk Analyst) available in the A2A specialist network"
                )
            })?;

        let audit_request = format!(
            "### AUDIT REQUEST ###\nACTION: {}\nRATIONALE: {}\n\nPlease evaluate the risk and provide a GO/NO-GO decision with a brief explanation.",
            args.action_to_audit, args.rationale
        );

        let result = auditor.process(&audit_request).await?;
        Ok(format!("### AUDIT RESULT ###\n\n{}", result))
    }
}
