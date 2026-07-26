use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

pub const DEFAULT_EXPERIENCE_NAMESPACE: &str = "system_experience";

fn default_contract_version() -> u32 {
    1
}

fn default_namespace() -> String {
    DEFAULT_EXPERIENCE_NAMESPACE.to_string()
}

fn default_confidence() -> f32 {
    0.5
}

fn default_status() -> ExperienceStatus {
    ExperienceStatus::Candidate
}

pub fn current_time_ms() -> i64 {
    Utc::now().timestamp_millis()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ExperienceScope {
    LocalWindows,
    Web,
    Workspace,
    Tool,
    Agent,
    Other(String),
}

impl ExperienceScope {
    pub fn as_key(&self) -> String {
        match self {
            Self::LocalWindows => "local_windows".to_string(),
            Self::Web => "web".to_string(),
            Self::Workspace => "workspace".to_string(),
            Self::Tool => "tool".to_string(),
            Self::Agent => "agent".to_string(),
            Self::Other(value) => format!("other:{}", normalize_key(value)),
        }
    }

    pub fn default_ttl_seconds(&self) -> Option<i64> {
        match self {
            Self::Web => Some(24 * 60 * 60),
            Self::LocalWindows | Self::Workspace | Self::Tool | Self::Agent => {
                Some(30 * 24 * 60 * 60)
            }
            Self::Other(_) => Some(7 * 24 * 60 * 60),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExperienceStatus {
    Candidate,
    Active,
    Retired,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PreflightKind {
    UrlReachable,
    DomStable,
    FileExists,
    DirectoryExists,
    ProcessAlive,
    PortOpen,
    GpuAvailable,
    ToolAvailable,
    WorkerAvailable,
    WorkspaceState,
    Custom(String),
}

impl PreflightKind {
    pub fn as_key(&self) -> String {
        match self {
            Self::UrlReachable => "url_reachable".to_string(),
            Self::DomStable => "dom_stable".to_string(),
            Self::FileExists => "file_exists".to_string(),
            Self::DirectoryExists => "directory_exists".to_string(),
            Self::ProcessAlive => "process_alive".to_string(),
            Self::PortOpen => "port_open".to_string(),
            Self::GpuAvailable => "gpu_available".to_string(),
            Self::ToolAvailable => "tool_available".to_string(),
            Self::WorkerAvailable => "worker_available".to_string(),
            Self::WorkspaceState => "workspace_state".to_string(),
            Self::Custom(value) => format!("custom:{}", normalize_key(value)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PreflightCheck {
    pub kind: PreflightKind,
    pub target: String,
    pub description: String,
    #[serde(default = "default_true")]
    pub required: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExperienceStep {
    pub label: String,
    pub action: String,
    #[serde(default)]
    pub evidence_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FailureSignature {
    pub fingerprint: String,
    pub cause: String,
    pub avoid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct EvidenceRefs {
    #[serde(default)]
    pub trace_id: Option<String>,
    #[serde(default)]
    pub witness_id: Option<String>,
    #[serde(default)]
    pub scorecard_id: Option<String>,
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub tool_receipt_ids: Vec<String>,
    #[serde(default)]
    pub artifact_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct UsageStats {
    #[serde(default)]
    pub selected_count: u64,
    #[serde(default)]
    pub preflight_pass_count: u64,
    #[serde(default)]
    pub preflight_fail_count: u64,
    #[serde(default)]
    pub success_count: u64,
    #[serde(default)]
    pub failure_count: u64,
    #[serde(default)]
    pub last_selected_at_ms: Option<i64>,
    #[serde(default)]
    pub last_failed_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskExperience {
    pub id: String,
    #[serde(default = "default_contract_version")]
    pub contract_version: u32,
    #[serde(default = "default_namespace")]
    pub namespace: String,
    #[serde(default = "default_status")]
    pub status: ExperienceStatus,
    pub task_signature: String,
    pub task_summary: String,
    pub scope: ExperienceScope,
    #[serde(default)]
    pub worker_role: Option<String>,
    #[serde(default)]
    pub tool_names: Vec<String>,
    #[serde(default)]
    pub successful_steps: Vec<ExperienceStep>,
    #[serde(default)]
    pub required_preflight: Vec<PreflightCheck>,
    #[serde(default)]
    pub failure_signatures: Vec<FailureSignature>,
    #[serde(default)]
    pub evidence_refs: EvidenceRefs,
    #[serde(default)]
    pub hints: Vec<String>,
    #[serde(default)]
    pub anti_patterns: Vec<String>,
    #[serde(default = "default_confidence")]
    pub confidence: f32,
    #[serde(default)]
    pub usage: UsageStats,
    #[serde(default)]
    pub ttl_seconds: Option<i64>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    #[serde(default)]
    pub last_verified_at_ms: Option<i64>,
    #[serde(default)]
    pub expires_at_ms: Option<i64>,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

impl TaskExperience {
    pub fn new(
        task_signature: impl Into<String>,
        task_summary: impl Into<String>,
        scope: ExperienceScope,
    ) -> Self {
        let now = current_time_ms();
        let ttl_seconds = scope.default_ttl_seconds();
        Self {
            id: Uuid::new_v4().to_string(),
            contract_version: default_contract_version(),
            namespace: default_namespace(),
            status: ExperienceStatus::Candidate,
            task_signature: task_signature.into(),
            task_summary: task_summary.into(),
            scope,
            worker_role: None,
            tool_names: Vec::new(),
            successful_steps: Vec::new(),
            required_preflight: Vec::new(),
            failure_signatures: Vec::new(),
            evidence_refs: EvidenceRefs::default(),
            hints: Vec::new(),
            anti_patterns: Vec::new(),
            confidence: default_confidence(),
            usage: UsageStats::default(),
            ttl_seconds,
            created_at_ms: now,
            updated_at_ms: now,
            last_verified_at_ms: None,
            expires_at_ms: ttl_seconds.map(|ttl| now.saturating_add(ttl.saturating_mul(1000))),
            metadata: BTreeMap::new(),
        }
    }

    pub fn normalize_before_store(&mut self, now_ms: i64) {
        self.namespace = normalize_namespace(&self.namespace);
        self.confidence = self.confidence.clamp(0.0, 1.0);
        self.updated_at_ms = now_ms;
        if self.created_at_ms <= 0 {
            self.created_at_ms = now_ms;
        }
        if self.ttl_seconds.is_none() {
            self.ttl_seconds = self.scope.default_ttl_seconds();
        }
        if self.expires_at_ms.is_none() {
            let basis = self.last_verified_at_ms.unwrap_or(self.updated_at_ms);
            self.expires_at_ms = self
                .ttl_seconds
                .map(|ttl| basis.saturating_add(ttl.saturating_mul(1000)));
        }
        dedup_strings(&mut self.tool_names);
        dedup_strings(&mut self.hints);
        dedup_strings(&mut self.anti_patterns);
    }

    pub fn is_expired_at(&self, now_ms: i64) -> bool {
        self.expires_at_ms
            .is_some_and(|deadline| deadline <= now_ms)
    }

    pub fn is_reusable_at(&self, now_ms: i64) -> bool {
        matches!(
            self.status,
            ExperienceStatus::Candidate | ExperienceStatus::Active
        ) && !self.is_expired_at(now_ms)
            && self.confidence > 0.0
    }

    pub fn mark_selected(&mut self, now_ms: i64) {
        self.usage.selected_count = self.usage.selected_count.saturating_add(1);
        self.usage.last_selected_at_ms = Some(now_ms);
        self.updated_at_ms = now_ms;
    }

    pub fn record_preflight_result(&mut self, passed: bool, now_ms: i64) {
        if passed {
            self.usage.preflight_pass_count = self.usage.preflight_pass_count.saturating_add(1);
            self.last_verified_at_ms = Some(now_ms);
            self.refresh_expiry(now_ms);
            self.confidence = (self.confidence + 0.03).clamp(0.0, 1.0);
        } else {
            self.usage.preflight_fail_count = self.usage.preflight_fail_count.saturating_add(1);
            self.usage.last_failed_at_ms = Some(now_ms);
            self.confidence = (self.confidence - 0.12).clamp(0.0, 1.0);
        }
        self.updated_at_ms = now_ms;
    }

    pub fn record_task_result(&mut self, succeeded: bool, now_ms: i64) {
        if succeeded {
            self.usage.success_count = self.usage.success_count.saturating_add(1);
            self.status = ExperienceStatus::Active;
            self.last_verified_at_ms = Some(now_ms);
            self.refresh_expiry(now_ms);
            self.confidence = (self.confidence + 0.08).clamp(0.0, 1.0);
        } else {
            self.usage.failure_count = self.usage.failure_count.saturating_add(1);
            self.usage.last_failed_at_ms = Some(now_ms);
            self.confidence = (self.confidence - 0.18).clamp(0.0, 1.0);
            if self.confidence <= 0.05 {
                self.status = ExperienceStatus::Retired;
            }
        }
        self.updated_at_ms = now_ms;
    }

    fn refresh_expiry(&mut self, now_ms: i64) {
        self.expires_at_ms = self
            .ttl_seconds
            .map(|ttl| now_ms.saturating_add(ttl.saturating_mul(1000)));
    }
}

pub(crate) fn normalize_namespace(value: &str) -> String {
    let normalized = normalize_key(value);
    if normalized.is_empty() {
        DEFAULT_EXPERIENCE_NAMESPACE.to_string()
    } else {
        normalized
    }
}

pub(crate) fn normalize_key(value: &str) -> String {
    value
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == ':' {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .split('_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}

fn dedup_strings(values: &mut Vec<String>) {
    let mut seen = std::collections::BTreeSet::new();
    values.retain(|value| {
        let normalized = value.trim().to_string();
        if normalized.is_empty() {
            return false;
        }
        seen.insert(normalized)
    });
}
