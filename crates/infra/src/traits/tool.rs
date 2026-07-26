use crate::agent::SafetyLevel;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Definition of a tool that can be sent to the LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Name of the tool
    pub name: String,
    /// Description for the LLM
    pub description: String,
    /// JSON Schema for parameters (Legacy/API)
    pub parameters: serde_json::Value,
    /// TypeScript interface definition (Preferred for System Prompt)
    pub parameters_ts: Option<String>,
    /// Whether this is a binary tool (e.g. Wasm)
    #[serde(default)]
    pub is_binary: bool,
    /// Whether the tool is verified/trusted
    #[serde(default)]
    pub is_verified: bool,
    /// Safety level for this tool
    #[serde(default)]
    pub safety_level: SafetyLevel,
    /// Usage guidelines for the LLM (When to use, When NOT to use)
    pub usage_guidelines: Option<String>,
}

/// Normalized catalog metadata for a registered tool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolCatalogEntry {
    pub name: String,
    pub description: String,
    pub capability_domain: String,
    pub tags: Vec<String>,
    pub source: String,
    pub scope: String,
    pub usage_guidelines: Option<String>,
    pub safety_level: SafetyLevel,
    pub is_binary: bool,
    pub is_verified: bool,
}

/// Optional catalog hints attached by runtime registration code.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolCatalogOverride {
    pub source: Option<String>,
    pub scope: Option<String>,
    pub capability_domain: Option<String>,
    pub tags: Vec<String>,
}

/// Trait for implementing tools that AI agents can call
#[async_trait]
pub trait Tool: Send + Sync {
    /// The name of this tool
    fn name(&self) -> String;

    /// Get the tool definition for the LLM
    async fn definition(&self) -> ToolDefinition;

    /// Execute the tool with the given arguments (JSON string)
    async fn call(&self, arguments: &str) -> anyhow::Result<String>;

    /// OPTIONAL: Pre-execution hook for security or state preparation
    async fn pre_call(&self, _arguments: &str) -> anyhow::Result<()> {
        Ok(())
    }
}
