use std::path::Path;

use serde_json::json;

use crate::tool::writing::novel_pipeline::{NovelPhase, PIPELINE_CONTRACT_VERSION};

/// Worker-facing submission contract for a model-generated phase output.
///
/// Pipeline ordering is owned by `novel_pipeline`; this packet only describes
/// how generated chapter prose is submitted back to the persistence tool.
pub(crate) fn writing_phase_contract(
    phase: NovelPhase,
    submission_action: &str,
    project_dir: &Path,
    chapter_number: usize,
    chapter_title: &str,
    objective: &str,
    chapter_unit_target: Option<usize>,
) -> serde_json::Value {
    json!({
        "schema_version": PIPELINE_CONTRACT_VERSION,
        "phase": phase,
        "objective": objective,
        "chapter": {
            "number": chapter_number,
            "title": chapter_title,
            "unit_target": chapter_unit_target
        },
        "required_runtime_order": [
            "read_context_package",
            "generate_phase_output",
            "self_check_against_contract",
            "submit_tool_action"
        ],
        "content_submission": {
            "tool": "novel_studio",
            "action": submission_action,
            "content_field": "content",
            "args": {
                "action": submission_action,
                "project_path": project_dir.to_string_lossy(),
                "chapter_number": chapter_number,
                "chapter_title": chapter_title
            },
            "required_fields": [
                "project_path",
                "chapter_number",
                "chapter_title",
                "content",
                "summary",
                "key_facts",
                "continuity_updates"
            ],
            "validation_owner": "novel_studio.chapter_quality",
            "authority_owner": "novel_studio.project_state"
        }
    })
}
