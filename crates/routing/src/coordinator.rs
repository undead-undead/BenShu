use crate::{capability_route_should_inject_system_message, CapabilityRouteHint};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinatorTaskMode {
    ChatLite,
    VisionLite,
    DocumentLite,
    ToolAgent,
}

pub fn select_coordinator_task_mode(
    route: Option<CapabilityRouteHint>,
    has_media_followup: bool,
) -> CoordinatorTaskMode {
    match route {
        Some(
            CapabilityRouteHint::DocumentUnderstanding | CapabilityRouteHint::VoiceUnderstanding,
        ) => CoordinatorTaskMode::DocumentLite,
        Some(CapabilityRouteHint::VisualUnderstanding) => CoordinatorTaskMode::VisionLite,
        Some(
            CapabilityRouteHint::RealtimeLookup(_)
            | CapabilityRouteHint::RuntimeSurface
            | CapabilityRouteHint::ExternalCliTools
            | CapabilityRouteHint::FileOps
            | CapabilityRouteHint::Writing
            | CapabilityRouteHint::Coding
            | CapabilityRouteHint::Communication
            | CapabilityRouteHint::Memory
            | CapabilityRouteHint::CapabilityGap,
        ) => CoordinatorTaskMode::ToolAgent,
        Some(CapabilityRouteHint::General) | None if has_media_followup => {
            CoordinatorTaskMode::VisionLite
        }
        _ => CoordinatorTaskMode::ChatLite,
    }
}

pub fn coordinator_task_mode_label(mode: CoordinatorTaskMode) -> &'static str {
    match mode {
        CoordinatorTaskMode::ChatLite => "chat_lite",
        CoordinatorTaskMode::VisionLite => "vision_lite",
        CoordinatorTaskMode::DocumentLite => "document_lite",
        CoordinatorTaskMode::ToolAgent => "tool_agent",
    }
}

pub fn coordinator_task_mode_system_message(mode: CoordinatorTaskMode) -> &'static str {
    match mode {
        CoordinatorTaskMode::ChatLite => {
            "### BENSHU_CHAT_LITE\nThis turn should remain in BenShu's frontstage chat mode.\nStay in the same BenShu persona. Answer directly when the request is lightweight, conversational, or clarifying. Keep tools and orchestration minimal unless a specialist is clearly needed."
        }
        CoordinatorTaskMode::VisionLite => {
            "### BENSHU_VISION_LITE\nThis turn should remain in BenShu's frontstage visual-understanding mode.\nStay in the same BenShu persona. Focus on visible content, screenshot interpretation, and concise follow-up. Do not expand into heavy tool or multi-agent orchestration unless the task truly requires specialist execution."
        }
        CoordinatorTaskMode::DocumentLite => {
            "### BENSHU_DOCUMENT_LITE\nThis turn should remain in BenShu's frontstage document-intake mode.\nStay in the same BenShu persona. Coordinate document, PDF, OCR, or attachment understanding cleanly, and only delegate or trigger execution surfaces when needed."
        }
        CoordinatorTaskMode::ToolAgent => {
            "### BENSHU_TOOL_AGENT\nThis turn is a frontstage coordination-for-execution mode.\nStay in the same BenShu persona. Classify, assign, verify, and synthesize. Prefer the narrowest specialist or minimal tool path. Do not turn BenShu into the heavy executor unless no specialist path exists."
        }
    }
}

pub fn coordinator_routing_judgment_only_message() -> &'static str {
    "### BENSHU_ROUTING_JUDGMENT_ONLY\nThis turn asks for coordination judgment only.\nStay in BenShu's frontstage coordinator role. Explain the narrowest specialist, execution surface, or next route you would choose, but do not execute tools, do not delegate, and do not claim execution results in this turn."
}

pub fn coordinator_task_mode_should_include_media_followup_prompt(
    mode: CoordinatorTaskMode,
    has_media_followup: bool,
) -> bool {
    has_media_followup
        && matches!(
            mode,
            CoordinatorTaskMode::VisionLite
                | CoordinatorTaskMode::DocumentLite
                | CoordinatorTaskMode::ToolAgent
        )
}

pub fn coordinator_task_mode_should_include_route_prompt(
    mode: CoordinatorTaskMode,
    route: CapabilityRouteHint,
) -> bool {
    capability_route_should_inject_system_message(route)
        && !matches!(mode, CoordinatorTaskMode::ChatLite)
}

pub fn coordinator_task_mode_should_include_truth_guidance(
    mode: CoordinatorTaskMode,
    truth_policy_active: bool,
    has_media_followup_contract: bool,
) -> bool {
    if truth_policy_active {
        return true;
    }

    has_media_followup_contract
        && matches!(
            mode,
            CoordinatorTaskMode::VisionLite
                | CoordinatorTaskMode::DocumentLite
                | CoordinatorTaskMode::ToolAgent
        )
}

pub fn coordinator_task_mode_should_include_tool_index(_mode: CoordinatorTaskMode) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_mode_matches_route_and_media_shape() {
        assert_eq!(
            select_coordinator_task_mode(None, false),
            CoordinatorTaskMode::ChatLite
        );
        assert_eq!(
            select_coordinator_task_mode(None, true),
            CoordinatorTaskMode::VisionLite
        );
        assert_eq!(
            select_coordinator_task_mode(Some(CapabilityRouteHint::VisualUnderstanding), false),
            CoordinatorTaskMode::VisionLite
        );
        assert_eq!(
            select_coordinator_task_mode(Some(CapabilityRouteHint::DocumentUnderstanding), false),
            CoordinatorTaskMode::DocumentLite
        );
        assert_eq!(
            select_coordinator_task_mode(Some(CapabilityRouteHint::Coding), false),
            CoordinatorTaskMode::ToolAgent
        );
    }

    #[test]
    fn prompt_gates_stay_stable() {
        assert!(!coordinator_task_mode_should_include_media_followup_prompt(
            CoordinatorTaskMode::ChatLite,
            true
        ));
        assert!(coordinator_task_mode_should_include_media_followup_prompt(
            CoordinatorTaskMode::VisionLite,
            true
        ));
        assert!(!coordinator_task_mode_should_include_route_prompt(
            CoordinatorTaskMode::ChatLite,
            CapabilityRouteHint::Memory
        ));
        assert!(coordinator_task_mode_should_include_route_prompt(
            CoordinatorTaskMode::ToolAgent,
            CapabilityRouteHint::Memory
        ));
        assert!(coordinator_task_mode_should_include_truth_guidance(
            CoordinatorTaskMode::ChatLite,
            true,
            false
        ));
        assert!(coordinator_task_mode_should_include_truth_guidance(
            CoordinatorTaskMode::VisionLite,
            false,
            true
        ));
        assert!(!coordinator_task_mode_should_include_tool_index(
            CoordinatorTaskMode::ToolAgent
        ));
    }
}
