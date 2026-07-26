use futures::stream;
use futures::StreamExt;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::agent::evolution::evolution_manager::EvolutionManager;
use crate::agent::governance::{GovernanceContext, GovernanceScope};
use crate::agent::memory::BackgroundEnvelope;
use crate::agent::message::{Content, ContentPart, Message, Role};
use crate::agent::protocol::*;
use crate::agent::session::SessionStatus;
use crate::agent::tactical::PostQuantumGuard;
use crate::error::{Error, Result};
use crate::hooks::{
    HookEngine, HookEvent, HookResult, HookTiming, RuntimeHookCapture, RuntimeHookRefs,
};
use crate::skills::tool::{
    capability_route_requires_real_tool_call, classify_query_capability_route,
    query_requests_document_understanding, SafetyLevel, ToolSet,
};
use crate::skills::RuntimeSecurityContext;
use benshu_hardness::{
    classify_failure, decide_execution_tool_reply_requirement,
    should_append_reflexion_recovery_prompt, ExecutionToolReplyRequirementInput,
};
use benshu_infra::traits::resource::ResourceSensor;
use benshu_loop_guard::LoopGuardAction;

const APPROVAL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300); // 5 minutes timeout for human approval
const LARGE_TOOL_SCHEMA_CHAR_THRESHOLD: usize = 1800;
const LARGE_TOOL_SCHEMA_MAX_LINES: usize = 18;
const LARGE_TOOL_SCHEMA_MAX_PROPERTIES: usize = 12;
const COORDINATION_TOOL_ARG_SECURITY_LIMIT: usize = 7_600;
const COORDINATION_TOOL_TEXT_FIELD_LIMIT: usize = 4_800;
const BROWSER_TOOL_ARG_SECURITY_LIMIT: usize = 7_600;
const BROWSER_RUNTIME_CONTEXT_FIELD_LIMIT: usize = 1_200;
const BROWSER_TEXT_FIELD_LIMIT: usize = 2_400;
const RUNTIME_TASK_CONTEXT_ARG_KEY: &str = "_benshu_task_context";
const SIMPLE_REALTIME_TOOL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

#[derive(Debug, Clone)]
struct EffectiveToolPolicyDecision {
    policy: ToolPolicy,
    reasons: Vec<String>,
}

fn is_coordination_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "delegate" | "handover" | "decomposition" | "multi_agent_audit"
    )
}

fn is_simple_realtime_lookup_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "weather_lookup" | "fx_lookup" | "price_lookup" | "latest_info_lookup"
    )
}

fn tool_timeout_for(
    tool_name: &str,
    default_timeout: std::time::Duration,
) -> Option<std::time::Duration> {
    if is_coordination_tool(tool_name) {
        None
    } else if is_simple_realtime_lookup_tool(tool_name) {
        Some(default_timeout.min(SIMPLE_REALTIME_TOOL_TIMEOUT))
    } else {
        Some(default_timeout)
    }
}

fn compress_executor_tool_output(
    output: &str,
    max_chars: usize,
) -> benshu_compression::ToolOutputCompression {
    benshu_compression::compress_tool_output(output, max_chars)
}

fn prehook_tool_output_for_context(
    tool_name: &str,
    output: &str,
    max_chars: usize,
) -> benshu_compression::ToolOutputCompression {
    if is_simple_realtime_lookup_tool(tool_name) {
        return benshu_compression::ToolOutputCompression {
            content: output.to_string(),
            original_chars: output.chars().count(),
            output_chars: output.chars().count(),
            omitted_chars: 0,
            truncated: false,
        };
    }

    compress_executor_tool_output(output, max_chars)
}

fn compact_argument_text(text: &str, max_chars: usize) -> String {
    let char_count = text.chars().count();
    if char_count <= max_chars {
        return text.to_string();
    }

    let head_len = max_chars.saturating_mul(3) / 5;
    let tail_len = max_chars.saturating_sub(head_len).saturating_sub(96);
    let head = text.chars().take(head_len).collect::<String>();
    let tail = text
        .chars()
        .skip(char_count.saturating_sub(tail_len))
        .collect::<String>();
    format!(
        "{head}\n\n[... runtime compacted oversized tool argument; omitted {} chars ...]\n\n{tail}",
        char_count.saturating_sub(head_len + tail_len)
    )
}

fn attach_provider_media_metadata_from_capture(
    message: &mut Message,
    hook_capture: &RuntimeHookCapture,
) {
    const PROVIDER_MEDIA_NOTE_MAPPINGS: [(&str, &str); 10] = [
        (
            "after_llm:provider_media_preprocess_consumed_by:",
            "provider_media_preprocess_consumed_by",
        ),
        (
            "after_llm:provider_media_preprocess_consumption_routes:",
            "provider_media_preprocess_consumption_routes",
        ),
        (
            "after_llm:provider_media_preprocess_outcomes:",
            "provider_media_preprocess_outcomes",
        ),
        (
            "after_llm:provider_media_preprocess_preprocess_failed_routes:",
            "provider_media_preprocess_preprocess_failed_routes",
        ),
        (
            "after_llm:provider_media_preprocess_model_failed_routes:",
            "provider_media_preprocess_model_failed_routes",
        ),
        (
            "after_llm:provider_media_preprocess_result_insufficient_routes:",
            "provider_media_preprocess_result_insufficient_routes",
        ),
        (
            "after_llm:provider_media_preprocess_followup_strategies:",
            "provider_media_preprocess_followup_strategies",
        ),
        (
            "after_llm:provider_media_preprocess_attachment_fallback_routes:",
            "provider_media_preprocess_attachment_fallback_routes",
        ),
        (
            "after_llm:provider_media_preprocess_alternate_model_fallback_routes:",
            "provider_media_preprocess_alternate_model_fallback_routes",
        ),
        (
            "after_llm:provider_media_preprocess_clarification_routes:",
            "provider_media_preprocess_clarification_routes",
        ),
    ];

    for note in &hook_capture.notes {
        for (prefix, metadata_key) in PROVIDER_MEDIA_NOTE_MAPPINGS {
            if let Some(value) = note.strip_prefix(prefix) {
                let trimmed = value.trim();
                if !trimmed.is_empty() {
                    message
                        .metadata
                        .insert(metadata_key.to_string(), trimmed.to_string());
                }
            }
        }
    }
}

fn query_requires_execution_tool_reply(messages: &[Message]) -> bool {
    let last_user_message = messages
        .iter()
        .rev()
        .find(|message| matches!(message.role, Role::User))
        .cloned();

    let Some(last_user_message) = last_user_message else {
        return false;
    };

    let has_media_input = matches!(
        &last_user_message.content,
        Content::Parts(parts)
            if parts.iter().any(|part| matches!(
                part,
                crate::agent::message::ContentPart::Image { .. }
                    | crate::agent::message::ContentPart::Audio { .. }
                    | crate::agent::message::ContentPart::Video { .. }
            ))
    );

    let user_text = last_user_message.content.as_text();
    let normalized = user_text.trim();
    decide_execution_tool_reply_requirement(ExecutionToolReplyRequirementInput {
        has_media_input,
        normalized_text_is_empty: normalized.is_empty(),
        document_understanding_turn: query_requests_document_understanding(normalized),
        capability_route_requires_real_tool_call: classify_query_capability_route(normalized)
            .is_some_and(capability_route_requires_real_tool_call),
    })
}

fn latest_user_input(messages: &[Message]) -> Option<String> {
    messages
        .iter()
        .rev()
        .find(|message| matches!(message.role, Role::User))
        .map(|message| message.text())
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

fn extract_retrieval_degradation_reason(tool_name: &str, output: &str) -> Option<String> {
    if tool_name != "knowledge_search" {
        return None;
    }

    output
        .lines()
        .find_map(|line| line.strip_prefix("Retrieval Degradation: "))
        .map(str::trim)
        .filter(|summary| !summary.is_empty())
        .map(|summary| {
            format!(
                "retrieval:{}",
                summary
                    .split(',')
                    .map(str::trim)
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join("|")
            )
        })
}

fn schema_text_is_large(text: &str) -> bool {
    text.chars().count() > LARGE_TOOL_SCHEMA_CHAR_THRESHOLD
}

fn summarize_typescript_schema(ts: &str) -> String {
    let mut lines = Vec::new();
    let mut hidden_lines = 0usize;

    for line in ts.lines() {
        if lines.len() < LARGE_TOOL_SCHEMA_MAX_LINES {
            lines.push(line);
        } else {
            hidden_lines += 1;
        }
    }

    let mut summary = lines.join("\n");
    if hidden_lines > 0 {
        summary.push_str(&format!(
            "\n// ... {} more schema lines omitted for compact first-use discovery",
            hidden_lines
        ));
    }
    summary
}

fn summarize_json_schema(schema: &serde_json::Value) -> String {
    let pretty = serde_json::to_string_pretty(schema).unwrap_or_default();
    if !schema_text_is_large(&pretty) {
        return pretty;
    }

    let mut summary = serde_json::Map::new();
    if let Some(schema_type) = schema.get("type") {
        summary.insert("type".to_string(), schema_type.clone());
    }
    if let Some(required) = schema.get("required") {
        summary.insert("required".to_string(), required.clone());
    }
    if let Some(properties) = schema.get("properties").and_then(|v| v.as_object()) {
        let mut compact_properties = serde_json::Map::new();
        let mut omitted = 0usize;
        for (index, (name, value)) in properties.iter().enumerate() {
            if index < LARGE_TOOL_SCHEMA_MAX_PROPERTIES {
                let mut property_summary = serde_json::Map::new();
                if let Some(property_type) = value.get("type") {
                    property_summary.insert("type".to_string(), property_type.clone());
                }
                if let Some(description) = value.get("description") {
                    property_summary.insert("description".to_string(), description.clone());
                }
                if let Some(items) = value.get("items").and_then(|item| item.get("type")) {
                    property_summary.insert("items".to_string(), items.clone());
                }
                if let Some(enum_values) = value.get("enum") {
                    property_summary.insert("enum".to_string(), enum_values.clone());
                }
                compact_properties
                    .insert(name.clone(), serde_json::Value::Object(property_summary));
            } else {
                omitted += 1;
            }
        }
        summary.insert(
            "properties".to_string(),
            serde_json::Value::Object(compact_properties),
        );
        if omitted > 0 {
            summary.insert(
                "compact_notice".to_string(),
                serde_json::Value::String(format!(
                    "{} additional properties omitted for compact first-use discovery",
                    omitted
                )),
            );
        }
    }

    serde_json::to_string_pretty(&serde_json::Value::Object(summary)).unwrap_or(pretty)
}

fn generate_tool_schema_injection(def: &crate::skills::tool::ToolDefinition) -> String {
    if let Some(ts) = &def.parameters_ts {
        let large = schema_text_is_large(ts);
        let header = if large {
            "#### Compact TypeScript Schema Summary (large schema)\n"
        } else {
            "#### Official TypeScript Schema:\n"
        };
        let body = if large {
            summarize_typescript_schema(ts)
        } else {
            ts.clone()
        };
        let mut section = String::from(header);
        section.push_str("```typescript\n");
        section.push_str(&body);
        section.push_str("\n```\n");
        if large {
            section.push_str(
                "This tool has a large schema, so only a compact first-use summary is shown.\n",
            );
        }
        return section;
    }

    let pretty = serde_json::to_string_pretty(&def.parameters).unwrap_or_default();
    let large = schema_text_is_large(&pretty);
    let mut section = if large {
        String::from("#### Compact JSON Schema Summary (large schema)\n")
    } else {
        String::from("#### Parameters (JSON Schema):\n")
    };
    section.push_str("```json\n");
    section.push_str(&summarize_json_schema(&def.parameters));
    section.push_str("\n```\n");
    if large {
        section.push_str(
            "This tool has a large schema, so only top-level fields are shown during first-use discovery.\n",
        );
    }
    section
}

/// Responsible for executing tool calls with safety and policy checks
#[derive(Clone)]
pub struct ActionExecutor {
    pub tools: ToolSet,
    pub config: ExecutorConfig,
    pub events: broadcast::Sender<AgentEvent>,
    pub governance: Arc<GovernanceContext>,
    pub evolution_manager: Option<Arc<EvolutionManager>>,
    pub session_id: Option<String>,
    pub(crate) cancel_token: Arc<parking_lot::RwLock<CancellationToken>>,
    pub seen_tools: Arc<parking_lot::RwLock<HashSet<String>>>,
    pub memory: Option<Arc<dyn crate::agent::memory::Memory>>,
    pub background_envelope: Arc<parking_lot::RwLock<Option<BackgroundEnvelope>>>,
    pub sensor: Option<Arc<parking_lot::RwLock<crate::infra::CapabilitySensor>>>,
    pub intervention: crate::agent::intervention::InterventionManager,
    pub metrics: Option<Arc<crate::infra::observable::MetricsRegistry>>,
    pub hook_engine: Arc<HookEngine>,
    pub runtime_hook_refs: Arc<parking_lot::RwLock<Option<RuntimeHookRefs>>>,
    pub runtime_hook_capture: Arc<parking_lot::RwLock<RuntimeHookCapture>>,
}

impl ActionExecutor {
    fn canonical_tool_call_block(
        id: &str,
        name: &str,
        args: &serde_json::Value,
    ) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "name": name,
            "arguments": args,
        })
    }

    fn build_tool_call_replay_receipts(
        assistant_text: &str,
        tool_calls: &[(String, String, serde_json::Value)],
    ) -> HashMap<String, ToolCallReplayReceipt> {
        let sampled_text = assistant_text.trim();
        let has_sampled_tool_text =
            sampled_text.contains("<|tool_call>") || sampled_text.contains("<tool_call|>");
        let can_replay_sampled_block_exactly = has_sampled_tool_text && tool_calls.len() == 1;

        tool_calls
            .iter()
            .map(|(id, name, args)| {
                let normalized_block = Self::canonical_tool_call_block(id, name, args);
                let normalized_block_text = normalized_block.to_string();
                let (replay_mode, sampled_call_ref, sampled_source) =
                    if can_replay_sampled_block_exactly {
                        (
                            "sampled_text_exact".to_string(),
                            format!("message://assistant/text/tool_call/{id}"),
                            sampled_text.to_string(),
                        )
                    } else {
                        (
                            "canonical_fallback".to_string(),
                            format!("message://assistant/content_part/tool_call/{id}"),
                            normalized_block_text.clone(),
                        )
                    };
                (
                    id.clone(),
                    ToolCallReplayReceipt {
                        tool_call_id: id.clone(),
                        replay_mode,
                        sampled_call_block: Some(sampled_source.clone()),
                        sampled_call_fingerprint: Self::runtime_fingerprint(&sampled_source),
                        sampled_call_ref,
                        normalized_call_fingerprint: Self::runtime_fingerprint(
                            &normalized_block_text,
                        ),
                    },
                )
            })
            .collect()
    }

    fn repair_tool_calls_from_runtime_receipts(
        &self,
        tool_calls: Vec<(String, String, serde_json::Value)>,
        messages: &[Message],
    ) -> Vec<(String, String, serde_json::Value)> {
        tool_calls
            .into_iter()
            .map(|(id, name, mut args)| {
                let mut name = name;
                let mut canonical = self.canonical_tool_call_name(&name);
                if let Some(promoted_name) =
                    self.promote_bare_invocation_content_tool_call(&canonical, &mut args)
                {
                    name = promoted_name;
                    canonical = self.canonical_tool_call_name(&name);
                }
                if let Some(promoted_name) =
                    self.promote_nested_action_tool_call(&canonical, &mut args)
                {
                    name = promoted_name;
                    canonical = self.canonical_tool_call_name(&name);
                }
                if canonical == "fetch_document" {
                    Self::repair_fetch_document_args_from_import_receipt(&mut args, messages);
                }
                (id, name, args)
            })
            .collect()
    }

    fn promote_nested_action_tool_call(
        &self,
        current_tool_name: &str,
        args: &mut serde_json::Value,
    ) -> Option<String> {
        Self::promote_nested_action_tool_call_for_tools(&self.tools, current_tool_name, args)
    }

    fn promote_bare_invocation_content_tool_call(
        &self,
        current_tool_name: &str,
        args: &mut serde_json::Value,
    ) -> Option<String> {
        Self::promote_bare_invocation_content_tool_call_for_tools(
            &self.tools,
            current_tool_name,
            args,
        )
    }

    fn promote_bare_invocation_content_tool_call_for_tools(
        tools: &ToolSet,
        current_tool_name: &str,
        args: &mut serde_json::Value,
    ) -> Option<String> {
        let content = args
            .get("content")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())?;
        let (candidate, promoted_args) = Self::parse_bare_tool_invocation(content)?;
        let promoted = Self::canonical_tool_call_name_for_tools(tools, &candidate);
        if promoted == current_tool_name || !tools.contains(&promoted) {
            return None;
        }
        *args = promoted_args;
        Some(promoted)
    }

    fn promote_nested_action_tool_call_for_tools(
        tools: &ToolSet,
        current_tool_name: &str,
        args: &mut serde_json::Value,
    ) -> Option<String> {
        let action = args
            .get("action")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())?;
        let promoted = Self::canonical_tool_call_name_for_tools(tools, action);
        if promoted == current_tool_name || !tools.contains(&promoted) {
            return None;
        }
        if let Some(object) = args.as_object_mut() {
            object.remove("action");
        }
        Some(promoted)
    }

    fn parse_bare_tool_invocation(text: &str) -> Option<(String, serde_json::Value)> {
        let trimmed = text.trim();
        let open = trimmed.find('(')?;
        if !trimmed.ends_with(')') {
            return None;
        }
        let name = trimmed[..open].trim();
        if name.is_empty()
            || !name
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.')
        {
            return None;
        }
        let inner = &trimmed[open + 1..trimmed.len().saturating_sub(1)];
        let mut args = serde_json::Map::new();
        for part in Self::split_invocation_args(inner)? {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let eq = Self::find_unquoted_char(part, '=')?;
            let key = part[..eq].trim();
            if key.is_empty()
                || !key
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
            {
                return None;
            }
            let value = Self::parse_invocation_value(part[eq + 1..].trim())?;
            args.insert(key.to_string(), value);
        }
        Some((name.to_string(), serde_json::Value::Object(args)))
    }

    fn split_invocation_args(text: &str) -> Option<Vec<String>> {
        let mut parts = Vec::new();
        let mut current = String::new();
        let mut quote: Option<char> = None;
        let mut escaped = false;
        let mut depth = 0usize;
        for ch in text.chars() {
            if escaped {
                current.push(ch);
                escaped = false;
                continue;
            }
            if ch == '\\' {
                current.push(ch);
                escaped = true;
                continue;
            }
            if let Some(active_quote) = quote {
                current.push(ch);
                if ch == active_quote {
                    quote = None;
                }
                continue;
            }
            match ch {
                '\'' | '"' => {
                    quote = Some(ch);
                    current.push(ch);
                }
                '(' | '[' | '{' => {
                    depth = depth.saturating_add(1);
                    current.push(ch);
                }
                ')' | ']' | '}' => {
                    depth = depth.checked_sub(1)?;
                    current.push(ch);
                }
                ',' if depth == 0 => {
                    parts.push(current.trim().to_string());
                    current.clear();
                }
                _ => current.push(ch),
            }
        }
        if quote.is_some() || depth != 0 {
            return None;
        }
        if !current.trim().is_empty() {
            parts.push(current.trim().to_string());
        }
        Some(parts)
    }

    fn find_unquoted_char(text: &str, needle: char) -> Option<usize> {
        let mut quote: Option<char> = None;
        let mut escaped = false;
        for (index, ch) in text.char_indices() {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if let Some(active_quote) = quote {
                if ch == active_quote {
                    quote = None;
                }
                continue;
            }
            if ch == '\'' || ch == '"' {
                quote = Some(ch);
                continue;
            }
            if ch == needle {
                return Some(index);
            }
        }
        None
    }

    fn parse_invocation_value(text: &str) -> Option<serde_json::Value> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Some(serde_json::Value::String(String::new()));
        }
        let quoted = (trimmed.starts_with('\'') && trimmed.ends_with('\''))
            || (trimmed.starts_with('"') && trimmed.ends_with('"'));
        if quoted && trimmed.len() >= 2 {
            let inner = &trimmed[1..trimmed.len().saturating_sub(1)];
            return Some(serde_json::Value::String(Self::unescape_invocation_string(
                inner,
            )));
        }
        match trimmed {
            "true" => return Some(serde_json::Value::Bool(true)),
            "false" => return Some(serde_json::Value::Bool(false)),
            "null" | "None" => return Some(serde_json::Value::Null),
            _ => {}
        }
        if let Ok(value) = trimmed.parse::<i64>() {
            return Some(serde_json::Value::Number(value.into()));
        }
        if let Ok(value) = trimmed.parse::<f64>() {
            if let Some(number) = serde_json::Number::from_f64(value) {
                return Some(serde_json::Value::Number(number));
            }
        }
        Some(serde_json::Value::String(trimmed.to_string()))
    }

    fn unescape_invocation_string(text: &str) -> String {
        let mut out = String::new();
        let mut escaped = false;
        for ch in text.chars() {
            if escaped {
                out.push(match ch {
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    other => other,
                });
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else {
                out.push(ch);
            }
        }
        if escaped {
            out.push('\\');
        }
        out
    }

    fn repair_fetch_document_args_from_import_receipt(
        args: &mut serde_json::Value,
        messages: &[Message],
    ) {
        let has_collection = args
            .get("collection")
            .and_then(|value| value.as_str())
            .is_some_and(|value| !value.trim().is_empty());
        let has_path = args
            .get("path")
            .and_then(|value| value.as_str())
            .is_some_and(|value| !value.trim().is_empty());
        if has_collection && has_path {
            return;
        }

        let source_url = args
            .get("source_url")
            .or_else(|| args.get("url"))
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let locator = if let Some(source_url) = source_url {
            Self::latest_knowledge_import_locator_for_source_url(messages, source_url)
        } else {
            Self::latest_knowledge_import_locator(messages)
        };
        let Some((collection, path)) = locator else {
            return;
        };
        let Some(object) = args.as_object_mut() else {
            return;
        };
        object.insert(
            "collection".to_string(),
            serde_json::Value::String(collection),
        );
        object.insert("path".to_string(), serde_json::Value::String(path));
    }

    fn latest_knowledge_import_locator(messages: &[Message]) -> Option<(String, String)> {
        messages.iter().rev().find_map(|message| {
            let text = message.content.as_text();
            if !(text.contains("knowledge.imported")
                || text.contains("Imported web knowledge")
                || text.contains("Knowledge document created"))
            {
                return None;
            }
            Self::knowledge_locator_from_receipt_text(&text)
        })
    }

    fn latest_knowledge_import_locator_for_source_url(
        messages: &[Message],
        source_url: &str,
    ) -> Option<(String, String)> {
        messages.iter().rev().find_map(|message| {
            let text = message.content.as_text();
            if !text.contains(source_url)
                || !(text.contains("knowledge.imported")
                    || text.contains("Imported web knowledge")
                    || text.contains("Knowledge document created"))
            {
                return None;
            }
            Self::knowledge_locator_from_receipt_text(&text)
        })
    }

    fn knowledge_locator_from_receipt_text(text: &str) -> Option<(String, String)> {
        let collection = Self::receipt_line_value(text, "collection:")
            .or_else(|| Self::between(text, "collection '", "' at path"))
            .or_else(|| Self::between(text, "collection `", "` at path"))?;
        let path = Self::receipt_line_value(text, "path:")
            .or_else(|| Self::between(text, "at path '", "'"))
            .or_else(|| Self::between(text, "at path `", "`"))?;
        if collection.trim().is_empty() || path.trim().is_empty() {
            return None;
        }
        Some((collection, path))
    }

    fn receipt_line_value(text: &str, prefix: &str) -> Option<String> {
        text.lines().find_map(|line| {
            let trimmed = line.trim();
            let value = trimmed.strip_prefix(prefix)?.trim();
            let value = value.trim_matches(['`', '\'', '"']);
            (!value.is_empty()).then_some(value.to_string())
        })
    }

    fn between(text: &str, start: &str, end: &str) -> Option<String> {
        let (_, tail) = text.split_once(start)?;
        let (value, _) = tail.split_once(end)?;
        let value = value.trim().trim_matches(['`', '\'', '"']);
        (!value.is_empty()).then_some(value.to_string())
    }

    fn loop_guard_tool_name(tool_name: &str, args: &serde_json::Value) -> String {
        if tool_name == "delegate" {
            if let Some(role) = args.get("role").and_then(|value| value.as_str()) {
                let role = role.trim().to_ascii_lowercase();
                if !role.is_empty() {
                    return format!("delegate::{role}");
                }
            }
        }

        if let Some(action) = args.get("action").and_then(|value| value.as_str()) {
            let action = action.trim().to_ascii_lowercase();
            if !action.is_empty() {
                return format!("{tool_name}::{action}");
            }
        }

        tool_name.to_string()
    }

    fn normalized_dedupe_text(text: &str) -> String {
        text.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_ascii_lowercase()
    }

    fn same_turn_tool_call_dedupe_key(tool_name: &str, args: &serde_json::Value) -> String {
        if tool_name == "delegate" {
            let role = args
                .get("role")
                .and_then(|value| value.as_str())
                .map(Self::normalized_dedupe_text)
                .unwrap_or_default();
            let task = args
                .get("task")
                .and_then(|value| value.as_str())
                .map(Self::normalized_dedupe_text)
                .unwrap_or_else(|| args.to_string());
            return format!("delegate::{role}::{task}");
        }

        format!("{tool_name}::{}", args)
    }

    fn runtime_fingerprint(text: &str) -> String {
        format!("{:016x}", seahash::hash(text.as_bytes()))
    }

    fn build_tool_outcome_meta(
        status: &str,
        tool_name: &str,
        output: &str,
        full_artifact_ref: Option<String>,
    ) -> ToolOutcomeMeta {
        ToolOutcomeMeta {
            status: status.to_string(),
            kind: Self::tool_outcome_kind(tool_name).to_string(),
            error_class: Self::tool_outcome_error_class(status, output),
            preview_chars: Some(output.chars().count()),
            full_artifact_ref,
            evidence_count: Self::tool_outcome_evidence_count(output),
            progress_signal: Self::tool_outcome_has_progress_signal(output),
        }
    }

    async fn spill_full_tool_output_if_truncated(
        session_id: Option<&str>,
        tool_name: &str,
        raw_output: &str,
        truncated: bool,
    ) -> Option<String> {
        if !truncated || raw_output.is_empty() {
            return None;
        }

        let cwd = std::env::current_dir().ok()?;
        let mut dir = cwd.join("data").join("generated").join("tool-results");
        if let Some(session_id) = session_id
            .map(Self::sanitize_artifact_path_segment)
            .filter(|value| !value.is_empty())
        {
            dir = dir.join(session_id);
        }

        if tokio::fs::create_dir_all(&dir).await.is_err() {
            return None;
        }

        let tool = Self::sanitize_artifact_path_segment(tool_name);
        let filename = format!(
            "{}-{}.txt",
            if tool.is_empty() {
                "tool"
            } else {
                tool.as_str()
            },
            uuid::Uuid::new_v4()
        );
        let path = dir.join(filename);
        if tokio::fs::write(&path, raw_output).await.is_err() {
            return None;
        }
        Some(path.to_string_lossy().to_string())
    }

    fn sanitize_artifact_path_segment(value: &str) -> String {
        value
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                    ch
                } else {
                    '_'
                }
            })
            .collect::<String>()
            .trim_matches('_')
            .chars()
            .take(96)
            .collect()
    }

    fn tool_outcome_kind(tool_name: &str) -> &'static str {
        match tool_name {
            "web_search" | "web_fetch" | "browser_browse" | "realtime_lookup" => "retrieval_result",
            "knowledge_import" | "knowledge_manage" | "knowledge_search" => "knowledge_result",
            "delegate" | "handover" | "decomposition" | "multi_agent_audit" => {
                "coordination_result"
            }
            "pdf_builder" | "writing" | "filesystem.write_file" | "write_file" => "artifact_result",
            _ => "tool_result",
        }
    }

    fn tool_outcome_error_class(status: &str, output: &str) -> Option<String> {
        if !matches!(status, "failed" | "skipped" | "unavailable") {
            return None;
        }

        let lowered = output.to_ascii_lowercase();
        let class = if lowered.contains("timeout") || lowered.contains("timed out") {
            "timeout"
        } else if lowered.contains("not found") || lowered.contains("unavailable") {
            "unavailable"
        } else if lowered.contains("blocked") || lowered.contains("permission") {
            "policy_blocked"
        } else if lowered.contains("loop") || lowered.contains("duplicate") {
            "no_progress"
        } else {
            "execution_error"
        };
        Some(class.to_string())
    }

    fn tool_outcome_evidence_count(output: &str) -> Option<usize> {
        let value = serde_json::from_str::<serde_json::Value>(output).ok()?;
        let count = value
            .get("results")
            .and_then(|v| v.as_array())
            .map(Vec::len)
            .or_else(|| {
                value
                    .get("evidence_bundle")
                    .and_then(|bundle| bundle.get("candidates").or_else(|| bundle.get("results")))
                    .and_then(|v| v.as_array())
                    .map(Vec::len)
            })
            .or_else(|| value.get("links").and_then(|v| v.as_array()).map(Vec::len))?;
        Some(count)
    }

    fn tool_outcome_has_progress_signal(output: &str) -> bool {
        let lowered = output.to_ascii_lowercase();
        lowered.contains("checkpoint")
            || lowered.contains("artifact")
            || lowered.contains("saved")
            || lowered.contains("stored")
            || lowered.contains("imported")
            || lowered.contains("completed")
            || lowered.contains("写入")
            || lowered.contains("保存")
            || lowered.contains("导入")
            || lowered.contains("完成")
    }

    fn canonical_tool_call_name(&self, requested: &str) -> String {
        Self::canonical_tool_call_name_for_tools(&self.tools, requested)
    }

    fn canonical_tool_call_name_for_tools(tools: &ToolSet, requested: &str) -> String {
        let trimmed = requested.trim();
        if tools.contains(trimmed) {
            return trimmed.to_string();
        }

        let normalized = trimmed
            .trim_matches(|ch: char| ch == '`' || ch == '"' || ch == '\'')
            .to_ascii_lowercase()
            .replace(['-', ' ', ':'], "_");
        for (tool_name, _) in tools.iter() {
            let normalized_tool = tool_name.to_ascii_lowercase();
            if normalized == normalized_tool
                || normalized.starts_with(&format!("{normalized_tool}."))
                || normalized.starts_with(&format!("{normalized_tool}_"))
            {
                return tool_name;
            }
        }

        let aliased = match normalized.as_str() {
            "runtime_surface.command_exec"
            | "runtime.command_exec"
            | "terminal.command_exec"
            | "shell.command_exec"
            | "execute_command"
            | "run_command"
            | "command" => Some("command_exec"),
            "browser.browse" | "browser.open" | "browser.navigate" | "browse" => {
                Some("browser_browse")
            }
            "web.search" | "browser.search" | "search_web" => Some("web_search"),
            "web.fetch" | "browser.fetch" | "fetch_url" | "fetch_web" => Some("web_fetch"),
            "filesystem.read_file" | "fs.read_file" | "file.read" | "read_path" => {
                Some("read_file")
            }
            "filesystem.write_file" | "fs.write_file" | "file.write" => Some("write_file"),
            "filesystem.list_directory" | "fs.list_directory" | "list_files" => {
                Some("list_directory")
            }
            "image_gen" | "image.generate" | "image.generate_image" | "generate_image_tool" => {
                Some("generate_image")
            }
            "ocr.ocr" | "image.ocr" | "vision.ocr" => Some("text_extract"),
            "office.parse" | "document.office_parse" | "parse_office" => Some("office_parse"),
            "pdf.parse" | "document.pdf_parse" | "parse_pdf" => Some("pdf_parse"),
            "data.transform" | "transform_data" => Some("data_transform"),
            "chart.generate" | "chart.create" | "generate_chart" => Some("chart"),
            "repo.git_ops" | "git.status" | "git.diff" | "git.log" => Some("git_ops"),
            "voice.transcribe" | "voice.transcribe_audio" | "stt" | "speech_to_text" => {
                Some("transcribe_audio")
            }
            "voice.speak" | "voice.text_to_speech" | "tts" | "speak" => Some("text_to_speech"),
            "knowledge.search_knowledge"
            | "knowledge.search"
            | "knowledge.lookup"
            | "knowledge.recall"
            | "knowledge_search_knowledge"
            | "search_knowledge"
            | "search_memory"
            | "knowledge_lookup" => Some("tiered_search"),
            "knowledge.fetch_document" | "knowledge.fetch" | "knowledge_fetch_document" => {
                Some("fetch_document")
            }
            "knowledge.import_url" | "knowledge.import" | "knowledge_import" => {
                Some("knowledge_import_url")
            }
            "knowledge.manage" | "knowledge.update" | "knowledge.delete" | "knowledge.remove"
            | "knowledge_manage" | "knowledge_update" | "update_knowledge" | "knowledge_delete"
            | "delete_knowledge" | "remove_knowledge" => Some("knowledge_manage_document"),
            _ => None,
        };

        if let Some(candidate) = aliased {
            if tools.contains(candidate) {
                return candidate.to_string();
            }
        }

        if let Some(suffix) = normalized.rsplit('.').next() {
            if suffix != normalized && tools.contains(suffix) {
                return suffix.to_string();
            }
        }

        trimmed.to_string()
    }

    fn normalize_tool_call_args(
        tool_name: &str,
        args: serde_json::Value,
        task_context: Option<&str>,
    ) -> serde_json::Value {
        if tool_name != "browser_browse" && tool_name != "browser" {
            return args;
        }

        let Some(object) = args.as_object() else {
            return args;
        };

        let mut normalized = object.clone();
        let has_action = normalized
            .get("action")
            .and_then(|value| value.as_str())
            .is_some_and(|value| !value.trim().is_empty());
        let url = normalized
            .get("url")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let text = normalized
            .get("text")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let query = normalized
            .get("query")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);

        if !has_action {
            if url.is_some() {
                normalized.insert(
                    "action".to_string(),
                    serde_json::Value::String("navigate".to_string()),
                );
            } else if let Some(query_text) = query.clone().or(text.clone()) {
                normalized.insert(
                    "action".to_string(),
                    serde_json::Value::String("search".to_string()),
                );
                normalized
                    .entry("text".to_string())
                    .or_insert_with(|| serde_json::Value::String(query_text));
            }
        }

        if let Some(task_context) = task_context
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            normalized
                .entry(RUNTIME_TASK_CONTEXT_ARG_KEY.to_string())
                .or_insert_with(|| serde_json::Value::String(task_context.to_string()));
        }

        if normalized
            .get("action")
            .and_then(|value| value.as_str())
            .is_some_and(|value| value == "navigate")
            && url.is_none()
        {
            if let Some(text_url) = text.filter(|value| value.starts_with("http")) {
                normalized.insert("url".to_string(), serde_json::Value::String(text_url));
            }
        }

        serde_json::Value::Object(normalized)
    }

    fn compact_tool_args_for_security_boundary(
        tool_name: &str,
        args: serde_json::Value,
    ) -> serde_json::Value {
        if tool_name == "browser_browse" || tool_name == "browser" {
            return Self::compact_browser_tool_args_for_security_boundary(args);
        }

        if !is_coordination_tool(tool_name)
            || args.to_string().chars().count() <= COORDINATION_TOOL_ARG_SECURITY_LIMIT
        {
            return args;
        }

        let Some(object) = args.as_object() else {
            return args;
        };

        let mut compacted = object.clone();
        for key in ["task", "full_user_request", "message", "content", "input"] {
            let Some(value) = compacted.get_mut(key) else {
                continue;
            };
            let Some(text) = value.as_str() else {
                continue;
            };
            *value = serde_json::Value::String(compact_argument_text(
                text,
                COORDINATION_TOOL_TEXT_FIELD_LIMIT,
            ));
        }

        let mut value = serde_json::Value::Object(compacted);
        if value.to_string().chars().count() <= COORDINATION_TOOL_ARG_SECURITY_LIMIT {
            return value;
        }

        if let Some(object) = value.as_object_mut() {
            for key in ["task", "full_user_request", "message", "content", "input"] {
                let Some(field) = object.get_mut(key) else {
                    continue;
                };
                let Some(text) = field.as_str() else {
                    continue;
                };
                *field = serde_json::Value::String(compact_argument_text(text, 2_400));
                if serde_json::Value::Object(object.clone())
                    .to_string()
                    .chars()
                    .count()
                    <= COORDINATION_TOOL_ARG_SECURITY_LIMIT
                {
                    break;
                }
            }
        }

        value
    }

    fn compact_browser_tool_args_for_security_boundary(
        args: serde_json::Value,
    ) -> serde_json::Value {
        let Some(object) = args.as_object() else {
            return args;
        };

        let mut compacted = object.clone();
        if let Some(value) = compacted.get_mut(RUNTIME_TASK_CONTEXT_ARG_KEY) {
            if let Some(text) = value.as_str() {
                *value = serde_json::Value::String(compact_argument_text(
                    text,
                    BROWSER_RUNTIME_CONTEXT_FIELD_LIMIT,
                ));
            }
        }

        let mut value = serde_json::Value::Object(compacted);
        if value.to_string().chars().count() <= BROWSER_TOOL_ARG_SECURITY_LIMIT {
            return value;
        }

        if let Some(object) = value.as_object_mut() {
            for key in ["text", "query", "selector", "script"] {
                let Some(field) = object.get_mut(key) else {
                    continue;
                };
                let Some(text) = field.as_str() else {
                    continue;
                };
                *field = serde_json::Value::String(compact_argument_text(
                    text,
                    BROWSER_TEXT_FIELD_LIMIT,
                ));
            }
        }

        value
    }

    fn latest_matching_tool_result(
        messages: &[Message],
        tool_name: &str,
        args_str: &str,
    ) -> Option<String> {
        messages.iter().rev().find_map(|message| {
            if message.role != Role::Tool {
                return None;
            }
            if !message
                .metadata
                .get("tool_name")
                .is_some_and(|name| name == tool_name)
            {
                return None;
            }
            if message
                .metadata
                .get("tool_error")
                .is_some_and(|value| value == "true")
            {
                return None;
            }
            if !message
                .metadata
                .get("tool_args")
                .is_some_and(|existing_args| existing_args == args_str)
            {
                return None;
            }

            match &message.content {
                Content::Parts(parts) => parts.iter().find_map(|part| {
                    let ContentPart::ToolResult { content, .. } = part else {
                        return None;
                    };
                    Some(content.clone())
                }),
                _ => Some(message.text()),
            }
        })
    }

    fn current_runtime_security_context(&self) -> RuntimeSecurityContext {
        let refs = self.runtime_hook_refs.read().clone();
        RuntimeSecurityContext {
            trace_id: refs.as_ref().and_then(|r| r.run_id.clone()),
            run_id: refs.as_ref().and_then(|r| r.run_id.clone()),
            task_id: refs.as_ref().and_then(|r| r.task_id.clone()),
            session_id: refs.and_then(|r| r.session_id),
        }
    }

    pub fn new(
        config: ExecutorConfig,
        tools: ToolSet,
        events: broadcast::Sender<AgentEvent>,
        governance: Arc<GovernanceContext>,
        evolution_manager: Option<Arc<EvolutionManager>>,
        session_id: Option<String>,
        cancel_token: Arc<parking_lot::RwLock<CancellationToken>>,
        seen_tools: Arc<parking_lot::RwLock<HashSet<String>>>,
        memory: Option<Arc<dyn crate::agent::memory::Memory>>,
        background_envelope: Arc<parking_lot::RwLock<Option<BackgroundEnvelope>>>,
        sensor: Option<Arc<parking_lot::RwLock<crate::infra::CapabilitySensor>>>,
        intervention: crate::agent::intervention::InterventionManager,
        metrics: Option<Arc<crate::infra::observable::MetricsRegistry>>,
        hook_engine: Arc<HookEngine>,
        runtime_hook_refs: Arc<parking_lot::RwLock<Option<RuntimeHookRefs>>>,
        runtime_hook_capture: Arc<parking_lot::RwLock<RuntimeHookCapture>>,
    ) -> Result<Self> {
        config.validate().map_err(Error::AgentConfig)?;

        Ok(Self {
            tools,
            config,
            events,
            governance,
            evolution_manager,
            session_id,
            cancel_token,
            seen_tools,
            memory,
            background_envelope,
            sensor,
            intervention,
            metrics,
            hook_engine,
            runtime_hook_refs,
            runtime_hook_capture,
        })
    }
    /// Resolve the effective policy for a tool, considering overrides and safety levels
    async fn resolve_effective_policy(
        &self,
        name: &str,
        def: &crate::skills::tool::ToolDefinition,
    ) -> ToolPolicy {
        let decision = self.resolve_executor_tool_policy(name, def);
        for reason in &decision.reasons {
            match reason.as_str() {
                "unverified_binary_requires_approval" => {
                    warn!(tool = %name, "Unverified binary skill detected. Enforcing manual approval.");
                }
                "red_safety_level_requires_approval" => {
                    info!(tool = %name, "Red safety level tool detected. Shifting from Auto to RequiresApproval.");
                }
                value if value.starts_with("inherited_risk_score_requires_approval:") => {
                    info!(tool = %name, "High runtime risk score - upgrading to RequiresApproval");
                }
                _ => {}
            }
        }
        decision.policy
    }

    fn resolve_executor_tool_policy(
        &self,
        name: &str,
        def: &crate::skills::tool::ToolDefinition,
    ) -> EffectiveToolPolicyDecision {
        let mut policy = self
            .config
            .tool_policy
            .overrides
            .get(name)
            .unwrap_or(&self.config.tool_policy.default_policy)
            .clone();
        let mut reasons = vec![format!("configured:{policy:?}")];

        if def.is_binary && !def.is_verified && policy != ToolPolicy::Disabled {
            policy = ToolPolicy::RequiresApproval;
            reasons.push("unverified_binary_requires_approval".to_string());
        }

        if def.safety_level == SafetyLevel::Red && policy == ToolPolicy::Auto {
            policy = ToolPolicy::RequiresApproval;
            reasons.push("red_safety_level_requires_approval".to_string());
        }

        if policy == ToolPolicy::Auto
            && self.config.inherited_risk_score > constants::HIGH_RISK_THRESHOLD
        {
            policy = ToolPolicy::RequiresApproval;
            reasons.push(format!(
                "inherited_risk_score_requires_approval:{:.3}",
                self.config.inherited_risk_score
            ));
        }

        EffectiveToolPolicyDecision { policy, reasons }
    }

    /// Send an agent event to the broadcast channel
    fn emit(&self, data: AgentEventData) {
        let _ = self.events.send(AgentEvent {
            session_id: self.session_id.clone(),
            data,
        });
    }

    fn build_hook_event(&self, timing: HookTiming) -> HookEvent {
        let mut event = HookEvent::new(timing);
        if let Some(refs) = self.runtime_hook_refs.read().clone() {
            let mut capture = self.runtime_hook_capture.write();
            capture.trace_injection_count = capture.trace_injection_count.saturating_add(1);
            if let Some(run_id) = refs.run_id {
                event.metadata.insert("run_id".to_string(), run_id);
            }
            if let Some(task_id) = refs.task_id {
                event.metadata.insert("task_id".to_string(), task_id);
            }
            if let Some(thread_id) = refs.thread_id {
                event.metadata.insert("thread_id".to_string(), thread_id);
            }
            if let Some(session_id) = refs.session_id {
                event.metadata.insert("session_id".to_string(), session_id);
            }
        }
        event
    }

    async fn apply_tool_error_hook(
        &self,
        tool_name: &str,
        args_str: &str,
        error_message: String,
    ) -> String {
        let mut error_hook = self
            .build_hook_event(HookTiming::OnError)
            .with_tool(tool_name.to_string(), args_str.to_string())
            .with_error(error_message.clone());
        error_hook.metadata.insert(
            "degradation_reason".to_string(),
            "tool_execution_failed".to_string(),
        );

        match self.hook_engine.fire(&error_hook).await {
            HookResult::Modify(modified_error) => modified_error,
            HookResult::Abort(reason) => format!(
                "Runtime tool error in `{}`: middleware aborted error handling: {}",
                tool_name, reason
            ),
            HookResult::Continue | HookResult::Skip => error_message,
        }
    }

    fn enqueue_tool_failure_analysis_if_enabled(
        &self,
        _tool_name: &str,
        _args_str: &str,
        _normalized_error: &str,
        _msgs: &[Message],
    ) {
        // Automatic evolution/failure mining is intentionally disabled on the
        // main execution path. Runtime failures remain visible through traces,
        // receipts, and session checkpoints without feeding a self-learning loop.
    }

    /// Generate first-use injection content for a tool
    fn generate_first_use_injection(
        &self,
        name: &str,
        def: &crate::skills::tool::ToolDefinition,
    ) -> String {
        let mut injection = format!("### NOTICE: First use of skill '{}'.\n", name);
        injection.push_str(&generate_tool_schema_injection(def));

        if let Some(guidelines) = &def.usage_guidelines {
            injection.push_str(&format!("#### Usage Guidelines:\n{}\n", guidelines));
        }
        injection
    }

    /// Save current state to persistent storage
    pub async fn checkpoint(
        &self,
        messages: &[Message],
        step: usize,
        status: SessionStatus,
    ) -> Result<()> {
        if let (Some(memory), Some(session_id)) = (&self.memory, &self.session_id) {
            let mut messages = messages.to_vec();

            let is_observing = self
                .evolution_manager
                .as_ref()
                .map(|em| em.observation_window().read().is_active())
                .unwrap_or(false);

            if is_observing {
                for msg in &mut messages {
                    msg.unverified = true;
                }
            }

            let executed_tools = self.seen_tools.read().iter().cloned().collect();

            let session = crate::agent::session::AgentSession {
                id: session_id.clone(),
                messages,
                step,
                status,
                updated_at: chrono::Utc::now(),
                is_distilled: false,
                hardened_skills: Vec::new(),
                agent_role: None,
                max_steps: 10,
                executed_tools,
                lifecycle: crate::agent::session::SessionLifecycle::default(),
                background_envelope: self.background_envelope.read().clone(),
            };
            memory.store_session(session).await?;
        }
        Ok(())
    }

    /// Execute a set of tool calls in parallel with policy and security checks
    /// Execute a set of tool calls and update the conversation history (Coresp. to coordinate_actions)
    pub async fn coordinate(
        &self,
        messages: &mut Vec<Message>,
        full_text: String,
        tool_calls: Vec<(String, String, serde_json::Value)>,
        steps: usize,
        history: &mut crate::agent::history::QueryHistory,
        tool_trace: &mut Vec<ToolCallData>,
    ) -> Result<()> {
        let tool_calls = self.repair_tool_calls_from_runtime_receipts(tool_calls, messages);
        let replay_receipts = Self::build_tool_call_replay_receipts(&full_text, &tool_calls);

        // 1. Append Assistant Message (Thought + Calls) to history
        let mut parts = Vec::new();
        if !full_text.is_empty() {
            parts.push(crate::agent::message::ContentPart::Text { text: full_text });
        }
        for (id, name, args) in &tool_calls {
            parts.push(crate::agent::message::ContentPart::ToolCall {
                id: id.clone(),
                name: name.clone(),
                arguments: args.clone(),
            });
        }
        let mut assistant_message = Message::assistant(Content::Parts(parts));
        if !replay_receipts.is_empty() {
            if let Ok(serialized) = serde_json::to_string(&replay_receipts) {
                assistant_message
                    .metadata
                    .insert("tool_replay_receipts".to_string(), serialized);
            }
        }
        let hook_capture = self.runtime_hook_capture.read().clone();
        attach_provider_media_metadata_from_capture(&mut assistant_message, &hook_capture);
        messages.push(assistant_message);

        // 2. Execute Tools (Parallel with Limit)
        let mut results = self
            .execute(tool_calls, replay_receipts, messages, steps, history)
            .await?;

        // 3. Append Tool Results to history
        for res in results.drain(..) {
            tool_trace.push(res);
        }

        // Phase 18: Hot-Interjection Persistence - Save checkpoint after each action block
        self.checkpoint(messages, steps, SessionStatus::Thinking)
            .await?;

        Ok(())
    }

    /// Core execution of tool calls
    pub async fn execute(
        &self,
        tool_calls: Vec<(String, String, serde_json::Value)>, // (id, name, args)
        replay_receipts: HashMap<String, ToolCallReplayReceipt>,
        messages: &mut Vec<Message>,
        steps: usize,
        history: &mut crate::agent::history::QueryHistory,
    ) -> Result<Vec<ToolCallData>> {
        // 0. Configuration Validation
        if self.config.max_parallel_tools == 0 {
            return Err(Error::agent_config("max_parallel_tools must be at least 1"));
        }
        if self.config.max_tool_output_chars == 0 {
            return Err(Error::agent_config(
                "max_tool_output_chars must be at least 100",
            ));
        }
        if self.config.loop_similarity_threshold < 0.0
            || self.config.loop_similarity_threshold > 1.0
        {
            return Err(Error::agent_config(
                "loop_similarity_threshold must be between 0.0 and 1.0",
            ));
        }

        let threshold = self.config.loop_similarity_threshold;
        let mut processed_calls = Vec::new();
        let mut skipped_duplicate_calls = Vec::new();
        let mut unavailable_tool_calls = Vec::new();
        let mut same_turn_seen = HashSet::new();

        // 1. Loop Detection
        for (id, name, args) in tool_calls {
            let name = self.canonical_tool_call_name(&name);
            let mut args = args;
            let mut args_str = args.to_string();
            let mut loop_guard_name = Self::loop_guard_tool_name(&name, &args);
            if !self.tools.contains(&name) {
                let mut available_tools = self
                    .tools
                    .iter()
                    .into_iter()
                    .map(|(tool_name, _)| tool_name)
                    .collect::<Vec<_>>();
                available_tools.sort();
                let available = if available_tools.is_empty() {
                    "none".to_string()
                } else {
                    available_tools.join(", ")
                };
                let error = format!(
                    "Runtime tool error in `{}`: tool is not equipped for this agent. Available tools right now: {}.",
                    name, available
                );
                let mut tool_error_message =
                    Message::runtime_tool_error_result(id.clone(), name.clone(), error.clone());
                tool_error_message
                    .metadata
                    .insert("tool_args".to_string(), args_str.clone());
                tool_error_message.metadata.insert(
                    "tool_error_kind".to_string(),
                    "tool_not_equipped".to_string(),
                );
                tool_error_message
                    .metadata
                    .insert("available_tools".to_string(), available);
                messages.push(tool_error_message);
                unavailable_tool_calls.push((id, name, args_str, error));
                continue;
            }
            // Upgraded Loop Detection: Similarity + Repeat Count
            let loop_alert = history.check_loop(&loop_guard_name, &args_str, threshold);
            if loop_alert
                .as_ref()
                .is_some_and(|alert| alert.action == LoopGuardAction::ReusePrevious)
            {
                if let Some(previous_result) =
                    Self::latest_matching_tool_result(messages, &name, &args_str)
                {
                    let mut reused_message =
                        Message::tool_result(id, previous_result).with_tool_name(name.clone());
                    reused_message
                        .metadata
                        .insert("tool_args".to_string(), args_str.clone());
                    reused_message.metadata.insert(
                        "loop_guard_action".to_string(),
                        "reuse_previous".to_string(),
                    );
                    reused_message
                        .metadata
                        .insert("loop_guard_reused_previous".to_string(), "true".to_string());
                    messages.push(reused_message);
                    continue;
                }
            }
            let mut hook_event = self
                .build_hook_event(HookTiming::BeforeToolCall)
                .with_tool(name.clone(), args_str.clone());
            if let Some(user_input) = latest_user_input(messages) {
                hook_event = hook_event.with_user_input(user_input);
            }
            if let Some(alert) = loop_alert.as_ref() {
                hook_event
                    .metadata
                    .insert("loop_warning".to_string(), alert.message.clone());
                hook_event.metadata.insert(
                    "loop_guard_action".to_string(),
                    match alert.action {
                        LoopGuardAction::Warn => "warn".to_string(),
                        LoopGuardAction::ReusePrevious => "block".to_string(),
                        LoopGuardAction::Block => "block".to_string(),
                    },
                );
            }

            match self.hook_engine.fire(&hook_event).await {
                HookResult::Continue => {}
                HookResult::Modify(modified_args) => {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&modified_args) {
                        args = parsed;
                        args_str = modified_args;
                        loop_guard_name = Self::loop_guard_tool_name(&name, &args);
                    }
                }
                HookResult::Abort(reason) => {
                    warn!(tool = %name, "Hook guard aborted tool call: {}", reason);
                    let mut tool_error_message = Message::runtime_tool_error_result(
                        id,
                        name.clone(),
                        format!("Runtime tool error in `{}`: {}", name, reason),
                    );
                    tool_error_message
                        .metadata
                        .insert("tool_args".to_string(), args_str.clone());
                    tool_error_message.metadata.insert(
                        "tool_error_kind".to_string(),
                        "runtime_guard_abort".to_string(),
                    );
                    messages.push(tool_error_message);
                    continue;
                }
                HookResult::Skip => {
                    let mut tool_error_message = Message::runtime_tool_error_result(
                        id,
                        name.clone(),
                        format!(
                            "Runtime tool error in `{}`: skipped by runtime hook guard before tool execution.",
                            name
                        ),
                    );
                    tool_error_message
                        .metadata
                        .insert("tool_args".to_string(), args_str.clone());
                    tool_error_message.metadata.insert(
                        "tool_error_kind".to_string(),
                        "runtime_guard_skip".to_string(),
                    );
                    messages.push(tool_error_message);
                    continue;
                }
            }

            let task_context = latest_user_input(messages);
            let normalized_args =
                Self::normalize_tool_call_args(&name, args.clone(), task_context.as_deref());
            if normalized_args != args {
                args = normalized_args;
                args_str = args.to_string();
                loop_guard_name = Self::loop_guard_tool_name(&name, &args);
            }
            let normalized_args = self
                .tools
                .normalize_arguments_from_cached_schema(&name, args.clone());
            if normalized_args != args {
                args = normalized_args;
                args_str = args.to_string();
                loop_guard_name = Self::loop_guard_tool_name(&name, &args);
            }
            let compacted_args = Self::compact_tool_args_for_security_boundary(&name, args.clone());
            if compacted_args != args {
                args = compacted_args;
                args_str = args.to_string();
                loop_guard_name = Self::loop_guard_tool_name(&name, &args);
            }

            let dedupe_key = Self::same_turn_tool_call_dedupe_key(&name, &args);
            if !same_turn_seen.insert(dedupe_key.clone()) {
                let duplicate_notice = format!(
                    "Runtime tool call skipped: duplicate `{}` call in the same model planning turn. The first matching call is being executed; this duplicate was not run.",
                    name
                );
                let mut duplicate_message =
                    Message::tool_result(id.clone(), duplicate_notice.clone())
                        .with_tool_name(name.clone());
                duplicate_message
                    .metadata
                    .insert("tool_args".to_string(), args_str.clone());
                duplicate_message
                    .metadata
                    .insert("tool_skipped".to_string(), "true".to_string());
                duplicate_message.metadata.insert(
                    "tool_skip_reason".to_string(),
                    "same_turn_duplicate".to_string(),
                );
                duplicate_message
                    .metadata
                    .insert("tool_dedupe_key".to_string(), dedupe_key);
                messages.push(duplicate_message);
                skipped_duplicate_calls.push((id, name, args_str, duplicate_notice));
                continue;
            }

            history.record(loop_guard_name, args_str.clone());
            processed_calls.push((id, name, args));
        }

        // 2. Parallel Execution
        let results: Vec<
            Result<(
                String,
                String,
                String,
                String,
                Option<crate::skills::BackupInfo>,
                u64,
                crate::skills::tool::SafetyLevel,
                Option<f32>,
                Option<f32>,
            )>,
        > = if processed_calls.is_empty() {
            Vec::new()
        } else {
            let max_parallel = self.config.max_parallel_tools;
            let current_messages = Arc::new(messages.clone());
            let this = self.clone();

            let cancel_token = self.cancel_token.read().clone();
            tokio::select! {
                _ = cancel_token.cancelled() => {
                    let _ = self.events.send(AgentEvent {
                        session_id: self.session_id.clone(),
                        data: AgentEventData::Cancelled { reason: "Tool execution aborted by user".to_string() }
                    });
                    return Err(Error::agent_config("Task cancelled during tool execution"));
                }
                res = stream::iter(processed_calls)
                .map(|(id, name, args)| {
                    let name_clone = name.clone();
                    let id_clone = id.clone();
                    let args_str = args.to_string();
                    let msgs = Arc::clone(&current_messages);

                    let this = this.clone();

                    async move {
                        // 1. Get tool definition
                        let tool_ref = self.tools.get(&name_clone).ok_or_else(|| Error::ToolNotFound(name_clone.clone()))?;
                        let def = tool_ref.definition().await;

                        // 2. Resolve Effective Policy
                        let effective_policy = this.resolve_effective_policy(&name_clone, &def).await;
                        let initial_decision = this.governance.build_tool_decision(
                            GovernanceScope::ExecuteTools,
                            name_clone.clone(),
                            effective_policy.clone(),
                            def.safety_level,
                            None,
                            Some("tool policy resolved".to_string()),
                        );
                        this.emit(AgentEventData::GovernanceDecision {
                            scope: initial_decision.scope.as_str().to_string(),
                            subject: Some(initial_decision.subject.clone()),
                            authority: initial_decision.authority.as_str().to_string(),
                            policy: Some(format!("{:?}", initial_decision.policy).to_lowercase()),
                            approved: initial_decision.approved,
                            risk_score: Some(initial_decision.risk_score),
                            detail: initial_decision.detail.clone(),
                        });

                        let start_time = std::time::Instant::now();
                        this.emit(AgentEventData::ToolExecutionStart {
                            tool: name_clone.clone(),
                            input: args_str.clone(),
                            safety: def.safety_level,
                        });

                        // 3. Security Pre-Check
                        self.governance.security_handler().pre_check_tool(&name_clone, &args_str).await
                            .map_err(|e| Error::tool_execution(name_clone.clone(), format!("Security violation: {}", e)))?;

                        let mut final_backup_path = None;
                        let mut result = match effective_policy {
                            ToolPolicy::Disabled => {
                                Err(Error::tool_execution(name_clone.clone(), "Tool execution is disabled by policy".to_string()))
                            }
                            ToolPolicy::RequiresApproval => {
                                this.emit(AgentEventData::ApprovalPending {
                                    tool: name_clone.clone(),
                                    input: args_str.clone(),
                                    safety: def.safety_level,
                                });

                                // Checkpoint before awaiting approval
                                this.checkpoint(&msgs, steps, SessionStatus::AwaitingApproval {
                                    tool_name: name_clone.clone(),
                                    arguments: args_str.clone()
                                }).await?;

                                 // Ask approval handler with global stability timeout
                                 let runtime_security_context = this.current_runtime_security_context();
                                 let approval_res = tokio::time::timeout(
                                     APPROVAL_TIMEOUT,
                                     crate::skills::CURRENT_RUNTIME_SECURITY_CONTEXT.scope(
                                         runtime_security_context,
                                         self.governance.approval_handler().approve(&name_clone, &args_str, def.safety_level)
                                     )
                                 ).await;

                                 match approval_res {
                                    Ok(Ok(true)) => {
                                        let budget = this.governance.register_tool_call();
                                        this.emit(AgentEventData::GovernanceBudget {
                                            budget_kind: "tool_calls".to_string(),
                                            limit: None,
                                            used: budget.tool_calls,
                                            remaining: None,
                                            exceeded: false,
                                            detail: Some(format!("approved tool invocation for {}", name_clone)),
                                        });
                                        let approved_decision = this.governance.build_tool_decision(
                                            GovernanceScope::ExecuteTools,
                                            name_clone.clone(),
                                            effective_policy.clone(),
                                            def.safety_level,
                                            Some(true),
                                            Some("manual approval granted".to_string()),
                                        );
                                        this.emit(AgentEventData::GovernanceDecision {
                                            scope: approved_decision.scope.as_str().to_string(),
                                            subject: Some(approved_decision.subject.clone()),
                                            authority: approved_decision.authority.as_str().to_string(),
                                            policy: Some(format!("{:?}", approved_decision.policy).to_lowercase()),
                                            approved: approved_decision.approved,
                                            risk_score: Some(approved_decision.risk_score),
                                            detail: Some(format!(
                                                "{} ({})",
                                                approved_decision.detail.clone().unwrap_or_default(),
                                                approved_decision.scope.as_str()
                                            )),
                                        });
                                        let backup = Arc::new(parking_lot::Mutex::new(None));
                                        let workspaces = this.governance.trusted_workspaces().to_vec();
                                        let tool_timeout =
                                            tool_timeout_for(&name_clone, this.config.tool_execution_timeout);
                                        let res = if let Some(tool_timeout) = tool_timeout {
                                            match tokio::time::timeout(tool_timeout, async {
                                                crate::skills::CURRENT_BACKUP.scope(backup.clone(), async {
                                                    crate::skills::CURRENT_WORKSPACES.scope(workspaces, async {
                                                        crate::skills::CURRENT_SECURITY.scope(this.governance.security_handler(), async {
                                                            this.tools.call(&name_clone, &args_str).await
                                                        }).await
                                                    }).await
                                                }).await
                                            }).await {
                                                Ok(call_res) => call_res,
                                                Err(_) => Err(anyhow::anyhow!(
                                                    "Tool execution timed out after {:?}",
                                                    tool_timeout
                                                )),
                                            }
                                        } else {
                                            let runtime_security_context =
                                                this.current_runtime_security_context();
                                            crate::skills::CURRENT_BACKUP.scope(backup.clone(), async {
                                                crate::skills::CURRENT_WORKSPACES.scope(workspaces, async {
                                                    crate::skills::CURRENT_SECURITY.scope(this.governance.security_handler(), async {
                                                        crate::skills::CURRENT_RUNTIME_SECURITY_CONTEXT.scope(
                                                            runtime_security_context,
                                                            this.tools.call(&name_clone, &args_str),
                                                        ).await
                                                    }).await
                                                }).await
                                            }).await
                                        };

                                        let backup_path = backup.lock().clone();
                                        final_backup_path = backup_path.clone();
                                        match res {
                                            Ok(output) => {
                                                let compressed_output =
                                                    prehook_tool_output_for_context(
                                                        &name_clone,
                                                        &output,
                                                        this.config.max_tool_output_chars,
                                                    );
                                                let mut output = compressed_output.content;
                                                let output_truncated = compressed_output.truncated;

                                                let mut after_tool_hook = this
                                                    .build_hook_event(HookTiming::AfterToolCall)
                                                    .with_tool(name_clone.clone(), args_str.clone())
                                                    .with_tool_result(output.clone());
                                                after_tool_hook.metadata.insert(
                                                    "tool_success".to_string(),
                                                    "true".to_string(),
                                                );
                                                if output_truncated {
                                                    after_tool_hook.metadata.insert(
                                                        "tool_output_truncated".to_string(),
                                                        "true".to_string(),
                                                    );
                                                }
                                                if let Some(reason) =
                                                    extract_retrieval_degradation_reason(
                                                        &name_clone,
                                                        &output,
                                                    )
                                                {
                                                    after_tool_hook.metadata.insert(
                                                        "degradation_reason".to_string(),
                                                        reason,
                                                    );
                                                }
                                                match this.hook_engine.fire(&after_tool_hook).await {
                                                    HookResult::Continue | HookResult::Skip => {}
                                                    HookResult::Modify(modified_output) => {
                                                        output = modified_output;
                                                    }
                                                    HookResult::Abort(reason) => {
                                                        return Err(Error::tool_execution(
                                                            name_clone.clone(),
                                                            format!("after-tool hook aborted execution: {reason}"),
                                                        ));
                                                    }
                                                }

                                                let preview =
                                                    benshu_compression::preview_text(&output, 100);
                                                self.governance.security_handler().log_action(self.session_id.as_deref(), &name_clone, &args_str, true, &preview, backup_path.clone());
                                                Ok(output)
                                            }
                                            Err(e) => {
                                                self.governance.security_handler().log_action(self.session_id.as_deref(), &name_clone, &args_str, false, &e.to_string(), backup_path.clone());

                                                let normalized_error = this
                                                    .apply_tool_error_hook(
                                                        &name_clone,
                                                        &args_str,
                                                        e.to_string(),
                                                    )
                                                    .await;

                                                self.enqueue_tool_failure_analysis_if_enabled(
                                                    &name_clone,
                                                    &args_str,
                                                    &normalized_error,
                                                    &msgs,
                                                );

                                                Err(Error::tool_execution(
                                                    name_clone.clone(),
                                                    normalized_error,
                                                ))
                                            }
                                        }
                                    }
                                    Ok(Ok(false)) => {
                                        let rejected_decision = this.governance.build_tool_decision(
                                            GovernanceScope::ExecuteTools,
                                            name_clone.clone(),
                                            effective_policy.clone(),
                                            def.safety_level,
                                            Some(false),
                                            Some("manual approval rejected".to_string()),
                                        );
                                        this.emit(AgentEventData::GovernanceDecision {
                                            scope: rejected_decision.scope.as_str().to_string(),
                                            subject: Some(rejected_decision.subject.clone()),
                                            authority: rejected_decision.authority.as_str().to_string(),
                                            policy: Some(format!("{:?}", rejected_decision.policy).to_lowercase()),
                                            approved: rejected_decision.approved,
                                            risk_score: Some(rejected_decision.risk_score),
                                            detail: rejected_decision.detail.clone(),
                                        });
                                        Err(Error::ToolApprovalRequired { tool_name: name_clone.clone() })
                                    }
                                    Ok(Err(e)) => {
                                        Err(Error::tool_execution(name_clone.clone(), format!("Approval check failed: {}", e)))
                                    }
                                    Err(_) => {
                                        Err(Error::tool_execution(name_clone.clone(), "Tool approval timed out (Fail-Safe triggered)".to_string()))
                                    }
                                }
                            }
                            ToolPolicy::Auto => {
                                let budget = this.governance.register_tool_call();
                                this.emit(AgentEventData::GovernanceBudget {
                                    budget_kind: "tool_calls".to_string(),
                                    limit: None,
                                    used: budget.tool_calls,
                                    remaining: None,
                                    exceeded: false,
                                    detail: Some(format!("automatic tool invocation for {}", name_clone)),
                                });
                                let auto_decision = this.governance.build_tool_decision(
                                    GovernanceScope::ExecuteTools,
                                    name_clone.clone(),
                                    effective_policy.clone(),
                                    def.safety_level,
                                    Some(true),
                                    Some("automatic policy execution".to_string()),
                                );
                                this.emit(AgentEventData::GovernanceDecision {
                                    scope: auto_decision.scope.as_str().to_string(),
                                    subject: Some(auto_decision.subject.clone()),
                                    authority: auto_decision.authority.as_str().to_string(),
                                    policy: Some(format!("{:?}", auto_decision.policy).to_lowercase()),
                                    approved: auto_decision.approved,
                                    risk_score: Some(auto_decision.risk_score),
                                    detail: auto_decision.detail.clone(),
                                });
                                let backup = Arc::new(parking_lot::Mutex::new(None));
                                let workspaces = this.governance.trusted_workspaces().to_vec();
                                let tool_timeout =
                                    tool_timeout_for(&name_clone, this.config.tool_execution_timeout);
                                let res = if let Some(tool_timeout) = tool_timeout {
                                    match tokio::time::timeout(tool_timeout, async {
                                        crate::skills::CURRENT_BACKUP.scope(backup.clone(), async {
                                            crate::skills::CURRENT_WORKSPACES.scope(workspaces, async {
                                                crate::skills::CURRENT_SECURITY.scope(this.governance.security_handler(), async {
                                                    this.tools.call(&name_clone, &args_str).await
                                                }).await
                                            }).await
                                        }).await
                                    }).await {
                                        Ok(call_res) => call_res,
                                        Err(_) => Err(anyhow::anyhow!(
                                            "Tool execution timed out after {:?}",
                                            tool_timeout
                                        )),
                                    }
                                } else {
                                    let runtime_security_context =
                                        this.current_runtime_security_context();
                                    crate::skills::CURRENT_BACKUP.scope(backup.clone(), async {
                                        crate::skills::CURRENT_WORKSPACES.scope(workspaces, async {
                                            crate::skills::CURRENT_SECURITY.scope(this.governance.security_handler(), async {
                                                crate::skills::CURRENT_RUNTIME_SECURITY_CONTEXT.scope(
                                                    runtime_security_context,
                                                    this.tools.call(&name_clone, &args_str),
                                                ).await
                                            }).await
                                        }).await
                                    }).await
                                };

                                let backup_path = backup.lock().clone();
                                final_backup_path = backup_path.clone();
                                match res {
                                    Ok(output) => {
                                        let compressed_output =
                                            prehook_tool_output_for_context(
                                                &name_clone,
                                                &output,
                                                this.config.max_tool_output_chars,
                                            );
                                        let mut output = compressed_output.content;
                                        let output_truncated = compressed_output.truncated;

                                        let mut after_tool_hook = this
                                            .build_hook_event(HookTiming::AfterToolCall)
                                            .with_tool(name_clone.clone(), args_str.clone())
                                            .with_tool_result(output.clone());
                                        after_tool_hook.metadata.insert(
                                            "tool_success".to_string(),
                                            "true".to_string(),
                                        );
                                        if output_truncated {
                                            after_tool_hook.metadata.insert(
                                                "tool_output_truncated".to_string(),
                                                "true".to_string(),
                                            );
                                        }
                                        if let Some(reason) =
                                            extract_retrieval_degradation_reason(
                                                &name_clone,
                                                &output,
                                            )
                                        {
                                            after_tool_hook.metadata.insert(
                                                "degradation_reason".to_string(),
                                                reason,
                                            );
                                        }
                                        match this.hook_engine.fire(&after_tool_hook).await {
                                            HookResult::Continue | HookResult::Skip => {}
                                            HookResult::Modify(modified_output) => {
                                                output = modified_output;
                                            }
                                            HookResult::Abort(reason) => {
                                                return Err(Error::tool_execution(
                                                    name_clone.clone(),
                                                    format!("after-tool hook aborted execution: {reason}"),
                                                ));
                                            }
                                        }

                                        let preview =
                                            benshu_compression::preview_text(&output, 100);
                                        self.governance.security_handler().log_action(self.session_id.as_deref(), &name_clone, &args_str, true, &preview, backup_path.clone());
                                        Ok(output)
                                    }
                                    Err(e) => {
                                        self.governance.security_handler().log_action(self.session_id.as_deref(), &name_clone, &args_str, false, &e.to_string(), backup_path.clone());

                                        let normalized_error = this
                                            .apply_tool_error_hook(
                                                &name_clone,
                                                &args_str,
                                                e.to_string(),
                                            )
                                            .await;

                                        self.enqueue_tool_failure_analysis_if_enabled(
                                            &name_clone,
                                            &args_str,
                                            &normalized_error,
                                            &msgs,
                                        );

                                        Err(Error::tool_execution(
                                            name_clone.clone(),
                                            normalized_error,
                                        ))
                                    }
                                }
                            }
                        };

                        // 4. Security Post-Filter
                        if let Ok(ref mut output) = result {
                            *output = self.governance.security_handler().post_filter_result(output).await;
                        }

                        // 5. Secret Redaction
                        if let Ok(output_str) = &mut result {
                            let (redacted, detections) = self.governance.security_handler().check_output(output_str);
                            if !detections.is_empty() {
                                warn!(tool = %name_clone, "Secret leak detected in tool output: {:?}", detections);
                                *output_str = redacted;
                            }
                        }

                        // 6. Lazy Skill Loading Injection
                        if result.is_ok() {
                            let is_first_use = {
                                !self.seen_tools.read().contains(&name_clone)
                            };

                            if is_first_use {
                                let injection = this.generate_first_use_injection(&name_clone, &def);
                                if let Ok(ref mut res_text) = result {
                                    *res_text = format!("{}\n\n---\n{}", res_text, injection);
                                }
                                self.seen_tools.write().insert(name_clone.clone());
                            }
                        }

                        let duration = start_time.elapsed().as_millis() as u64;

                        match result {
                            Ok(output) => {
                                let preview =
                                    benshu_compression::preview_text(&output, 100);

                                this.emit(AgentEventData::ToolExecutionEnd {
                                    tool: name_clone.clone(),
                                    output_preview: preview.clone(),
                                    duration_ms: duration,
                                    success: true
                                });

                                 if let Some(metrics) = &this.metrics {
                                     let prefix = format!("tool:{}", name_clone);
                                     metrics.counter_inc(&format!("{}:success", prefix), 1);
                                     metrics.histogram_observe(&format!("{}:duration_ms", prefix), duration as f64);
                                 }

                                 let (cpu_pressure, vram_pressure) = if let Some(sensor) = &this.sensor {
                                     let resources = sensor.write().check_resources(false);

                                     if let Some(metrics) = &this.metrics {
                                         metrics.gauge_set("system:vram_pressure", resources.vram_pressure_pct() as f64);
                                         metrics.gauge_set("system:cpu_usage", resources.cpu_usage as f64);
                                     }
                                     (Some(resources.cpu_usage), Some(resources.vram_pressure_pct()))
                                 } else {
                                     (None, None)
                                 };

                                 Ok((id_clone, name_clone, output, args_str, final_backup_path, duration, def.safety_level.clone(), cpu_pressure, vram_pressure))
                            },
                            Err(e) => {
                                let mut error_msg = format!("Error executing tool '{}': {}", name_clone, e);
                                let failure_classification = classify_failure(&e.to_string());

                                if should_append_reflexion_recovery_prompt(
                                    this.config.enable_reflexion,
                                    query_requires_execution_tool_reply(msgs.as_ref()),
                                    failure_classification,
                                ) {
                                    error_msg.push_str("\n\n");
                                    error_msg.push_str(&this.intervention.get_reflexion_prompt(&e.to_string()));
                                }

                                this.emit(AgentEventData::ToolExecutionEnd {
                                    tool: name_clone.clone(),
                                    output_preview: error_msg.clone(),
                                    duration_ms: duration,
                                    success: false
                                });

                                this.emit(AgentEventData::Error { message: e.to_string() });

                                 let (cpu_pressure, vram_pressure) = if let Some(sensor) = &this.sensor {
                                     let resources = sensor.write().check_resources(false);

                                     if let Some(metrics) = &this.metrics {
                                         metrics.gauge_set("system:vram_pressure", resources.vram_pressure_pct() as f64);
                                         metrics.gauge_set("system:cpu_usage", resources.cpu_usage as f64);
                                     }
                                     (Some(resources.cpu_usage), Some(resources.vram_pressure_pct()))
                                 } else {
                                     (None, None)
                                 };

                                 Ok((id_clone, name_clone, error_msg, args_str, final_backup_path, duration, def.safety_level.clone(), cpu_pressure, vram_pressure))
                            }
                        }
                    }
                })
                .buffer_unordered(max_parallel)
                .collect::<Vec<_>>() => res
            }
        };

        // 3. Process Results (Partial Failure Support)
        let mut tool_results = Vec::new();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        for (id, name, args, output) in skipped_duplicate_calls {
            let args_fingerprint = Self::runtime_fingerprint(&args);
            let result_fingerprint = Self::runtime_fingerprint(&output);
            tool_results.push(ToolCallData {
                receipt_id: Some(uuid::Uuid::new_v4().to_string()),
                tool_call_id: Some(id.clone()),
                name: name.clone(),
                args,
                result: Some(output.clone()),
                backup: None,
                duration_ms: 0,
                timestamp,
                caller_id: self.session_id.clone(),
                safety_level: SafetyLevel::Green,
                cpu_pressure: None,
                vram_pressure: None,
                result_truncated: false,
                result_original_chars: None,
                result_omitted_chars: None,
                args_fingerprint: Some(args_fingerprint),
                result_fingerprint: Some(result_fingerprint),
                outcome: Some(Self::build_tool_outcome_meta(
                    "skipped", &name, &output, None,
                )),
                replay: replay_receipts.get(&id).cloned(),
            });
        }
        for (id, name, args, output) in unavailable_tool_calls {
            let args_fingerprint = Self::runtime_fingerprint(&args);
            let result_fingerprint = Self::runtime_fingerprint(&output);
            tool_results.push(ToolCallData {
                receipt_id: Some(uuid::Uuid::new_v4().to_string()),
                tool_call_id: Some(id.clone()),
                name: name.clone(),
                args,
                result: Some(output.clone()),
                backup: None,
                duration_ms: 0,
                timestamp,
                caller_id: self.session_id.clone(),
                safety_level: SafetyLevel::Red,
                cpu_pressure: None,
                vram_pressure: None,
                result_truncated: false,
                result_original_chars: None,
                result_omitted_chars: None,
                args_fingerprint: Some(args_fingerprint),
                result_fingerprint: Some(result_fingerprint),
                outcome: Some(Self::build_tool_outcome_meta(
                    "unavailable",
                    &name,
                    &output,
                    None,
                )),
                replay: replay_receipts.get(&id).cloned(),
            });
        }

        for res in results {
            match res {
                Ok((id, name, output, args_str, backup, duration, safety, cpu, vram)) => {
                    let guarded_output =
                        compress_executor_tool_output(&output, self.config.max_tool_output_chars);
                    let full_artifact_ref = Self::spill_full_tool_output_if_truncated(
                        self.session_id.as_deref(),
                        &name,
                        &output,
                        guarded_output.truncated,
                    )
                    .await;
                    let output = guarded_output.content;
                    let args_fingerprint = Self::runtime_fingerprint(&args_str);
                    let result_fingerprint = Self::runtime_fingerprint(&output);
                    tool_results.push(ToolCallData {
                        receipt_id: Some(uuid::Uuid::new_v4().to_string()),
                        tool_call_id: Some(id.clone()),
                        name: name.clone(),
                        args: args_str.clone(),
                        result: Some(output.clone()),
                        backup: backup.clone(),
                        duration_ms: duration,
                        timestamp,
                        caller_id: self.session_id.clone(),
                        safety_level: safety,
                        cpu_pressure: cpu,
                        vram_pressure: vram,
                        result_truncated: guarded_output.truncated,
                        result_original_chars: Some(guarded_output.original_chars),
                        result_omitted_chars: guarded_output
                            .truncated
                            .then_some(guarded_output.omitted_chars),
                        args_fingerprint: Some(args_fingerprint),
                        result_fingerprint: Some(result_fingerprint),
                        outcome: Some(Self::build_tool_outcome_meta(
                            "completed",
                            &name,
                            &output,
                            full_artifact_ref,
                        )),
                        replay: replay_receipts.get(&id).cloned(),
                    });
                    let mut tool_msg =
                        Message::tool_result(id, output.clone()).with_tool_name(name.clone());
                    tool_msg
                        .metadata
                        .insert("tool_args".to_string(), args_str.clone());

                    // Phase 16.4: Quantum-Safe Signatures for Red tools
                    if safety == SafetyLevel::Red {
                        let sig = PostQuantumGuard::sign_tool_call(
                            &name,
                            &serde_json::json!({ "output": output }),
                        );
                        tool_msg.metadata.insert("pqc_signature".to_string(), sig);
                    }

                    messages.push(tool_msg);
                }
                Err(e) => {
                    // Log error but continue with other results
                    warn!(tool = %e.tool_name(), "Parallel tool execution failed: {}", e);
                    let raw_error = format!("Error: {}", e);
                    let guarded_error = compress_executor_tool_output(
                        &raw_error,
                        self.config.max_tool_output_chars,
                    );
                    let full_artifact_ref = Self::spill_full_tool_output_if_truncated(
                        self.session_id.as_deref(),
                        e.tool_name(),
                        &raw_error,
                        guarded_error.truncated,
                    )
                    .await;
                    let args = e.args().to_string();
                    let output = guarded_error.content.clone();
                    let args_fingerprint = Self::runtime_fingerprint(&args);
                    let result_fingerprint = Self::runtime_fingerprint(&output);
                    tool_results.push(ToolCallData {
                        receipt_id: Some(uuid::Uuid::new_v4().to_string()),
                        tool_call_id: None,
                        name: e.tool_name().to_string(),
                        args,
                        result: Some(output.clone()),
                        backup: None,
                        duration_ms: 0,
                        timestamp,
                        caller_id: self.session_id.clone(),
                        safety_level: SafetyLevel::Red,
                        cpu_pressure: None,
                        vram_pressure: None,
                        result_truncated: guarded_error.truncated,
                        result_original_chars: Some(guarded_error.original_chars),
                        result_omitted_chars: guarded_error
                            .truncated
                            .then_some(guarded_error.omitted_chars),
                        args_fingerprint: Some(args_fingerprint),
                        result_fingerprint: Some(result_fingerprint),
                        outcome: Some(Self::build_tool_outcome_meta(
                            "failed",
                            e.tool_name(),
                            &output,
                            full_artifact_ref,
                        )),
                        replay: None,
                    });
                    let mut tool_error_message = Message::runtime_tool_error_result(
                        uuid::Uuid::new_v4().to_string(),
                        e.tool_name().to_string(),
                        guarded_error.content,
                    );
                    tool_error_message
                        .metadata
                        .insert("tool_args".to_string(), e.args().to_string());
                    tool_error_message.metadata.insert(
                        "tool_error_kind".to_string(),
                        "execution_failure".to_string(),
                    );
                    messages.push(tool_error_message);
                }
            }
        }

        Ok(tool_results)
    }

    /// Execute a single tool call with full lifecycle management
    pub async fn execute_single(&self, name: &str, arguments: &str) -> Result<String> {
        let name = self.canonical_tool_call_name(name);
        let normalized_arguments = serde_json::from_str::<serde_json::Value>(arguments)
            .map(|args| Self::normalize_tool_call_args(&name, args, None))
            .map(|args| {
                self.tools
                    .normalize_arguments_from_cached_schema(&name, args)
            })
            .map(|args| Self::compact_tool_args_for_security_boundary(&name, args))
            .and_then(|args| serde_json::to_string(&args))
            .unwrap_or_else(|_| arguments.to_string());
        // 0. Get Tool Definition
        let tool_ref = self
            .tools
            .get(&name)
            .ok_or_else(|| Error::ToolNotFound(name.to_string()))?;
        let def = tool_ref.definition().await;

        // 1. Resolve Effective Policy
        let effective_policy = self.resolve_effective_policy(&name, &def).await;

        match effective_policy {
            ToolPolicy::Disabled => {
                return Err(ToolExecutionError::PolicyDisabled(name.to_string()).into());
            }
            ToolPolicy::RequiresApproval => {
                self.emit(AgentEventData::ApprovalPending {
                    tool: name.to_string(),
                    input: arguments.to_string(),
                    safety: def.safety_level,
                });

                match crate::skills::CURRENT_RUNTIME_SECURITY_CONTEXT
                    .scope(
                        self.current_runtime_security_context(),
                        self.governance.approval_handler().approve(
                            &name,
                            &normalized_arguments,
                            def.safety_level,
                        ),
                    )
                    .await
                {
                    Ok(true) => {}
                    Ok(false) => {
                        return Err(ToolExecutionError::ApprovalRequired(name.to_string()).into())
                    }
                    Err(e) => {
                        return Err(Error::tool_execution(
                            name.to_string(),
                            format!("Approval check failed: {}", e),
                        ))
                    }
                }
            }
            ToolPolicy::Auto => {}
        }

        self.emit(AgentEventData::ToolCall {
            tool: name.to_string(),
            input: normalized_arguments.clone(),
            safety: def.safety_level,
        });

        let start_time = std::time::Instant::now();

        // 2. Throttling & Resource Pressure
        let (throttle, pressure) = if let Some(sensor) = &self.sensor {
            let mut s = sensor.write();
            let stats = s.check_resources(false);
            let level = if stats.cpu_usage > 90.0 || stats.free_memory_pct < 5.0 {
                crate::skills::ThrottleLevel::Low
            } else if stats.cpu_usage > 60.0 || stats.free_memory_pct < 15.0 {
                crate::skills::ThrottleLevel::Medium
            } else {
                crate::skills::ThrottleLevel::High
            };

            debug!(
                tool = %name,
                cpu_usage = stats.cpu_usage,
                free_memory_pct = stats.free_memory_pct,
                throttle_level = ?level,
                "Resource-based throttle level determined for tool call"
            );
            (level, stats.is_low_memory)
        } else {
            (self.config.default_throttle, false)
        };

        let tool_timeout = tool_timeout_for(&name, self.config.tool_execution_timeout);
        let call_name = name.clone();
        let runtime_security_context = self.current_runtime_security_context();
        let tool_call = async move {
            crate::skills::CURRENT_THROTTLE
                .scope(throttle, async move {
                    crate::skills::CURRENT_PRESSURE
                        .scope(pressure, async move {
                            crate::skills::CURRENT_RUNTIME_SECURITY_CONTEXT
                                .scope(
                                    runtime_security_context,
                                    self.tools.call(&call_name, &normalized_arguments),
                                )
                                .await
                        })
                        .await
                })
                .await
        };
        let result = if let Some(tool_timeout) = tool_timeout {
            match tokio::time::timeout(tool_timeout, tool_call).await {
                Ok(res) => res,
                Err(_) => Err(anyhow::anyhow!(
                    "Tool execution timed out after {:?}",
                    tool_timeout
                )),
            }
        } else {
            tool_call.await
        };

        let duration = start_time.elapsed().as_millis() as u64;

        match result {
            Ok(output) => {
                // Quota Protection
                let output =
                    compress_executor_tool_output(&output, self.config.max_tool_output_chars)
                        .content;

                self.emit(AgentEventData::ToolResult {
                    tool: name.to_string(),
                    output: output.clone(),
                });

                if let Some(metrics) = &self.metrics {
                    let prefix = format!("tool:{}", name);
                    metrics.counter_inc(&format!("{}:success", prefix), 1);
                    metrics.histogram_observe(&format!("{}:duration_ms", prefix), duration as f64);
                }

                Ok(output)
            }
            Err(e) => {
                self.emit(AgentEventData::Error {
                    message: e.to_string(),
                });

                if let Some(metrics) = &self.metrics {
                    let prefix = format!("tool:{}", name);
                    metrics.counter_inc(&format!("{}:failure", prefix), 1);
                    metrics.histogram_observe(&format!("{}:duration_ms", prefix), duration as f64);
                }

                Err(Error::tool_execution(name.to_string(), e.to_string()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        attach_provider_media_metadata_from_capture, generate_tool_schema_injection,
        query_requires_execution_tool_reply, tool_timeout_for, ActionExecutor,
        COORDINATION_TOOL_ARG_SECURITY_LIMIT, RUNTIME_TASK_CONTEXT_ARG_KEY,
    };
    use crate::agent::message::Message;
    use crate::hooks::RuntimeHookCapture;
    use crate::skills::tool::{ToolDefinition, ToolSet};

    #[test]
    fn large_typescript_schema_uses_compact_summary() {
        let ts = (0..120)
            .map(|i| format!("  field_{}: string; // {}", i, "detail ".repeat(8)))
            .collect::<Vec<_>>()
            .join("\n");
        let def = ToolDefinition {
            name: "big_schema".to_string(),
            description: "big".to_string(),
            parameters: serde_json::json!({}),
            parameters_ts: Some(format!("interface BigSchema {{\n{}\n}}", ts)),
            is_binary: false,
            is_verified: true,
            safety_level: Default::default(),
            usage_guidelines: None,
        };

        let injection = generate_tool_schema_injection(&def);
        assert!(injection.contains("Compact TypeScript Schema Summary"));
        assert!(injection.contains("omitted for compact first-use discovery"));
    }

    #[test]
    fn large_json_schema_uses_compact_summary() {
        let mut properties = serde_json::Map::new();
        for index in 0..80 {
            properties.insert(
                format!("field_{}", index),
                serde_json::json!({
                    "type": "string",
                    "description": format!("field {} {}", index, "detail ".repeat(10)),
                }),
            );
        }
        let def = ToolDefinition {
            name: "big_json_schema".to_string(),
            description: "big".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "required": ["field_0"],
                "properties": properties,
            }),
            parameters_ts: None,
            is_binary: false,
            is_verified: true,
            safety_level: Default::default(),
            usage_guidelines: None,
        };

        let injection = generate_tool_schema_injection(&def);
        assert!(injection.contains("Compact JSON Schema Summary"));
        assert!(injection.contains("additional properties omitted"));
    }

    #[test]
    fn fetch_document_args_can_recover_collection_path_from_import_receipt_url() {
        let messages = vec![Message::tool_result(
            "knowledge-import",
            "runtime_effect: knowledge.imported\nstorage_target: durable_knowledge_store\ncollection: references\npath: web/example/doc-123\ntitle: Example\nsource_url: https://example.com/doc\n\nImported web knowledge into collection 'references' at path 'web/example/doc-123' with title 'Example'. Source URL: https://example.com/doc",
        )
        .with_tool_name("knowledge_import_url")];
        let mut args = serde_json::json!({
            "url": "https://example.com/doc"
        });

        ActionExecutor::repair_fetch_document_args_from_import_receipt(&mut args, &messages);

        assert_eq!(args["collection"], "references");
        assert_eq!(args["path"], "web/example/doc-123");
    }

    #[test]
    fn fetch_document_args_can_recover_latest_collection_path_without_url() {
        let messages = vec![Message::tool_result(
            "knowledge-import",
            "runtime_effect: knowledge.imported\nstorage_target: durable_knowledge_store\ncollection: references\npath: web/example/latest-doc\ntitle: Example\nsource_url: https://example.com/latest\n\nImported web knowledge into collection 'references' at path 'web/example/latest-doc' with title 'Example'. Source URL: https://example.com/latest",
        )
        .with_tool_name("knowledge_import_url")];
        let mut args = serde_json::json!({});

        ActionExecutor::repair_fetch_document_args_from_import_receipt(&mut args, &messages);

        assert_eq!(args["collection"], "references");
        assert_eq!(args["path"], "web/example/latest-doc");
    }

    #[test]
    fn nested_action_tool_name_is_promoted_to_top_level_tool_call() {
        let tools = ToolSet::new();
        tools.add(crate::simple_tool!(
            name: "novel_studio",
            description: "novel",
            parameters: serde_json::json!({}),
            handler: |_args: &str| async { Ok("novel".to_string()) }
        ));
        tools.add(crate::simple_tool!(
            name: "fetch_document",
            description: "fetch",
            parameters: serde_json::json!({}),
            handler: |_args: &str| async { Ok("doc".to_string()) }
        ));
        let mut args = serde_json::json!({
            "action": "fetch_document",
            "collection": "references",
            "path": "web/example"
        });

        let promoted = ActionExecutor::promote_nested_action_tool_call_for_tools(
            &tools,
            "novel_studio",
            &mut args,
        );

        assert_eq!(promoted.as_deref(), Some("fetch_document"));
        assert!(args.get("action").is_none());
        assert_eq!(args["collection"], "references");
        assert_eq!(args["path"], "web/example");
    }

    #[test]
    fn bare_tool_invocation_content_is_promoted_to_top_level_tool_call() {
        let tools = ToolSet::new();
        tools.add(crate::simple_tool!(
            name: "write_file",
            description: "write",
            parameters: serde_json::json!({}),
            handler: |_args: &str| async { Ok("write".to_string()) }
        ));
        tools.add(crate::simple_tool!(
            name: "fetch_document",
            description: "fetch",
            parameters: serde_json::json!({}),
            handler: |_args: &str| async { Ok("doc".to_string()) }
        ));
        let mut args = serde_json::json!({
            "path": "data/logs/recovery_log.txt",
            "content": "fetch_document(collection='references', path='web/example/doc-123')"
        });

        let promoted = ActionExecutor::promote_bare_invocation_content_tool_call_for_tools(
            &tools,
            "write_file",
            &mut args,
        );

        assert_eq!(promoted.as_deref(), Some("fetch_document"));
        assert_eq!(args["collection"], "references");
        assert_eq!(args["path"], "web/example/doc-123");
        assert!(args.get("content").is_none());
    }

    #[test]
    fn canonical_tool_name_accepts_dotted_action_on_equipped_tool() {
        let tools = ToolSet::new();
        tools.add(crate::simple_tool!(
            name: "browser_browse",
            description: "browse",
            parameters: serde_json::json!({}),
            handler: |_args: &str| async { Ok("ok".to_string()) }
        ));

        assert_eq!(
            ActionExecutor::canonical_tool_call_name_for_tools(&tools, "browser_browse.search"),
            "browser_browse"
        );
        assert_eq!(
            ActionExecutor::canonical_tool_call_name_for_tools(&tools, "browser_browse.navigate"),
            "browser_browse"
        );
    }

    #[test]
    fn assistant_message_persists_provider_media_followup_metadata_from_runtime_capture() {
        let mut message = Message::assistant("captured");
        let mut hook_capture = RuntimeHookCapture::default();
        hook_capture.notes.push(
            "after_llm:provider_media_preprocess_followup_strategies:extract_video_frames:alternate_model_fallback,normalize_audio:clarification_or_manual_review".to_string(),
        );
        hook_capture.notes.push(
            "after_llm:provider_media_preprocess_alternate_model_fallback_routes:extract_video_frames".to_string(),
        );
        hook_capture.notes.push(
            "after_llm:provider_media_preprocess_clarification_routes:normalize_audio".to_string(),
        );

        attach_provider_media_metadata_from_capture(&mut message, &hook_capture);

        assert_eq!(
            message
                .metadata
                .get("provider_media_preprocess_followup_strategies")
                .map(String::as_str),
            Some(
                "extract_video_frames:alternate_model_fallback,normalize_audio:clarification_or_manual_review",
            )
        );
        assert_eq!(
            message
                .metadata
                .get("provider_media_preprocess_alternate_model_fallback_routes")
                .map(String::as_str),
            Some("extract_video_frames")
        );
        assert_eq!(
            message
                .metadata
                .get("provider_media_preprocess_clarification_routes")
                .map(String::as_str),
            Some("normalize_audio")
        );
    }

    #[test]
    fn execution_requests_skip_reflexion_append_path() {
        assert!(query_requires_execution_tool_reply(&[Message::user(
            "请帮我生成一张猫咪图片".to_string()
        )]));
        assert!(!query_requires_execution_tool_reply(&[Message::user(
            "请解释一下 Transformer 是什么".to_string()
        )]));
        assert!(!query_requires_execution_tool_reply(&[Message::user(
            crate::agent::message::Content::Parts(vec![
                crate::agent::message::ContentPart::Text {
                    text: "请描述这张图片里有什么".to_string(),
                },
                crate::agent::message::ContentPart::Image {
                    source: crate::agent::message::ImageSource::Url {
                        url: "file:///tmp/test.png".to_string(),
                    },
                },
            ]),
        )]));
    }

    #[test]
    fn coordination_tools_do_not_use_foreground_timeout() {
        let timeout = tool_timeout_for("delegate", std::time::Duration::from_secs(120));
        assert_eq!(timeout, None);
    }

    #[test]
    fn coordination_tool_args_are_compacted_before_security_boundary() {
        let args = serde_json::json!({
            "role": "knowledge",
            "task": "Import the following evidence:\n".to_string() + &"source row ".repeat(1200),
            "full_user_request": "keep the user's original goal visible"
        });

        let compacted = ActionExecutor::compact_tool_args_for_security_boundary("delegate", args);
        let encoded = compacted.to_string();

        assert!(encoded.chars().count() <= COORDINATION_TOOL_ARG_SECURITY_LIMIT);
        assert!(encoded.contains("runtime compacted oversized tool argument"));
        assert!(encoded.contains("full_user_request"));
    }

    #[test]
    fn browser_tool_args_receive_runtime_task_context() {
        let args = serde_json::json!({
            "action": "search",
            "text": "site:example.com ranking"
        });

        let normalized = ActionExecutor::normalize_tool_call_args(
            "browser_browse",
            args,
            Some("Find the top free downloadable fantasy records and save them"),
        );

        assert_eq!(
            normalized
                .get(RUNTIME_TASK_CONTEXT_ARG_KEY)
                .and_then(|value| value.as_str()),
            Some("Find the top free downloadable fantasy records and save them")
        );
    }

    #[test]
    fn browser_tool_args_compact_runtime_context_before_security_boundary() {
        let args = serde_json::json!({
            "action": "search",
            "text": "popular downloadable fantasy novels",
            RUNTIME_TASK_CONTEXT_ARG_KEY: "Delegated contract:\n".to_string() + &"preserve constraints and tool rules. ".repeat(500),
        });

        let compacted =
            ActionExecutor::compact_tool_args_for_security_boundary("browser_browse", args);
        let encoded = compacted.to_string();

        assert!(encoded.chars().count() <= COORDINATION_TOOL_ARG_SECURITY_LIMIT);
        assert!(encoded.contains("runtime compacted oversized tool argument"));
        assert!(encoded.contains("popular downloadable fantasy novels"));
    }

    #[test]
    fn non_coordination_tools_keep_default_timeout() {
        let timeout = tool_timeout_for("web_search", std::time::Duration::from_secs(120));
        assert_eq!(timeout, Some(std::time::Duration::from_secs(120)));
    }

    #[test]
    fn simple_realtime_lookup_tools_are_capped_for_foreground_responsiveness() {
        let timeout = tool_timeout_for("price_lookup", std::time::Duration::from_secs(120));
        assert_eq!(timeout, Some(std::time::Duration::from_secs(30)));

        let already_tighter =
            tool_timeout_for("weather_lookup", std::time::Duration::from_secs(10));
        assert_eq!(already_tighter, Some(std::time::Duration::from_secs(10)));
    }

    #[test]
    fn tool_call_replay_receipts_record_sampled_and_normalized_fingerprints() {
        let calls = vec![(
            "call_1".to_string(),
            "web_search".to_string(),
            serde_json::json!({"query": "北京天气"}),
        )];

        let receipts = ActionExecutor::build_tool_call_replay_receipts(
            r#"<|tool_call>{"name":"web_search","arguments":{"query":"北京天气"}}</tool_call|>"#,
            &calls,
        );
        let receipt = receipts.get("call_1").expect("receipt should exist");

        assert_eq!(receipt.tool_call_id, "call_1");
        assert_eq!(receipt.replay_mode, "sampled_text_exact");
        assert!(receipt.sampled_call_ref.ends_with("/tool_call/call_1"));
        assert!(!receipt.sampled_call_fingerprint.is_empty());
        assert!(!receipt.normalized_call_fingerprint.is_empty());
    }

    #[test]
    fn delegate_loop_guard_key_is_scoped_by_role() {
        let researcher = serde_json::json!({
            "role": "researcher",
            "task": "Find sources"
        });
        let knowledge = serde_json::json!({
            "role": "knowledge",
            "task": "Import source"
        });

        assert_eq!(
            ActionExecutor::loop_guard_tool_name("delegate", &researcher),
            "delegate::researcher"
        );
        assert_eq!(
            ActionExecutor::loop_guard_tool_name("delegate", &knowledge),
            "delegate::knowledge"
        );
        assert_eq!(
            ActionExecutor::loop_guard_tool_name(
                "web_fetch",
                &serde_json::json!({"url": "https://example.com"})
            ),
            "web_fetch"
        );
    }

    #[test]
    fn same_turn_delegate_dedupe_key_collapses_whitespace_and_case() {
        let first = serde_json::json!({
            "role": "Researcher",
            "task": "Find   public sources\nfor this request"
        });
        let second = serde_json::json!({
            "role": "researcher",
            "task": "find public sources for this request"
        });

        assert_eq!(
            ActionExecutor::same_turn_tool_call_dedupe_key("delegate", &first),
            ActionExecutor::same_turn_tool_call_dedupe_key("delegate", &second)
        );
    }
}
