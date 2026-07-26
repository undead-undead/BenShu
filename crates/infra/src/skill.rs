use crate::resource::ThrottleLevel;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Specification for a model dependency required by a skill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSpec {
    /// Unique identifier / filename for the model (e.g., "whisper-tiny")
    pub name: String,
    /// Download URL (supports HTTPS, Hugging Face hub shorthand, or local path)
    pub source: String,
    /// Model format: "onnx", "gguf", "safetensors", "pytorch", "custom"
    #[serde(default = "default_model_format")]
    pub format: String,
    /// Expected size in MB (used for progress reporting and disk space checks)
    pub size_mb: Option<u64>,
    /// SHA256 checksum for integrity verification (hex string)
    pub sha256: Option<String>,
}

fn default_model_format() -> String {
    "onnx".to_string()
}

/// Filesystem access declared by a dynamic skill.
///
/// The default is intentionally read-only inside the skill package. Higher
/// privileges must be explicitly declared so the loader can surface the risk
/// and execution runtimes can enforce a matching sandbox policy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillFilesystemAccess {
    None,
    ReadSkill,
    ReadWriteSkill,
    WorkspaceRead,
    WorkspaceReadWrite,
}

impl Default for SkillFilesystemAccess {
    fn default() -> Self {
        Self::ReadSkill
    }
}

/// Permission declaration for dynamic skills.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillPermissions {
    #[serde(default)]
    pub filesystem: SkillFilesystemAccess,
    #[serde(default)]
    pub network: bool,
    #[serde(default)]
    pub browser: bool,
    #[serde(default)]
    pub env: Vec<String>,
    #[serde(default)]
    pub allowed_paths: Vec<String>,
}

impl Default for SkillPermissions {
    fn default() -> Self {
        Self {
            filesystem: SkillFilesystemAccess::default(),
            network: false,
            browser: false,
            env: Vec::new(),
            allowed_paths: Vec::new(),
        }
    }
}

/// Resource limits declared by a skill manifest.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SkillResourceLimits {
    pub timeout_secs: Option<u64>,
    pub max_output_bytes: Option<usize>,
    pub max_memory_mb: Option<usize>,
    pub max_cpu_percent: Option<usize>,
    pub max_net_bps: Option<u64>,
    pub max_disk_bps: Option<u64>,
}

/// Wasm-specific ABI contract.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WasmToolContract {
    #[serde(default = "default_wasm_abi")]
    pub abi: String,
    #[serde(default = "default_wasm_entrypoint")]
    pub entrypoint: String,
    pub sha256: Option<String>,
}

impl Default for WasmToolContract {
    fn default() -> Self {
        Self {
            abi: default_wasm_abi(),
            entrypoint: default_wasm_entrypoint(),
            sha256: None,
        }
    }
}

fn default_wasm_abi() -> String {
    "wasi-component-run-string-v1".to_string()
}

fn default_wasm_entrypoint() -> String {
    "run".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMetadata {
    /// Name of the skill
    pub name: String,
    /// Short description
    pub description: String,
    /// Optional homepage URL
    pub homepage: Option<String>,
    /// Arguments schema (JSON Schema) - DEPRECATED: use parameters_ts
    pub parameters: Option<Value>,
    /// Arguments as TypeScript interface (Preferred)
    pub interface: Option<String>,
    /// Script to execute
    pub script: Option<String>,
    /// Language or runtime for the script
    pub runtime: Option<String>,
    /// Standard Smithery metadata object
    #[serde(default)]
    pub metadata: Value,
    /// Kind of skill (e.g., 'tool', 'knowledge', 'agent')
    #[serde(default = "default_skill_kind")]
    pub kind: String,
    /// Optional usage guidelines for LLM reasoning
    pub usage_guidelines: Option<String>,
    /// List of conda/pixi dependencies
    #[serde(default)]
    pub dependencies: Vec<String>,
    /// Whether this skill requires a browser
    #[serde(default)]
    pub use_browser: bool,
    /// Model dependencies for ML/AI skills
    #[serde(default)]
    pub models: Vec<ModelSpec>,
    /// Phase 15.4: Original source file for fallback if binary fails
    pub source_fallback: Option<String>,
    /// Phase 15.4: Security audit status
    pub safety_audit: Option<String>,
    /// Permission declaration used by plugin runtimes and approval policy.
    #[serde(default)]
    pub permissions: SkillPermissions,
    /// Runtime resource limits. These are applied before execution.
    #[serde(default)]
    pub resources: SkillResourceLimits,
    /// Wasm-specific tool protocol declaration.
    #[serde(default)]
    pub wasm: Option<WasmToolContract>,
}

fn default_skill_kind() -> String {
    "tool".to_string()
}

/// Configuration for skill execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillExecutionConfig {
    /// Maximum execution time in seconds
    pub timeout_secs: u64,
    /// Maximum output size in bytes (to prevent memory exhaustion)
    pub max_output_bytes: usize,
    /// Whether to allow network access (future: implement via sandbox)
    pub allow_network: bool,
    /// Whether to provide a pre-configured headless browser
    pub use_browser: bool,
    /// Maximum memory in megabytes
    pub max_memory_mb: Option<usize>,
    /// Maximum CPU percentage (0-100)
    pub max_cpu_percent: Option<usize>,
    /// Maximum network bandwidth in bytes per second
    pub max_net_bps: Option<u64>,
    /// Maximum disk I/O in bytes per second
    pub max_disk_bps: Option<u64>,
    /// Custom environment variables
    pub env_vars: HashMap<String, String>,
    /// Resource throttling level
    pub throttle: ThrottleLevel,
    /// Signal to runtime that host is under heavy pressure
    pub is_low_resource: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupInfo {
    pub original_path: String,
    pub backup_path: String,
}

impl Default for SkillExecutionConfig {
    fn default() -> Self {
        Self {
            timeout_secs: 30,
            max_output_bytes: 1024 * 1024, // 1MB
            allow_network: false,
            use_browser: false,
            max_memory_mb: None,
            max_cpu_percent: None,
            max_net_bps: None,
            max_disk_bps: None,
            env_vars: HashMap::new(),
            throttle: ThrottleLevel::Medium,
            is_low_resource: false,
        }
    }
}

tokio::task_local! {
    /// Task-local storage for the current resource throttle level
    pub static CURRENT_THROTTLE: ThrottleLevel;
    /// Signal to runtime that host is under heavy pressure
    pub static CURRENT_PRESSURE: bool;
    /// Current session's calculated risk score
    pub static CURRENT_RISK_SCORE: f32;
    /// Overriding the model name (Auto-Stepdown)
    pub static CURRENT_MODEL_OVERRIDE: Option<String>;
    /// Capture the shadow backup path during tool pre_call
    pub static CURRENT_BACKUP: std::sync::Arc<parking_lot::Mutex<Option<BackupInfo>>>;
    /// List of trusted workspace paths
    pub static CURRENT_WORKSPACES: Vec<std::path::PathBuf>;
    /// Security handler for tools to manage secrets/vault
    pub static CURRENT_SECURITY: std::sync::Arc<dyn crate::traits::security::SecurityHandler>;
}
