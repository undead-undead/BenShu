use super::*;

pub fn creation_draft_approval_requested(message: &str) -> bool {
    matches!(
        classify_creation_draft_turn_intent(message),
        CreationDraftTurnIntent::ApproveAndStart
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreationDraftTurnIntent {
    ClarifyOrPlan,
    UpdateContract,
    ApproveAndStart,
    DeferStart,
    ReadStatus,
    Discard,
    Unknown,
}

pub fn classify_creation_draft_turn_intent(message: &str) -> CreationDraftTurnIntent {
    classify_creation_draft_turn_intent_with_context(message, true, None, None, None)
}

pub fn classify_creation_draft_turn_intent_with_context(
    message: &str,
    session_has_draft: bool,
    draft_status: Option<CreationDraftLifecycleStatus>,
    latest_project_path: Option<&str>,
    active_task_status: Option<&str>,
) -> CreationDraftTurnIntent {
    let decision = intent_policy::decide(WritingIntentInput {
        message,
        session_has_draft,
        draft_status,
        latest_project_path,
        active_task_status,
    });
    if decision.confidence < 0.2 {
        return CreationDraftTurnIntent::Unknown;
    }
    match decision.intent {
        WritingIntent::CancelDraft => CreationDraftTurnIntent::Discard,
        WritingIntent::UpdateContract => {
            if decision.route_hint == "defer_start" {
                CreationDraftTurnIntent::DeferStart
            } else {
                CreationDraftTurnIntent::UpdateContract
            }
        }
        WritingIntent::ApproveContract => CreationDraftTurnIntent::ApproveAndStart,
        WritingIntent::ReadProjectStatus => CreationDraftTurnIntent::ReadStatus,
        WritingIntent::StartContract => CreationDraftTurnIntent::ClarifyOrPlan,
        _ => CreationDraftTurnIntent::Unknown,
    }
}

pub fn creation_draft_planning_dialogue_requested(message: &str) -> bool {
    matches!(
        classify_creation_draft_turn_intent(message),
        CreationDraftTurnIntent::ClarifyOrPlan
            | CreationDraftTurnIntent::UpdateContract
            | CreationDraftTurnIntent::DeferStart
    )
}
