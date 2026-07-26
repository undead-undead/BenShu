//! OpenAI provider implementation
//!
//! Also compatible with OpenAI-compatible APIs like Groq, Mistral, etc.

use async_trait::async_trait;
use futures::{Stream, StreamExt};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONNECTION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::{
    Error, HttpConfig, Message, Provider, Result, StreamingChoice, StreamingResponse,
    ToolDefinition,
};
use benshu_protocol_core::{Content, Role};
use benshu_provider_core::{
    ContextLimitError, ContinuationTelemetry, FinishReason, ProviderTelemetry, Usage,
};

const MAX_LOCAL_DATA_URL_MEDIA_BYTES: u64 = 100 * 1024 * 1024;
const LOCAL_OPENAI_COMPAT_TIMEOUT_SECS: u64 = 900;

/// OpenAI API client
#[derive(Clone)]
pub struct OpenAI {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
    local_request_gate: Option<Arc<Semaphore>>,
}

impl OpenAI {
    const LOCAL_PSEUDO_TOOL_CALL_OPEN: &'static str = "<|tool_call>";
    const LOCAL_PSEUDO_TOOL_CALL_CLOSE: &'static str = "<tool_call|>";

    fn tool_replay_blocks_from_metadata(
        metadata: &HashMap<String, String>,
    ) -> HashMap<String, String> {
        let Some(raw) = metadata.get("tool_replay_receipts") else {
            return HashMap::new();
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
            return HashMap::new();
        };
        let Some(object) = value.as_object() else {
            return HashMap::new();
        };

        object
            .iter()
            .filter_map(|(tool_call_id, receipt)| {
                let replay_mode = receipt
                    .get("replay_mode")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default();
                if replay_mode != "sampled_text_exact" {
                    return None;
                }
                let block = receipt
                    .get("sampled_call_block")
                    .and_then(|value| value.as_str())?;
                Some((tool_call_id.clone(), block.to_string()))
            })
            .collect()
    }

    fn message_uses_sampled_tool_replay(message: &Message) -> bool {
        let replay_blocks = Self::tool_replay_blocks_from_metadata(&message.metadata);
        if replay_blocks.is_empty() {
            return false;
        }
        let Content::Parts(parts) = &message.content else {
            return false;
        };
        parts.iter().any(|part| {
            matches!(
                part,
                benshu_protocol_core::ContentPart::ToolCall { id, .. }
                    if replay_blocks.contains_key(id)
            )
        })
    }

    fn messages_use_sampled_tool_replay(messages: &[Message]) -> bool {
        messages.iter().any(Self::message_uses_sampled_tool_replay)
    }

    fn text_contains_local_tool_call_tag(text: &str) -> bool {
        text.contains(Self::LOCAL_PSEUDO_TOOL_CALL_OPEN)
            || text.contains(Self::LOCAL_PSEUDO_TOOL_CALL_CLOSE)
    }

    fn should_use_non_stream_local_continuous_text_step(
        is_local: bool,
        extra_params: Option<&serde_json::Value>,
    ) -> bool {
        is_local
            && extra_params
                .and_then(|params| {
                    params
                        .get("force_non_stream_local_continuous_text_step")
                        .and_then(|value| value.as_bool())
                        .or_else(|| {
                            params
                                .get("local_continuous_text_step_non_stream")
                                .and_then(|value| value.as_bool())
                        })
                })
                .unwrap_or_else(|| {
                    std::env::var("BENSHU_LOCAL_CONTINUOUS_TEXT_NON_STREAM")
                        .ok()
                        .is_some_and(|value| {
                            matches!(
                                value.trim().to_ascii_lowercase().as_str(),
                                "1" | "true" | "yes" | "on"
                            )
                        })
                })
    }

    fn normalized_local_gate_key(base_url: &str) -> String {
        base_url
            .trim()
            .trim_end_matches('/')
            .to_ascii_lowercase()
            .to_string()
    }

    fn local_request_gate_for_base_url(base_url: &str) -> Arc<Semaphore> {
        static LOCAL_REQUEST_GATES: OnceLock<Mutex<HashMap<String, Arc<Semaphore>>>> =
            OnceLock::new();
        let key = Self::normalized_local_gate_key(base_url);
        let gates = LOCAL_REQUEST_GATES.get_or_init(|| Mutex::new(HashMap::new()));
        let mut guard = gates
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard
            .entry(key)
            .or_insert_with(|| Arc::new(Semaphore::new(1)))
            .clone()
    }

    async fn acquire_local_request_permit(&self) -> Result<Option<OwnedSemaphorePermit>> {
        let Some(gate) = &self.local_request_gate else {
            return Ok(None);
        };
        gate.clone()
            .acquire_owned()
            .await
            .map(Some)
            .map_err(|e| Error::Internal(format!("Local provider request gate closed: {}", e)))
    }

    fn summarize_request_messages(messages: &[OpenAIMessage]) -> String {
        messages
            .iter()
            .enumerate()
            .rev()
            .take(8)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|(idx, message)| {
                let content_preview = if message.content.is_null() {
                    "<null>".to_string()
                } else if let Some(text) = message.content.as_str() {
                    let trimmed = text.trim();
                    if trimmed.is_empty() {
                        "<empty>".to_string()
                    } else {
                        let preview = trimmed.chars().take(120).collect::<String>();
                        if trimmed.chars().count() > 120 {
                            format!("{preview}...")
                        } else {
                            preview
                        }
                    }
                } else if message.content.is_array() {
                    "[multipart]".to_string()
                } else {
                    let serialized = serde_json::to_string(&message.content)
                        .unwrap_or_else(|_| "<unserializable>".to_string());
                    let preview = serialized.chars().take(120).collect::<String>();
                    if serialized.chars().count() > 120 {
                        format!("{preview}...")
                    } else {
                        preview
                    }
                };

                let tool_call_count = message
                    .tool_calls
                    .as_ref()
                    .map(|calls| calls.len())
                    .unwrap_or(0);
                format!(
                    "#{idx} role={} tool_call_id={} tool_calls={} content={}",
                    message.role,
                    message.tool_call_id.as_deref().unwrap_or("<none>"),
                    tool_call_count,
                    content_preview.replace('\n', "\\n")
                )
            })
            .collect::<Vec<_>>()
            .join(" | ")
    }

    fn dump_failed_request_snapshot(path: &str, request: &OpenAIChatRequest, response_body: &str) {
        let snapshot = serde_json::json!({
            "model": request.model,
            "stream": request.stream,
            "tool_count": request.tools.len(),
            "messages": request.messages,
            "response_body": response_body,
        });

        if let Ok(serialized) = serde_json::to_string_pretty(&snapshot) {
            let _ = std::fs::write(path, serialized);
        }
    }

    fn message_content_contains_multimodal_parts(content: &serde_json::Value) -> bool {
        let Some(parts) = content.as_array() else {
            return false;
        };

        parts.iter().any(|part| {
            part.get("type")
                .and_then(|value| value.as_str())
                .is_some_and(|kind| matches!(kind, "image_url" | "input_audio"))
        })
    }

    fn infer_local_media_type(path: &Path) -> &'static str {
        match path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref()
        {
            Some("png") => "image/png",
            Some("jpg" | "jpeg") => "image/jpeg",
            Some("webp") => "image/webp",
            Some("gif") => "image/gif",
            Some("bmp") => "image/bmp",
            Some("wav") => "audio/wav",
            Some("mp3") => "audio/mpeg",
            Some("ogg") => "audio/ogg",
            Some("m4a") => "audio/mp4",
            Some("flac") => "audio/flac",
            _ => "application/octet-stream",
        }
    }

    fn resolve_local_media_path(uri: &str) -> Option<std::path::PathBuf> {
        if uri.starts_with("file://") {
            let url = reqwest::Url::parse(uri).ok()?;
            return url.to_file_path().ok();
        }

        let path = Path::new(uri);
        if path.is_absolute() && path.exists() {
            return Some(path.to_path_buf());
        }

        None
    }

    fn local_file_data_url(uri: &str) -> Option<String> {
        let path = Self::resolve_local_media_path(uri)?;
        if std::fs::metadata(&path).ok()?.len() > MAX_LOCAL_DATA_URL_MEDIA_BYTES {
            return None;
        }
        let bytes = std::fs::read(&path).ok()?;
        let media_type = Self::infer_local_media_type(&path);
        use base64::Engine;
        let data = base64::engine::general_purpose::STANDARD.encode(bytes);
        Some(format!("data:{};base64,{}", media_type, data))
    }

    fn host_looks_local(host: &str) -> bool {
        let host = host.trim().trim_matches(['[', ']']);
        if host.is_empty() {
            return false;
        }

        let host_lower = host.to_ascii_lowercase();
        if host_lower == "localhost"
            || host_lower.ends_with(".local")
            || host_lower.contains("ollama")
            || host_lower.contains("llama")
            || host_lower.contains("candle")
        {
            return true;
        }

        let Ok(ip) = host.parse::<IpAddr>() else {
            return false;
        };

        match ip {
            IpAddr::V4(ipv4) => {
                ipv4.is_loopback()
                    || ipv4.is_private()
                    || ipv4.is_link_local()
                    || ipv4.octets()[0] == 127
                    || ipv4 == Ipv4Addr::new(0, 0, 0, 0)
            }
            IpAddr::V6(ipv6) => {
                ipv6.is_loopback()
                    || ipv6.is_unique_local()
                    || ipv6.is_unicast_link_local()
                    || ipv6 == Ipv6Addr::UNSPECIFIED
            }
        }
    }

    fn extract_pseudo_tool_calls(content: &str) -> Vec<OpenAIToolCall> {
        let mut calls = Vec::new();
        let mut remaining = content;
        let mut ordinal = 0usize;

        while let Some(start_idx) = remaining.find(Self::LOCAL_PSEUDO_TOOL_CALL_OPEN) {
            let after_open = &remaining[start_idx + Self::LOCAL_PSEUDO_TOOL_CALL_OPEN.len()..];
            let Some(end_idx) = after_open.find(Self::LOCAL_PSEUDO_TOOL_CALL_CLOSE) else {
                break;
            };

            let body = after_open[..end_idx].trim();
            if let Some((name, arguments)) = Self::parse_pseudo_tool_call_body(body) {
                ordinal += 1;
                calls.push(OpenAIToolCall {
                    id: format!("pseudo-tool-call-{ordinal}"),
                    call_type: "function".to_string(),
                    function: OpenAIFunction {
                        name,
                        arguments: arguments.to_string(),
                    },
                });
            }

            remaining = &after_open[end_idx + Self::LOCAL_PSEUDO_TOOL_CALL_CLOSE.len()..];
        }

        calls
    }

    fn extract_visible_candidate_from_reasoning(reasoning: &str) -> Option<String> {
        if let Some(candidate) =
            Self::extract_structured_visible_candidate_from_reasoning(reasoning)
        {
            return Some(candidate);
        }

        let mut candidates = Vec::new();
        let mut current = String::new();
        let mut in_quote = false;
        let mut escaped = false;

        for ch in reasoning.chars() {
            if in_quote {
                if escaped {
                    current.push(match ch {
                        'n' => '\n',
                        't' => '\t',
                        'r' => '\r',
                        other => other,
                    });
                    escaped = false;
                    continue;
                }
                match ch {
                    '\\' => escaped = true,
                    '"' => {
                        let candidate = current.trim();
                        if Self::reasoning_visible_candidate_looks_safe(candidate) {
                            candidates.push(candidate.to_string());
                        }
                        current.clear();
                        in_quote = false;
                    }
                    other => current.push(other),
                }
            } else if ch == '"' {
                in_quote = true;
                escaped = false;
                current.clear();
            }
        }

        candidates.into_iter().last()
    }

    fn extract_structured_visible_candidate_from_reasoning(reasoning: &str) -> Option<String> {
        let normalized = reasoning.replace("\r\n", "\n");
        let start_markers = ["[Document Metadata]", "### ", "# ", "标题：", "文档标题："];
        let mut starts = start_markers
            .iter()
            .flat_map(|marker| normalized.match_indices(marker).map(|(idx, _)| idx))
            .collect::<Vec<_>>();
        starts.sort_unstable();
        starts.dedup();

        starts.into_iter().find_map(|start| {
            let candidate = Self::trim_structured_candidate_to_visible_tail(&normalized[start..])?;
            Self::structured_reasoning_candidate_looks_safe(&candidate).then_some(candidate)
        })
    }

    fn trim_structured_candidate_to_visible_tail(text: &str) -> Option<String> {
        let next_hook_start = text.find("下一步钩子：")?;
        let next_hook_tail = &text[next_hook_start..];
        let next_hook_line_len = next_hook_tail.find('\n').unwrap_or(next_hook_tail.len());
        let end = next_hook_start + next_hook_line_len;
        Some(text[..end].trim().to_string())
    }

    fn structured_reasoning_candidate_looks_safe(candidate: &str) -> bool {
        if candidate.chars().count() < 120 {
            return false;
        }
        if !candidate.contains("连续性记录：") || !candidate.contains("下一步钩子：") {
            return false;
        }
        let trimmed = candidate.trim_start();
        if !trimmed.starts_with("[Document Metadata]")
            && !trimmed.starts_with("### ")
            && !trimmed.starts_with("# ")
            && !trimmed.starts_with("标题：")
            && !trimmed.starts_with("文档标题：")
        {
            return false;
        }

        let lowered_prefix = trimmed
            .chars()
            .take(400)
            .collect::<String>()
            .to_ascii_lowercase();
        let reasoning_markers = [
            "task:",
            "requirement:",
            "constraint:",
            "draft:",
            "word count:",
            "let's",
            "wait,",
            "the prompt",
            "i should",
            "analysis:",
            "reasoning:",
            "check:",
        ];
        !reasoning_markers
            .iter()
            .any(|marker| lowered_prefix.contains(marker))
    }

    fn reasoning_visible_candidate_looks_safe(candidate: &str) -> bool {
        if candidate.chars().count() < 16 {
            return false;
        }
        let lowered = candidate.to_ascii_lowercase();
        let reasoning_markers = [
            "task:",
            "requirement",
            "constraint",
            "draft",
            "word count",
            "let's",
            "wait,",
            "the prompt",
            "i should",
            "analysis",
            "reasoning",
            "check:",
        ];
        if reasoning_markers
            .iter()
            .any(|marker| lowered.contains(marker))
        {
            return false;
        }
        let visible_markers = ["连续性记录：", "下一步钩子：", "###", "# ", "。", ".", "\n"];
        visible_markers
            .iter()
            .any(|marker| candidate.contains(marker))
    }

    fn parse_pseudo_tool_call_body(body: &str) -> Option<(String, serde_json::Value)> {
        let stripped = body.strip_prefix("call:")?.trim();
        let args_start = match (stripped.find('{'), stripped.find('(')) {
            (Some(brace), Some(paren)) => brace.min(paren),
            (Some(brace), None) => brace,
            (None, Some(paren)) => paren,
            (None, None) => return None,
        };
        let tool_head = stripped[..args_start].trim().trim_end_matches(':');
        let tool_name = tool_head.rsplit(':').next()?.trim();
        if tool_name.is_empty() {
            return None;
        }

        let args_literal = stripped[args_start..].trim();
        let arguments = if args_literal.starts_with('{') {
            serde_yaml_ng::from_str::<serde_json::Value>(args_literal)
                .or_else(|_| serde_json::from_str::<serde_json::Value>(args_literal))
                .ok()?
        } else {
            Self::parse_parenthesized_pseudo_tool_args(args_literal)?
        };

        Some((tool_name.to_string(), arguments))
    }

    fn parse_parenthesized_pseudo_tool_args(args_literal: &str) -> Option<serde_json::Value> {
        let inner = args_literal.strip_prefix('(')?.strip_suffix(')')?.trim();
        let mut object = serde_json::Map::new();
        if inner.is_empty() {
            return Some(serde_json::Value::Object(object));
        }

        for pair in Self::split_pseudo_tool_arg_pairs(inner) {
            let Some((key, value)) = pair.split_once('=') else {
                continue;
            };
            let key = key.trim().trim_matches(['"', '\'']);
            if key.is_empty() {
                continue;
            }
            object.insert(
                key.to_string(),
                Self::parse_pseudo_tool_arg_value(value.trim()),
            );
        }

        Some(serde_json::Value::Object(object))
    }

    fn split_pseudo_tool_arg_pairs(input: &str) -> Vec<String> {
        let mut parts = Vec::new();
        let mut current = String::new();
        let mut quote: Option<char> = None;
        let mut escape = false;
        let mut depth = 0usize;

        for ch in input.chars() {
            if escape {
                current.push(ch);
                escape = false;
                continue;
            }

            if ch == '\\' {
                current.push(ch);
                escape = true;
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
                '"' | '\'' => {
                    quote = Some(ch);
                    current.push(ch);
                }
                '[' | '{' | '(' => {
                    depth += 1;
                    current.push(ch);
                }
                ']' | '}' | ')' => {
                    depth = depth.saturating_sub(1);
                    current.push(ch);
                }
                ',' if depth == 0 => {
                    if !current.trim().is_empty() {
                        parts.push(current.trim().to_string());
                    }
                    current.clear();
                }
                _ => current.push(ch),
            }
        }

        if !current.trim().is_empty() {
            parts.push(current.trim().to_string());
        }

        parts
    }

    fn parse_pseudo_tool_arg_value(value: &str) -> serde_json::Value {
        let trimmed = value.trim();
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(trimmed) {
            return parsed;
        }
        if let Ok(parsed) = serde_yaml_ng::from_str::<serde_json::Value>(trimmed) {
            return parsed;
        }

        serde_json::Value::String(
            trimmed
                .trim_matches(|ch| ch == '"' || ch == '\'')
                .to_string(),
        )
    }

    /// Create from API key
    pub fn new(api_key: impl Into<String>) -> Result<Self> {
        Self::with_base_url(api_key, "https://api.openai.com/v1")
    }

    /// Create from environment variable
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("OPENAI_API_KEY")
            .map_err(|_| Error::ProviderAuth("OPENAI_API_KEY not set".to_string()))?;
        crate::utils::validate_api_key(&api_key, "openai")?;
        tracing::info!(
            "Initializing OpenAI from environment with key: {}",
            crate::utils::mask_api_key(&api_key)
        );
        Self::new(api_key)
    }

    /// Create with custom base URL (for compatible APIs)
    pub fn with_base_url(api_key: impl Into<String>, base_url: impl Into<String>) -> Result<Self> {
        let base_url = base_url.into();
        let mut config = HttpConfig::default();
        if Self::base_url_looks_local(&base_url) {
            config.timeout_secs = config.timeout_secs.max(LOCAL_OPENAI_COMPAT_TIMEOUT_SECS);
        }
        let client = config.build_client()?;
        let local_request_gate = if Self::base_url_looks_local(&base_url) {
            Some(Self::local_request_gate_for_base_url(&base_url))
        } else {
            None
        };

        Ok(Self {
            client,
            api_key: api_key.into(),
            base_url,
            local_request_gate,
        })
    }

    fn base_url_looks_local(base_url: &str) -> bool {
        let Ok(parsed) = reqwest::Url::parse(base_url) else {
            return false;
        };
        parsed.host_str().is_some_and(Self::host_looks_local)
    }

    /// Create for Groq
    pub fn groq(api_key: impl Into<String>) -> Result<Self> {
        Self::with_base_url(api_key, "https://api.groq.com/openai/v1")
    }

    /// Create for Mistral
    pub fn mistral(api_key: impl Into<String>) -> Result<Self> {
        Self::with_base_url(api_key, "https://api.mistral.ai/v1")
    }

    /// Create for MiniMax
    pub fn minimax(api_key: impl Into<String>) -> Result<Self> {
        Self::with_base_url(api_key, "https://api.minimax.io/v1")
    }

    fn build_headers(&self) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", self.api_key))
                .map_err(|e| Error::Internal(e.to_string()))?,
        );
        if self.is_local() {
            headers.insert(CONNECTION, HeaderValue::from_static("close"));
        }
        Ok(headers)
    }

    /// Create embeddings for a piece of text
    pub async fn create_embedding(&self, text: &str) -> Result<Vec<f32>> {
        let request = serde_json::json!({
            "model": "text-embedding-ada-002",
            "input": text,
        });

        let response = self
            .client
            .post(format!("{}/embeddings", self.base_url))
            .headers(self.build_headers()?)
            .json(&request)
            .send()
            .await
            .map_err(|e| Error::Http(e))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(Error::ProviderApi(format!(
                "OpenAI Embedding Error {}: {}",
                status, text
            )));
        }

        let res_json: serde_json::Value = response.json().await.map_err(|e| Error::Http(e))?;

        let embedding = res_json["data"][0]["embedding"]
            .as_array()
            .ok_or_else(|| {
                Error::Internal(
                    "Invalid embedding response structure: missing data[0].embedding".to_string(),
                )
            })?
            .iter()
            .map(|v| v.as_f64().unwrap_or(0.0) as f32)
            .collect();

        Ok(embedding)
    }

    pub async fn create_image(&self, prompt: &str, size: &str) -> Result<Vec<u8>> {
        let request = serde_json::json!({
            "model": "dall-e-3",
            "prompt": prompt,
            "n": 1,
            "size": size,
            "response_format": "b64_json"
        });

        let response = self
            .client
            .post(format!("{}/images/generations", self.base_url))
            .headers(self.build_headers()?)
            .json(&request)
            .send()
            .await
            .map_err(|e| Error::Http(e))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(Error::ProviderApi(format!(
                "OpenAI Image Generation Error {}: {}",
                status, text
            )));
        }

        let res_json: serde_json::Value = response.json().await.map_err(|e| Error::Http(e))?;

        let b64 = res_json["data"][0]["b64_json"].as_str().ok_or_else(|| {
            Error::Internal("Invalid image generation response structure".to_string())
        })?;

        use base64::Engine;
        let data = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| Error::Internal(format!("Failed to decode base64 image: {}", e)))?;

        Ok(data)
    }
}

#[derive(Debug, Serialize)]
struct OpenAIMessage {
    role: String,
    content: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAIToolCall>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct OpenAIToolCall {
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: OpenAIFunction,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct OpenAIFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Serialize)]
struct OpenAITool {
    #[serde(rename = "type")]
    tool_type: String,
    function: OpenAIToolFunction,
}

#[derive(Debug, Serialize)]
struct OpenAIToolFunction {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct OpenAIChatRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<OpenAITool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<serde_json::Value>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<StreamOptions>,
}

#[derive(Debug, Serialize)]
struct StreamOptions {
    include_usage: bool,
}

/// Streaming chunk from OpenAI
#[derive(Debug, Deserialize)]
struct StreamChunk {
    choices: Vec<StreamChoice>,
    usage: Option<OpenAIUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAIUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    delta: StreamDelta,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamDelta {
    content: Option<String>,
    tool_calls: Option<Vec<StreamToolCall>>,
}

#[derive(Debug, Deserialize)]
struct StreamToolCall {
    index: Option<usize>,
    id: Option<String>,
    function: Option<StreamFunction>,
}

#[derive(Debug, Deserialize)]
struct StreamFunction {
    name: Option<String>,
    arguments: Option<String>,
}

impl OpenAI {
    fn convert_messages(system_prompt: Option<&str>, messages: Vec<Message>) -> Vec<OpenAIMessage> {
        let mut result = Vec::with_capacity(messages.len() + 1);

        // Add system message if present
        if let Some(prompt) = system_prompt {
            result.push(OpenAIMessage {
                role: "system".to_string(),
                content: serde_json::Value::String(prompt.to_string()),
                name: None,
                tool_call_id: None,
                tool_calls: None,
            });
        }

        // Convert messages
        for msg in messages {
            let replay_blocks = Self::tool_replay_blocks_from_metadata(&msg.metadata);
            let role = match msg.role {
                Role::System => "system",
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::Tool => "tool",
            };

            let mut tool_calls = Vec::new();
            let mut tool_call_id = None;
            let final_content: serde_json::Value;
            match msg.content {
                Content::Text(text) => {
                    final_content = serde_json::Value::String(text);
                }
                Content::Fact { fact } => {
                    final_content = serde_json::Value::String(format!(
                        "[Fact: {}] {}",
                        fact.category, fact.content
                    ));
                }
                Content::SystemNotification { notice } => {
                    final_content = serde_json::Value::String(format!("[System] {}", notice));
                }
                Content::Cancelled { reason } => {
                    final_content =
                        serde_json::Value::String(format!("[Cancelled] Reason: {}", reason));
                }
                Content::Parts(parts) => {
                    let mut json_parts = Vec::new();
                    let mut text_acc = String::new();

                    for part in parts {
                        match part {
                            benshu_protocol_core::ContentPart::Text { text } => {
                                text_acc.push_str(&text);
                                json_parts.push(serde_json::json!({
                                    "type": "text",
                                    "text": text
                                }));
                            }
                            benshu_protocol_core::ContentPart::Image { source } => {
                                // Fix #8: Support Images (Url and Base64)
                                let url = match source {
                                    benshu_protocol_core::ImageSource::Url { url } => {
                                        Self::local_file_data_url(&url).unwrap_or(url)
                                    }
                                    benshu_protocol_core::ImageSource::Base64 {
                                        media_type,
                                        data,
                                    } => {
                                        format!("data:{};base64,{}", media_type, data)
                                    }
                                };

                                if url.starts_with("file://") {
                                    tracing::warn!(
                                        "OpenAI provider is about to forward a raw file:// image URL; local inlining did not happen"
                                    );
                                } else if Path::new(&url).is_absolute() {
                                    tracing::warn!(
                                        "OpenAI provider is about to forward a raw absolute local image path; local inlining did not happen"
                                    );
                                } else if url.starts_with("data:") {
                                    tracing::info!(
                                        "OpenAI provider inlined local image input as data URL for multimodal request"
                                    );
                                }

                                json_parts.push(serde_json::json!({
                                    "type": "image_url",
                                    "image_url": {
                                        "url": url
                                        // "detail": "auto" // Default
                                    }
                                }));
                            }
                            benshu_protocol_core::ContentPart::ToolCall {
                                id,
                                name,
                                arguments,
                            } => {
                                if let Some(block) = replay_blocks.get(&id) {
                                    if !text_acc.contains(block) {
                                        if !text_acc.trim().is_empty() {
                                            text_acc.push('\n');
                                        }
                                        text_acc.push_str(block);
                                    }
                                }
                                tool_calls.push(OpenAIToolCall {
                                    id,
                                    call_type: "function".to_string(),
                                    function: OpenAIFunction {
                                        name,
                                        arguments: arguments.to_string(),
                                    },
                                });
                            }
                            benshu_protocol_core::ContentPart::ToolResult {
                                tool_call_id: id,
                                content,
                                ..
                            } => {
                                tool_call_id = Some(id);
                                text_acc = content; // Tool result content is simple string usually
                            }
                            benshu_protocol_core::ContentPart::Audio { source } => {
                                let url = match source {
                                    benshu_protocol_core::AudioSource::Url { url } => url,
                                    benshu_protocol_core::AudioSource::Base64 {
                                        media_type: _,
                                        data,
                                    } => data,
                                };
                                json_parts.push(serde_json::json!({
                                    "type": "input_audio",
                                    "input_audio": {
                                        "data": url,
                                        "format": "wav"
                                    }
                                }));
                            }
                            benshu_protocol_core::ContentPart::Video { source } => {
                                let url = match source {
                                    benshu_protocol_core::VideoSource::Url { url } => url,
                                    benshu_protocol_core::VideoSource::Base64 {
                                        media_type,
                                        data,
                                    } => {
                                        format!("data:{};base64,{}", media_type, data)
                                    }
                                };
                                text_acc.push_str(&format!("\n[Video Attachment: {}]\n", url));
                            }
                        }
                    }

                    if tool_call_id.is_some() || (!tool_calls.is_empty()) {
                        // If tool related, content is usually null or the text string
                        if text_acc.is_empty() {
                            final_content = serde_json::Value::Null;
                        } else {
                            final_content = serde_json::Value::String(text_acc);
                        }
                    } else if json_parts
                        .iter()
                        .any(|p| p["type"] == "image_url" || p["type"] == "input_audio")
                    {
                        // Multi-modal content
                        final_content = serde_json::Value::Array(json_parts);
                    } else {
                        // Simple text
                        final_content = serde_json::Value::String(text_acc);
                    }
                }
            }

            result.push(OpenAIMessage {
                role: role.to_string(),
                content: final_content,
                name: msg.name,
                tool_call_id,
                tool_calls: if tool_calls.is_empty() {
                    None
                } else {
                    Some(tool_calls)
                },
            });
        }

        Self::normalize_system_messages_for_chat_template(result)
    }

    fn normalize_system_messages_for_chat_template(
        messages: Vec<OpenAIMessage>,
    ) -> Vec<OpenAIMessage> {
        let mut system_parts = Vec::new();
        let mut non_system = Vec::new();

        for message in messages {
            if message.role == "system" {
                let content = if let Some(text) = message.content.as_str() {
                    text.trim().to_string()
                } else if message.content.is_null() {
                    String::new()
                } else {
                    message.content.to_string()
                };
                if !content.is_empty() {
                    system_parts.push(content);
                }
            } else {
                non_system.push(message);
            }
        }

        if system_parts.is_empty() {
            return non_system;
        }

        let mut normalized = Vec::with_capacity(non_system.len() + 1);
        normalized.push(OpenAIMessage {
            role: "system".to_string(),
            content: serde_json::Value::String(system_parts.join("\n\n")),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        });
        normalized.extend(non_system);
        normalized
    }

    fn sanitize_local_tool_history_for_thinking_runtime(
        messages: Vec<OpenAIMessage>,
    ) -> Vec<OpenAIMessage> {
        messages
            .into_iter()
            .map(|mut message| {
                if message.role == "assistant" && message.tool_calls.is_some() {
                    let mut transcript = String::new();
                    if let Some(text) = message.content.as_str() {
                        if !text.trim().is_empty() {
                            transcript.push_str(text.trim());
                            transcript.push('\n');
                        }
                    }

                    if let Some(tool_calls) = message.tool_calls.take() {
                        if !Self::text_contains_local_tool_call_tag(&transcript) {
                            for tool_call in tool_calls {
                                transcript.push_str(&format!(
                                    "[Assistant tool request] {}({})\n",
                                    tool_call.function.name, tool_call.function.arguments
                                ));
                            }
                        }
                    }

                    message.role = "user".to_string();
                    message.content = serde_json::Value::String(transcript.trim().to_string());
                    message.tool_call_id = None;
                    message.name = None;
                    return message;
                }

                if message.role == "tool" {
                    let tool_call_id = message.tool_call_id.take().unwrap_or_default();
                    let content = message
                        .content
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| message.content.to_string());

                    message.role = "user".to_string();
                    message.content = serde_json::Value::String(format!(
                        "[Tool result for {tool_call_id}]\n{content}"
                    ));
                    message.tool_calls = None;
                    message.name = None;
                }

                message
            })
            .collect()
    }

    fn sanitize_local_text_step_messages(messages: Vec<OpenAIMessage>) -> Vec<OpenAIMessage> {
        messages
            .into_iter()
            .map(|mut message| {
                if let Some(text) = message.content.as_str() {
                    message.content =
                        serde_json::Value::String(Self::strip_local_runtime_channel_markers(text));
                }
                message
            })
            .collect()
    }

    fn strip_local_runtime_channel_markers(text: &str) -> String {
        text.replace("<|channel>thought\n<channel|>", "")
            .replace("<|channel>thought\r\n<channel|>", "")
            .replace("<|channel>analysis\n<channel|>", "")
            .replace("<|channel>analysis\r\n<channel|>", "")
            .replace("<|channel>final\n<channel|>", "")
            .replace("<|channel>final\r\n<channel|>", "")
            .replace("<|channel>commentary\n<channel|>", "")
            .replace("<|channel>commentary\r\n<channel|>", "")
            .replace("<|channel>thought", "")
            .replace("<|channel>analysis", "")
            .replace("<|channel>final", "")
            .replace("<|channel>commentary", "")
            .replace("<channel|>", "")
            .replace('\u{fffd}', "")
    }

    fn sanitize_visible_message_text(text: &str) -> String {
        let mut cleaned = text.to_string();
        for marker in [
            "<think>\n\n</think>",
            "<think>\r\n\r\n</think>",
            "<think></think>",
            "<think>\n</think>",
            "<think>\r\n</think>",
        ] {
            cleaned = cleaned.replace(marker, "");
        }
        cleaned.trim_start().to_string()
    }

    fn convert_tools(tools: Vec<ToolDefinition>) -> Vec<OpenAITool> {
        tools
            .into_iter()
            .map(|t| {
                let description = if let Some(ts) = &t.parameters_ts {
                    format!("{}\n\nUse this TypeScript interface for parameter structure:\n```typescript\n{}\n```", t.description, ts)
                } else {
                    t.description.clone()
                };

                OpenAITool {
                    tool_type: "function".to_string(),
                    function: OpenAIToolFunction {
                        name: t.name,
                        description,
                        parameters: t.parameters,
                    },
                }
            })
            .collect()
    }

    fn local_request_can_use_native_tools(messages: &[OpenAIMessage]) -> bool {
        messages.iter().any(|message| message.role == "user")
    }

    #[cfg(test)]
    fn stream_from_nonstream_response(body: &str) -> Result<StreamingResponse> {
        Self::stream_from_nonstream_response_with_replay(body, false)
    }

    fn stream_from_nonstream_response_with_replay(
        body: &str,
        tool_exact_replay_used: bool,
    ) -> Result<StreamingResponse> {
        let parsed: serde_json::Value = serde_json::from_str(body).map_err(|e| {
            Error::ProviderApi(format!(
                "Failed to parse non-stream response (first 500 chars): {}: {}",
                &body[..body.len().min(500)],
                e
            ))
        })?;

        let mut builder = benshu_provider_core::MockStreamBuilder::new();
        let native_tool_calls = parsed
            .get("choices")
            .and_then(|choices| choices.get(0))
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("tool_calls"))
            .and_then(|value| serde_json::from_value::<Vec<OpenAIToolCall>>(value.clone()).ok())
            .unwrap_or_default();

        let pseudo_tool_calls = if native_tool_calls.is_empty() {
            parsed
                .get("choices")
                .and_then(|choices| choices.get(0))
                .and_then(|choice| choice.get("message"))
                .and_then(|message| message.get("content"))
                .and_then(|value| value.as_str())
                .map(Self::extract_pseudo_tool_calls)
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        let effective_tool_calls = if native_tool_calls.is_empty() {
            pseudo_tool_calls
        } else {
            native_tool_calls
        };

        if !effective_tool_calls.is_empty() {
            for tool_call in &effective_tool_calls {
                let arguments =
                    serde_json::from_str::<serde_json::Value>(&tool_call.function.arguments)
                        .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new()));
                builder = builder.tool_call(
                    tool_call.id.clone(),
                    tool_call.function.name.clone(),
                    arguments,
                );
            }
        }

        if effective_tool_calls.is_empty() {
            let mut has_visible_text = false;
            let message = parsed
                .get("choices")
                .and_then(|choices| choices.get(0))
                .and_then(|choice| choice.get("message"))
                .cloned();
            if let Some(content) = message.as_ref().and_then(|message| message.get("content")) {
                match content {
                    serde_json::Value::String(text) => {
                        let text = Self::sanitize_visible_message_text(text);
                        if !text.is_empty() {
                            has_visible_text = true;
                            builder = builder.message(text);
                        }
                    }
                    serde_json::Value::Array(parts) => {
                        for part in parts {
                            if let Some(text) = part.get("text").and_then(|value| value.as_str()) {
                                let text = Self::sanitize_visible_message_text(text);
                                if !text.is_empty() {
                                    has_visible_text = true;
                                    builder = builder.message(text);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            if !has_visible_text {
                if let Some(candidate) = message
                    .as_ref()
                    .and_then(|message| message.get("reasoning_content"))
                    .and_then(|value| value.as_str())
                    .and_then(Self::extract_visible_candidate_from_reasoning)
                {
                    tracing::warn!(
                        "OpenAI-compatible response had empty visible content; recovered quoted visible candidate from reasoning_content"
                    );
                    builder = builder.message(candidate);
                }
            }
        }

        if let Some(usage) = parsed.get("usage") {
            let prompt_tokens = usage
                .get("prompt_tokens")
                .and_then(|value| value.as_u64())
                .unwrap_or(0) as u32;
            let completion_tokens = usage
                .get("completion_tokens")
                .and_then(|value| value.as_u64())
                .unwrap_or(0) as u32;
            let total_tokens = usage
                .get("total_tokens")
                .and_then(|value| value.as_u64())
                .unwrap_or((prompt_tokens + completion_tokens) as u64)
                as u32;
            builder = builder.usage(Usage {
                prompt_tokens,
                completion_tokens,
                total_tokens,
            });
        }

        if let Some(finish_reason) = parsed
            .get("choices")
            .and_then(|choices| choices.get(0))
            .and_then(|choice| choice.get("finish_reason"))
            .and_then(|value| value.as_str())
        {
            let finish = match finish_reason {
                "length" => FinishReason::Length,
                "tool_calls" => FinishReason::ToolCalls,
                "content_filter" => FinishReason::ContentFilter,
                _ => FinishReason::Stop,
            };
            builder = builder.finish(finish);
        } else if !effective_tool_calls.is_empty() {
            builder = builder.finish(FinishReason::ToolCalls);
        }

        let mut continuation = Self::continuation_telemetry_from_response(&parsed);
        if tool_exact_replay_used {
            let telemetry = continuation.get_or_insert_with(|| ContinuationTelemetry {
                mode: "tool_replay".to_string(),
                cache_source: "message_history".to_string(),
                ..Default::default()
            });
            telemetry.tool_exact_replay_used = true;
        }

        builder = builder.telemetry(ProviderTelemetry {
            provider_name: Some("openai".to_string()),
            model: parsed
                .get("model")
                .and_then(|value| value.as_str())
                .map(str::to_string),
            continuation,
            extra: parsed
                .get("id")
                .and_then(|value| value.as_str())
                .map(|id| {
                    let mut extra = std::collections::HashMap::new();
                    extra.insert("request_id".to_string(), id.to_string());
                    extra
                })
                .unwrap_or_default(),
            latency_ms: None,
        });

        Ok(builder.done().build())
    }

    fn continuation_telemetry_from_response(
        parsed: &serde_json::Value,
    ) -> Option<ContinuationTelemetry> {
        let usage = parsed.get("usage");
        let timings = parsed.get("timings");
        if usage.is_none() && timings.is_none() {
            return None;
        }

        let prompt_tokens = usage
            .and_then(|usage| usage.get("prompt_tokens"))
            .and_then(|value| value.as_u64())
            .map(|value| value as u32)
            .or_else(|| {
                timings
                    .and_then(|timings| timings.get("prompt_n"))
                    .and_then(|value| value.as_u64())
                    .map(|value| value as u32)
            });

        Some(ContinuationTelemetry {
            mode: "openai_compatible_provider_reported".to_string(),
            cache_source: "provider_usage".to_string(),
            prompt_tokens,
            prefill_ms: timings
                .and_then(|timings| timings.get("prompt_ms"))
                .and_then(|value| value.as_f64())
                .map(|value| value.max(0.0) as u64),
            decode_ms: timings
                .and_then(|timings| timings.get("predicted_ms"))
                .and_then(|value| value.as_f64())
                .map(|value| value.max(0.0) as u64),
            ..Default::default()
        })
    }

    fn continuation_telemetry_from_stream_usage(usage: &OpenAIUsage) -> ContinuationTelemetry {
        ContinuationTelemetry {
            mode: "openai_compatible_provider_reported".to_string(),
            cache_source: "provider_stream_usage".to_string(),
            prompt_tokens: Some(usage.prompt_tokens),
            ..Default::default()
        }
    }

    fn estimated_chat_request_prompt_tokens(request: &OpenAIChatRequest) -> u32 {
        let message_chars = request
            .messages
            .iter()
            .map(|message| {
                message.role.chars().count()
                    + serde_json::to_string(&message.content)
                        .map(|text| text.chars().count())
                        .unwrap_or_default()
            })
            .sum::<usize>();
        let tool_chars = request
            .tools
            .iter()
            .map(|tool| {
                serde_json::to_string(tool)
                    .map(|text| text.len())
                    .unwrap_or(0)
            })
            .sum::<usize>();
        ((message_chars + tool_chars) / 4).max(1) as u32
    }

    fn largest_request_section_label(request: &OpenAIChatRequest) -> String {
        let mut largest_label = "messages".to_string();
        let mut largest_chars = 0usize;
        for (index, message) in request.messages.iter().enumerate() {
            let chars = serde_json::to_string(&message.content)
                .map(|text| text.chars().count())
                .unwrap_or_default();
            if chars > largest_chars {
                largest_chars = chars;
                largest_label = format!("message[{index}].{}", message.role);
            }
        }
        let tool_chars = request
            .tools
            .iter()
            .map(|tool| {
                serde_json::to_string(tool)
                    .map(|text| text.len())
                    .unwrap_or(0)
            })
            .sum::<usize>();
        if tool_chars > largest_chars {
            "tools".to_string()
        } else {
            largest_label
        }
    }

    fn context_limit_error_from_provider_failure(
        request: &OpenAIChatRequest,
        configured_context_tokens: usize,
        response_text: &str,
    ) -> Option<ContextLimitError> {
        if !ContextLimitError::looks_like_context_limit_message(response_text) {
            return None;
        }

        let configured_context_tokens = u32::try_from(configured_context_tokens)
            .unwrap_or(u32::MAX)
            .max(1);
        let prompt_tokens = Self::estimated_chat_request_prompt_tokens(request);
        let requested_output_tokens = request
            .max_tokens
            .and_then(|tokens| u32::try_from(tokens).ok())
            .unwrap_or(0);

        Some(
            ContextLimitError::new(
                prompt_tokens,
                configured_context_tokens,
                requested_output_tokens,
            )
            .with_largest_section(Self::largest_request_section_label(request)),
        )
    }
}

#[async_trait]
impl Provider for OpenAI {
    async fn stream_completion(
        &self,
        request: benshu_provider_core::ChatRequest,
    ) -> Result<StreamingResponse> {
        let benshu_provider_core::ChatRequest {
            model,
            system_prompt,
            messages,
            tools,
            temperature,
            max_tokens,
            extra_params,
            enable_cache_control: _,
            ..
        } = request;

        let tool_exact_replay_used = Self::messages_use_sampled_tool_replay(&messages);

        // Check for response_format in extra_params
        let mut response_format = if let Some(params) = &extra_params {
            if let Some(format_val) = params.get("response_format") {
                serde_json::from_value(format_val.clone()).ok()
            } else {
                None
            }
        } else {
            None
        };

        let mut request_messages =
            Self::convert_messages(system_prompt.as_deref(), messages.clone());

        let has_multimodal_request = request_messages
            .iter()
            .any(|message| Self::message_content_contains_multimodal_parts(&message.content));
        let mut effective_system_prompt = system_prompt;
        if self.is_local() && has_multimodal_request && response_format.is_some() {
            tracing::warn!(
                "OpenAI-compatible local multimodal runtime does not reliably support response_format; falling back to prompt-enforced JSON output for this request"
            );
            let json_instruction = "Return exactly one valid JSON object. Do not wrap it in markdown fences. Do not add any explanation before or after the JSON.";
            effective_system_prompt = Some(match effective_system_prompt {
                Some(existing) if !existing.trim().is_empty() => {
                    format!("{existing}\n\n{json_instruction}")
                }
                _ => json_instruction.to_string(),
            });
            request_messages = Self::convert_messages(effective_system_prompt.as_deref(), messages);
            response_format = None;
        }

        if let Ok(serialized_messages) = serde_json::to_string(&request_messages) {
            if serialized_messages.contains("file://")
                || serialized_messages.contains("\"/home/")
                || serialized_messages.contains("\\/home\\/")
            {
                tracing::warn!(
                    "OpenAI provider outgoing request still contains a local file reference"
                );
            }
        }

        // If tools have TS interfaces, we might want to prioritize them.
        // For OpenAI, we still MUST send the JSON schema in the `tools` parameter.
        // However, we can enhance the system prompt or tool descriptions.

        let has_multimodal_parts = request_messages
            .iter()
            .any(|message| Self::message_content_contains_multimodal_parts(&message.content));
        let use_non_stream_local_multimodal = self.is_local() && has_multimodal_parts;
        let use_non_stream_local_tooling = self.is_local() && !tools.is_empty();
        let use_non_stream_local_text_step = Self::should_use_non_stream_local_continuous_text_step(
            self.is_local(),
            extra_params.as_ref(),
        );
        let use_non_stream_local = use_non_stream_local_multimodal
            || use_non_stream_local_tooling
            || use_non_stream_local_text_step;

        if use_non_stream_local_tooling {
            request_messages =
                Self::sanitize_local_tool_history_for_thinking_runtime(request_messages);
        }
        if use_non_stream_local_text_step {
            response_format = None;
            request_messages = Self::sanitize_local_text_step_messages(request_messages);
        }

        let native_tools_allowed =
            !self.is_local() || Self::local_request_can_use_native_tools(&request_messages);
        if self.is_local() && !tools.is_empty() && !native_tools_allowed {
            tracing::warn!(
                "OpenAI-compatible local request had tools but no user message; suppressing native tools to avoid chat-template parser failure"
            );
        }

        let api_request = OpenAIChatRequest {
            model: model.to_string(),
            messages: request_messages,
            temperature,
            max_tokens,
            tools: if native_tools_allowed {
                Self::convert_tools(tools)
            } else {
                Vec::new()
            },
            response_format,
            stream: !use_non_stream_local,
            stream_options: if use_non_stream_local || self.is_local() {
                None
            } else {
                Some(StreamOptions {
                    include_usage: true,
                })
            },
        };

        let local_request_permit = self.acquire_local_request_permit().await?;
        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .headers(self.build_headers()?)
            .json(&api_request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            if let Some(context_error) = Self::context_limit_error_from_provider_failure(
                &api_request,
                self.get_context_window(&api_request.model),
                &text,
            ) {
                return Err(Error::ProviderApi(
                    context_error.to_provider_error_message(),
                ));
            }
            if status.as_u16() == 400
                && text.contains("Assistant response prefill is incompatible with enable_thinking")
            {
                tracing::warn!(
                    request_messages = %Self::summarize_request_messages(&api_request.messages),
                    tool_count = api_request.tools.len(),
                    "OpenAI provider request hit assistant-prefill incompatibility"
                );
                Self::dump_failed_request_snapshot(
                    "/tmp/benshu_openai_provider_prefill_400.json",
                    &api_request,
                    &text,
                );
            }
            return Err(Error::ProviderApi(format!(
                "OpenAI API error {}: {}",
                status, text
            )));
        }

        if use_non_stream_local {
            let body_bytes = response.bytes().await?;
            let body = String::from_utf8_lossy(&body_bytes).into_owned();
            tracing::info!(
                "OpenAI provider used non-stream fallback for local request (multimodal={}, tooling={}, continuous_text_step={})",
                use_non_stream_local_multimodal,
                use_non_stream_local_tooling,
                use_non_stream_local_text_step
            );
            return Self::stream_from_nonstream_response_with_replay(&body, tool_exact_replay_used);
        }

        // Parse SSE stream
        let stream = response.bytes_stream();
        let parsed_stream = parse_sse_stream_with_tool_replay(stream, tool_exact_replay_used);
        if let Some(permit) = local_request_permit {
            let guarded_stream = parsed_stream.map(move |item| {
                let _permit = &permit;
                item
            });
            return Ok(StreamingResponse::from_stream(guarded_stream));
        }

        Ok(StreamingResponse::from_stream(parsed_stream))
    }

    fn name(&self) -> &str {
        "openai"
    }

    fn is_local(&self) -> bool {
        let url = self.base_url.to_lowercase();
        if url.starts_with('/') || url.contains(":\\") {
            return true;
        }

        if let Ok(parsed) = reqwest::Url::parse(&self.base_url) {
            if let Some(host) = parsed.host_str() {
                return Self::host_looks_local(host);
            }
        }

        url.contains("localhost")
            || url.contains("127.0.0.1")
            || url.contains("::1")
            || url.contains(".local")
            || url.contains("ollama")
            || url.contains("llama")
            || url.contains("candle")
    }

    fn tool_contract_mode(&self) -> &'static str {
        "native_tool_calling"
    }

    fn mainline_stability(&self) -> &'static str {
        "stable"
    }

    fn metadata() -> benshu_provider_core::ProviderMetadata {
        benshu_provider_core::ProviderMetadata {
            id: "openai".to_string(),
            name: "OpenAI".to_string(),
            description: "Industry standard LLM provider supporting OpenAI chat, reasoning, vision, tools, and streaming models."
                .to_string(),
            icon: "🤖".to_string(),
            fields: vec![
                benshu_provider_core::ProviderField {
                    key: "OPENAI_API_KEY".to_string(),
                    label: "API Key".to_string(),
                    field_type: "password".to_string(),
                    description: "Your OpenAI API Key".to_string(),
                    required: true,
                    default: None,
                },
                benshu_provider_core::ProviderField {
                    key: "OPENAI_BASE_URL".to_string(),
                    label: "Base URL".to_string(),
                    field_type: "text".to_string(),
                    description: "Optional custom endpoint (e.g. for Groq, OpenRouter)".to_string(),
                    required: false,
                    default: Some("https://api.openai.com/v1".to_string()),
                },
            ],
            capabilities: vec!["vision".into(), "tools".into(), "streaming".into()],
            preferred_models: vec![
                "gpt-5.4-thinking".into(),
                "gpt-5.4-pro".into(),
            ],
        }
    }
}

/// Parse Server-Sent Events stream from OpenAI
fn sse_message_data(message: &str) -> Option<String> {
    let mut lines = Vec::new();
    for line in message.lines() {
        let line = line.trim_end_matches('\r');
        let Some(data) = line.trim_start().strip_prefix("data:") else {
            continue;
        };
        lines.push(data.trim_start().to_string());
    }
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn parse_sse_stream_with_tool_replay<S>(
    stream: S,
    tool_exact_replay_used: bool,
) -> impl Stream<Item = std::result::Result<StreamingChoice, Error>>
where
    S: Stream<Item = std::result::Result<bytes::Bytes, reqwest::Error>> + Send + Unpin + 'static,
{
    // Tool call accumulator state
    struct ToolCallState {
        id: Option<String>,
        name: Option<String>,
        arguments: String,
    }

    let sse_buffer = crate::utils::SseBuffer::new();
    let current_tools: std::collections::HashMap<usize, ToolCallState> =
        std::collections::HashMap::new();
    let pending_messages: std::collections::VecDeque<String> = std::collections::VecDeque::new();
    let pending_choices: std::collections::VecDeque<std::result::Result<StreamingChoice, Error>> =
        std::collections::VecDeque::new();

    futures::stream::unfold(
        (
            stream,
            sse_buffer,
            current_tools,
            pending_messages,
            pending_choices,
        ),
        move |(
            mut stream,
            mut bytes_buffer,
            mut current_tools,
            mut pending_messages,
            mut pending_choices,
        )| async move {
            loop {
                if let Some(choice) = pending_choices.pop_front() {
                    return Some((
                        choice,
                        (
                            stream,
                            bytes_buffer,
                            current_tools,
                            pending_messages,
                            pending_choices,
                        ),
                    ));
                }

                // 1. Process pending messages from buffer first
                if let Some(message) = pending_messages.pop_front() {
                    // Parse the SSE message. OpenAI-compatible servers are not
                    // perfectly consistent: some emit `data: {...}`, some emit
                    // `data:{...}`, and multi-line data frames are legal SSE.
                    if let Some(data) = sse_message_data(&message) {
                        let data = data.trim();
                        if data == "[DONE]" {
                            return Some((
                                Ok(StreamingChoice::Done),
                                (
                                    stream,
                                    bytes_buffer,
                                    current_tools,
                                    pending_messages,
                                    pending_choices,
                                ),
                            ));
                        }

                        match serde_json::from_str::<StreamChunk>(data) {
                            Ok(chunk) => {
                                // Check for usage (usually in the last chunk with stream_options)
                                if let Some(usage) = chunk.usage {
                                    let mut continuation =
                                        OpenAI::continuation_telemetry_from_stream_usage(&usage);
                                    continuation.tool_exact_replay_used = tool_exact_replay_used;
                                    pending_choices.push_back(Ok(StreamingChoice::Telemetry(
                                        ProviderTelemetry {
                                            provider_name: Some("openai".to_string()),
                                            model: None,
                                            latency_ms: None,
                                            continuation: Some(continuation),
                                            extra: Default::default(),
                                        },
                                    )));
                                    return Some((
                                        Ok(StreamingChoice::Usage(benshu_provider_core::Usage {
                                            prompt_tokens: usage.prompt_tokens,
                                            completion_tokens: usage.completion_tokens,
                                            total_tokens: usage.total_tokens,
                                        })),
                                        (
                                            stream,
                                            bytes_buffer,
                                            current_tools,
                                            pending_messages,
                                            pending_choices,
                                        ),
                                    ));
                                }

                                if let Some(choice) = chunk.choices.first() {
                                    // Check for content
                                    if let Some(content) = &choice.delta.content {
                                        let content =
                                            OpenAI::sanitize_visible_message_text(content);
                                        if !content.is_empty() {
                                            return Some((
                                                Ok(StreamingChoice::Message(content)),
                                                (
                                                    stream,
                                                    bytes_buffer,
                                                    current_tools,
                                                    pending_messages,
                                                    pending_choices,
                                                ),
                                            ));
                                        }
                                    }

                                    // Check for tool calls
                                    if let Some(tool_calls) = &choice.delta.tool_calls {
                                        for tc in tool_calls {
                                            let index = tc.index.unwrap_or(0);
                                            let state = current_tools.entry(index).or_insert(
                                                ToolCallState {
                                                    id: None,
                                                    name: None,
                                                    arguments: String::new(),
                                                },
                                            );

                                            // Update ID
                                            if let Some(id) = &tc.id {
                                                state.id = Some(id.clone());
                                            }

                                            // Update Name
                                            if let Some(func) = &tc.function {
                                                if let Some(name) = &func.name {
                                                    state.name = Some(name.clone());
                                                }
                                                // Update Arguments
                                                if let Some(args) = &func.arguments {
                                                    state.arguments.push_str(args);
                                                }
                                            }
                                        }
                                    }

                                    // Check if tool calls are complete
                                    if choice.finish_reason.as_deref() == Some("tool_calls") {
                                        let mut tools_map = std::collections::HashMap::new();
                                        for (index, state) in current_tools.drain() {
                                            if let (Some(id), Some(name)) = (state.id, state.name) {
                                                if let Ok(args) =
                                                    serde_json::from_str(&state.arguments)
                                                {
                                                    tools_map.insert(
                                                        index,
                                                        benshu_protocol_core::ToolCall {
                                                            id,
                                                            name,
                                                            arguments: args,
                                                        },
                                                    );
                                                }
                                            }
                                        }

                                        if !tools_map.is_empty() {
                                            return Some((
                                                Ok(StreamingChoice::ParallelToolCalls(tools_map)),
                                                (
                                                    stream,
                                                    bytes_buffer,
                                                    current_tools,
                                                    pending_messages,
                                                    pending_choices,
                                                ),
                                            ));
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!("Failed to parse SSE chunk: {}", e);
                            }
                        }
                    }
                    continue;
                }

                // 2. Need more data from stream
                match stream.next().await {
                    Some(Ok(bytes)) => {
                        if let Err(e) = bytes_buffer.extend_from_slice(&bytes) {
                            return Some((
                                Err(e),
                                (
                                    stream,
                                    bytes_buffer,
                                    current_tools,
                                    pending_messages,
                                    pending_choices,
                                ),
                            ));
                        }
                        match bytes_buffer.extract_messages() {
                            Ok(messages) => {
                                pending_messages.extend(messages);
                            }
                            Err(e) => {
                                return Some((
                                    Err(e),
                                    (
                                        stream,
                                        bytes_buffer,
                                        current_tools,
                                        pending_messages,
                                        pending_choices,
                                    ),
                                ));
                            }
                        }
                    }
                    Some(Err(e)) => {
                        return Some((
                            Err(Error::Http(e)),
                            (
                                stream,
                                bytes_buffer,
                                current_tools,
                                pending_messages,
                                pending_choices,
                            ),
                        ));
                    }
                    None => return None,
                }
            }
        },
    )
}

/// Explicit sentinel used when a cloud model has not been selected by runtime binding.
pub const OPENAI_UNCONFIGURED_MODEL: &str = "benshu-unconfigured-model";
/// GPT-4 Turbo
pub const GPT_4_TURBO: &str = "gpt-4-turbo";
/// GPT-3.5 Turbo
pub const GPT_35_TURBO: &str = "gpt-3.5-turbo";

/// GPT-5.4 Thinking - Advanced reasoning (2026)
pub const GPT_5_4_THINKING: &str = "gpt-5.4-thinking";
/// GPT-5.4 Pro - Frontier performance (2026)
pub const GPT_5_4_PRO: &str = "gpt-5.4-pro";

#[cfg(test)]
mod local_contract_tests {
    use super::*;

    #[test]
    fn test_message_conversion() {
        let messages = vec![Message::user("Hello"), Message::assistant("Hi there!")];

        let converted = OpenAI::convert_messages(Some("Be helpful"), messages);

        assert_eq!(converted.len(), 3);
        assert_eq!(converted[0].role, "system");
        assert_eq!(converted[1].role, "user");
        assert_eq!(converted[2].role, "assistant");
    }

    #[test]
    fn convert_messages_merges_scattered_system_messages_first() {
        let messages = vec![
            Message::user("Hello"),
            Message::system("Runtime contract"),
            Message::assistant("Hi"),
            Message::system("Verification contract"),
        ];

        let converted = OpenAI::convert_messages(Some("Base contract"), messages);

        assert_eq!(converted.len(), 3);
        assert_eq!(converted[0].role, "system");
        let system = converted[0]
            .content
            .as_str()
            .expect("merged system content should be string");
        assert!(system.contains("Base contract"));
        assert!(system.contains("Runtime contract"));
        assert!(system.contains("Verification contract"));
        assert!(converted[1..]
            .iter()
            .all(|message| message.role != "system"));
        assert_eq!(converted[1].role, "user");
        assert_eq!(converted[2].role, "assistant");
    }

    #[test]
    fn visible_message_sanitizer_removes_empty_think_wrapper() {
        assert_eq!(
            OpenAI::sanitize_visible_message_text("<think>\n\n</think>\n\n收到"),
            "收到"
        );
    }
}

// --- benshu-inference Integration (Phase 21.4) ---

use benshu_inference::backend::{EmbeddingBackend, ModelBackend, OcrBackend};
use benshu_inference::engine::KvEngine;
use benshu_inference::GenerationConfig;
use benshu_knowledge::rag::Embeddings;
use parking_lot::RwLock;
use std::sync::Arc;

#[async_trait]
impl ModelBackend for OpenAI {
    fn model_info(&self) -> String {
        format!("Cloud-OpenAI: {} (Base: {})", self.name(), self.base_url)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn generate(
        &self,
        request_id: &str,
        prompt: &str,
        images: Option<Vec<image::DynamicImage>>,
        config: GenerationConfig,
        _kv_engine: Arc<RwLock<KvEngine>>,
    ) -> benshu_inference::Result<String> {
        let mut messages = Vec::new();
        // Handle images if provided for Vision models
        if let Some(imgs) = images {
            let mut parts = Vec::new();
            parts.push(benshu_protocol_core::ContentPart::Text {
                text: prompt.to_string(),
            });
            for img in imgs {
                // In a real impl, we'd convert image::DynamicImage to base64
                // For now, we assume a placeholder as this is cloud-side.
                parts.push(benshu_protocol_core::ContentPart::Image {
                    source: benshu_protocol_core::ImageSource::Url {
                        url: "local-image-attachment".into(),
                    },
                });
            }
            messages.push(Message::new(Role::User, Content::Parts(parts)));
        } else {
            messages.push(Message::user(prompt));
        }

        let request = benshu_provider_core::ChatRequest {
            model: config
                .session_id
                .unwrap_or_else(|| OPENAI_UNCONFIGURED_MODEL.to_string()),
            messages,
            temperature: Some(config.temperature as f64),
            max_tokens: Some(config.max_new_tokens as u64),
            ..Default::default()
        };

        let mut stream = self.stream_completion(request).await.map_err(|e| {
            benshu_inference::InferenceError::Execution(e.to_string(), request_id.to_string())
        })?;

        let mut final_text = String::new();
        while let Some(chunk) = stream.next().await {
            if let Ok(StreamingChoice::Message(text)) = chunk {
                final_text.push_str(&text);
            }
        }
        Ok(final_text)
    }

    async fn stream_generate(
        &self,
        request_id: &str,
        prompt: &str,
        images: Option<Vec<image::DynamicImage>>,
        config: GenerationConfig,
        kv_engine: Arc<RwLock<KvEngine>>,
        tx: tokio::sync::mpsc::Sender<benshu_inference::Result<String>>,
    ) -> benshu_inference::Result<()> {
        let res = self
            .generate(request_id, prompt, images, config, kv_engine)
            .await?;
        let _ = tx.send(Ok(res)).await;
        Ok(())
    }

    fn device_info(&self) -> benshu_inference::backend::DeviceType {
        benshu_inference::backend::DeviceType::Cloud
    }

    fn estimated_memory_usage(&self) -> u64 {
        0
    }
}

#[async_trait]
impl EmbeddingBackend for OpenAI {
    fn model_info(&self) -> String {
        format!("Cloud-OpenAI-Embeddings: {}", self.base_url)
    }

    fn dimension(&self) -> usize {
        1536 // Default for text-embedding-ada-002
    }

    fn device_info(&self) -> benshu_inference::backend::DeviceType {
        benshu_inference::backend::DeviceType::Cloud
    }

    fn estimated_memory_usage(&self) -> u64 {
        0 // Cloud models don't use local VRAM
    }

    async fn embed(&self, text: &str) -> benshu_inference::Result<Vec<f32>> {
        let res = <Self as Embeddings>::embed(self, text).await;
        res.map_err(|e| {
            benshu_inference::InferenceError::Execution(e.to_string(), "embeddings".to_string())
        })
    }
}

#[async_trait]
impl Embeddings for OpenAI {
    async fn embed(&self, text: &str) -> benshu_infra::error::Result<Vec<f32>> {
        self.create_embedding(text)
            .await
            .map_err(|e| benshu_infra::error::Error::Internal(e.to_string()))
    }
}

#[async_trait]
impl OcrBackend for OpenAI {
    fn model_info(&self) -> String {
        format!("Cloud-OpenAI-OCR (VLM): {}", self.base_url)
    }

    async fn recognize(&self, image: &image::DynamicImage) -> benshu_inference::Result<String> {
        self.generate(
            "ocr",
            "Extract text from image",
            Some(vec![image.clone()]),
            GenerationConfig::default(),
            Arc::new(RwLock::new(KvEngine::new(Default::default()))),
        )
        .await
        .map_err(|e| benshu_inference::InferenceError::Execution(e.to_string(), "ocr".to_string()))
    }

    fn estimated_memory_usage(&self) -> u64 {
        0
    }
    fn device_info(&self) -> benshu_inference::backend::DeviceType {
        benshu_inference::backend::DeviceType::Cloud
    }
}

#[async_trait]
impl benshu_inference::backend::ImageGenBackend for OpenAI {
    fn model_info(&self) -> String {
        format!("Cloud-OpenAI-DALL-E: {}", self.base_url)
    }

    async fn generate_image(
        &self,
        prompt: &str,
        size: (u32, u32),
        _config: benshu_inference::backend::DiffusionConfig,
    ) -> benshu_inference::Result<image::DynamicImage> {
        let size_str = format!("{}x{}", size.0, size.1);
        let data = self.create_image(prompt, &size_str).await.map_err(|e| {
            benshu_inference::InferenceError::Execution(e.to_string(), "image-gen".to_string())
        })?;

        image::load_from_memory(&data).map_err(|e| {
            benshu_inference::InferenceError::Internal(format!(
                "Failed to load generated image: {}",
                e
            ))
        })
    }

    async fn generate_image_img2img(
        &self,
        _prompt: &str,
        _initial_image: &image::DynamicImage,
        _config: benshu_inference::backend::DiffusionConfig,
    ) -> benshu_inference::Result<image::DynamicImage> {
        Err(benshu_inference::InferenceError::Internal(
            "Img2Img not implemented for Cloud-OpenAI".into(),
        ))
    }

    async fn generate_image_inpainting(
        &self,
        _prompt: &str,
        _initial_image: &image::DynamicImage,
        _mask: &image::DynamicImage,
        _config: benshu_inference::backend::DiffusionConfig,
    ) -> benshu_inference::Result<image::DynamicImage> {
        Err(benshu_inference::InferenceError::Internal(
            "Inpainting not implemented for Cloud-OpenAI".into(),
        ))
    }

    fn estimated_memory_usage(&self) -> u64 {
        0
    }

    fn device_info(&self) -> benshu_inference::backend::DeviceType {
        benshu_inference::backend::DeviceType::Cloud
    }
}

#[cfg(test)]
mod tests {
    use super::{sse_message_data, OpenAI, OpenAIChatRequest, OpenAIMessage};
    use crate::{Provider, StreamingChoice};
    use benshu_protocol_core::{Content, Message, Role};
    use benshu_provider_core::{ContextLimitError, FinishReason};
    use futures::StreamExt;
    use std::sync::Arc;

    #[test]
    fn local_openai_compatible_endpoint_keeps_native_stable_contract() {
        let provider = OpenAI::with_base_url("test-key", "http://localhost:11434/v1")
            .expect("provider should construct");

        assert!(provider.is_local());
        assert_eq!(provider.tool_contract_mode(), "native_tool_calling");
        assert_eq!(provider.mainline_stability(), "stable");
    }

    #[test]
    fn local_openai_compatible_endpoints_share_request_gate_per_base_url() {
        let first = OpenAI::with_base_url("test-key", "http://localhost:11434/v1")
            .expect("provider should construct");
        let second = OpenAI::with_base_url("test-key", "http://LOCALHOST:11434/v1/")
            .expect("provider should construct");
        let remote = OpenAI::with_base_url("test-key", "https://api.openai.com/v1")
            .expect("provider should construct");

        let first_gate = first.local_request_gate.as_ref().expect("local gate");
        let second_gate = second.local_request_gate.as_ref().expect("local gate");
        assert!(Arc::ptr_eq(first_gate, second_gate));
        assert!(remote.local_request_gate.is_none());
    }

    #[test]
    fn local_continuous_text_step_uses_streaming_by_default() {
        let provider = OpenAI::with_base_url("test-key", "http://localhost:11434/v1")
            .expect("provider should construct");
        let extra = serde_json::json!({
            "inference_runtime_owner": "continuous_text_step"
        });

        assert!(!OpenAI::should_use_non_stream_local_continuous_text_step(
            provider.is_local(),
            Some(&extra)
        ));
    }

    #[test]
    fn local_continuous_text_step_can_force_non_stream_path() {
        let provider = OpenAI::with_base_url("test-key", "http://localhost:11434/v1")
            .expect("provider should construct");
        let extra = serde_json::json!({
            "inference_runtime_owner": "continuous_text_step",
            "force_non_stream_local_continuous_text_step": true
        });

        assert!(OpenAI::should_use_non_stream_local_continuous_text_step(
            provider.is_local(),
            Some(&extra)
        ));
    }

    #[test]
    fn remote_continuous_text_step_keeps_provider_streaming_available() {
        let provider = OpenAI::with_base_url("test-key", "https://api.openai.com/v1")
            .expect("provider should construct");
        let extra = serde_json::json!({
            "inference_runtime_owner": "continuous_text_step"
        });

        assert!(!OpenAI::should_use_non_stream_local_continuous_text_step(
            provider.is_local(),
            Some(&extra)
        ));
    }

    #[test]
    fn sse_message_data_accepts_data_without_space() {
        assert_eq!(
            sse_message_data("data:{\"choices\":[]}\r\n\r\n").as_deref(),
            Some("{\"choices\":[]}")
        );
    }

    #[test]
    fn sse_message_data_combines_multiline_data_frames() {
        assert_eq!(
            sse_message_data("event: message\ndata: {\"a\":1}\ndata: {\"b\":2}\n\n").as_deref(),
            Some("{\"a\":1}\n{\"b\":2}")
        );
    }

    #[test]
    fn pseudo_tool_call_tags_are_parsed_for_local_bridge_outputs() {
        let calls = OpenAI::extract_pseudo_tool_calls(
            "<|tool_call>call:default_tool:web_search{queries: [\"The Lancet latest research heart disease treatments 2026\"]}<tool_call|>\n\nextra prose",
        );

        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.name, "web_search");
        assert_eq!(
            calls[0].function.arguments,
            serde_json::json!({
                "queries": ["The Lancet latest research heart disease treatments 2026"]
            })
            .to_string()
        );
    }

    #[test]
    fn parenthesized_pseudo_tool_call_tags_are_parsed_for_local_bridge_outputs() {
        let calls = OpenAI::extract_pseudo_tool_calls(
            "<|tool_call>call:fs.read_file(path=\"/tmp/sample.txt\", preview=true)<tool_call|>",
        );

        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.name, "fs.read_file");
        assert_eq!(
            calls[0].function.arguments,
            serde_json::json!({
                "path": "/tmp/sample.txt",
                "preview": true
            })
            .to_string()
        );
    }

    #[test]
    fn non_stream_response_repairs_pseudo_tool_call_content_into_tool_calls() {
        let response = serde_json::json!({
            "choices": [{
                "message": {
                    "content": "<|tool_call>call:default_tool:web_search{queries: [\"latest lancet heart treatment papers\"]}<tool_call|>\n\nnotice"
                }
            }],
            "model": "local-bridge"
        })
        .to_string();

        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let collected: Vec<_> = runtime.block_on(async {
            let stream = OpenAI::stream_from_nonstream_response(&response).expect("stream");
            stream.collect().await
        });

        assert!(collected.iter().any(|choice| matches!(
            choice,
            Ok(StreamingChoice::ToolCall { name, .. }) if name == "web_search"
        )));
        assert!(collected
            .iter()
            .any(|choice| matches!(choice, Ok(StreamingChoice::Finish(FinishReason::ToolCalls)))));
        assert!(!collected.iter().any(|choice| matches!(
            choice,
            Ok(StreamingChoice::Message(text)) if text.contains("<|tool_call>")
        )));
    }

    #[test]
    fn non_stream_response_maps_provider_reported_continuation_telemetry() {
        let response = serde_json::json!({
            "choices": [{
                "finish_reason": "stop",
                "message": {
                    "content": "在线"
                }
            }],
            "model": "local-bridge",
            "usage": {
                "prompt_tokens": 17,
                "completion_tokens": 2,
                "total_tokens": 19,
            },
            "timings": {
                "prompt_n": 17,
                "prompt_ms": 220.0,
                "predicted_ms": 33.0
            }
        })
        .to_string();

        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let collected: Vec<_> = runtime.block_on(async {
            let stream = OpenAI::stream_from_nonstream_response(&response).expect("stream");
            stream.collect().await
        });

        assert!(collected.iter().any(|choice| matches!(
        choice,
        Ok(StreamingChoice::Telemetry(telemetry))
            if telemetry
                .continuation
                .as_ref()
                .and_then(|continuation| continuation.prompt_tokens)
                == Some(17)
                && telemetry
                    .continuation
                    .as_ref()
                    .and_then(|continuation| continuation.prefill_ms)
                    == Some(220)
        )));
    }

    #[test]
    fn provider_context_limit_failure_becomes_structured_error() {
        let request = OpenAIChatRequest {
            model: "local-bridge".to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: serde_json::Value::String("x".repeat(8_000)),
                name: None,
                tool_call_id: None,
                tool_calls: None,
            }],
            temperature: None,
            max_tokens: Some(512),
            tools: Vec::new(),
            response_format: None,
            stream: true,
            stream_options: None,
        };

        let error = OpenAI::context_limit_error_from_provider_failure(
            &request,
            1024,
            "maximum context length exceeded",
        )
        .expect("context error");

        assert!(error.prompt_tokens > 1024);
        assert_eq!(error.configured_context_tokens, 1024);
        assert_eq!(error.requested_output_tokens, 512);
        assert!(error
            .to_provider_error_message()
            .contains(ContextLimitError::PROVIDER_ERROR_MARKER));
    }

    #[test]
    fn local_native_tools_require_user_message_for_chat_template() {
        let system_only = vec![OpenAIMessage {
            role: "system".to_string(),
            content: serde_json::Value::String("system".to_string()),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }];
        assert!(!OpenAI::local_request_can_use_native_tools(&system_only));

        let with_user = vec![
            OpenAIMessage {
                role: "system".to_string(),
                content: serde_json::Value::String("system".to_string()),
                name: None,
                tool_call_id: None,
                tool_calls: None,
            },
            OpenAIMessage {
                role: "user".to_string(),
                content: serde_json::Value::String("hello".to_string()),
                name: None,
                tool_call_id: None,
                tool_calls: None,
            },
        ];
        assert!(OpenAI::local_request_can_use_native_tools(&with_user));
    }

    #[test]
    fn convert_messages_uses_sampled_tool_replay_block_when_available() {
        let mut metadata = std::collections::HashMap::new();
        metadata.insert(
            "tool_replay_receipts".to_string(),
            serde_json::json!({
                "call_123": {
                    "tool_call_id": "call_123",
                    "replay_mode": "sampled_text_exact",
                    "sampled_call_block": "<|tool_call>{\"name\":\"web_search\",\"arguments\":{\"query\":\"北京天气\"}}<tool_call|>",
                    "sampled_call_fingerprint": "sampled",
                    "sampled_call_ref": "message://assistant/text/tool_call/call_123",
                    "normalized_call_fingerprint": "normalized"
                }
            })
            .to_string(),
        );
        let messages = vec![Message {
            role: Role::Assistant,
            content: Content::Parts(vec![benshu_protocol_core::ContentPart::ToolCall {
                id: "call_123".to_string(),
                name: "web_search".to_string(),
                arguments: serde_json::json!({"query": "北京天气"}),
            }]),
            name: None,
            unverified: false,
            source_collection: None,
            source_path: None,
            utility_score: 0.5,
            last_accessed: chrono::Utc::now(),
            created_at: chrono::Utc::now(),
            confidence: 1.0,
            used_experience_ids: Vec::new(),
            used_anti_pattern_ids: Vec::new(),
            metadata,
        }];

        assert!(OpenAI::messages_use_sampled_tool_replay(&messages));
        let converted = OpenAI::convert_messages(None, messages);
        assert!(converted[0]
            .content
            .as_str()
            .is_some_and(|content| content.contains("<|tool_call>")));
        assert!(converted[0].tool_calls.is_some());
    }

    #[test]
    fn non_stream_response_recovers_quoted_visible_candidate_from_reasoning_content() {
        let response = serde_json::json!({
            "choices": [{
                "finish_reason": "length",
                "message": {
                    "content": "",
                    "reasoning_content": "* Task: write text\n* Draft:\n    \"系统完成了本轮检查，核心状态稳定。\n连续性记录：测试。\n下一步钩子：测试。\"\n\nWait, ensure suffix."
                }
            }],
            "model": "local-bridge"
        })
        .to_string();

        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let collected: Vec<_> = runtime.block_on(async {
            let stream = OpenAI::stream_from_nonstream_response(&response).expect("stream");
            stream.collect().await
        });

        assert!(collected.iter().any(|choice| matches!(
            choice,
            Ok(StreamingChoice::Message(text))
                if text.contains("系统完成了本轮检查")
                    && text.contains("连续性记录：测试。")
                    && text.contains("下一步钩子：测试。")
                    && !text.contains("Task:")
                    && !text.contains("Wait,")
        )));
    }

    #[test]
    fn non_stream_response_recovers_structured_visible_artifact_from_reasoning_content() {
        let reasoning_content = "I should draft the artifact, but only the final artifact block is safe to emit.\n\n[Document Metadata]\n标题：星火归航\n类型：长篇文本\n主角/主体：林澈\n目标规模：连续生成\n当前进度：1/100\n\n### 第1步 星火初燃\n林澈在旧城边缘醒来时，雪线已经压到屋檐。他先确认怀里的铜灯还亮着，再把昨夜记录的十条来源边界重新刻进木片，提醒自己只能学习公开榜单的题材趋势，不能复刻任何具体情节。街口传来巡夜人的脚步，他决定带着铜灯穿过废井，寻找能解释寒潮来源的第一枚灰色符印。这个选择让他失去安全屋，却换来一条通往地底书库的线索。\n\n连续性记录：标题锁定为《星火归航》；主角为林澈；核心目标是寻找寒潮来源并建立自己的修行路径；不得复刻真实作品设定。\n\n下一步钩子：林澈进入废井后，发现井壁符印正在回应铜灯。 \n\nWait, check whether this is long enough.";
        assert!(
            OpenAI::extract_structured_visible_candidate_from_reasoning(reasoning_content)
                .is_some()
        );
        let response = serde_json::json!({
            "choices": [{
                "finish_reason": "length",
                "message": {
                    "content": "",
                    "reasoning_content": reasoning_content
                }
            }],
            "model": "local-bridge"
        })
        .to_string();

        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let collected: Vec<_> = runtime.block_on(async {
            let stream = OpenAI::stream_from_nonstream_response(&response).expect("stream");
            stream.collect().await
        });

        assert!(collected.iter().any(|choice| matches!(
            choice,
            Ok(StreamingChoice::Message(text))
                if text.contains("[Document Metadata]")
                    && text.contains("### 第1步 星火初燃")
                    && text.contains("连续性记录：标题锁定")
                    && text.contains("下一步钩子：林澈进入废井")
                    && !text.contains("I should draft")
                    && !text.contains("Wait,")
        )));
    }

    #[test]
    fn reasoning_candidate_recovery_does_not_emit_unquoted_reasoning() {
        let response = serde_json::json!({
            "choices": [{
                "message": {
                    "content": "",
                    "reasoning_content": "* Task: think through the problem\n* Requirement: do not expose this"
                }
            }],
            "model": "local-bridge"
        })
        .to_string();

        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let collected: Vec<_> = runtime.block_on(async {
            let stream = OpenAI::stream_from_nonstream_response(&response).expect("stream");
            stream.collect().await
        });

        assert!(!collected
            .iter()
            .any(|choice| matches!(choice, Ok(StreamingChoice::Message(_)))));
    }
}
