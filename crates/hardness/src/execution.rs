use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionToolReplyRequirementInput {
    pub has_media_input: bool,
    pub normalized_text_is_empty: bool,
    pub document_understanding_turn: bool,
    pub capability_route_requires_real_tool_call: bool,
}

pub fn decide_execution_tool_reply_requirement(input: ExecutionToolReplyRequirementInput) -> bool {
    if input.normalized_text_is_empty {
        return false;
    }

    let simple_multimodal_qa = input.has_media_input && input.document_understanding_turn;
    if simple_multimodal_qa {
        return false;
    }

    input.capability_route_requires_real_tool_call
}
