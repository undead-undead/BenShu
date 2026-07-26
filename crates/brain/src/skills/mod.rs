// compiler, capabilities, and forge implementations moved to 'builtin-tools' crate.
pub mod runtime;
// sandbox module moved to 'security' crate
pub mod tool;

pub use benshu_infra::resource::ThrottleLevel;
pub use benshu_infra::skill::{BackupInfo, ModelSpec, SkillExecutionConfig, SkillMetadata};

#[derive(Debug, Clone, Default)]
pub struct RuntimeSecurityContext {
    pub trace_id: Option<String>,
    pub run_id: Option<String>,
    pub task_id: Option<String>,
    pub session_id: Option<String>,
}

tokio::task_local! {
    /// Task-local storage for the current resource throttle level (Roadmap Phase 7.2)
    pub static CURRENT_THROTTLE: ThrottleLevel;
    /// Phase 8: Task-local for signaling low-resource availability
    pub static CURRENT_PRESSURE: bool;
    /// Phase 6.2: Task-local for capturing the shadow backup path during tool pre_call
    pub static CURRENT_BACKUP: std::sync::Arc<parking_lot::Mutex<Option<BackupInfo>>>;
    /// Phase 7.1: Task-local for the list of trusted workspace paths
    pub static CURRENT_WORKSPACES: Vec<std::path::PathBuf>;
    /// Phase 18.5: Task-local for the security handler to allow tools to manage secrets/vault
    pub static CURRENT_SECURITY: std::sync::Arc<dyn benshu_infra::traits::security::SecurityHandler>;
    /// Phase 20.x: Task-local runtime refs for security receipts and audit correlation.
    pub static CURRENT_RUNTIME_SECURITY_CONTEXT: RuntimeSecurityContext;
}

// Heavy implementations (DynamicSkill, SkillLoader, etc.) moved to 'benshu-skill' or 'builtin-tools' crate.
