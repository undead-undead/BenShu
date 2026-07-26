use serde::{Deserialize, Serialize};

/// Shared contract for long-running worker workflows.
///
/// This trait is intentionally small and system-level. It describes how a
/// worker-owned workflow can be driven by the runtime without putting
/// tool-specific policy into this crate.
pub trait WorkflowDriver {
    fn descriptor(&self) -> WorkflowDriverDescriptor;

    fn inspect(&self, snapshot: &WorkflowRunSnapshot) -> WorkflowInspection {
        let descriptor = self.descriptor();
        let next_phase = descriptor
            .phases
            .iter()
            .find(|phase| {
                !snapshot
                    .completed_phases
                    .iter()
                    .any(|done| done == &phase.id)
            })
            .map(|phase| phase.id.clone());

        WorkflowInspection {
            workflow_id: descriptor.id,
            status: snapshot.status,
            current_phase: snapshot.current_phase.clone(),
            next_phase,
            blockers: snapshot.blockers.clone(),
            progress: snapshot.progress.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowDriverDescriptor {
    pub id: String,
    pub version: String,
    pub domain: String,
    pub owner_role: String,
    pub tool_name: String,
    pub entry_actions: Vec<String>,
    pub phases: Vec<WorkflowPhaseDescriptor>,
    pub capabilities: WorkflowCapabilities,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowPhaseDescriptor {
    pub id: String,
    pub kind: WorkflowPhaseKind,
    pub tool_action: Option<String>,
    pub model_required: bool,
    pub artifact_effect: Option<String>,
    pub required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowPhaseKind {
    Intake,
    Contract,
    Plan,
    Context,
    Architecture,
    Generate,
    Verify,
    Revise,
    Persist,
    Approval,
    Snapshot,
    Export,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowCapabilities {
    pub resumable: bool,
    pub interruptible: bool,
    pub single_active_run_recommended: bool,
    pub keeps_large_output_out_of_chat: bool,
    pub requires_worker_model: bool,
    pub owns_private_model: bool,
}

impl Default for WorkflowCapabilities {
    fn default() -> Self {
        Self {
            resumable: true,
            interruptible: true,
            single_active_run_recommended: true,
            keeps_large_output_out_of_chat: true,
            requires_worker_model: true,
            owns_private_model: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowRunStatus {
    Pending,
    Running,
    WaitingForModel,
    WaitingForTool,
    Blocked,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowRunSnapshot {
    pub workflow_id: String,
    pub status: WorkflowRunStatus,
    pub current_phase: Option<String>,
    pub completed_phases: Vec<String>,
    pub blockers: Vec<WorkflowBlocker>,
    pub progress: WorkflowProgress,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct WorkflowProgress {
    pub current_step: Option<String>,
    pub completed_steps: usize,
    pub total_steps: Option<usize>,
    pub artifact_path: Option<String>,
    pub summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowBlocker {
    pub code: String,
    pub message: String,
    pub recoverable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowInspection {
    pub workflow_id: String,
    pub status: WorkflowRunStatus,
    pub current_phase: Option<String>,
    pub next_phase: Option<String>,
    pub blockers: Vec<WorkflowBlocker>,
    pub progress: WorkflowProgress,
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TinyDriver;

    impl WorkflowDriver for TinyDriver {
        fn descriptor(&self) -> WorkflowDriverDescriptor {
            WorkflowDriverDescriptor {
                id: "tiny".to_string(),
                version: "v1".to_string(),
                domain: "test".to_string(),
                owner_role: "worker".to_string(),
                tool_name: "tool".to_string(),
                entry_actions: vec!["start".to_string()],
                phases: vec![
                    WorkflowPhaseDescriptor {
                        id: "plan".to_string(),
                        kind: WorkflowPhaseKind::Plan,
                        tool_action: Some("plan".to_string()),
                        model_required: true,
                        artifact_effect: None,
                        required: true,
                    },
                    WorkflowPhaseDescriptor {
                        id: "export".to_string(),
                        kind: WorkflowPhaseKind::Export,
                        tool_action: Some("export".to_string()),
                        model_required: false,
                        artifact_effect: Some("artifact.exported".to_string()),
                        required: true,
                    },
                ],
                capabilities: WorkflowCapabilities::default(),
            }
        }
    }

    #[test]
    fn default_inspection_selects_first_unfinished_phase() {
        let snapshot = WorkflowRunSnapshot {
            workflow_id: "tiny".to_string(),
            status: WorkflowRunStatus::Running,
            current_phase: Some("plan".to_string()),
            completed_phases: vec!["plan".to_string()],
            blockers: Vec::new(),
            progress: WorkflowProgress::default(),
        };

        let inspection = TinyDriver.inspect(&snapshot);

        assert_eq!(inspection.workflow_id, "tiny");
        assert_eq!(inspection.next_phase.as_deref(), Some("export"));
    }
}
