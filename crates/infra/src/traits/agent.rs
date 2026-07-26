use crate::agent::SafetyLevel;
use async_trait::async_trait;
use std::time::Duration;

/// Handler for user approvals for risky actions
#[async_trait]
pub trait ApprovalHandler: Send + Sync {
    /// Request approval for a tool call
    async fn approve(
        &self,
        tool_name: &str,
        arguments: &str,
        safety: SafetyLevel,
    ) -> anyhow::Result<bool> {
        // Default 2-minute safety timeout for approvals
        self.approve_with_timeout(tool_name, arguments, safety, Duration::from_secs(120))
            .await
    }

    /// Request approval with a specific timeout
    async fn approve_with_timeout(
        &self,
        tool_name: &str,
        arguments: &str,
        safety: SafetyLevel,
        timeout: Duration,
    ) -> anyhow::Result<bool>;
}

/// Handler for human-in-the-loop interactions (getting text input)
#[async_trait]
pub trait InteractionHandler: Send + Sync {
    /// Ask the user a question and get a string response
    async fn ask(&self, question: &str) -> anyhow::Result<String>;
}
