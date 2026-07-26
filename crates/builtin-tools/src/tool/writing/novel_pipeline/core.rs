use benshu_runtime_policy_core::{
    WorkflowCapabilities, WorkflowDriver, WorkflowDriverDescriptor, WorkflowPhaseDescriptor,
    WorkflowPhaseKind,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub(crate) const PIPELINE_CONTRACT_VERSION: &str = "benshu.novel_pipeline.v3";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum NovelPhase {
    SourceIntake,
    StoryContract,
    ContextPackage,
    ChapterExecutionPackage,
    Draft,
    Audit,
    Revision,
    TruthSettlement,
    TruthValidation,
    Approval,
    Snapshot,
    Export,
}

impl NovelPhase {
    pub(crate) const ALL: [Self; 12] = [
        Self::SourceIntake,
        Self::StoryContract,
        Self::ContextPackage,
        Self::ChapterExecutionPackage,
        Self::Draft,
        Self::Audit,
        Self::Revision,
        Self::TruthSettlement,
        Self::TruthValidation,
        Self::Approval,
        Self::Snapshot,
        Self::Export,
    ];

    pub(crate) const CHAPTER_LOOP: [Self; 9] = [
        Self::ContextPackage,
        Self::ChapterExecutionPackage,
        Self::Draft,
        Self::Audit,
        Self::Revision,
        Self::TruthSettlement,
        Self::TruthValidation,
        Self::Approval,
        Self::Snapshot,
    ];

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::SourceIntake => "source_intake",
            Self::StoryContract => "story_contract",
            Self::ContextPackage => "context_package",
            Self::ChapterExecutionPackage => "chapter_execution_package",
            Self::Draft => "draft",
            Self::Audit => "audit",
            Self::Revision => "revision",
            Self::TruthSettlement => "truth_settlement",
            Self::TruthValidation => "truth_validation",
            Self::Approval => "approval",
            Self::Snapshot => "snapshot",
            Self::Export => "export",
        }
    }

    pub(crate) const fn tool_action(self) -> &'static str {
        match self {
            Self::SourceIntake => "add_source",
            Self::StoryContract => "set_contract",
            Self::ContextPackage => "compose_context",
            Self::ChapterExecutionPackage => "persist_execution_package",
            Self::Draft => "write_draft",
            Self::Audit => "audit_chapter",
            Self::Revision => "revise_draft",
            Self::TruthSettlement => "settle_chapter_state",
            Self::TruthValidation => "validate_chapter_state",
            Self::Approval => "approve_chapter",
            Self::Snapshot => "snapshot",
            Self::Export => "export",
        }
    }

    const fn kind(self) -> WorkflowPhaseKind {
        match self {
            Self::SourceIntake => WorkflowPhaseKind::Intake,
            Self::StoryContract => WorkflowPhaseKind::Contract,
            Self::ContextPackage => WorkflowPhaseKind::Context,
            Self::ChapterExecutionPackage => WorkflowPhaseKind::Plan,
            Self::Draft => WorkflowPhaseKind::Generate,
            Self::Audit | Self::TruthValidation => WorkflowPhaseKind::Verify,
            Self::Revision => WorkflowPhaseKind::Revise,
            Self::TruthSettlement => WorkflowPhaseKind::Persist,
            Self::Approval => WorkflowPhaseKind::Approval,
            Self::Snapshot => WorkflowPhaseKind::Snapshot,
            Self::Export => WorkflowPhaseKind::Export,
        }
    }

    const fn model_required(self) -> bool {
        matches!(
            self,
            Self::StoryContract
                | Self::ChapterExecutionPackage
                | Self::Draft
                | Self::Audit
                | Self::Revision
        )
    }

    const fn required(self) -> bool {
        !matches!(
            self,
            Self::SourceIntake | Self::Revision | Self::Snapshot | Self::Export
        )
    }

    const fn artifact_effect(self) -> &'static str {
        match self {
            Self::Draft | Self::Revision => "artifact.written",
            Self::Audit | Self::TruthValidation | Self::Approval => "artifact.checked",
            Self::Export => "artifact.exported",
            _ => "artifact.checkpointed",
        }
    }
}

pub(crate) fn phase_ids(phases: &[NovelPhase]) -> Vec<&'static str> {
    phases.iter().map(|phase| phase.as_str()).collect()
}

pub(crate) fn action_ids(phases: &[NovelPhase]) -> Vec<&'static str> {
    phases.iter().map(|phase| phase.tool_action()).collect()
}

pub(crate) fn phase_for_action(action: &str) -> Option<NovelPhase> {
    let canonical = NovelPhase::ALL
        .into_iter()
        .find(|phase| phase.tool_action() == action);
    canonical.or_else(|| match action {
        "add_chapter" => Some(NovelPhase::Draft),
        "review_chapter" => Some(NovelPhase::Audit),
        "revise_chapter" => Some(NovelPhase::Revision),
        _ => None,
    })
}

pub(crate) struct NovelWorkflowDefinition;

impl WorkflowDriver for NovelWorkflowDefinition {
    fn descriptor(&self) -> WorkflowDriverDescriptor {
        WorkflowDriverDescriptor {
            id: "writing.longform_fiction".to_string(),
            version: PIPELINE_CONTRACT_VERSION.to_string(),
            domain: "writing".to_string(),
            owner_role: "writer".to_string(),
            tool_name: "novel_studio".to_string(),
            entry_actions: vec!["run_project".to_string(), "run_next_chapter".to_string()],
            phases: NovelPhase::ALL
                .into_iter()
                .map(|phase| WorkflowPhaseDescriptor {
                    id: phase.as_str().to_string(),
                    kind: phase.kind(),
                    tool_action: Some(phase.tool_action().to_string()),
                    model_required: phase.model_required(),
                    artifact_effect: Some(phase.artifact_effect().to_string()),
                    required: phase.required(),
                })
                .collect(),
            capabilities: WorkflowCapabilities {
                resumable: true,
                interruptible: true,
                single_active_run_recommended: true,
                keeps_large_output_out_of_chat: true,
                requires_worker_model: true,
                owns_private_model: false,
            },
        }
    }
}

pub(crate) fn novel_workflow_descriptor() -> WorkflowDriverDescriptor {
    NovelWorkflowDefinition.descriptor()
}

pub(crate) fn novel_workflow_descriptor_json() -> serde_json::Value {
    serde_json::to_value(novel_workflow_descriptor()).unwrap_or_else(|_| {
        json!({
            "id": "writing.longform_fiction",
            "version": PIPELINE_CONTRACT_VERSION,
            "tool_name": "novel_studio"
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_is_generated_from_the_canonical_phase_order() {
        let descriptor = novel_workflow_descriptor();
        let ids = descriptor
            .phases
            .iter()
            .map(|phase| phase.id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(ids, phase_ids(&NovelPhase::ALL));
        assert!(
            ids.iter().position(|id| *id == "context_package")
                < ids.iter().position(|id| *id == "chapter_execution_package")
        );
        assert!(descriptor.capabilities.requires_worker_model);
        assert!(!descriptor.capabilities.owns_private_model);
    }

    #[test]
    fn conditional_phases_are_not_declared_required() {
        let descriptor = novel_workflow_descriptor();
        for id in ["source_intake", "revision", "snapshot", "export"] {
            assert_eq!(
                descriptor
                    .phases
                    .iter()
                    .find(|phase| phase.id == id)
                    .map(|phase| phase.required),
                Some(false),
                "{id} must remain conditional"
            );
        }
    }

    #[test]
    fn deterministic_truth_phases_do_not_request_another_model_turn() {
        let descriptor = novel_workflow_descriptor();
        for id in ["context_package", "truth_settlement", "truth_validation"] {
            assert_eq!(
                descriptor
                    .phases
                    .iter()
                    .find(|phase| phase.id == id)
                    .map(|phase| phase.model_required),
                Some(false),
                "{id} is executed deterministically by the tool"
            );
        }
    }

    #[test]
    fn public_compatibility_actions_map_to_canonical_phases() {
        assert_eq!(phase_for_action("revise_draft"), Some(NovelPhase::Revision));
        assert_eq!(
            phase_for_action("revise_chapter"),
            Some(NovelPhase::Revision)
        );
        assert_eq!(phase_for_action("review_chapter"), Some(NovelPhase::Audit));
    }
}
