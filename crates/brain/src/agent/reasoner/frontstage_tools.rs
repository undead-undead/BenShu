use crate::skills::tool::ToolDefinition;

pub(super) fn compact_frontstage_core_tool_definition(mut tool: ToolDefinition) -> ToolDefinition {
    let object_schema = |properties: serde_json::Value, required: Vec<&str>| {
        serde_json::json!({
            "type": "object",
            "properties": properties,
            "required": required,
        })
    };

    let compact = match tool.name.as_str() {
        "delegate" => Some((
            "Delegate execution to one specialist worker.".to_string(),
            object_schema(
                serde_json::json!({
                    "role": {"type": "string"},
                    "task": {"type": "string"}
                }),
                vec!["role", "task"],
            ),
        )),
        "shared_board" => Some((
            "Read or write lightweight same-session coordination notes.".to_string(),
            object_schema(
                serde_json::json!({
                    "action": {"type": "string", "enum": ["write", "read", "list"]},
                    "key": {"type": "string"},
                    "value": {"type": "string"},
                    "ttl_seconds": {"type": "integer"}
                }),
                vec!["action"],
            ),
        )),
        "tool_search" => Some((
            "Find the right specialist/tool when routing is unclear.".to_string(),
            object_schema(
                serde_json::json!({
                    "query": {"type": "string"},
                    "limit": {"type": "integer"}
                }),
                vec!["query"],
            ),
        )),
        "search_history" => Some((
            "Search durable user memory or previous conversations.".to_string(),
            object_schema(
                serde_json::json!({
                    "query": {"type": "string"},
                    "limit": {"type": "integer"}
                }),
                vec!["query"],
            ),
        )),
        "remember_this" => Some((
            "Save an explicit user-requested memory.".to_string(),
            object_schema(
                serde_json::json!({
                    "title": {"type": "string"},
                    "content": {"type": "string"},
                    "collection": {"type": "string"}
                }),
                vec!["title", "content"],
            ),
        )),
        "manage_facts" => Some((
            "Manage curated facts only when the user asks.".to_string(),
            object_schema(
                serde_json::json!({
                    "action": {"type": "string", "enum": ["upsert", "list", "delete", "update_importance", "find_related", "get_status", "pin", "unpin", "protect", "unprotect", "set_core_identity", "clear_core_identity"]},
                    "content": {"type": "string"},
                    "category": {"type": "string"},
                    "fact_id": {"type": "string"},
                    "importance": {"type": "number"},
                    "depth": {"type": "integer"}
                }),
                vec!["action"],
            ),
        )),
        "transcribe_audio" => Some((
            "Transcribe an attached or local audio file.".to_string(),
            object_schema(
                serde_json::json!({
                    "file_path": {"type": "string"},
                    "language": {"type": "string"},
                    "prompt": {"type": "string"}
                }),
                vec!["file_path"],
            ),
        )),
        "text_to_speech" => Some((
            "Generate a speech audio file from text.".to_string(),
            object_schema(
                serde_json::json!({
                    "text": {"type": "string"},
                    "voice": {"type": "string"},
                    "model": {"type": "string"},
                    "speed": {"type": "number"},
                    "output_filename": {"type": "string"}
                }),
                vec!["text"],
            ),
        )),
        "generate_image" => Some((
            "Generate or edit an image when explicitly requested.".to_string(),
            object_schema(
                serde_json::json!({
                    "prompt": {"type": "string"},
                    "size": {"type": "string"},
                    "output_filename": {"type": "string"},
                    "input_image": {"type": "string"},
                    "mask_image": {"type": "string"}
                }),
                vec!["prompt"],
            ),
        )),
        _ => None,
    };

    if let Some((description, parameters)) = compact {
        tool.description = description;
        tool.parameters = parameters;
        tool.parameters_ts = None;
        tool.usage_guidelines = None;
    }

    tool
}
