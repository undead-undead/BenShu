use serde::{Deserialize, Serialize};

/// Default configuration constants for the agent runtime.
pub mod constants {
    pub const DEFAULT_MAX_HISTORY_MESSAGES: usize = 20;
    pub const DEFAULT_MAX_TOOL_OUTPUT_CHARS: usize = 8192;
    pub const DEFAULT_MAX_PARALLEL_TOOLS: usize = 5;
    pub const DEFAULT_LOOP_SIMILARITY_THRESHOLD: f64 = 0.8;
    pub const DEFAULT_STATUS_RECAP_THRESHOLD_STEPS: usize = 12;
    pub const DEFAULT_STATUS_RECAP_THRESHOLD_CHARS: usize = 5000;
    pub const DEFAULT_MAX_REFLEXION_RETRIES: usize = 3;
    pub const DEFAULT_MAX_STEPS: usize = 15;
    pub const DEFAULT_RESPONSE_RESERVE: usize = 4096;
    pub const DEFAULT_TOOL_EXECUTION_TIMEOUT_SECS: u64 = 120;
    pub const HIGH_RISK_THRESHOLD: f32 = 0.7;
    pub const DEFAULT_APPROVAL_CHANNEL_CAPACITY: usize = 64;
    pub const DEFAULT_APPROVAL_TIMEOUT_SECS: u64 = 120;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolPolicy {
    Auto,
    RequiresApproval,
    Disabled,
}

impl Default for ToolPolicy {
    fn default() -> Self {
        Self::Auto
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RiskyToolPolicy {
    pub default_policy: ToolPolicy,
    pub overrides: std::collections::HashMap<String, ToolPolicy>,
}

#[derive(Debug, Clone, Default)]
pub struct ExecutorConfig {
    pub tool_policy: RiskyToolPolicy,
    pub inherited_risk_score: f32,
    pub max_parallel_tools: usize,
    pub loop_similarity_threshold: f64,
    pub max_tool_output_chars: usize,
    pub enable_reflexion: bool,
    pub default_throttle: benshu_infra::resource::ThrottleLevel,
    pub trusted_workspaces: Vec<std::path::PathBuf>,
    pub tool_execution_timeout: std::time::Duration,
}

impl ExecutorConfig {
    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.max_parallel_tools == 0 {
            return Err("max_parallel_tools must be at least 1".to_string());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningStrategy {
    #[default]
    ReAct,
    TreeOfThoughts,
    Reflexion,
    Planning,
}

#[derive(Debug, Clone, Default)]
pub struct ReasonerConfig {
    pub agent_name: Option<String>,
    pub model: String,
    pub preamble: String,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u64>,
    pub session_id: Option<String>,
    pub inference_priority: i8,
    pub json_mode: bool,
    pub extra_params: Option<serde_json::Value>,
    pub enable_cache_control: bool,
    pub max_history_messages: usize,
    pub smart_pruning: bool,
    pub efficiency_trigger_secs: u64,
    pub max_reflexion_retries: usize,
    pub llm_timeout: std::time::Duration,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterventionConfig {
    pub status_recap_threshold_steps: usize,
    pub status_recap_threshold_chars: usize,
    pub enable_reflexion: bool,
    pub max_reflexion_retries: usize,
    pub name: String,
    pub status_recap_prompt: Option<String>,
    pub reflexion_prompt: Option<String>,
}

impl InterventionConfig {
    pub fn validate(&self) -> std::result::Result<(), String> {
        Ok(())
    }
}

impl Default for InterventionConfig {
    fn default() -> Self {
        Self {
            status_recap_threshold_steps: 5,
            status_recap_threshold_chars: 2000,
            enable_reflexion: true,
            max_reflexion_retries: 3,
            name: "default".to_string(),
            status_recap_prompt: None,
            reflexion_prompt: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executor_config_rejects_zero_parallel_tools() {
        let mut config = ExecutorConfig::default();
        config.max_parallel_tools = 0;

        assert!(config.validate().is_err());
    }

    #[test]
    fn policies_keep_expected_defaults() {
        assert_eq!(ToolPolicy::default(), ToolPolicy::Auto);
        assert_eq!(ReasoningStrategy::default(), ReasoningStrategy::ReAct);
        assert!(RiskyToolPolicy::default().overrides.is_empty());
    }
}
